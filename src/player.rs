use crate::api::{map_reqwest_error, sanitize_output, Track, PLAY_MAX_BYTES};
use anyhow::{anyhow, Result};
use rodio::{Decoder, DeviceSinkBuilder, Player, Source};
use serde_json::Value;
use std::io::{Cursor, Read};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    mpsc::{Receiver, Sender},
    Arc, Mutex,
};
use std::thread;
use std::time::Duration;

pub enum PlayerCmd {
    PlayTrack { track: Track },
    Preload { track: Track },
    PauseResume,
    Stop,
    SetVolume(f32),
    Seek(u64),
    Shutdown,
}

struct PreloadEntry {
    owner_id: i64,
    id: i64,
    bytes: Arc<Vec<u8>>,
}

#[derive(Clone, Debug)]
pub enum PlayerEvent {
    Downloading { artist: String, title: String, generation: u64 },
    Playing { artist: String, title: String, duration_secs: Option<u64>, generation: u64 },
    Paused { artist: String, title: String, position_secs: u64, duration_secs: Option<u64> },
    Resumed { artist: String, title: String, position_secs: u64, duration_secs: Option<u64> },
    Stopped,
    Ended { generation: u64 },
    Volume(f32),
    Error(String),
}

pub fn spawn_player_worker(
    client: reqwest::blocking::Client,
    cmd_rx: Receiver<PlayerCmd>,
    evt_tx: Sender<PlayerEvent>,
    ctx: eframe::egui::Context,
) -> thread::JoinHandle<()> {
    thread::spawn(move || worker_loop(client, cmd_rx, evt_tx, ctx))
}

fn send_evt(evt_tx: &Sender<PlayerEvent>, ctx: &eframe::egui::Context, evt: PlayerEvent) {
    let _ = evt_tx.send(evt);
    ctx.request_repaint();
}

struct PlayingState {
    _stream: rodio::MixerDeviceSink,
    player: Arc<Player>,
    artist: String,
    title: String,
    duration_secs: Option<u64>,
    paused: bool,
    stop_flag: Arc<AtomicBool>,
    seeking: Arc<AtomicBool>,
    monitor: Option<thread::JoinHandle<()>>,
    bytes: Arc<Vec<u8>>,
}

impl PlayingState {
    fn stop(mut self) {
        self.stop_flag.store(true, Ordering::Relaxed);
        self.player.stop();
        if let Some(h) = self.monitor.take() {
            let _ = h.join();
        }
    }
}

