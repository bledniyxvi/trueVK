use crate::api::{self, AudioGetResponse, Track, DEFAULT_ANDROID_UA};
use crate::player::{PlayerCmd, PlayerEvent};
use eframe::egui;
use std::sync::mpsc::{Receiver, Sender};
use std::time::{Duration, Instant};

struct NowPlaying {
    artist: String,
    title: String,
    base_secs: f64,
    started_at: Option<Instant>,
    duration_secs: Option<u64>,
    paused: bool,
    track_index: Option<usize>,
}

impl NowPlaying {
    fn position_secs(&self) -> u64 {
        let mut pos = self.base_secs;
        if !self.paused {
            if let Some(started) = self.started_at {
                pos += started.elapsed().as_secs_f64();
            }
        }
        if let Some(dur) = self.duration_secs {
            pos = pos.min(dur as f64);
        }
        if pos < 0.0 { 0 } else { pos as u64 }
    }
}

pub fn run_gui(token: Option<String>, ua: Option<String>) -> eframe::Result<()> {
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel::<PlayerCmd>();
    let (evt_tx, evt_rx) = std::sync::mpsc::channel::<PlayerEvent>();
    let (ui_evt_tx, ui_evt_rx) = std::sync::mpsc::channel::<UiEvent>();

    let mut options = eframe::NativeOptions::default();
    options.glow_options.vsync = true;
    eframe::run_native(
        "vkmusic",
        options,
        Box::new(move |cc| {
            let ctx = cc.egui_ctx.clone();
            let client_ua = ua.clone().unwrap_or_else(|| DEFAULT_ANDROID_UA.to_string());
            let client = api::build_client(&client_ua).unwrap_or_else(|_| reqwest::blocking::Client::new());
            crate::player::spawn_player_worker(client, cmd_rx, evt_tx, ctx.clone());
            Ok(Box::new(GuiAppImpl::new(cmd_tx, evt_rx, ui_evt_tx, ui_evt_rx, token, ua, ctx)))
        }),
    )
}

enum UiEvent {
    TracksLoaded { offset: usize, count: Option<u64>, items: Vec<Track> },
    LoadError(String),
}

struct GuiAppImpl {
    cmd_tx: Sender<PlayerCmd>,
    evt_rx: Receiver<PlayerEvent>,
    ui_evt_tx: Sender<UiEvent>,
    ui_evt_rx: Receiver<UiEvent>,
    token: String,
    ua: String,
    logged_in: bool,
    login_input: String,
    ua_input: String,
    tracks: Vec<Track>,
    tracks_count: Option<u64>,
    loading_tracks: bool,
    next_offset: usize,
    status: String,
    now_playing: Option<NowPlaying>,
    volume: f32,
    seek_drag: Option<u64>,
    pending_play_index: Option<usize>,
    current_gen: u64,
    fps_enabled: bool,
    fps_passes: u64,
    fps_frames: u64,
    fps_last: Instant,
    last_frame: Instant,
    egui_ctx: egui::Context,
}

impl GuiAppImpl {
    fn new(
        cmd_tx: Sender<PlayerCmd>,
        evt_rx: Receiver<PlayerEvent>,
        ui_evt_tx: Sender<UiEvent>,
        ui_evt_rx: Receiver<UiEvent>,
        token: Option<String>,
        ua: Option<String>,
        egui_ctx: egui::Context,
    ) -> Self {
        let logged_in = token.as_ref().map(|t| !t.is_empty()).unwrap_or(false)
            && ua.as_ref().map(|u| !u.is_empty()).unwrap_or(false);
        let ua_val = ua.unwrap_or_else(|| DEFAULT_ANDROID_UA.to_string());
        let mut app = Self {
            cmd_tx,
            evt_rx,
            ui_evt_tx,
            ui_evt_rx,
            token: token.unwrap_or_default(),
            ua: ua_val.clone(),
            logged_in,
            login_input: String::new(),
            ua_input: ua_val,
            tracks: Vec::new(),
            tracks_count: None,
            loading_tracks: false,
            next_offset: 0,
            status: if logged_in { "Ready".to_string() } else { "Login required".to_string() },
            now_playing: None,
            volume: 1.0,
            seek_drag: None,
            pending_play_index: None,
            current_gen: 0,
            fps_enabled: std::env::var_os("VKMUSIC_FPS").as_deref() == Some(std::ffi::OsStr::new("1")),
            fps_passes: 0,
            fps_frames: 0,
            fps_last: Instant::now(),
            last_frame: Instant::now(),
            egui_ctx,
        };
        if app.logged_in {
            app.request_tracks_load();
        }
        app
    }

