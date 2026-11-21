use crate::api::{map_reqwest_error, sanitize_output, Track, PLAY_MAX_BYTES};
use anyhow::{anyhow, Result};
use rodio::{Decoder, DeviceSinkBuilder, Player, Source};
use serde_json::Value;
use std::io::{Cursor, Read};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::{Receiver, Sender},
    Arc,
};
use std::thread;
use std::time::Duration;

pub enum PlayerCmd {
    PlayTrack { track: Track },
    PauseResume,
    Stop,
    SetVolume(f32),
    Seek(u64),
    Shutdown,
}

#[derive(Clone, Debug)]
pub enum PlayerEvent {
    Downloading { artist: String, title: String },
    Playing { artist: String, title: String, duration_secs: Option<u64> },
    Paused { artist: String, title: String, position_secs: u64, duration_secs: Option<u64> },
    Resumed { artist: String, title: String, position_secs: u64, duration_secs: Option<u64> },
    Stopped,
    Ended,
    Position { position_secs: u64, duration_secs: Option<u64> },
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

    while let Ok(cmd) = cmd_rx.recv() {
        match cmd {
            PlayerCmd::Shutdown => {
                if let Some(s) = state.take() {
                    s.stop();
                }
                break;
            }
            PlayerCmd::Stop => {
                if let Some(s) = state.take() {
                    s.stop();
                }
                send_evt(&evt_tx, &ctx, PlayerEvent::Stopped);
            }
            PlayerCmd::SetVolume(vol) => {
                volume = vol.clamp(0.0, 1.5);
                if let Some(s) = state.as_ref() {
                    s.player.set_volume(volume);
                }
                send_evt(&evt_tx, &ctx, PlayerEvent::Volume(volume));
            }
            PlayerCmd::Seek(sec) => {
                if let Some(s) = state.as_mut() {
                    let target = Duration::from_secs(sec);
                    let pos = if s.player.try_seek(target).is_ok() {
                        target
                    } else {
                        let decoder = match Decoder::try_from(Cursor::new(s.bytes.as_ref().clone())) {
                            Ok(d) => d,
                            Err(_) => {
                                send_evt(&evt_tx, &ctx, PlayerEvent::Error("request failed (decode)".to_string()));
                                continue;
                            }
                        };
                        let source = decoder.skip_duration(target);
                        s.player.clear();
                        s.player.append(source);
                        if s.paused {
                            s.player.pause();
                        }
                        target
                    };
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
                        s.player.play();
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
                        s.player.pause();
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
                    s.stop();
                }
                send_evt(
                    &evt_tx,
                    &ctx,
                    PlayerEvent::Downloading {
                        artist: track.artist.clone(),
                        title: track.title.clone(),
                    },
                );
                match play_track(&client, track, &evt_tx, &ctx, volume) {
                    Ok(s) => state = Some(s),
                    Err(e) => {
                        send_evt(&evt_tx, &ctx, PlayerEvent::Error(sanitize_output(&format!("{e:#}"))));
                    }
                }
            }
        }

        // drain stale Ended events handling: monitor thread sends Ended; UI handles it.
        // Cleanup finished state if ended
        if let Some(s) = state.as_ref() {
            if s.player.empty() && !s.paused {
                // monitor will report Ended; keep state until Stop/PlayTrack/Shutdown
            }
        }
    }
}