fn worker_loop(
    client: reqwest::blocking::Client,
    cmd_rx: Receiver<PlayerCmd>,
    evt_tx: Sender<PlayerEvent>,
    ctx: eframe::egui::Context,
) {
    let mut state: Option<PlayingState> = None;
    let mut volume: f32 = 1.0;
    let current_generation = Arc::new(AtomicU64::new(0));
    let preload_slot: Arc<Mutex<Option<PreloadEntry>>> = Arc::new(Mutex::new(None));
    let preload_req = Arc::new(AtomicU64::new(0));

    while let Ok(cmd) = cmd_rx.recv() {
        match cmd {
            PlayerCmd::Shutdown => {
                *preload_slot.lock().unwrap_or_else(|e| e.into_inner()) = None;
                if let Some(s) = state.take() {
                    if !s.paused {
                        fade_out(&s.player, s.player.volume());
                    }
                    s.stop();
                }
                break;
            }
            PlayerCmd::Stop => {
                *preload_slot.lock().unwrap_or_else(|e| e.into_inner()) = None;
                if let Some(s) = state.take() {
                    if !s.paused {
                        fade_out(&s.player, s.player.volume());
                    }
                    s.stop();
                }
                send_evt(&evt_tx, &ctx, PlayerEvent::Stopped);
            }
            PlayerCmd::SetVolume(vol) => {
                volume = vol.clamp(0.0, 1.0);
                if let Some(s) = state.as_ref() {
                    s.player.set_volume(volume);
                }
                send_evt(&evt_tx, &ctx, PlayerEvent::Volume(volume));
            }
            PlayerCmd::Seek(sec) => {
                if let Some(s) = state.as_mut() {
                    let target = Duration::from_secs(sec);
                    s.seeking.store(true, Ordering::Relaxed);
                    let pos = match s.player.try_seek(target) {
                        Ok(()) => target,
                        Err(_) => {
                            crate::log_status_error("try_seek failed");
                            let decoder = match make_decoder(Arc::clone(&s.bytes)) {
                                Ok(d) => d,
                                Err(_) => {
                                    s.seeking.store(false, Ordering::Relaxed);
                                    send_evt(&evt_tx, &ctx, PlayerEvent::Error("\u{041e}\u{0448}\u{0438}\u{0431}\u{043a}\u{0430} \u{0437}\u{0430}\u{043f}\u{0440}\u{043e}\u{0441}\u{0430} (\u{0434}\u{0430}\u{043d}\u{043d}\u{044b}\u{0435})".to_string()));
                                    continue;
                                }
                            };
                            let source = decoder.skip_duration(target);
                            s.player.clear();
                            s.player.append(source);
                            if s.paused {
                                s.player.pause();
                            } else {
                                s.player.play();
                            }
                            target
                        }
                    };
                    s.seeking.store(false, Ordering::Relaxed);
                    if s.paused {
                        send_evt(&evt_tx, &ctx, PlayerEvent::Paused { artist: s.artist.clone(), title: s.title.clone(), position_secs: pos.as_secs(), duration_secs: s.duration_secs });
                    } else {
                        send_evt(&evt_tx, &ctx, PlayerEvent::Resumed { artist: s.artist.clone(), title: s.title.clone(), position_secs: pos.as_secs(), duration_secs: s.duration_secs });
                    }
                }
            }
            PlayerCmd::PauseResume => {
                if let Some(s) = state.as_mut() {
                    if s.paused {
                        s.player.set_volume(0.0);
                        s.player.play();
                        fade_in(&s.player, volume);
                        s.paused = false;
                        send_evt(
                            &evt_tx,
                            &ctx,
                            PlayerEvent::Resumed {
                                artist: s.artist.clone(),
                                title: s.title.clone(),
                                position_secs: s.player.get_pos().as_secs(),
                                duration_secs: s.duration_secs,
                            },
                        );
                    } else {
                        fade_out(&s.player, s.player.volume());
                        s.player.pause();
                        s.player.set_volume(volume);
                        s.paused = true;
                        send_evt(
                            &evt_tx,
                            &ctx,
                            PlayerEvent::Paused {
                                artist: s.artist.clone(),
                                title: s.title.clone(),
                                position_secs: s.player.get_pos().as_secs(),
                                duration_secs: s.duration_secs,
                            },
                        );
                    }
                }
            }
            PlayerCmd::PlayTrack { track } => {
                if let Some(s) = state.take() {
                    if !s.paused {
                        fade_out(&s.player, s.player.volume());
                    }
                    s.stop();
                }
                preload_req.fetch_add(1, Ordering::Relaxed);
                let preloaded = {
                    let mut slot = preload_slot.lock().unwrap_or_else(|e| e.into_inner());
                    match slot.as_ref() {
                        Some(entry) if entry.owner_id == track.owner_id && entry.id == track.id => {
                            let bytes = Arc::clone(&entry.bytes);
                            *slot = None;
                            Some(bytes)
                        }
                        _ => None,
                    }
                };
        let generation = current_generation.fetch_add(1, Ordering::Relaxed) + 1;
                send_evt(
                    &evt_tx,
                    &ctx,
                    PlayerEvent::Downloading {
                        artist: track.artist.clone(),
                        title: track.title.clone(),
                        generation,
                    },
                );
                match play_track(&client, track, &evt_tx, &ctx, volume, generation, Arc::clone(&current_generation), preloaded) {
                    Ok(s) => state = Some(s),
                    Err(e) => {
                        send_evt(&evt_tx, &ctx, PlayerEvent::Error(sanitize_output(&format!("{e:#}"))));
                    }
                }
            }
            PlayerCmd::Preload { track } => {
                let already = {
                    let slot = preload_slot.lock().unwrap_or_else(|e| e.into_inner());
                    slot.as_ref().map(|e| (e.owner_id, e.id)) == Some((track.owner_id, track.id))
                };
                if already {
                    continue;
                }
                let req = preload_req.fetch_add(1, Ordering::Relaxed) + 1;
                let slot = Arc::clone(&preload_slot);
                let req_flag = Arc::clone(&preload_req);
                let client2 = client.clone();
                thread::spawn(move || {
                    let url = match select_play_url(&client2, &track) {
                        Ok(u) => u,
                        Err(_) => return,
                    };
                    let bytes = match download_track_bytes(&client2, &url) {
                        Ok(b) => Arc::new(b),
                        Err(_) => return,
                    };
                    if req_flag.load(Ordering::Relaxed) != req {
                        return;
                    }
                    let mut slot = slot.lock().unwrap_or_else(|e| e.into_inner());
                    if req_flag.load(Ordering::Relaxed) == req {
                        *slot = Some(PreloadEntry { owner_id: track.owner_id, id: track.id, bytes });
                    }
                });
            }
        }
    }
}

