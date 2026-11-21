use crate::api::{self, AudioGetResponse, Track, DEFAULT_ANDROID_UA};
use crate::player::{PlayerCmd, PlayerEvent};
use crate::storage;
use eframe::egui;
use std::sync::mpsc::{Receiver, Sender};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    MyMusic,
    Playlists,
    Search,
}

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

pub fn run_gui(env_token: Option<String>, env_ua: Option<String>) -> eframe::Result<()> {
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel::<PlayerCmd>();
    let (evt_tx, evt_rx) = std::sync::mpsc::channel::<PlayerEvent>();
    let (ui_evt_tx, ui_evt_rx) = std::sync::mpsc::channel::<UiEvent>();

    let mut options = eframe::NativeOptions::default();
    options.glow_options.vsync = true;
    options.viewport = options
        .viewport
        .with_title("vkmusic")
        .with_inner_size([520.0, 640.0])
        .with_min_inner_size([400.0, 400.0]);
    eframe::run_native(
        "vkmusic",
        options,
        Box::new(move |cc| {
            let ctx = cc.egui_ctx.clone();
            let (token, ua, from_saved) = resolve_startup_credentials(env_token, env_ua);
            let client_ua = ua.clone().unwrap_or_else(|| DEFAULT_ANDROID_UA.to_string());
            let client = api::build_client(&client_ua).unwrap_or_else(|_| reqwest::blocking::Client::new());
            crate::player::spawn_player_worker(client, cmd_rx, evt_tx, ctx.clone());
            Ok(Box::new(GuiAppImpl::new(
                cmd_tx, evt_rx, ui_evt_tx, ui_evt_rx, token, ua, from_saved, ctx,
            )))
        }),
    )
}

fn resolve_startup_credentials(
    env_token: Option<String>,
    env_ua: Option<String>,
) -> (Option<String>, Option<String>, bool) {
    let env_token = env_token.filter(|v| !v.trim().is_empty());
    let env_ua = env_ua.filter(|v| !v.trim().is_empty());
    if env_token.is_some() && env_ua.is_some() {
        return (env_token, env_ua, false);
    }
    let (saved_token, saved_ua) = storage::load_saved_credentials();
    if saved_token.is_some() && saved_ua.is_some() {
        return (saved_token, saved_ua, true);
    }
    (None, None, false)
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
    selected_index: Option<usize>,
    active_tab: Tab,
    current_gen: u64,
    fps_enabled: bool,
    fps_passes: u64,
    fps_frames: u64,
    fps_last: Instant,
    last_frame: Instant,
    egui_ctx: egui::Context,
    using_saved_credentials: bool,
    credentials_saved: bool,
}

impl GuiAppImpl {
    fn new(
        cmd_tx: Sender<PlayerCmd>,
        evt_rx: Receiver<PlayerEvent>,
        ui_evt_tx: Sender<UiEvent>,
        ui_evt_rx: Receiver<UiEvent>,
        token: Option<String>,
        ua: Option<String>,
        using_saved_credentials: bool,
        egui_ctx: egui::Context,
    ) -> Self {
        let logged_in = token.as_ref().map(|t| !t.is_empty()).unwrap_or(false)
            && ua.as_ref().map(|u| !u.is_empty()).unwrap_or(false);
        let ua_val = ua.unwrap_or_else(|| DEFAULT_ANDROID_UA.to_string());
        let volume = storage::load_volume().clamp(0.0, 1.0);
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
            volume,
            seek_drag: None,
            pending_play_index: None,
            selected_index: None,
            active_tab: Tab::MyMusic,
            current_gen: 0,
            fps_enabled: std::env::var_os("VKMUSIC_FPS").as_deref() == Some(std::ffi::OsStr::new("1")),
            fps_passes: 0,
            fps_frames: 0,
            fps_last: Instant::now(),
            last_frame: Instant::now(),
            egui_ctx,
            using_saved_credentials,
            credentials_saved: false,
        };
        let _ = app.cmd_tx.send(PlayerCmd::SetVolume(app.volume));
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

    fn logout_to_login(&mut self, message: Option<String>) {
        storage::delete_credentials();
        self.token.clear();
        self.login_input.clear();
        self.logged_in = false;
        self.using_saved_credentials = false;
        self.credentials_saved = false;
        self.tracks.clear();
        self.tracks_count = None;
        self.loading_tracks = false;
        self.next_offset = 0;
        self.now_playing = None;
        self.pending_play_index = None;
        self.selected_index = None;
        self.seek_drag = None;
        self.current_gen = 0;
        self.active_tab = Tab::MyMusic;
        let _ = self.cmd_tx.send(PlayerCmd::Stop);
        self.status = message.unwrap_or_else(|| "Login required".to_string());
    }

