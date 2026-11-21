use crate::api::{self, AudioGetResponse, Track, DEFAULT_ANDROID_UA};
use crate::player::{PlayerCmd, PlayerEvent};
use crate::storage;
use eframe::egui;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState, hotkey::{Code, HotKey}};
use std::sync::mpsc::{Receiver, Sender};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    MyMusic,
    Playlists,
    Search,
    Recommendations,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum QueueSource {
    MyMusic,
    Playlist,
    Search,
    Recommendations,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
struct TrackId {
    owner_id: i64,
    id: i64,
}

impl TrackId {
    fn from_track(t: &Track) -> Self {
        Self { owner_id: t.owner_id, id: t.id }
    }
}

#[derive(Clone, Debug)]
enum AddState {
    Idle,
    Running,
    Done,
    Error(String),
}

#[derive(Clone, Debug)]
enum DeleteState {
    Idle,
    Confirm,
    Running,
}

#[derive(Clone, Debug)]
struct PlaylistView {
    owner_id: i64,
    id: i64,
    access_key: Option<String>,
    title: String,
    count: Option<u64>,
}

impl PlaylistView {
    fn from_api(p: api::Playlist) -> Self {
        Self { owner_id: p.owner_id, id: p.id, access_key: p.access_key, title: p.title, count: p.count }
    }
}

struct NowPlaying {
    artist: String,
    title: String,
    base_secs: f64,
    started_at: Option<Instant>,
    duration_secs: Option<u64>,
    paused: bool,
    track_id: Option<TrackId>,
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

#[derive(Clone, Copy, Debug)]
enum MediaHotkey {
    PlayPause,
    Stop,
    Next,
    Prev,
}

enum UiEvent {
    TracksLoaded { offset: usize, count: Option<u64>, items: Vec<Track> },
    PlaylistsLoaded { offset: usize, count: Option<u64>, items: Vec<PlaylistView> },
    PlaylistTracksLoaded { playlist_id: i64, owner_id: i64, offset: usize, count: Option<u64>, items: Vec<Track> },
    SearchLoaded { query: String, request_id: u64, offset: usize, count: Option<u64>, items: Vec<Track> },
    RecommendationsLoaded { request_id: u64, replace: bool, items: Vec<Track> },
    RecommendationsError { request_id: u64, msg: String },
    AddDone { track_id: TrackId },
    AddError { track_id: TrackId, msg: String },
    DeleteDone { track_id: TrackId },
    DeleteError { track_id: TrackId, msg: String },
    MediaKey(MediaHotkey),
    RestoredTrackResolved { track: Track },
    RestoredTrackUnavailable(String),
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
    my_user_id: Option<i64>,

    tracks: Vec<Track>,
    tracks_count: Option<u64>,
    loading_tracks: bool,
    next_offset: usize,
    my_status: String,

    playlists: Vec<PlaylistView>,
    playlists_count: Option<u64>,
    loading_playlists: bool,
    playlists_offset: usize,
    playlists_status: String,
    selected_playlist: Option<usize>,
    playlist_tracks: Vec<Track>,
    playlist_tracks_count: Option<u64>,
    loading_playlist_tracks: bool,
    playlist_tracks_offset: usize,
    playlist_status: String,

    search_query: String,
    search_input: String,
    search_tracks: Vec<Track>,
    search_count: Option<u64>,
    loading_search: bool,
    search_offset: usize,
    search_status: String,
    search_request_id: u64,

    rec_tracks: Vec<Track>,
    rec_loaded_once: bool,
    rec_loading: bool,
    rec_status: String,
    rec_no_more: bool,
    rec_request_id: u64,
    rec_waiting_next: bool,

    queue: Vec<Track>,
    queue_source: QueueSource,
    queue_playlist: Option<(i64, i64)>,
    queue_search_query: Option<String>,

    status: String,
    now_playing: Option<NowPlaying>,
    volume: f32,
    seek_drag: Option<u64>,
    pending_play_track_id: Option<TrackId>,
    selected_index: Option<usize>,
    active_tab: Tab,
    current_gen: u64,
    add_states: std::collections::HashMap<TrackId, AddState>,
    delete_states: std::collections::HashMap<TrackId, DeleteState>,
    fps_enabled: bool,
    fps_passes: u64,
    fps_frames: u64,
    fps_logic: u64,
    fps_last: Instant,
    last_frame: Instant,
    egui_ctx: egui::Context,
    using_saved_credentials: bool,
    credentials_saved: bool,
    _hotkey_manager: Option<GlobalHotKeyManager>,
    hotkey_status: String,
    restored_track: Option<Track>,
    restored_track_pending: bool,
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
            my_user_id: None,

            tracks: Vec::new(),
            tracks_count: None,
            loading_tracks: false,
            next_offset: 0,
            my_status: String::new(),

            playlists: Vec::new(),
            playlists_count: None,
            loading_playlists: false,
            playlists_offset: 0,
            playlists_status: String::new(),
            selected_playlist: None,
            playlist_tracks: Vec::new(),
            playlist_tracks_count: None,
            loading_playlist_tracks: false,
            playlist_tracks_offset: 0,
            playlist_status: String::new(),

            search_query: String::new(),
            search_input: String::new(),
            search_tracks: Vec::new(),
            search_count: None,
            loading_search: false,
            search_offset: 0,
            search_status: String::new(),
            search_request_id: 0,

            rec_tracks: Vec::new(),
            rec_loaded_once: false,
            rec_loading: false,
            rec_status: String::new(),
            rec_no_more: false,
            rec_request_id: 0,
            rec_waiting_next: false,

            queue: Vec::new(),
            queue_source: QueueSource::MyMusic,
            queue_playlist: None,
            queue_search_query: None,

            status: if logged_in { String::new() } else { "Login required".to_string() },
            now_playing: None,
            volume,
            seek_drag: None,
            pending_play_track_id: None,
            selected_index: None,
            active_tab: Tab::MyMusic,
            current_gen: 0,
            add_states: std::collections::HashMap::new(),
            delete_states: std::collections::HashMap::new(),
            fps_enabled: std::env::var_os("VKMUSIC_FPS").as_deref() == Some(std::ffi::OsStr::new("1")),
            fps_passes: 0,
            fps_frames: 0,
            fps_logic: 0,
            fps_last: Instant::now(),
            last_frame: Instant::now(),
            egui_ctx: egui_ctx.clone(),
            using_saved_credentials,
            credentials_saved: false,
            _hotkey_manager: None,
            hotkey_status: String::new(),
            restored_track: storage::load_last_track().map(|t| Track {
                artist: t.artist,
                title: t.title,
                duration_secs: t.duration,
                url: None,
                owner_id: t.owner_id,
                id: t.audio_id,
                access_key: t.access_key,
                audio_streams: None,
            }),
            restored_track_pending: false,
        };
        let _ = app.cmd_tx.send(PlayerCmd::SetVolume(app.volume));
        app.register_media_hotkeys();
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
        self.my_status = "загрузка...".to_string();
        let token = self.token.clone();
        let ua = self.ua.clone();
        let offset = self.next_offset;
        let tx = self.ui_evt_tx.clone();
        let ctx = self.egui_ctx.clone();
        std::thread::spawn(move || {
            let res = (|| -> anyhow::Result<(Option<u64>, Vec<Track>)> {
                let client = api::build_client(&ua)?;
                let offset_s = offset.to_string();
                let resp: AudioGetResponse = api::vk_call(
                    &client,
                    &token,
                    "audio.get",
                    &[("count", "100"), ("offset", offset_s.as_str())],
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

    fn request_playlists_load(&mut self) {
        if self.loading_playlists { return; }
        let need_more = match self.playlists_count {
            Some(c) => (self.playlists_offset as u64) < c,
            None => true,
        };
        if !need_more { return; }
        self.loading_playlists = true;
        self.playlists_status = "загрузка...".to_string();
        let token = self.token.clone();
        let ua = self.ua.clone();
        let offset = self.playlists_offset;
        let tx = self.ui_evt_tx.clone();
        let ctx = self.egui_ctx.clone();
        std::thread::spawn(move || {
            let res = (|| -> anyhow::Result<(Option<u64>, Vec<PlaylistView>)> {
                let client = api::build_client(&ua)?;
                let users: Vec<serde_json::Value> = api::vk_call(&client, &token, "users.get", &[])?;
                let user = users.first().ok_or_else(|| anyhow::anyhow!("users.get returned no user"))?;
                let user_id = user.get("id").and_then(serde_json::Value::as_i64).ok_or_else(|| anyhow::anyhow!("users.get missing id"))?;
                let owner_s = user_id.to_string();
                let offset_s = offset.to_string();
                let resp: api::PlaylistsGetResponse = api::vk_call(
                    &client,
                    &token,
                    "audio.getPlaylists",
                    &[("owner_id", owner_s.as_str()), ("count", "100"), ("offset", offset_s.as_str())],
                )?;
                let items = resp.items.unwrap_or_default();
                let playlists = items.iter().filter_map(api::Playlist::from_value).map(PlaylistView::from_api).collect::<Vec<_>>();
                Ok((resp.count, playlists))
            })();
            match res {
                Ok((count, items)) => { let _ = tx.send(UiEvent::PlaylistsLoaded { offset, count, items }); }
                Err(e) => { let _ = tx.send(UiEvent::LoadError(api::sanitize_output(&format!("{e:#}")))); }
            }
            ctx.request_repaint();
        });
    }

    fn open_playlist(&mut self, index: usize) {
        if index >= self.playlists.len() { return; }
        self.selected_playlist = Some(index);
        self.playlist_tracks.clear();
        self.playlist_tracks_count = None;
        self.loading_playlist_tracks = false;
        self.playlist_tracks_offset = 0;
        self.playlist_status.clear();
        self.request_playlist_tracks_load();
    }

    fn request_playlist_tracks_load(&mut self) {
        let Some(pidx) = self.selected_playlist else { return; };
        let Some(pl) = self.playlists.get(pidx).cloned() else { return; };
        if self.loading_playlist_tracks { return; }
        let need_more = match self.playlist_tracks_count {
            Some(c) => (self.playlist_tracks_offset as u64) < c,
            None => true,
        };
        if !need_more { return; }
        self.loading_playlist_tracks = true;
        self.playlist_status = "загрузка...".to_string();
        let token = self.token.clone();
        let ua = self.ua.clone();
        let offset = self.playlist_tracks_offset;
        let tx = self.ui_evt_tx.clone();
        let ctx = self.egui_ctx.clone();
        std::thread::spawn(move || {
            let res = (|| -> anyhow::Result<(Option<u64>, Vec<Track>)> {
                let client = api::build_client(&ua)?;
                let owner_s = pl.owner_id.to_string();
                let album_s = pl.id.to_string();
                let offset_s = offset.to_string();
                let mut params: Vec<(&str, &str)> = vec![
                    ("owner_id", owner_s.as_str()),
                    ("album_id", album_s.as_str()),
                    ("count", "100"),
                    ("offset", offset_s.as_str()),
                ];
                if let Some(ak) = pl.access_key.as_deref() {
                    params.push(("access_key", ak));
                }
                let resp: AudioGetResponse = api::vk_call(&client, &token, "audio.get", &params)?;
                let items = resp.items.unwrap_or_default();
                let tracks = items.iter().map(Track::from_value).collect::<Vec<_>>();
                Ok((resp.count, tracks))
            })();
            match res {
                Ok((count, items)) => { let _ = tx.send(UiEvent::PlaylistTracksLoaded { playlist_id: pl.id, owner_id: pl.owner_id, offset, count, items }); }
                Err(e) => { let _ = tx.send(UiEvent::LoadError(api::sanitize_output(&format!("{e:#}")))); }
            }
            ctx.request_repaint();
        });
    }

    fn start_search(&mut self) {
        let q = self.search_input.trim().to_string();
        self.search_query = q.clone();
        self.search_tracks.clear();
        self.search_count = None;
        self.search_offset = 0;
        self.loading_search = false;
        self.search_status.clear();
        self.search_request_id = self.search_request_id.wrapping_add(1);
        if q.is_empty() {
            self.search_status = "пустой запрос".to_string();
            return;
        }
        self.request_search_load();
    }

    fn request_search_load(&mut self) {
        if self.search_query.trim().is_empty() { return; }
        if self.loading_search { return; }
        let need_more = match self.search_count {
            Some(c) => (self.search_offset as u64) < c,
            None => true,
        };
        if !need_more { return; }
        self.loading_search = true;
        self.search_status = "загрузка...".to_string();
        let token = self.token.clone();
        let ua = self.ua.clone();
        let offset = self.search_offset;
        let query = self.search_query.clone();
        let request_id = self.search_request_id;
        let tx = self.ui_evt_tx.clone();
        let ctx = self.egui_ctx.clone();
        std::thread::spawn(move || {
            let res = (|| -> anyhow::Result<(Option<u64>, Vec<Track>)> {
                let client = api::build_client(&ua)?;
                let offset_s = offset.to_string();
                let resp: AudioGetResponse = api::vk_call(
                    &client,
                    &token,
                    "audio.search",
                    &[("q", query.as_str()), ("count", "50"), ("offset", offset_s.as_str())],
                )?;
                let items = resp.items.unwrap_or_default();
                let tracks = items.iter().map(Track::from_value).collect::<Vec<_>>();
                Ok((resp.count, tracks))
            })();
            match res {
                Ok((count, items)) => { let _ = tx.send(UiEvent::SearchLoaded { query, request_id, offset, count, items }); }
                Err(e) => { let _ = tx.send(UiEvent::LoadError(api::sanitize_output(&format!("{e:#}")))); }
            }
            ctx.request_repaint();
        });
    }

        fn request_recommendations(&mut self, replace: bool) {
        if self.rec_loading { return; }
        if !replace && self.rec_no_more { return; }
        self.rec_loading = true;
        self.rec_status = "загрузка...".to_string();
        self.rec_request_id = self.rec_request_id.wrapping_add(1);
        if replace {
            self.rec_no_more = false;
        }
        let request_id = self.rec_request_id;
        let token = self.token.clone();
        let ua = self.ua.clone();
        let tx = self.ui_evt_tx.clone();
        let ctx = self.egui_ctx.clone();
        std::thread::spawn(move || {
            let res = (|| -> anyhow::Result<Vec<Track>> {
                let client = api::build_client(&ua)?;
                let resp: AudioGetResponse = api::vk_call(&client, &token, "audio.getRecommendations", &[("count", "50")])?;
                let items = resp.items.unwrap_or_default();
                Ok(items.iter().map(Track::from_value).collect::<Vec<_>>())
            })();
            match res {
                Ok(items) => { let _ = tx.send(UiEvent::RecommendationsLoaded { request_id, replace, items }); }
                Err(e) => { let _ = tx.send(UiEvent::RecommendationsError { request_id, msg: api::sanitize_output(&format!("{e:#}")) }); }
            }
            ctx.request_repaint();
        });
    }

    fn request_recommendations_more(&mut self) {
        if self.rec_loading || self.rec_no_more { return; }
        if !self.rec_loaded_once { self.request_recommendations(false); return; }
        self.rec_loading = true;
        self.rec_status = "загрузка...".to_string();
        self.rec_request_id = self.rec_request_id.wrapping_add(1);
        let request_id = self.rec_request_id;
        let replace = false;
        let token = self.token.clone();
        let ua = self.ua.clone();
        let tx = self.ui_evt_tx.clone();
        let ctx = self.egui_ctx.clone();
        std::thread::spawn(move || {
            let res = (|| -> anyhow::Result<Vec<Track>> {
                let client = api::build_client(&ua)?;
                let resp: AudioGetResponse = api::vk_call(&client, &token, "audio.getRecommendations", &[("count", "50")])?;
                let items = resp.items.unwrap_or_default();
                Ok(items.iter().map(Track::from_value).collect::<Vec<_>>())
            })();
            match res {
                Ok(items) => { let _ = tx.send(UiEvent::RecommendationsLoaded { request_id, replace, items }); }
                Err(e) => { let _ = tx.send(UiEvent::RecommendationsError { request_id, msg: api::sanitize_output(&format!("{e:#}")) }); }
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
        self.my_user_id = None;

        self.tracks.clear(); self.tracks_count = None; self.loading_tracks = false; self.next_offset = 0; self.my_status.clear();
        self.playlists.clear(); self.playlists_count = None; self.loading_playlists = false; self.playlists_offset = 0; self.playlists_status.clear();
        self.selected_playlist = None; self.playlist_tracks.clear(); self.playlist_tracks_count = None; self.loading_playlist_tracks = false; self.playlist_tracks_offset = 0; self.playlist_status.clear();
        self.search_query.clear(); self.search_input.clear(); self.search_tracks.clear(); self.search_count = None; self.loading_search = false; self.search_offset = 0; self.search_status.clear(); self.search_request_id = 0;
        self.rec_tracks.clear(); self.rec_loaded_once = false; self.rec_loading = false; self.rec_status.clear(); self.rec_no_more = false; self.rec_request_id = 0; self.rec_waiting_next = false;

        self.queue.clear(); self.queue_source = QueueSource::MyMusic; self.queue_playlist = None; self.queue_search_query = None;
        self.add_states.clear();
        self.delete_states.clear();

        self.now_playing = None;
        self.restored_track = None;
        self.restored_track_pending = false;
        self.pending_play_track_id = None;
        self.selected_index = None;
        self.seek_drag = None;
        self.current_gen = 0;
        self.active_tab = Tab::MyMusic;
        let _ = self.cmd_tx.send(PlayerCmd::Stop);
        self.status = message.unwrap_or_else(|| "Login required".to_string());
    }
fn set_queue_for_my_music(&mut self) {
        self.queue = self.tracks.clone();
        self.queue_source = QueueSource::MyMusic;
        self.queue_playlist = None;
        self.queue_search_query = None;
    }

    fn set_queue_for_playlist(&mut self) {
        let Some(pidx) = self.selected_playlist else { return; };
        let Some(pl) = self.playlists.get(pidx) else { return; };
        self.queue = self.playlist_tracks.clone();
        self.queue_source = QueueSource::Playlist;
        self.queue_playlist = Some((pl.owner_id, pl.id));
        self.queue_search_query = None;
    }

    fn set_queue_for_search(&mut self) {
        self.queue = self.search_tracks.clone();
        self.queue_source = QueueSource::Search;
        self.queue_playlist = None;
        self.queue_search_query = Some(self.search_query.clone());
    }

    fn set_queue_for_recommendations(&mut self) {
        self.queue = self.rec_tracks.clone();
        self.queue_source = QueueSource::Recommendations;
        self.queue_playlist = None;
        self.queue_search_query = None;
    }

    fn play_from_list(&mut self, source: QueueSource, index: usize) {
        let tracks = match source {
            QueueSource::MyMusic => self.tracks.clone(),
            QueueSource::Playlist => self.playlist_tracks.clone(),
            QueueSource::Search => self.search_tracks.clone(),
            QueueSource::Recommendations => self.rec_tracks.clone(),
        };
        let Some(track) = tracks.get(index).cloned() else { return; };
        if !track.is_playable() { return; }
        match source {
            QueueSource::MyMusic => self.set_queue_for_my_music(),
            QueueSource::Playlist => self.set_queue_for_playlist(),
            QueueSource::Search => self.set_queue_for_search(),
            QueueSource::Recommendations => self.set_queue_for_recommendations(),
        }
        self.selected_index = Some(index);
        let tid = TrackId::from_track(&track);
        self.pending_play_track_id = Some(tid);
        let _ = self.cmd_tx.send(PlayerCmd::PlayTrack { track });
    }

    fn play_track_id(&mut self, track_id: TrackId) {
        let Some(pos) = self.queue.iter().position(|t| TrackId::from_track(t) == track_id) else { return; };
        let track = self.queue[pos].clone();
        if !track.is_playable() { return; }
        self.pending_play_track_id = Some(track_id);
        let _ = self.cmd_tx.send(PlayerCmd::PlayTrack { track });
    }

    fn find_next_playable_id(&self, from: TrackId, dir: i32) -> Option<TrackId> {
        if self.queue.is_empty() { return None; }
        let Some(pos) = self.queue.iter().position(|t| TrackId::from_track(t) == from) else { return None; };
        let mut i = pos as i32 + dir;
        while i >= 0 && (i as usize) < self.queue.len() {
            let idx = i as usize;
            if self.queue[idx].is_playable() {
                return Some(TrackId::from_track(&self.queue[idx]));
            }
            i += dir;
        }
        None
    }

    fn play_next(&mut self) {
        let Some(cur) = self.now_playing.as_ref().and_then(|np| np.track_id) else { return; };
        if let Some(next) = self.find_next_playable_id(cur, 1) {
            self.play_track_id(next);
            return;
        }
        if self.queue_source == QueueSource::Recommendations && !self.rec_no_more {
            self.rec_waiting_next = true;
            self.request_recommendations_more();
            return;
        }
        self.now_playing = None;
    }

    fn play_prev(&mut self) {
        let Some(cur) = self.now_playing.as_ref().and_then(|np| np.track_id) else { return; };
        if let Some(prev) = self.find_next_playable_id(cur, -1) {
            self.play_track_id(prev);
        }
    }

    fn selected_or_playing_for_current_list(&self) -> Option<(QueueSource, usize)> {
        match self.active_tab {
            Tab::MyMusic => {
                let idx = self.selected_index.or_else(|| {
                    if self.queue_source == QueueSource::MyMusic {
                        self.now_playing.as_ref().and_then(|np| np.track_id).and_then(|tid| self.tracks.iter().position(|t| TrackId::from_track(t) == tid))
                    } else { None }
                })?;
                Some((QueueSource::MyMusic, idx))
            }
            Tab::Playlists => {
                if self.selected_playlist.is_some() {
                    let idx = self.selected_index.or_else(|| {
                        let is_current = self.queue_source == QueueSource::Playlist && self.queue_playlist == self.selected_playlist.as_ref().map(|i| { let p=&self.playlists[*i]; (p.owner_id,p.id) });
                        if is_current { self.now_playing.as_ref().and_then(|np| np.track_id).and_then(|tid| self.playlist_tracks.iter().position(|t| TrackId::from_track(t) == tid)) } else { None }
                    })?;
                    Some((QueueSource::Playlist, idx))
                } else {
                    None
                }
            }
            Tab::Search => {
                let idx = self.selected_index.or_else(|| {
                    let is_current = self.queue_source == QueueSource::Search && self.queue_search_query.as_deref() == Some(self.search_query.as_str());
                    if is_current { self.now_playing.as_ref().and_then(|np| np.track_id).and_then(|tid| self.search_tracks.iter().position(|t| TrackId::from_track(t) == tid)) } else { None }
                })?;
                Some((QueueSource::Search, idx))
            }
            Tab::Recommendations => {
                let idx = self.selected_index.or_else(|| {
                    let is_current = self.queue_source == QueueSource::Recommendations;
                    if is_current { self.now_playing.as_ref().and_then(|np| np.track_id).and_then(|tid| self.rec_tracks.iter().position(|t| TrackId::from_track(t) == tid)) } else { None }
                })?;
                Some((QueueSource::Recommendations, idx))
            }
        }
    }

    fn register_media_hotkeys(&mut self) {
        let manager = match GlobalHotKeyManager::new() {
            Ok(m) => m,
            Err(e) => {
                self.hotkey_status = format!("media keys unavailable: {}", api::sanitize_output(&format!("{e:#}")));
                return;
            }
        };
        let hotkeys = [
            (MediaHotkey::PlayPause, HotKey::new(None, Code::MediaPlayPause)),
            (MediaHotkey::Stop, HotKey::new(None, Code::MediaStop)),
            (MediaHotkey::Next, HotKey::new(None, Code::MediaTrackNext)),
            (MediaHotkey::Prev, HotKey::new(None, Code::MediaTrackPrevious)),
        ];
        let mut failed: Vec<&str> = Vec::new();
        let mut map: std::collections::HashMap<u32, MediaHotkey> = std::collections::HashMap::new();
        for (kind, hk) in hotkeys {
            let id = hk.id();
            match manager.register(hk) {
                Ok(()) => { map.insert(id, kind); }
                Err(_) => {
                    failed.push(match kind {
                        MediaHotkey::PlayPause => "PlayPause",
                        MediaHotkey::Stop => "Stop",
                        MediaHotkey::Next => "Next",
                        MediaHotkey::Prev => "Prev",
                    });
                }
            }
        }
        if !failed.is_empty() {
            self.hotkey_status = format!("media keys busy: {}", failed.join(", "));
        }
        let tx = self.ui_evt_tx.clone();
        let ctx = self.egui_ctx.clone();
        let map = std::sync::Arc::new(map);
        let _ = GlobalHotKeyEvent::set_event_handler(Some(move |event: GlobalHotKeyEvent| {
            if event.state != HotKeyState::Pressed { return; }
            if let Some(kind) = map.get(&event.id).copied() {
                let _ = tx.send(UiEvent::MediaKey(kind));
                ctx.request_repaint();
            }
        }));
        self._hotkey_manager = Some(manager);
    }

    fn save_last_played_track(&self, track: &Track) {
        let st = storage::SavedTrack {
            owner_id: track.owner_id,
            audio_id: track.id,
            access_key: track.access_key.clone(),
            artist: track.artist.clone(),
            title: track.title.clone(),
            duration: track.duration_secs,
        };
        storage::save_last_track(&st);
    }

    fn resolve_restored_track(&mut self) {
        if self.restored_track_pending { return; }
        let Some(saved) = self.restored_track.clone() else { return; };
        if !self.logged_in || self.token.trim().is_empty() { return; }
        self.restored_track_pending = true;
        self.status = "загрузка...".to_string();
        let token = self.token.clone();
        let ua = self.ua.clone();
        let tx = self.ui_evt_tx.clone();
        let ctx = self.egui_ctx.clone();
        let audios = format!("{}_{}", saved.owner_id, saved.id);
        std::thread::spawn(move || {
            let res = (|| -> anyhow::Result<Track> {
                let client = api::build_client(&ua)?;
                let items: Vec<serde_json::Value> = api::vk_call(&client, &token, "audio.getById", &[("audios", audios.as_str())])?;
                let item = items.into_iter().next().ok_or_else(|| anyhow::anyhow!("track unavailable"))?;
                let track = Track::from_value(&item);
                if !track.is_playable() {
                    anyhow::bail!("track unavailable");
                }
                Ok(track)
            })();
            match res {
                Ok(track) => { let _ = tx.send(UiEvent::RestoredTrackResolved { track }); }
                Err(_) => { let _ = tx.send(UiEvent::RestoredTrackUnavailable("saved track unavailable".to_string())); }
            }
            ctx.request_repaint();
        });
    }

    fn handle_media_hotkey(&mut self, kind: MediaHotkey) {
        match kind {
            MediaHotkey::PlayPause => {
                if self.now_playing.is_some() {
                    let _ = self.cmd_tx.send(PlayerCmd::PauseResume);
                } else if self.restored_track.is_some() {
                    self.resolve_restored_track();
                }
            }
            MediaHotkey::Stop => { let _ = self.cmd_tx.send(PlayerCmd::Stop); }
            MediaHotkey::Next => { self.play_next(); }
            MediaHotkey::Prev => { self.play_prev(); }
        }
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
            if i.consume_key(egui::Modifiers::NONE, egui::Key::Enter) { play_selected = true; }
            if i.consume_key(egui::Modifiers::NONE, egui::Key::Space) { pause_resume = true; }
            if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowRight) { next = true; }
            if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowLeft) { prev = true; }
        });
        if play_selected {
            if let Some((src, idx)) = self.selected_or_playing_for_current_list() {
                self.play_from_list(src, idx);
            }
        }
        if pause_resume {
            if self.now_playing.is_some() {
                let _ = self.cmd_tx.send(PlayerCmd::PauseResume);
            } else if self.restored_track.is_some() {
                self.resolve_restored_track();
            }
        }
        if next { self.play_next(); }
        if prev { self.play_prev(); }
    }
    fn refresh_my_music_after_add(&mut self) {
        self.tracks.clear();
        self.tracks_count = None;
        self.next_offset = 0;
        self.loading_tracks = false;
        self.request_tracks_load();
    }

        fn append_recommendations(&mut self, request_id: u64, replace: bool, items: Vec<Track>) {
        if request_id != self.rec_request_id { return; }
        self.rec_loading = false;
        self.rec_loaded_once = true;
        if replace {
            let mut seen: std::collections::HashSet<TrackId> = std::collections::HashSet::new();
            let mut fresh: Vec<Track> = Vec::with_capacity(items.len());
            for t in items {
                let tid = TrackId::from_track(&t);
                if seen.insert(tid) { fresh.push(t); }
            }
            self.rec_tracks = fresh;
            self.rec_no_more = false;
            self.rec_status.clear();
            // keep playing track and queue intact for QueueSource::Recommendations
            return;
        }
        let mut added = 0usize;
        let mut existing: std::collections::HashSet<TrackId> = self.rec_tracks.iter().map(TrackId::from_track).collect();
        for t in items {
            let tid = TrackId::from_track(&t);
            if existing.insert(tid) {
                self.rec_tracks.push(t);
                added += 1;
            }
        }
        if added == 0 {
            self.rec_no_more = true;
            self.rec_status = "больше нет".to_string();
        } else {
            self.rec_status.clear();
        }
        if self.queue_source == QueueSource::Recommendations {
            self.queue = self.rec_tracks.clone();
        }
    }
fn remove_track_locally(&mut self, track_id: TrackId) {
        self.tracks.retain(|t| TrackId::from_track(t) != track_id);
        if let Some(c) = self.tracks_count.as_mut() {
            *c = c.saturating_sub(1);
        }
        if self.queue_source == QueueSource::MyMusic {
            self.queue.retain(|t| TrackId::from_track(t) != track_id);
        }
        if let Some(sel) = self.selected_index {
            if sel >= self.tracks.len() {
                self.selected_index = if self.tracks.is_empty() { None } else { Some(self.tracks.len() - 1) };
            }
        }
    }

    fn track_is_mine(&self, track: &Track) -> bool {
        self.my_user_id.map(|id| track.owner_id == id).unwrap_or(false)
    }

    fn ensure_my_user_id(&mut self) {
        if self.my_user_id.is_some() || !self.logged_in { return; }
        let token = self.token.clone();
        let ua = self.ua.clone();
        let tx = self.ui_evt_tx.clone();
        let ctx = self.egui_ctx.clone();
        std::thread::spawn(move || {
            let res = (|| -> anyhow::Result<i64> {
                let client = api::build_client(&ua)?;
                let users: Vec<serde_json::Value> = api::vk_call(&client, &token, "users.get", &[])?;
                let user = users.first().ok_or_else(|| anyhow::anyhow!("users.get returned no user"))?;
                user.get("id").and_then(serde_json::Value::as_i64).ok_or_else(|| anyhow::anyhow!("users.get missing id"))
            })();
            if let Err(e) = res {
                let _ = tx.send(UiEvent::LoadError(api::sanitize_output(&format!("{e:#}"))));
            } else if let Ok(id) = res {
                // send via LoadError? no; use a dedicated event - simpler: piggyback by setting in ui thread via event not available.
                // We'll handle by storing in status-less event; easiest is to add UiEvent::MyUserId, but to keep small, reuse LoadError impossible.
                let _ = tx.send(UiEvent::LoadError(format!("__MY_USER_ID__:{id}")));
            }
            ctx.request_repaint();
        });
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
                        self.my_status.clear();
                        if !self.credentials_saved {
                            storage::save_credentials(&self.token, &self.ua);
                            self.credentials_saved = true;
                            self.using_saved_credentials = false;
                        }
                        if self.queue_source == QueueSource::MyMusic {
                            self.queue = self.tracks.clone();
                        }
                    }
                }
                UiEvent::PlaylistsLoaded { offset, count, items } => {
                    if offset == self.playlists_offset {
                        self.playlists_offset += items.len();
                        self.playlists.extend(items);
                        if let Some(c) = count { self.playlists_count = Some(c); }
                        self.loading_playlists = false;
                        self.playlists_status.clear();
                    }
                }
                UiEvent::PlaylistTracksLoaded { playlist_id, owner_id, offset, count, items } => {
                    let current = self.selected_playlist.and_then(|i| self.playlists.get(i)).map(|p| (p.owner_id, p.id));
                    if current == Some((owner_id, playlist_id)) && offset == self.playlist_tracks_offset {
                        self.playlist_tracks_offset += items.len();
                        self.playlist_tracks.extend(items);
                        if let Some(c) = count { self.playlist_tracks_count = Some(c); }
                        self.loading_playlist_tracks = false;
                        self.playlist_status.clear();
                        if self.queue_source == QueueSource::Playlist && self.queue_playlist == Some((owner_id, playlist_id)) {
                            self.queue = self.playlist_tracks.clone();
                        }
                    }
                }
                UiEvent::SearchLoaded { query, request_id, offset, count, items } => {
                    if request_id == self.search_request_id && query == self.search_query && offset == self.search_offset {
                        self.search_offset += items.len();
                        self.search_tracks.extend(items);
                        if let Some(c) = count { self.search_count = Some(c); }
                        self.loading_search = false;
                        self.search_status.clear();
                        if self.queue_source == QueueSource::Search && self.queue_search_query.as_deref() == Some(self.search_query.as_str()) {
                            self.queue = self.search_tracks.clone();
                        }
                    }
                }
                                UiEvent::RecommendationsLoaded { request_id, replace, items } => {
                    let was_waiting_next = self.rec_waiting_next;
                    let cur = self.now_playing.as_ref().and_then(|np| np.track_id);
                    self.append_recommendations(request_id, replace, items);
                    if was_waiting_next {
                        self.rec_waiting_next = false;
                        if self.queue_source == QueueSource::Recommendations {
                            if let Some(cur_id) = cur {
                                if let Some(next) = self.find_next_playable_id(cur_id, 1) {
                                    self.play_track_id(next);
                                } else if self.rec_no_more {
                                    self.now_playing = None;
                                }
                            } else if self.rec_no_more {
                                self.now_playing = None;
                            }
                        }
                    }
                }UiEvent::RecommendationsError { request_id, msg } => {
                    if request_id != self.rec_request_id { continue; }
                    self.rec_loading = false;
                    self.rec_waiting_next = false;
                    if self.using_saved_credentials && api::is_auth_error(&anyhow::anyhow!("{}", msg)) {
                        self.logout_to_login(Some("token expired, log in again".to_string()));
                    } else {
                        self.rec_status = msg;
                    }
                }
                                UiEvent::MediaKey(kind) => {
                    self.handle_media_hotkey(kind);
                }
                UiEvent::RestoredTrackResolved { track } => {
                    self.restored_track_pending = false;
                    self.status.clear();
                    self.restored_track = Some(track.clone());
                    self.set_queue_for_my_music();
                    self.pending_play_track_id = Some(TrackId::from_track(&track));
                    let _ = self.cmd_tx.send(PlayerCmd::PlayTrack { track });
                }
                UiEvent::RestoredTrackUnavailable(msg) => {
                    self.restored_track_pending = false;
                    self.restored_track = None;
                    self.status = msg;
                }                UiEvent::AddDone { track_id } => {
                    self.add_states.insert(track_id, AddState::Done);
                    self.refresh_my_music_after_add();
                }
                UiEvent::AddError { track_id, msg } => {
                    if self.using_saved_credentials && api::is_auth_error(&anyhow::anyhow!("{}", msg)) {
                        self.logout_to_login(Some("token expired, log in again".to_string()));
                    } else {
                        self.add_states.insert(track_id, AddState::Error(msg.clone()));
                        self.status = msg;
                    }
                }
                UiEvent::DeleteDone { track_id } => {
                    self.delete_states.remove(&track_id);
                    self.remove_track_locally(track_id);
                    self.add_states.remove(&track_id);
                }
                UiEvent::DeleteError { track_id, msg } => {
                    self.delete_states.insert(track_id, DeleteState::Idle);
                    if self.using_saved_credentials && api::is_auth_error(&anyhow::anyhow!("{}", msg)) {
                        self.logout_to_login(Some("token expired, log in again".to_string()));
                    } else {
                        self.status = msg;
                    }
                }
                UiEvent::LoadError(msg) => {
                    if let Some(id_s) = msg.strip_prefix("__MY_USER_ID__:") {
                        if let Ok(id) = id_s.parse::<i64>() {
                            self.my_user_id = Some(id);
                        }
                        continue;
                    }
                    self.loading_tracks = false;
                    self.loading_playlists = false;
                    self.loading_playlist_tracks = false;
                    self.loading_search = false;
                    if self.using_saved_credentials && api::is_auth_error(&anyhow::anyhow!("{}", msg)) {
                        self.logout_to_login(Some("token expired, log in again".to_string()));
                    } else {
                        self.status = msg.clone();
                        if self.active_tab == Tab::MyMusic { self.my_status = msg.clone(); }
                        if self.active_tab == Tab::Playlists {
                            if self.selected_playlist.is_some() { self.playlist_status = msg.clone(); } else { self.playlists_status = msg.clone(); }
                        }
                        if self.active_tab == Tab::Search { self.search_status = msg; }
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
                    let track_id = self.pending_play_track_id.take();
                    if let Some(tid) = track_id {
                        let t = self.queue.iter().find(|t| TrackId::from_track(t) == tid).cloned()
                            .or_else(|| self.tracks.iter().find(|t| TrackId::from_track(t) == tid).cloned())
                            .or_else(|| self.restored_track.clone().filter(|t| TrackId::from_track(t) == tid));
                        let save_t = t.unwrap_or_else(|| Track { artist: artist.clone(), title: title.clone(), duration_secs, url: None, owner_id: tid.owner_id, id: tid.id, access_key: None, audio_streams: None });
                        self.restored_track = Some(save_t.clone());
                        self.save_last_played_track(&save_t);
                    }
                    self.now_playing = Some(NowPlaying {
                        artist, title, base_secs: 0.0, started_at: Some(Instant::now()), duration_secs, paused: false,
                        track_id,
                    });
                }                PlayerEvent::Paused { artist, title, position_secs, duration_secs } => {
                    let track_id = self.now_playing.as_ref().and_then(|np| np.track_id).or(self.pending_play_track_id);
                    self.now_playing = Some(NowPlaying { artist, title, base_secs: position_secs as f64, started_at: None, duration_secs, paused: true, track_id });
                }
                PlayerEvent::Resumed { artist, title, position_secs, duration_secs } => {
                    let track_id = self.now_playing.as_ref().and_then(|np| np.track_id).or(self.pending_play_track_id);
                    self.now_playing = Some(NowPlaying { artist, title, base_secs: position_secs as f64, started_at: Some(Instant::now()), duration_secs, paused: false, track_id });
                }
                PlayerEvent::Stopped => {
                    self.now_playing = None;
                    self.pending_play_track_id = None;
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
                PlayerEvent::Volume(vol) => { self.volume = vol.clamp(0.0, 1.0); }
                PlayerEvent::Error(msg) => {
                    self.pending_play_track_id = None;
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
                    self.status.clear();
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

    fn render_track_row(&mut self, ui: &mut egui::Ui, i: usize, track: &Track, width: f32, row_h: f32, source: QueueSource, playing_in_this_list: bool) {
        let is_selected = self.selected_index == Some(i);
        let is_playing = playing_in_this_list;
        let playable = track.is_playable();
        let dur = track.duration_secs.map(api::format_mm_ss).unwrap_or_else(|| "--:--".to_string());
        let idx_text = format!("{:>3}", i + 1);
        let title_text = format!("{} - {}", track.artist, track.title);
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
        } else if resp.hovered() && playable {
            ui.painter().rect_filled(rect, 0.0, ui.visuals().widgets.hovered.bg_fill);
        }
        let inner = rect.shrink2(egui::vec2(8.0, 0.0));
        let mut row_ui = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(egui::Layout::left_to_right(egui::Align::Center)));
        row_ui.set_min_height(row_h);
        let muted = ui.visuals().weak_text_color();
        let normal = ui.visuals().text_color();
        let color = if playable { normal } else { muted };
        row_ui.add(egui::Label::new(egui::RichText::new(idx_text).monospace().color(color)).selectable(false));
        row_ui.add_space(8.0);

        let tid = TrackId::from_track(track);
        let action_clicked = self.render_track_action(&mut row_ui, track, source, &tid, &resp);

        row_ui.add_space(6.0);
        let mut title_rt = egui::RichText::new(title_text).color(color);
        if is_playing { title_rt = title_rt.strong(); }
        row_ui.add(egui::Label::new(title_rt).selectable(false).truncate());
        row_ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add(egui::Label::new(egui::RichText::new(dur).color(color)).selectable(false));
        });
        if !action_clicked && resp.clicked() {
            self.selected_index = Some(i);
        }
        if !action_clicked && resp.double_clicked() && playable {
            self.play_from_list(source, i);
        }
    }

    fn render_track_action(&mut self, ui: &mut egui::Ui, track: &Track, source: QueueSource, tid: &TrackId, row_resp: &egui::Response) -> bool {
        match source {
            QueueSource::MyMusic => self.render_delete_action(ui, track, tid, row_resp),
            QueueSource::Playlist | QueueSource::Search | QueueSource::Recommendations => self.render_add_action(ui, track, tid),
        }
    }

    fn render_add_action(&mut self, ui: &mut egui::Ui, track: &Track, tid: &TrackId) -> bool {
        if self.track_is_mine(track) { return false; }
        let state = self.add_states.get(tid).cloned().unwrap_or(AddState::Idle);
        let (label, enabled) = match &state {
            AddState::Idle => ("+".to_string(), true),
            AddState::Running => ("...".to_string(), false),
            AddState::Done => ("ok".to_string(), false),
            AddState::Error(_) => ("+".to_string(), true),
        };
        let resp = ui.add_enabled(enabled, egui::Button::new(label).small());
        if let AddState::Error(msg) = &state {
            resp.clone().on_hover_text(msg.clone());
        }
        if resp.clicked() {
            self.start_add_track(track.clone());
            return true;
        }
        false
    }

    fn render_delete_action(&mut self, ui: &mut egui::Ui, track: &Track, tid: &TrackId, row_resp: &egui::Response) -> bool {
        let state = self.delete_states.get(tid).cloned().unwrap_or(DeleteState::Idle);
        match state {
            DeleteState::Idle => {
                let resp = ui.small_button("x");
                if resp.clicked() {
                    self.delete_states.insert(*tid, DeleteState::Confirm);
                    return true;
                }
                false
            }
            DeleteState::Running => {
                ui.add_enabled(false, egui::Button::new("...").small());
                false
            }
            DeleteState::Confirm => {
                let mut clicked_any = false;
                let yes = ui.small_button("Да");
                let no = ui.small_button("Нет");
                ui.label("Удалить?");
                if yes.clicked() {
                    clicked_any = true;
                    self.start_delete_track(track.clone());
                }
                if no.clicked() {
                    clicked_any = true;
                    self.delete_states.insert(*tid, DeleteState::Idle);
                }
                if !clicked_any && row_resp.clicked() && !yes.hovered() && !no.hovered() {
                    self.delete_states.insert(*tid, DeleteState::Idle);
                }
                clicked_any || yes.clicked() || no.clicked()
            }
        }
    }

    fn start_add_track(&mut self, track: Track) {
        let tid = TrackId::from_track(&track);
        self.add_states.insert(tid, AddState::Running);
        let token = self.token.clone();
        let ua = self.ua.clone();
        let tx = self.ui_evt_tx.clone();
        let ctx = self.egui_ctx.clone();
        std::thread::spawn(move || {
            let res = (|| -> anyhow::Result<()> {
                let client = api::build_client(&ua)?;
                let owner_s = track.owner_id.to_string();
                let audio_s = track.id.to_string();
                let mut params: Vec<(&str, &str)> = vec![("owner_id", owner_s.as_str()), ("audio_id", audio_s.as_str())];
                let ak = track.access_key.clone();
                if let Some(ak) = ak.as_deref() {
                    params.push(("access_key", ak));
                }
                let result: serde_json::Value = api::vk_call(&client, &token, "audio.add", &params)?;
                // VK may return audio_id or 0/response; treat any non-error as success
                let _ = result;
                Ok(())
            })();
            match res {
                Ok(()) => { let _ = tx.send(UiEvent::AddDone { track_id: tid }); }
                Err(e) => {
                    let msg = api::sanitize_output(&format!("{e:#}"));
                    if msg.contains("already") || msg.contains("Audio already added") || msg.contains("already added") {
                        let _ = tx.send(UiEvent::AddDone { track_id: tid });
                    } else {
                        let _ = tx.send(UiEvent::AddError { track_id: tid, msg });
                    }
                }
            }
            ctx.request_repaint();
        });
    }

    fn start_delete_track(&mut self, track: Track) {
        let tid = TrackId::from_track(&track);
        self.delete_states.insert(tid, DeleteState::Running);
        let token = self.token.clone();
        let ua = self.ua.clone();
        let tx = self.ui_evt_tx.clone();
        let ctx = self.egui_ctx.clone();
        std::thread::spawn(move || {
            let res = (|| -> anyhow::Result<()> {
                let client = api::build_client(&ua)?;
                let owner_s = track.owner_id.to_string();
                let audio_s = track.id.to_string();
                let params: Vec<(&str, &str)> = vec![("owner_id", owner_s.as_str()), ("audio_id", audio_s.as_str())];
                let _: serde_json::Value = api::vk_call(&client, &token, "audio.delete", &params)?;
                Ok(())
            })();
            match res {
                Ok(()) => { let _ = tx.send(UiEvent::DeleteDone { track_id: tid }); }
                Err(e) => { let _ = tx.send(UiEvent::DeleteError { track_id: tid, msg: api::sanitize_output(&format!("{e:#}")) }); }
            }
            ctx.request_repaint();
        });
    }

    fn current_playing_matches(&self, source: QueueSource) -> bool {
        match source {
            QueueSource::MyMusic => self.queue_source == QueueSource::MyMusic,
            QueueSource::Playlist => {
                self.queue_source == QueueSource::Playlist
                    && self.queue_playlist == self.selected_playlist.as_ref().map(|i| { let p=&self.playlists[*i]; (p.owner_id,p.id) })
            }
            QueueSource::Search => {
                self.queue_source == QueueSource::Search
                    && self.queue_search_query.as_deref() == Some(self.search_query.as_str())
            }
            QueueSource::Recommendations => self.queue_source == QueueSource::Recommendations,
        }
    }
}

impl eframe::App for GuiAppImpl {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.fps_enabled {
            self.fps_logic += 1;
        }
        self.handle_events(ctx);
    }

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
        self.handle_keyboard(&ctx);
        if self.fps_enabled {
            self.fps_passes += 1;
            if pass_index == 0 { self.fps_frames += 1; }
            let elapsed = self.fps_last.elapsed();
            if elapsed >= Duration::from_secs(10) {
                let logic_only = self.fps_logic.saturating_sub(self.fps_frames);
                eprintln!("passes: {}, frames: {}, logic: {}", self.fps_passes, self.fps_frames, logic_only);
                self.fps_passes = 0; self.fps_frames = 0; self.fps_logic = 0; self.fps_last = Instant::now();
            }
        }

        egui::Panel::bottom("bottom").show(ui, |ui| {
                        let now_text = if let Some(np) = &self.now_playing {
                format!("{} - {}", np.artist, np.title)
            } else if let Some(t) = &self.restored_track {
                format!("{} - {} (готов)", t.artist, t.title)
            } else {
                "Ничего не играет".to_string()
            };
            ui.add(egui::Label::new(now_text).selectable(false));
            ui.horizontal(|ui| {
                let pos = self.now_playing.as_ref().map(|np| np.position_secs()).unwrap_or(0);
                let dur = self.now_playing.as_ref().and_then(|np| np.duration_secs).unwrap_or(pos.max(1));
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
                if seek_resp.changed() { self.seek_drag = Some(seek_pos); }
                if seek_resp.drag_stopped() {
                    let target = self.seek_drag.unwrap_or(seek_pos);
                    self.seek_drag = None;
                    let _ = self.cmd_tx.send(PlayerCmd::Seek(target));
                }
                ui.add(egui::Label::new(api::format_mm_ss(dur)).selectable(false));
            });

            ui.horizontal(|ui| {
                if ui.button("Prev").clicked() { self.play_prev(); }
                let pause_label = if self.now_playing.as_ref().map(|n| n.paused).unwrap_or(false) { "Resume" } else { "Pause" };
                if ui.button(pause_label).clicked() {
                    if self.now_playing.is_some() {
                        let _ = self.cmd_tx.send(PlayerCmd::PauseResume);
                    } else if self.restored_track.is_some() {
                        self.resolve_restored_track();
                    }
                }
                if ui.button("Next").clicked() { self.play_next(); }
                if ui.button("Stop").clicked() { let _ = self.cmd_tx.send(PlayerCmd::Stop); }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let vol_resp = ui.add(egui::Slider::new(&mut self.volume, 0.0..=1.0).show_value(false));
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

            if !self.hotkey_status.is_empty() {
                ui.add(egui::Label::new(&self.hotkey_status).selectable(false));
            }
            if !self.status.is_empty() {
                ui.add(egui::Label::new(&self.status).selectable(false));
            }
        });

        egui::CentralPanel::default().show(ui, |ui| {
            if !self.logged_in {
                self.login_ui(ui);
                return;
            }

            self.ensure_my_user_id();

            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.active_tab, Tab::MyMusic, "Моя музыка");
                ui.selectable_value(&mut self.active_tab, Tab::Playlists, "Плейлисты");
                ui.selectable_value(&mut self.active_tab, Tab::Search, "Поиск");
                ui.selectable_value(&mut self.active_tab, Tab::Recommendations, "Рекомендации");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("Log out").clicked() {
                        self.logout_to_login(None);
                    }
                });
            });
            ui.separator();

            let row_h = (ui.text_style_height(&egui::TextStyle::Body) + 14.0).max(28.0);
            match self.active_tab {
                Tab::MyMusic => {
                    if !self.my_status.is_empty() { ui.add(egui::Label::new(&self.my_status).selectable(false)); }
                    let total = self.tracks_count.unwrap_or(self.tracks.len() as u64) as usize;
                    let playing_tid = if self.current_playing_matches(QueueSource::MyMusic) { self.now_playing.as_ref().and_then(|np| np.track_id) } else { None };
                    egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(ui, row_h, total, |ui, range| {
                        let width = ui.available_width();
                        for i in range.clone() {
                            if i >= self.tracks.len() {
                                if !self.loading_tracks { self.request_tracks_load(); }
                                ui.allocate_ui_with_layout(egui::vec2(width, row_h), egui::Layout::left_to_right(egui::Align::Center), |ui| { ui.add(egui::Label::new("загрузка...").selectable(false)); });
                                continue;
                            }
                            let track = self.tracks[i].clone();
                            let playing_here = playing_tid == Some(TrackId::from_track(&track));
                            self.render_track_row(ui, i, &track, width, row_h, QueueSource::MyMusic, playing_here);
                        }
                        if range.end + 10 >= self.tracks.len() && !self.loading_tracks { self.request_tracks_load(); }
                    });
                }
                Tab::Playlists => {
                    if let Some(pidx) = self.selected_playlist {
                        let pl_title = self.playlists.get(pidx).map(|p| p.title.clone()).unwrap_or_default();
                        ui.horizontal(|ui| {
                            if ui.button("Назад").clicked() {
                                self.selected_playlist = None;
                                self.selected_index = None;
                                return;
                            }
                            ui.add(egui::Label::new(egui::RichText::new(pl_title).strong()).selectable(false));
                        });
                        if self.selected_playlist.is_some() {
                            if !self.playlist_status.is_empty() { ui.add(egui::Label::new(&self.playlist_status).selectable(false)); }
                            let total = self.playlist_tracks_count.unwrap_or(self.playlist_tracks.len() as u64) as usize;
                            let playing_tid = if self.current_playing_matches(QueueSource::Playlist) { self.now_playing.as_ref().and_then(|np| np.track_id) } else { None };
                            egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(ui, row_h, total, |ui, range| {
                                let width = ui.available_width();
                                for i in range.clone() {
                                    if i >= self.playlist_tracks.len() {
                                        if !self.loading_playlist_tracks { self.request_playlist_tracks_load(); }
                                        ui.allocate_ui_with_layout(egui::vec2(width, row_h), egui::Layout::left_to_right(egui::Align::Center), |ui| { ui.add(egui::Label::new("загрузка...").selectable(false)); });
                                        continue;
                                    }
                                    let track = self.playlist_tracks[i].clone();
                            let playing_here = playing_tid == Some(TrackId::from_track(&track));
                                    self.render_track_row(ui, i, &track, width, row_h, QueueSource::Playlist, playing_here);
                                }
                                if range.end + 10 >= self.playlist_tracks.len() && !self.loading_playlist_tracks { self.request_playlist_tracks_load(); }
                            });
                        }
                    } else {
                        if self.playlists.is_empty() && !self.loading_playlists && self.playlists_count.is_none() {
                            self.request_playlists_load();
                        }
                        if !self.playlists_status.is_empty() { ui.add(egui::Label::new(&self.playlists_status).selectable(false)); }
                        let total = self.playlists_count.unwrap_or(self.playlists.len() as u64) as usize;
                        egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(ui, row_h, total, |ui, range| {
                            let width = ui.available_width();
                            for i in range.clone() {
                                if i >= self.playlists.len() {
                                    if !self.loading_playlists { self.request_playlists_load(); }
                                    ui.allocate_ui_with_layout(egui::vec2(width, row_h), egui::Layout::left_to_right(egui::Align::Center), |ui| { ui.add(egui::Label::new("загрузка...").selectable(false)); });
                                    continue;
                                }
                                let pl = self.playlists[i].clone();
                                let count_txt = pl.count.map(|c| c.to_string()).unwrap_or_else(|| "?".to_string());
                                let line = format!("{} ({})", pl.title, count_txt);
                                let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, row_h), egui::Sense::click());
                                if resp.hovered() { ui.painter().rect_filled(rect, 0.0, ui.visuals().widgets.hovered.bg_fill); }
                                let inner = rect.shrink2(egui::vec2(8.0, 0.0));
                                let mut row_ui = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(egui::Layout::left_to_right(egui::Align::Center)));
                                row_ui.add(egui::Label::new(line).selectable(false).truncate());
                                if resp.clicked() { self.open_playlist(i); }
                            }
                            if range.end + 10 >= self.playlists.len() && !self.loading_playlists { self.request_playlists_load(); }
                        });
                    }
                }
                Tab::Search => {
                    ui.horizontal(|ui| {
                        let resp = ui.add(egui::TextEdit::singleline(&mut self.search_input).desired_width(ui.available_width() - 80.0));
                        let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                        if ui.button("Найти").clicked() || enter {
                            self.start_search();
                        }
                    });
                    if !self.search_status.is_empty() { ui.add(egui::Label::new(&self.search_status).selectable(false)); }
                    let total = self.search_count.unwrap_or(self.search_tracks.len() as u64) as usize;
                    let playing_tid = if self.current_playing_matches(QueueSource::Search) { self.now_playing.as_ref().and_then(|np| np.track_id) } else { None };
                    egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(ui, row_h, total, |ui, range| {
                        let width = ui.available_width();
                        for i in range.clone() {
                            if i >= self.search_tracks.len() {
                                if !self.loading_search && !self.search_query.trim().is_empty() { self.request_search_load(); }
                                ui.allocate_ui_with_layout(egui::vec2(width, row_h), egui::Layout::left_to_right(egui::Align::Center), |ui| { ui.add(egui::Label::new("загрузка...").selectable(false)); });
                                continue;
                            }
                            let track = self.search_tracks[i].clone();
                            let playing_here = playing_tid == Some(TrackId::from_track(&track));
                            self.render_track_row(ui, i, &track, width, row_h, QueueSource::Search, playing_here);
                        }
                        if range.end + 10 >= self.search_tracks.len() && !self.loading_search && !self.search_query.trim().is_empty() { self.request_search_load(); }
                    });
                }
                Tab::Recommendations => {
                    if !self.rec_loaded_once && !self.rec_loading {
                        self.request_recommendations(false);
                    }
                    ui.horizontal(|ui| {
                        if ui.button("Обновить").clicked() && !self.rec_loading {
                            self.request_recommendations(true);
                        }
                        if self.rec_loading {
                            ui.add(egui::Label::new("загрузка...").selectable(false));
                        }
                    });
                    if !self.rec_status.is_empty() {
                        ui.add(egui::Label::new(&self.rec_status).selectable(false));
                    }
                    let playing_tid = if self.current_playing_matches(QueueSource::Recommendations) { self.now_playing.as_ref().and_then(|np| np.track_id) } else { None };
                    let show_footer = self.rec_no_more || self.rec_loading || !self.rec_loaded_once;
                    let total = self.rec_tracks.len() + if show_footer { 1 } else { 0 };
                    egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(ui, row_h, total.max(1), |ui, range| {
                        let width = ui.available_width();
                        for i in range.clone() {
                            if i >= self.rec_tracks.len() {
                                let label = if self.rec_no_more { "больше нет" } else { "загрузка..." };
                                if !self.rec_no_more && !self.rec_loading && self.rec_loaded_once {
                                    self.request_recommendations_more();
                                }
                                ui.allocate_ui_with_layout(egui::vec2(width, row_h), egui::Layout::left_to_right(egui::Align::Center), |ui| { ui.add(egui::Label::new(label).selectable(false)); });
                                continue;
                            }
                            let track = self.rec_tracks[i].clone();
                            let playing_here = playing_tid == Some(TrackId::from_track(&track));
                            self.render_track_row(ui, i, &track, width, row_h, QueueSource::Recommendations, playing_here);
                        }
                        if self.rec_loaded_once && !self.rec_no_more && !self.rec_loading && range.end + 5 >= self.rec_tracks.len() {
                            self.request_recommendations_more();
                        }
                    });
                }
            }
        });
    }
}