fn fade_out(player: &Player, start: f32) {
    let steps = 6u32;
    for i in 1..=steps {
        let t = i as f32 / steps as f32;
        player.set_volume(start * (1.0 - t));
        thread::sleep(Duration::from_millis(5));
    }
    player.set_volume(0.0);
}

fn fade_in(player: &Player, target: f32) {
    let steps = 6u32;
    for i in 1..=steps {
        let t = i as f32 / steps as f32;
        player.set_volume(target * t);
        thread::sleep(Duration::from_millis(5));
    }
    player.set_volume(target);
}

fn play_track(
    client: &reqwest::blocking::Client,
    track: Track,
    evt_tx: &Sender<PlayerEvent>,
    ctx: &eframe::egui::Context,
    volume: f32,
    generation: u64,
    current_generation: Arc<AtomicU64>,
    preloaded: Option<Arc<Vec<u8>>>,
) -> Result<PlayingState> {
    let bytes = match preloaded {
        Some(b) => b,
        None => {
            let url = select_play_url(client, &track)?;
            Arc::new(download_track_bytes(client, &url)?)
        }
    };
    let duration_secs = decode_duration(Arc::clone(&bytes)).map(|d| d.as_secs()).or(track.duration_secs);

    let mut stream = DeviceSinkBuilder::open_default_sink().map_err(|_| anyhow!("\u{041e}\u{0448}\u{0438}\u{0431}\u{043a}\u{0430} \u{0437}\u{0430}\u{043f}\u{0440}\u{043e}\u{0441}\u{0430} (\u{0434}\u{0430}\u{043d}\u{043d}\u{044b}\u{0435})"))?;
    stream.log_on_drop(false);
    let player = Arc::new(Player::connect_new(&stream.mixer()));
    player.set_volume(volume);
    let decoder = make_decoder(Arc::clone(&bytes))?;
    player.append(decoder);

    send_evt(
        evt_tx,
        ctx,
        PlayerEvent::Playing {
            artist: track.artist.clone(),
            title: track.title.clone(),
            duration_secs,
            generation,
        },
    );

    let stop_flag = Arc::new(AtomicBool::new(false));
    let seeking = Arc::new(AtomicBool::new(false));
    let monitor = spawn_monitor(
        Arc::clone(&player),
        Arc::clone(&stop_flag),
        Arc::clone(&seeking),
        generation,
        current_generation,
        evt_tx.clone(),
        ctx.clone(),
        duration_secs,
    );

    Ok(PlayingState {
        _stream: stream,
        player,
        artist: track.artist,
        title: track.title,
        duration_secs,
        paused: false,
        stop_flag,
        seeking,
        monitor: Some(monitor),
        bytes,
    })
}

fn spawn_monitor(
    player: Arc<Player>,
    stop_flag: Arc<AtomicBool>,
    seeking: Arc<AtomicBool>,
    generation: u64,
    current_generation: Arc<AtomicU64>,
    evt_tx: Sender<PlayerEvent>,
    ctx: eframe::egui::Context,
    _duration_secs: Option<u64>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        loop {
            thread::sleep(Duration::from_millis(250));
            if stop_flag.load(Ordering::Relaxed) {
                break;
            }
            if current_generation.load(Ordering::Relaxed) != generation {
                break;
            }
            if seeking.load(Ordering::Relaxed) {
                continue;
            }
            if player.empty() {
                if current_generation.load(Ordering::Relaxed) == generation && !seeking.load(Ordering::Relaxed) {
                    let _ = evt_tx.send(PlayerEvent::Ended { generation });
                    ctx.request_repaint();
                }
                break;
            }
        }
    })
}