    fn play_index(&mut self, index: usize) {
        if index >= self.tracks.len() { return; }
        self.selected_index = Some(index);
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
            }
        }
    }

    fn selected_or_playing(&self) -> Option<usize> {
        self.selected_index.or_else(|| self.now_playing.as_ref().and_then(|np| np.track_index))
    }

    fn handle_keyboard(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        let mut play_selected = false;
        let mut pause_resume = false;
        let mut next = false;
        let mut prev = false;
        ctx.input_mut(|i| {
            if i.consume_key(egui::Modifiers::NONE, egui::Key::Enter) {
                play_selected = true;
            }
            if i.consume_key(egui::Modifiers::NONE, egui::Key::Space) {
                pause_resume = true;
            }
            if i.consume_key(egui::Modifiers::CTRL, egui::Key::ArrowRight) {
                next = true;
            }
            if i.consume_key(egui::Modifiers::CTRL, egui::Key::ArrowLeft) {
                prev = true;
            }
        });
        if play_selected {
            if let Some(idx) = self.selected_or_playing() {
                self.play_index(idx);
            }
        }
        if pause_resume {
            let _ = self.cmd_tx.send(PlayerCmd::PauseResume);
        }
        if next {
            self.play_next();
        }
        if prev {
            self.play_prev();
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
                        self.status.clear();
                        if !self.credentials_saved {
                            storage::save_credentials(&self.token, &self.ua);
                            self.credentials_saved = true;
                            self.using_saved_credentials = false;
                        }
                    }
                }
                UiEvent::LoadError(msg) => {
                    self.loading_tracks = false;
                    if self.using_saved_credentials && api::is_auth_error(&anyhow::anyhow!("{}", msg)) {
                        self.logout_to_login(Some("token expired, log in again".to_string()));
                    } else {
                        self.status = msg;
                    }
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
                    self.volume = vol.clamp(0.0, 1.0);
                }
                PlayerEvent::Error(msg) => {
                    self.pending_play_index = None;
                    if self.using_saved_credentials && api::is_auth_error(&anyhow::anyhow!("{}", msg)) {
                        self.logout_to_login(Some("token expired, log in again".to_string()));
                    } else {
                        self.status = msg;
                    }
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
                    self.using_saved_credentials = false;
                    self.credentials_saved = false;
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
        self.handle_keyboard(&ctx);
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

        egui::Panel::bottom("bottom").show(ui, |ui| {
            let now_text = if let Some(np) = &self.now_playing {
                format!("{} - {}", np.artist, np.title)
            } else {
                "ничего не играет".to_string()
            };
            ui.add(egui::Label::new(now_text).selectable(false));

            ui.horizontal(|ui| {
                let pos = self.now_playing.as_ref().map(|np| np.position_secs()).unwrap_or(0);
                let dur = self
                    .now_playing
                    .as_ref()
                    .and_then(|np| np.duration_secs)
                    .unwrap_or(pos.max(1));
                let shown_pos = self.seek_drag.unwrap_or(pos).min(dur.max(1));
                ui.add(egui::Label::new(api::format_mm_ss(shown_pos)).selectable(false));
                let mut seek_pos = shown_pos;
                let dur_w = 44.0;
                let slider_w = (ui.available_width() - dur_w - ui.spacing().item_spacing.x).max(40.0);
                let old_slider_w = ui.spacing().slider_width;
                ui.spacing_mut().slider_width = slider_w;
                let seek_resp = ui.add_enabled(
                    self.now_playing.is_some(),
                    egui::Slider::new(&mut seek_pos, 0..=dur.max(1))
                        .show_value(false)
                        .clamping(egui::SliderClamping::Always),
                );
                ui.spacing_mut().slider_width = old_slider_w;
                if seek_resp.changed() {
                    self.seek_drag = Some(seek_pos);
                }
                if seek_resp.drag_stopped() {
                    let target = self.seek_drag.unwrap_or(seek_pos);
                    self.seek_drag = None;
                    let _ = self.cmd_tx.send(PlayerCmd::Seek(target));
                }
                ui.add(egui::Label::new(api::format_mm_ss(dur)).selectable(false));
            });

            ui.horizontal(|ui| {
                if ui.button("Prev").clicked() {
                    self.play_prev();
                }
                let pause_label = if self.now_playing.as_ref().map(|n| n.paused).unwrap_or(false) {
                    "Resume"
                } else {
                    "Pause"
                };
                if ui.button(pause_label).clicked() {
                    let _ = self.cmd_tx.send(PlayerCmd::PauseResume);
                }
                if ui.button("Next").clicked() {
                    self.play_next();
                }
                if ui.button("Stop").clicked() {
                    let _ = self.cmd_tx.send(PlayerCmd::Stop);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let vol_resp = ui.add(
                        egui::Slider::new(&mut self.volume, 0.0..=1.0)
                            .show_value(false),
                    );
                    if vol_resp.changed() || vol_resp.drag_stopped() {
                        self.volume = self.volume.clamp(0.0, 1.0);
                        let _ = self.cmd_tx.send(PlayerCmd::SetVolume(self.volume));
                        storage::save_volume(self.volume);
                    }
                    let pct = (self.volume * 100.0).round().clamp(0.0, 100.0) as i32;
                    ui.add(egui::Label::new(format!("{pct}%")).selectable(false));
                    ui.add(egui::Label::new("Громкость").selectable(false));
                });
            });

            if !self.status.is_empty() {
                ui.add(egui::Label::new(&self.status).selectable(false));
            }
        });

        egui::CentralPanel::default().show(ui, |ui| {
            if !self.logged_in {
                self.login_ui(ui);
                return;
            }

            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.active_tab, Tab::MyMusic, "Моя музыка");
                ui.selectable_value(&mut self.active_tab, Tab::Playlists, "Плейлисты");
                ui.selectable_value(&mut self.active_tab, Tab::Search, "Поиск");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("Log out").clicked() {
                        self.logout_to_login(None);
                    }
                });
            });
            ui.separator();

            match self.active_tab {
                Tab::MyMusic => {
                    let row_h = (ui.text_style_height(&egui::TextStyle::Body) + 14.0).max(28.0);
                    let total = self.tracks_count.unwrap_or(self.tracks.len() as u64) as usize;
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show_rows(ui, row_h, total, |ui, range| {
                            let width = ui.available_width();
                            for i in range.clone() {
                                if i >= self.tracks.len() {
                                    if !self.loading_tracks {
                                        self.request_tracks_load();
                                    }
                                    ui.allocate_ui_with_layout(
                                        egui::vec2(width, row_h),
                                        egui::Layout::left_to_right(egui::Align::Center),
                                        |ui| {
                                            ui.add(egui::Label::new("Loading...").selectable(false));
                                        },
                                    );
                                    continue;
                                }

                                let is_selected = self.selected_index == Some(i);
                                let is_playing = self.now_playing.as_ref().and_then(|np| np.track_index) == Some(i);
                                let dur = self.tracks[i]
                                    .duration_secs
                                    .map(api::format_mm_ss)
                                    .unwrap_or_else(|| "--:--".to_string());
                                let idx_text = format!("{:>3}", i + 1);
                                let title_text = format!("{} - {}", self.tracks[i].artist, self.tracks[i].title);
                                let fill = if is_playing {
                                    Some(ui.visuals().selection.bg_fill)
                                } else if is_selected {
                                    Some(ui.visuals().widgets.active.bg_fill.gamma_multiply(0.35))
                                } else {
                                    None
                                };

                                let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, row_h), egui::Sense::click());
                                if let Some(fill) = fill {
                                    ui.painter().rect_filled(rect, 0.0, fill);
                                } else if resp.hovered() {
                                    ui.painter().rect_filled(rect, 0.0, ui.visuals().widgets.hovered.bg_fill);
                                }

                                let inner = rect.shrink2(egui::vec2(8.0, 0.0));
                                let mut row_ui = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(egui::Layout::left_to_right(egui::Align::Center)));
                                row_ui.set_min_height(row_h);
                                row_ui.add(egui::Label::new(egui::RichText::new(idx_text).monospace()).selectable(false));
                                row_ui.add_space(8.0);
                                let title = if is_playing {
                                    egui::RichText::new(title_text).strong()
                                } else {
                                    egui::RichText::new(title_text)
                                };
                                row_ui.add(egui::Label::new(title).selectable(false).truncate());
                                row_ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    ui.add(egui::Label::new(dur).selectable(false));
                                });

                                if resp.clicked() {
                                    self.selected_index = Some(i);
                                }
                                if resp.double_clicked() {
                                    self.play_index(i);
                                }
                            }
                            if range.end + 10 >= self.tracks.len() && !self.loading_tracks {
                                self.request_tracks_load();
                            }
                        });
                }
                Tab::Playlists | Tab::Search => {
                    ui.centered_and_justified(|ui| {
                        ui.add(egui::Label::new("скоро").selectable(false));
                    });
                }
            }
        });
    }
}