    fn request_tracks_load(&mut self) {
        if self.loading_tracks { return; }
        let need_more = match self.tracks_count {
            Some(c) => (self.next_offset as u64) < c,
            None => true,
        };
        if !need_more { return; }
        self.loading_tracks = true;
        self.status = "Loading...".to_string();
        let token = self.token.clone();
        let ua = self.ua.clone();
        let offset = self.next_offset;
        let tx = self.ui_evt_tx.clone();
        let ctx = self.egui_ctx.clone();
        std::thread::spawn(move || {
            let res = (|| -> anyhow::Result<(Option<u64>, Vec<Track>)> {
                let client = api::build_client(&ua)?;
                let offset_s = offset.to_string();
                let count_s = "100".to_string();
                let resp: AudioGetResponse = api::vk_call(
                    &client,
                    &token,
                    "audio.get",
                    &[("count", count_s.as_str()), ("offset", offset_s.as_str())],
                )?;
                let items = resp.items.unwrap_or_default();
                let tracks = items.iter().map(Track::from_value).collect::<Vec<_>>();
                Ok((resp.count, tracks))
            })();
            match res {
                Ok((count, items)) => { let _ = tx.send(UiEvent::TracksLoaded { offset, count, items }); }
                Err(e) => { let _ = tx.send(UiEvent::LoadError(api::sanitize_output(&format!("{e:#}")))); }
            }
            ctx.request_repaint();
        });
    }

    fn play_index(&mut self, index: usize) {
        if index >= self.tracks.len() { return; }
        self.pending_play_index = Some(index);
        let track = self.tracks[index].clone();
        let _ = self.cmd_tx.send(PlayerCmd::PlayTrack { track });
    }

    fn play_next(&mut self) {
        if let Some(idx) = self.now_playing.as_ref().and_then(|np| np.track_index) {
            let next = idx + 1;
            if next < self.tracks.len() {
                self.play_index(next);
            } else {
                self.now_playing = None;
                self.status = "Idle".to_string();
            }
        }
    }

    fn play_prev(&mut self) {
        if let Some(idx) = self.now_playing.as_ref().and_then(|np| np.track_index) {
            if idx > 0 {
                self.play_index(idx - 1);
            }
        }
    }

    fn handle_events(&mut self, ctx: &egui::Context) {
        while let Ok(evt) = self.ui_evt_rx.try_recv() {
            match evt {
                UiEvent::TracksLoaded { offset, count, items } => {
                    if offset == self.next_offset {
                        self.next_offset += items.len();
                        self.tracks.extend(items);
                        if let Some(c) = count { self.tracks_count = Some(c); }
                        self.loading_tracks = false;
                        self.status = format!("Tracks: {}", self.tracks.len());
                    }
                }
                UiEvent::LoadError(msg) => {
                    self.loading_tracks = false;
                    self.status = msg;
                }
            }
        }
        while let Ok(evt) = self.evt_rx.try_recv() {
            match evt {
                PlayerEvent::Downloading { artist, title, generation } => {
                    self.current_gen = generation;
                    self.status = format!("Downloading: {artist} - {title}");
                }
                PlayerEvent::Playing { artist, title, duration_secs, generation } => {
                    self.current_gen = generation;
                    self.status = String::new();
                    self.seek_drag = None;
                    self.now_playing = Some(NowPlaying {
                        artist, title, base_secs: 0.0, started_at: Some(Instant::now()), duration_secs, paused: false,
                        track_index: self.pending_play_index.take(),
                    });
                }
                PlayerEvent::Paused { artist, title, position_secs, duration_secs } => {
                    let track_index = self.now_playing.as_ref().and_then(|np| np.track_index).or(self.pending_play_index);
                    self.now_playing = Some(NowPlaying { artist, title, base_secs: position_secs as f64, started_at: None, duration_secs, paused: true, track_index });
                }
                PlayerEvent::Resumed { artist, title, position_secs, duration_secs } => {
                    let track_index = self.now_playing.as_ref().and_then(|np| np.track_index).or(self.pending_play_index);
                    self.now_playing = Some(NowPlaying { artist, title, base_secs: position_secs as f64, started_at: Some(Instant::now()), duration_secs, paused: false, track_index });
                }
                PlayerEvent::Stopped => {
                    self.now_playing = None;
                    self.pending_play_index = None;
                    self.current_gen = 0;
                    self.status = "Idle".to_string();
                }
                PlayerEvent::Ended { generation } => {
                    if generation == self.current_gen {
                        self.play_next();
                    }
                }
                PlayerEvent::Position { position_secs, duration_secs } => {
                    if let Some(np) = self.now_playing.as_mut() {
                        np.base_secs = position_secs as f64;
                        np.started_at = if np.paused { None } else { Some(Instant::now()) };
                        if duration_secs.is_some() { np.duration_secs = duration_secs; }
                    }
                }
                PlayerEvent::Volume(vol) => {
                    self.volume = vol;
                }
                PlayerEvent::Error(msg) => {
                    self.status = msg;
                    self.pending_play_index = None;
                }
            }
        }
        if self.now_playing.as_ref().map(|np| !np.paused).unwrap_or(false) {
            ctx.request_repaint_after(Duration::from_secs(1));
        }
    }