#[derive(Clone)]
struct SharedBytes(Arc<Vec<u8>>);

impl AsRef<[u8]> for SharedBytes {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

fn make_decoder(bytes: Arc<Vec<u8>>) -> Result<Decoder<Cursor<SharedBytes>>> {
    let len = bytes.len() as u64;
    Decoder::builder()
        .with_data(Cursor::new(SharedBytes(bytes)))
        .with_byte_len(len)
        .with_seekable(true)
        .build()
        .map_err(|_| anyhow!("\u{041e}\u{0448}\u{0438}\u{0431}\u{043a}\u{0430} \u{0437}\u{0430}\u{043f}\u{0440}\u{043e}\u{0441}\u{0430} (\u{0434}\u{0430}\u{043d}\u{043d}\u{044b}\u{0435})"))
}

fn decode_duration(bytes: Arc<Vec<u8>>) -> Option<Duration> {
    let decoder = make_decoder(bytes).ok()?;
    decoder.total_duration()
}

fn select_play_url(client: &reqwest::blocking::Client, track: &Track) -> Result<String> {
    let main_url = track
        .url
        .as_deref()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("\u{041d}\u{0435}\u{0442} \u{0434}\u{043e}\u{0441}\u{0442}\u{0443}\u{043f}\u{043d}\u{043e}\u{0433}\u{043e} mp3"))?;
    match probe_first_bytes(client, main_url) {
        Ok(bytes) if is_mp3_bytes(&bytes) => Ok(main_url.to_string()),
        Ok(_) | Err(_) => {
            let streams = track
                .audio_streams
                .as_ref()
                .and_then(Value::as_array)
                .ok_or_else(|| anyhow!("\u{041d}\u{0435}\u{0442} \u{0434}\u{043e}\u{0441}\u{0442}\u{0443}\u{043f}\u{043d}\u{043e}\u{0433}\u{043e} mp3"))?;
            for stream in streams {
                let stype = stream.get("type").and_then(Value::as_str).unwrap_or("");
                if stype != "mp3" {
                    continue;
                }
                let url = stream
                    .get("url")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty());
                if let Some(url) = url {
                    return Ok(url.to_string());
                }
            }
            Err(anyhow!("\u{041d}\u{0435}\u{0442} \u{0434}\u{043e}\u{0441}\u{0442}\u{0443}\u{043f}\u{043d}\u{043e}\u{0433}\u{043e} mp3"))
        }
    }
}

fn probe_first_bytes(client: &reqwest::blocking::Client, url: &str) -> Result<Vec<u8>> {
    let mut resp = crate::api::send_with_retry(|| client.get(url).header(reqwest::header::RANGE, "bytes=0-15"))?;
    let status = resp.status();
    if !status.is_success() {
        return Err(anyhow!("\u{041e}\u{0448}\u{0438}\u{0431}\u{043a}\u{0430} \u{0437}\u{0430}\u{043f}\u{0440}\u{043e}\u{0441}\u{0430} (\u{043a}\u{043e}\u{0434} {})", status.as_u16()));
    }
    let mut buf = [0u8; 16];
    let mut read_total = 0usize;
    loop {
        if read_total >= buf.len() {
            break;
        }
        let n = resp
            .read(&mut buf[read_total..])
            .map_err(|_| anyhow!("\u{041e}\u{0448}\u{0438}\u{0431}\u{043a}\u{0430} \u{0437}\u{0430}\u{043f}\u{0440}\u{043e}\u{0441}\u{0430} (\u{0434}\u{0430}\u{043d}\u{043d}\u{044b}\u{0435})"))?;
        if n == 0 {
            break;
        }
        read_total += n;
    }
    Ok(buf[..read_total].to_vec())
}