fn play_track(
    client: &reqwest::blocking::Client,
    track: Track,
    evt_tx: &Sender<PlayerEvent>,
    ctx: &eframe::egui::Context,
    volume: f32,
) -> Result<PlayingState> {
    let url = select_play_url(client, &track)?;
    let bytes = download_track_bytes(client, &url)?;
    let duration_secs = decode_duration(&bytes).map(|d| d.as_secs()).or(track.duration_secs);

    let mut stream = DeviceSinkBuilder::open_default_sink().map_err(|_| anyhow!("request failed (decode)"))?;
    stream.log_on_drop(false);
    let player = Arc::new(Player::connect_new(&stream.mixer()));
    player.set_volume(volume);
    let bytes = Arc::new(bytes);
    let decoder = Decoder::try_from(Cursor::new(bytes.as_ref().clone())).map_err(|_| anyhow!("request failed (decode)"))?;
    player.append(decoder);

    send_evt(
        evt_tx,
        ctx,
        PlayerEvent::Playing {
            artist: track.artist.clone(),
            title: track.title.clone(),
            duration_secs,
        },
    );

    let stop_flag = Arc::new(AtomicBool::new(false));
    let monitor = spawn_monitor(
        Arc::clone(&player),
        Arc::clone(&stop_flag),
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
        monitor: Some(monitor),
        bytes,
    })
}

fn spawn_monitor(
    player: Arc<Player>,
    stop_flag: Arc<AtomicBool>,
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
            if player.empty() {
                let _ = evt_tx.send(PlayerEvent::Ended);
                ctx.request_repaint();
                break;
            }
        }
    })
}

fn decode_duration(bytes: &[u8]) -> Option<Duration> {
    let decoder = Decoder::try_from(Cursor::new(bytes.to_vec())).ok()?;
    decoder.total_duration()
}

fn select_play_url(client: &reqwest::blocking::Client, track: &Track) -> Result<String> {
    let main_url = track
        .url
        .as_deref()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("no playable mp3"))?;
    match probe_first_bytes(client, main_url) {
        Ok(bytes) if is_mp3_bytes(&bytes) => Ok(main_url.to_string()),
        Ok(_) | Err(_) => {
            let streams = track
                .audio_streams
                .as_ref()
                .and_then(Value::as_array)
                .ok_or_else(|| anyhow!("no playable mp3"))?;
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
            Err(anyhow!("no playable mp3"))
        }
    }
}

fn probe_first_bytes(client: &reqwest::blocking::Client, url: &str) -> Result<Vec<u8>> {
    let mut resp = client
        .get(url)
        .header(reqwest::header::RANGE, "bytes=0-15")
        .send()
        .map_err(map_reqwest_error)?;
    let status = resp.status();
    if !status.is_success() {
        return Err(anyhow!("request failed (status {})", status.as_u16()));
    }
    let mut buf = [0u8; 16];
    let mut read_total = 0usize;
    loop {
        if read_total >= buf.len() {
            break;
        }
        let n = resp
            .read(&mut buf[read_total..])
            .map_err(|_| anyhow!("request failed (decode)"))?;
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

fn download_track_bytes(client: &reqwest::blocking::Client, url: &str) -> Result<Vec<u8>> {
    let mut resp = client.get(url).send().map_err(map_reqwest_error)?;
    let status = resp.status();
    if !status.is_success() {
        return Err(anyhow!("request failed (status {})", status.as_u16()));
    }
    let content_length = resp.content_length();
    if let Some(len) = content_length {
        if len as usize > PLAY_MAX_BYTES {
            return Err(anyhow!("request failed (decode)"));
        }
    }
    let mut buf: Vec<u8> = Vec::with_capacity(
        content_length
            .map(|l| (l as usize).min(PLAY_MAX_BYTES))
            .unwrap_or(0),
    );
    let mut chunk = [0u8; 64 * 1024];
    loop {
        let n = resp
            .read(&mut chunk)
            .map_err(|_| anyhow!("request failed (decode)"))?;
        if n == 0 {
            break;
        }
        if buf.len().saturating_add(n) > PLAY_MAX_BYTES {
            return Err(anyhow!("request failed (decode)"));
        }
        buf.extend_from_slice(&chunk[..n]);
    }
    if let Some(len) = content_length {
        if buf.len() != len as usize {
            return Err(anyhow!("request failed (decode)"));
        }
    }
    Ok(buf)
}