    fn login_ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Login");
        ui.label("Paste full browser URL or access_token:");
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.login_input).password(true).desired_width(ui.available_width() - 60.0));
            if ui.small_button("Paste").clicked() {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::RequestPaste);
            }
        });
        ui.label("User-Agent:");
        ui.add(egui::TextEdit::singleline(&mut self.ua_input).desired_width(f32::INFINITY));
        if ui.button("Login").clicked() {
            if let Some(token) = api::extract_access_token(&self.login_input) {
                let ua = self.ua_input.trim().to_string();
                if ua.is_empty() {
                    self.status = "VK API error: missing User-Agent".to_string();
                } else {
                    self.token = token;
                    self.ua = ua;
                    self.logged_in = true;
                    self.login_input.clear();
                    self.status = "Ready".to_string();
                    self.tracks.clear();
                    self.tracks_count = None;
                    self.next_offset = 0;
                    self.request_tracks_load();
                }
            } else {
                self.status = "VK API error: access_token not found".to_string();
            }
        }
    }
}

impl eframe::App for GuiAppImpl {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let pass_index = ctx.current_pass_index();
        if pass_index == 0 {
            let elapsed = self.last_frame.elapsed();
            if elapsed < Duration::from_millis(16) {
                std::thread::sleep(Duration::from_millis(16) - elapsed);
            }
            self.last_frame = Instant::now();
        }
        self.handle_events(&ctx);
        if self.fps_enabled {
            self.fps_passes += 1;
            if pass_index == 0 {
                self.fps_frames += 1;
            }
            let elapsed = self.fps_last.elapsed();
            if elapsed >= Duration::from_secs(10) {
                eprintln!("passes: {}, frames: {}", self.fps_passes, self.fps_frames);
                self.fps_passes = 0;
                self.fps_frames = 0;
                self.fps_last = Instant::now();
            }
        }

        egui::Panel::bottom("bottom").show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                let np_text = if let Some(np) = &self.now_playing {
                    let dur = np.duration_secs.map(api::format_mm_ss).unwrap_or_else(|| "--:--".to_string());
                    let pos = self.seek_drag.unwrap_or(np.position_secs());
                    format!("now playing: {} - {}  {} / {}", np.artist, np.title, api::format_mm_ss(pos), dur)
                } else {
                    "now playing: -".to_string()
                };
                ui.label(np_text);
                if ui.button("Prev").clicked() { self.play_prev(); }
                if ui.button(if self.now_playing.as_ref().map(|n| n.paused).unwrap_or(false) { "Resume" } else { "Pause" }).clicked() {
                    let _ = self.cmd_tx.send(PlayerCmd::PauseResume);
                }
                if ui.button("Next").clicked() { self.play_next(); }
                if ui.button("Stop").clicked() { let _ = self.cmd_tx.send(PlayerCmd::Stop); }
            });
            ui.horizontal(|ui| {
                ui.label("Volume");
                let vol_resp = ui.add(egui::Slider::new(&mut self.volume, 0.0..=1.5).show_value(true));
                if vol_resp.changed() || vol_resp.drag_stopped() {
                    let _ = self.cmd_tx.send(PlayerCmd::SetVolume(self.volume));
                }
            });
            ui.horizontal(|ui| {
                ui.label("Seek");
                let (pos, dur) = self.now_playing.as_ref()
                    .map(|np| {
                        let p = np.position_secs();
                        (self.seek_drag.unwrap_or(p), np.duration_secs.unwrap_or(p.max(1)))
                    })
                    .unwrap_or((0, 1));
                let mut seek_pos = pos.min(dur);
                let seek_resp = ui.add_enabled(self.now_playing.is_some(), egui::Slider::new(&mut seek_pos, 0..=dur.max(1)).text("s").clamping(egui::SliderClamping::Always));
                if seek_resp.changed() {
                    self.seek_drag = Some(seek_pos);
                }
                if seek_resp.drag_stopped() {
                    let target = self.seek_drag.unwrap_or(seek_pos);
                    self.seek_drag = None;
                    let _ = self.cmd_tx.send(PlayerCmd::Seek(target));
                }
            });
            ui.label(&self.status);
        });

        egui::CentralPanel::default().show_inside(ui, |ui| {
            if !self.logged_in {
                self.login_ui(ui);
                return;
            }
            ui.heading("My Music");
            let row_h = ui.text_style_height(&egui::TextStyle::Body) + 6.0;
            let total = self.tracks_count.unwrap_or(self.tracks.len() as u64) as usize;
            egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(ui, row_h, total, |ui, range| {
                for i in range.clone() {
                    if i >= self.tracks.len() {
                        if !self.loading_tracks {
                            self.request_tracks_load();
                        }
                        ui.label("Loading...");
                        continue;
                    }
                    let line = self.tracks[i].display_line();
                    let is_playing = self.now_playing.as_ref().and_then(|np| np.track_index) == Some(i);
                    let text = if is_playing { egui::RichText::new(line).strong() } else { egui::RichText::new(line) };
                    let resp = ui.add(egui::Label::new(text).selectable(false).sense(egui::Sense::click()));
                    if resp.double_clicked() {
                        self.play_index(i);
                    }
                }
                if range.end + 10 >= self.tracks.len() && !self.loading_tracks {
                    self.request_tracks_load();
                }
            });
        });
    }
}