fn is_mp3_bytes(bytes: &[u8]) -> bool {
    if bytes.starts_with(b"ID3") {
        return true;
    }
    bytes.len() >= 2 && bytes[0] == 0xFF && matches!(bytes[1], 0xE2..=0xFF)
}

struct DownloadFail {
    retryable: bool,
    msg: String,
}

fn download_track_bytes(client: &reqwest::blocking::Client, url: &str) -> Result<Vec<u8>> {
    let mut attempt: u64 = 0;
    loop {
        match download_once(client, url) {
            Ok(bytes) => return Ok(bytes),
            Err(f) => {
                if f.retryable && attempt < 2 {
                    attempt += 1;
                    std::thread::sleep(Duration::from_millis(400 * attempt));
                    continue;
                }
                return Err(anyhow!("{}", f.msg));
            }
        }
    }
}

fn download_once(client: &reqwest::blocking::Client, url: &str) -> std::result::Result<Vec<u8>, DownloadFail> {
    let mut resp = client.get(url).send().map_err(|e| DownloadFail {
        retryable: e.is_timeout() || e.is_connect(),
        msg: format!("{:#}", map_reqwest_error(e)),
    })?;
    let status = resp.status();
    if !status.is_success() {
        return Err(DownloadFail {
            retryable: status.is_server_error(),
            msg: format!("\u{041e}\u{0448}\u{0438}\u{0431}\u{043a}\u{0430} \u{0437}\u{0430}\u{043f}\u{0440}\u{043e}\u{0441}\u{0430} (\u{043a}\u{043e}\u{0434} {})", status.as_u16()),
        });
    }
    let Some(len) = resp.content_length() else {
        return Err(DownloadFail {
            retryable: false,
            msg: "\u{041e}\u{0448}\u{0438}\u{0431}\u{043a}\u{0430} \u{0437}\u{0430}\u{043f}\u{0440}\u{043e}\u{0441}\u{0430} (\u{043d}\u{0435}\u{0442} \u{0440}\u{0430}\u{0437}\u{043c}\u{0435}\u{0440}\u{0430})".to_string(),
        });
    };
    if len as usize > PLAY_MAX_BYTES {
        return Err(DownloadFail {
            retryable: false,
            msg: "\u{041e}\u{0448}\u{0438}\u{0431}\u{043a}\u{0430} \u{0437}\u{0430}\u{043f}\u{0440}\u{043e}\u{0441}\u{0430} (\u{0434}\u{0430}\u{043d}\u{043d}\u{044b}\u{0435})".to_string(),
        });
    }
    let mut buf: Vec<u8> = Vec::with_capacity(len as usize);
    let mut chunk = [0u8; 64 * 1024];
    loop {
        let n = resp.read(&mut chunk).map_err(|_| DownloadFail {
            retryable: true,
            msg: "\u{041e}\u{0448}\u{0438}\u{0431}\u{043a}\u{0430} \u{0437}\u{0430}\u{043f}\u{0440}\u{043e}\u{0441}\u{0430} (\u{0434}\u{0430}\u{043d}\u{043d}\u{044b}\u{0435})".to_string(),
        })?;
        if n == 0 {
            break;
        }
        if buf.len().saturating_add(n) > PLAY_MAX_BYTES {
            return Err(DownloadFail {
                retryable: false,
                msg: "\u{041e}\u{0448}\u{0438}\u{0431}\u{043a}\u{0430} \u{0437}\u{0430}\u{043f}\u{0440}\u{043e}\u{0441}\u{0430} (\u{0434}\u{0430}\u{043d}\u{043d}\u{044b}\u{0435})".to_string(),
            });
        }
        buf.extend_from_slice(&chunk[..n]);
    }
    if buf.len() != len as usize {
        return Err(DownloadFail {
            retryable: true,
            msg: "\u{041e}\u{0448}\u{0438}\u{0431}\u{043a}\u{0430} \u{0437}\u{0430}\u{043f}\u{0440}\u{043e}\u{0441}\u{0430} (\u{0434}\u{0430}\u{043d}\u{043d}\u{044b}\u{0435})".to_string(),
        });
    }
    Ok(buf)
}
