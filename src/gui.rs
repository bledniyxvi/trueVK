use crate::api::{self, AudioGetResponse, Track, DEFAULT_ANDROID_UA};
use crate::player::{PlayerCmd, PlayerEvent};
use crate::storage;
use crate::theme;
use eframe::egui;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState, hotkey::{Code, HotKey}};
use tray_icon::{Icon as TrayIconImage, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent, menu::{Menu, MenuEvent, MenuItem}};
use std::collections::HashSet;
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const FILTER_AUTOLOAD_ERROR_TEXT: &str = "\u{0410}\u{0432}\u{0442}\u{043E}\u{0437}\u{0430}\u{0433}\u{0440}\u{0443}\u{0437}\u{043A}\u{0430}: \u{043E}\u{0448}\u{0438}\u{0431}\u{043A}\u{0430}";
const PLAYER_CHANNEL_CLOSED_TEXT: &str = "\u{043F}\u{043B}\u{0435}\u{0435}\u{0440} \u{043D}\u{0435}\u{0434}\u{043E}\u{0441}\u{0442}\u{0443}\u{043F}\u{0435}\u{043D}";
const SHUFFLE_LABEL: &str = "\u{041F}\u{0435}\u{0440}\u{0435}\u{043C}\u{0435}\u{0448}\u{0430}\u{0442}\u{044C}";
const REC_NO_SUITABLE_TRACKS_TEXT: &str = "\u{0411}\u{043E}\u{043B}\u{044C}\u{0448}\u{0435} \u{043D}\u{0435}\u{0442} \u{043F}\u{043E}\u{0434}\u{0445}\u{043E}\u{0434}\u{044F}\u{0449}\u{0438}\u{0445} \u{0442}\u{0440}\u{0435}\u{043A}\u{043E}\u{0432}";
const TRACK_ROW_HEIGHT: f32 = 44.0;
const ADD_TRACK_TOOLTIP: &str = "\u{0414}\u{043E}\u{0431}\u{0430}\u{0432}\u{0438}\u{0442}\u{044C} \u{0432} \u{043C}\u{043E}\u{044E} \u{043C}\u{0443}\u{0437}\u{044B}\u{043A}\u{0443}";
const REMOVE_TRACK_TOOLTIP: &str = "\u{0423}\u{0434}\u{0430}\u{043B}\u{0438}\u{0442}\u{044C}";
const NOTHING_PLAYING_TEXT: &str = "\u{041D}\u{0438}\u{0447}\u{0435}\u{0433}\u{043E} \u{043D}\u{0435} \u{0438}\u{0433}\u{0440}\u{0430}\u{0435}\u{0442}";
const PREV_TOOLTIP: &str = "\u{041F}\u{0440}\u{0435}\u{0434}\u{044B}\u{0434}\u{0443}\u{0449}\u{0438}\u{0439}";
const PLAY_TOOLTIP: &str = "\u{0418}\u{0433}\u{0440}\u{0430}\u{0442}\u{044C}";
const PAUSE_TOOLTIP: &str = "\u{041F}\u{0430}\u{0443}\u{0437}\u{0430}";
const NEXT_TOOLTIP: &str = "\u{0421}\u{043B}\u{0435}\u{0434}\u{0443}\u{044E}\u{0449}\u{0438}\u{0439}";
const STOP_TOOLTIP: &str = "\u{0421}\u{0442}\u{043E}\u{043F}";
const REPEAT_LABEL: &str = "\u{041F}\u{043E}\u{0432}\u{0442}\u{043E}\u{0440}";
const REPEAT_ALL_LABEL: &str = "\u{041F}\u{043E}\u{0432}\u{0442}\u{043E}\u{0440}: \u{0432}\u{0441}\u{0435}";
const REPEAT_ONE_LABEL: &str = "\u{041F}\u{043E}\u{0432}\u{0442}\u{043E}\u{0440}: 1";
const REPEAT_TOOLTIP: &str = "\u{0420}\u{0435}\u{0436}\u{0438}\u{043C} \u{043F}\u{043E}\u{0432}\u{0442}\u{043E}\u{0440}\u{0430}";
const SHUFFLE_TOOLTIP: &str = "\u{041F}\u{0435}\u{0440}\u{0435}\u{043C}\u{0435}\u{0448}\u{0438}\u{0442}\u{044C} \u{0432}\u{043E}\u{0441}\u{043F}\u{0440}\u{043E}\u{0438}\u{0437}\u{0432}\u{0435}\u{0434}\u{0435}\u{043D}\u{0438}\u{0435}";
const MUTE_TOOLTIP: &str = "\u{0412}\u{044B}\u{043A}\u{043B}\u{044E}\u{0447}\u{0438}\u{0442}\u{044C} \u{0437}\u{0432}\u{0443}\u{043A}";
const UNMUTE_TOOLTIP: &str = "\u{0412}\u{043A}\u{043B}\u{044E}\u{0447}\u{0438}\u{0442}\u{044C} \u{0437}\u{0432}\u{0443}\u{043A}";
const MENU_TOOLTIP: &str = "\u{041C}\u{0435}\u{043D}\u{044E}";
const LOGOUT_LABEL: &str = "\u{0412}\u{044B}\u{0439}\u{0442}\u{0438}";
const FILTER_HINT: &str = "\u{041F}\u{043E}\u{0438}\u{0441}\u{043A} \u{043F}\u{043E} \u{043C}\u{043E}\u{0435}\u{0439} \u{043C}\u{0443}\u{0437}\u{044B}\u{043A}\u{0438}";
const TRACKS_SUFFIX: &str = " \u{0442}\u{0440}\u{0435}\u{043A}\u{043E}\u{0432}";
const LOADING_MORE_TEXT: &str = "\u{0437}\u{0430}\u{0433}\u{0440}\u{0443}\u{0436}\u{0430}\u{044E} \u{0435}\u{0449}\u{0451}\u{2026}";
const EMPTY_MY_MUSIC_MAIN: &str = "\u{043F}\u{0443}\u{0441}\u{0442}\u{043E}. \u{043A}\u{0430}\u{043A} \u{0432}\u{043D}\u{0443}\u{0442}\u{0440}\u{0438}.";
const EMPTY_MY_MUSIC_SUB: &str = "\u{0434}\u{043E}\u{0431}\u{0430}\u{0432}\u{044C} \u{0447}\u{0442}\u{043E}-\u{043D}\u{0438}\u{0431}\u{0443}\u{0434}\u{044C} \u{0447}\u{0435}\u{0440}\u{0435}\u{0437} \u{043F}\u{043E}\u{0438}\u{0441}\u{043A}";
const EMPTY_FILTER_MAIN: &str = "\u{043D}\u{0438}\u{0447}\u{0435}\u{0433}\u{043E} \u{043D}\u{0435} \u{043D}\u{0430}\u{0448}\u{043B}\u{043E}\u{0441}\u{044C}";
const EMPTY_FILTER_SUB: &str = "\u{0442}\u{0430}\u{043A}\u{043E}\u{0433}\u{043E} \u{0443} \u{0442}\u{0435}\u{0431}\u{044F} \u{043D}\u{0435}\u{0442}";
const EMPTY_SEARCH_IDLE_MAIN: &str = "\u{0447}\u{0442}\u{043E} \u{0438}\u{0449}\u{0435}\u{043C}?";
const EMPTY_SEARCH_IDLE_SUB: &str = "\u{043D}\u{0430}\u{0437}\u{0432}\u{0430}\u{043D}\u{0438}\u{0435} \u{0438}\u{043B}\u{0438} \u{0430}\u{0440}\u{0442}\u{0438}\u{0441}\u{0442}";
const EMPTY_SEARCH_NONE_MAIN: &str = "\u{043D}\u{0438}\u{0447}\u{0435}\u{0433}\u{043E}. \u{0434}\u{0430}\u{0436}\u{0435} \u{044D}\u{0445}\u{043E} \u{043C}\u{043E}\u{043B}\u{0447}\u{0438}\u{0442}.";
const EMPTY_SEARCH_NONE_SUB: &str = "\u{043F}\u{043E}\u{043F}\u{0440}\u{043E}\u{0431}\u{0443}\u{0439} \u{043D}\u{0430}\u{043F}\u{0438}\u{0441}\u{0430}\u{0442}\u{044C} \u{0438}\u{043D}\u{0430}\u{0447}\u{0435}";
const EMPTY_PLAYLISTS_MAIN: &str = "\u{043F}\u{043B}\u{0435}\u{0439}\u{043B}\u{0438}\u{0441}\u{0442}\u{043E}\u{0432} \u{043D}\u{0435}\u{0442}";
const EMPTY_PLAYLISTS_SUB: &str = "\u{0441}\u{0442}\u{0440}\u{0430}\u{043D}\u{043D}\u{043E}, \u{043D}\u{043E} \u{043B}\u{0430}\u{0434}\u{043D}\u{043E}";
const EMPTY_REC_NO_MORE_MAIN: &str = "\u{0431}\u{043E}\u{043B}\u{044C}\u{0448}\u{0435} \u{043D}\u{0435}\u{0442}. \u{0432}\u{043E}\u{043E}\u{0431}\u{0449}\u{0435}.";
const EMPTY_REC_NO_MORE_SUB: &str = "\u{043D}\u{0430}\u{0436}\u{043C}\u{0438} \u{00AB}\u{041E}\u{0431}\u{043D}\u{043E}\u{0432}\u{0438}\u{0442}\u{044C}\u{00BB}, \u{0432}\u{0434}\u{0440}\u{0443}\u{0433} \u{043F}\u{043E}\u{0432}\u{0435}\u{0437}\u{0451}\u{0442}";
const TOP_BAR_H: f32 = 44.0;
const TOP_TAB_GAP: f32 = 22.0;
const TOP_MENU_SIZE: f32 = 28.0;
const PANEL_MARGIN_X: f32 = 12.0;
const PANEL_BOTTOM_MARGIN: f32 = 10.0;
const PANEL_PROGRESS_HIT_H: f32 = 14.0;
const PANEL_ZONES_H: f32 = 92.0;
const PANEL_CENTER_W: f32 = 240.0;
const PANEL_RIGHT_W: f32 = 220.0;
const PANEL_ZONE_GAP: f32 = 3.0;
const LOGIN_OPEN_LABEL: &str = "\u{41e}\u{442}\u{43a}\u{440}\u{44b}\u{442}\u{44c}\u{20}\u{56}\u{4b}";
const LOGIN_COPY_LABEL: &str = "\u{421}\u{43a}\u{43e}\u{43f}\u{438}\u{440}\u{43e}\u{432}\u{430}\u{442}\u{44c}\u{20}\u{441}\u{441}\u{44b}\u{43b}\u{43a}\u{443}";
const LOGIN_PASTE_LABEL: &str = "\u{412}\u{441}\u{442}\u{430}\u{432}\u{438}\u{442}\u{44c}";
const LOGIN_SUBMIT_LABEL: &str = "\u{412}\u{43e}\u{439}\u{442}\u{438}";
const LOGIN_LINK_HINT: &str = "\u{412}\u{441}\u{442}\u{430}\u{432}\u{44c}\u{20}\u{441}\u{441}\u{44b}\u{43b}\u{43a}\u{443}\u{20}\u{441}\u{44e}\u{434}\u{430}";
const LOGIN_REQUIRED_TEXT: &str = "\u{41d}\u{443}\u{436}\u{435}\u{43d}\u{20}\u{432}\u{445}\u{43e}\u{434}";
const LOGIN_STEP_CIRCLE: f32 = 24.0;
const LOGIN_STEP_GAP: f32 = 12.0;
const LOGIN_STEP_ROW_GAP: f32 = 8.0;
const LOGIN_PASTE_BUTTON_W: f32 = 96.0;
const LOGIN_CARD_ESTIMATE_H: f32 = 520.0;
const LOGIN_HEADING: &str = "\u{41f}\u{440}\u{438}\u{432}\u{435}\u{442}\u{2e}\u{20}\u{414}\u{430}\u{432}\u{430}\u{439}\u{20}\u{43f}\u{43e}\u{434}\u{43a}\u{43b}\u{44e}\u{447}\u{438}\u{43c}\u{20}\u{442}\u{432}\u{43e}\u{439}\u{20}\u{56}\u{4b}\u{2e}";
const LOGIN_SUBHEADING: &str = "\u{42d}\u{442}\u{43e}\u{20}\u{434}\u{435}\u{43b}\u{430}\u{435}\u{442}\u{441}\u{44f}\u{20}\u{43e}\u{434}\u{438}\u{43d}\u{20}\u{440}\u{430}\u{437}\u{2e}";
const LOGIN_STEP_ONE: &str = "\u{41d}\u{430}\u{436}\u{43c}\u{438}\u{20}\u{ab}\u{41e}\u{442}\u{43a}\u{440}\u{44b}\u{442}\u{44c}\u{20}\u{56}\u{4b}\u{bb}\u{20}\u{438}\u{20}\u{432}\u{43e}\u{439}\u{434}\u{438}\u{20}\u{432}\u{20}\u{430}\u{43a}\u{43a}\u{430}\u{443}\u{43d}\u{442}\u{2e}";
const LOGIN_STEP_TWO: &str = "\u{420}\u{430}\u{437}\u{440}\u{435}\u{448}\u{438}\u{20}\u{434}\u{43e}\u{441}\u{442}\u{443}\u{43f}\u{2e}\u{20}\u{41e}\u{442}\u{43a}\u{440}\u{43e}\u{435}\u{442}\u{441}\u{44f}\u{20}\u{43f}\u{443}\u{441}\u{442}\u{430}\u{44f}\u{20}\u{441}\u{442}\u{440}\u{430}\u{43d}\u{438}\u{446}\u{430}\u{2c}\u{20}\u{44d}\u{442}\u{43e}\u{20}\u{43d}\u{43e}\u{440}\u{43c}\u{430}\u{43b}\u{44c}\u{43d}\u{43e}\u{2e}";
const LOGIN_STEP_THREE: &str = "\u{421}\u{43a}\u{43e}\u{43f}\u{438}\u{440}\u{443}\u{439}\u{20}\u{430}\u{434}\u{440}\u{435}\u{441}\u{20}\u{44d}\u{442}\u{43e}\u{439}\u{20}\u{441}\u{442}\u{440}\u{430}\u{43d}\u{438}\u{446}\u{44b}\u{20}\u{446}\u{435}\u{43b}\u{438}\u{43a}\u{43e}\u{43c}\u{20}\u{28}\u{43}\u{74}\u{72}\u{6c}\u{2b}\u{4c}\u{2c}\u{20}\u{43f}\u{43e}\u{442}\u{43e}\u{43c}\u{20}\u{43}\u{74}\u{72}\u{6c}\u{2b}\u{43}\u{29}\u{20}\u{438}\u{20}\u{432}\u{441}\u{442}\u{430}\u{432}\u{44c}\u{20}\u{435}\u{433}\u{43e}\u{20}\u{441}\u{44e}\u{434}\u{430}\u{2e}";
const LOGIN_PRIVACY: &str = "\u{42d}\u{442}\u{430}\u{20}\u{441}\u{441}\u{44b}\u{43b}\u{43a}\u{430}\u{20}\u{440}\u{430}\u{431}\u{43e}\u{442}\u{430}\u{435}\u{442}\u{20}\u{43a}\u{430}\u{43a}\u{20}\u{43f}\u{430}\u{440}\u{43e}\u{43b}\u{44c}\u{2e}\u{20}\u{41d}\u{438}\u{43a}\u{43e}\u{43c}\u{443}\u{20}\u{435}\u{451}\u{20}\u{43d}\u{435}\u{20}\u{43e}\u{442}\u{43f}\u{440}\u{430}\u{432}\u{43b}\u{44f}\u{439}\u{2e}\u{20}\u{41f}\u{43b}\u{435}\u{435}\u{440}\u{20}\u{445}\u{440}\u{430}\u{43d}\u{438}\u{442}\u{20}\u{435}\u{451}\u{20}\u{442}\u{43e}\u{43b}\u{44c}\u{43a}\u{43e}\u{20}\u{443}\u{20}\u{442}\u{435}\u{431}\u{44f}\u{20}\u{43d}\u{430}\u{20}\u{43a}\u{43e}\u{43c}\u{43f}\u{44c}\u{44e}\u{442}\u{435}\u{440}\u{435}\u{2c}\u{20}\u{432}\u{20}\u{445}\u{440}\u{430}\u{43d}\u{438}\u{43b}\u{438}\u{449}\u{435}\u{20}\u{43f}\u{430}\u{440}\u{43e}\u{43b}\u{435}\u{439}\u{20}\u{57}\u{69}\u{6e}\u{64}\u{6f}\u{77}\u{73}\u{2e}\u{20}\u{412}\u{44b}\u{439}\u{442}\u{438}\u{20}\u{43c}\u{43e}\u{436}\u{43d}\u{43e}\u{20}\u{432}\u{20}\u{43b}\u{44e}\u{431}\u{43e}\u{439}\u{20}\u{43c}\u{43e}\u{43c}\u{435}\u{43d}\u{442}\u{2e}";
const LOGIN_DISCLAIMER: &str = "\u{421}\u{43f}\u{43e}\u{441}\u{43e}\u{431}\u{20}\u{43d}\u{435}\u{43e}\u{444}\u{438}\u{446}\u{438}\u{430}\u{43b}\u{44c}\u{43d}\u{44b}\u{439}\u{3a}\u{20}\u{56}\u{4b}\u{20}\u{43c}\u{43e}\u{436}\u{435}\u{442}\u{20}\u{435}\u{433}\u{43e}\u{20}\u{437}\u{430}\u{43a}\u{440}\u{44b}\u{442}\u{44c}\u{20}\u{438}\u{43b}\u{438}\u{20}\u{43f}\u{43e}\u{43f}\u{440}\u{43e}\u{441}\u{438}\u{442}\u{44c}\u{20}\u{43f}\u{43e}\u{434}\u{442}\u{432}\u{435}\u{440}\u{434}\u{438}\u{442}\u{44c}\u{20}\u{432}\u{445}\u{43e}\u{434}\u{2e}\u{20}\u{415}\u{441}\u{43b}\u{438}\u{20}\u{441}\u{441}\u{44b}\u{43b}\u{43a}\u{430}\u{20}\u{43f}\u{435}\u{440}\u{435}\u{441}\u{442}\u{430}\u{43b}\u{430}\u{20}\u{440}\u{430}\u{431}\u{43e}\u{442}\u{430}\u{442}\u{44c}\u{2c}\u{20}\u{43f}\u{440}\u{43e}\u{441}\u{442}\u{43e}\u{20}\u{43f}\u{440}\u{43e}\u{439}\u{434}\u{438}\u{20}\u{448}\u{430}\u{433}\u{438}\u{20}\u{437}\u{430}\u{43d}\u{43e}\u{432}\u{43e}\u{2e}";

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

#[derive(Clone, PartialEq, Eq)]
struct ShuffleQueueKey {
    source: QueueSource,
    playlist: Option<(i64, i64)>,
    search_query: Option<String>,
    filter: String,
}

struct Xorshift64(u64);

impl Xorshift64 {
    fn from_seed(seed: u64) -> Self {
        Self(if seed == 0 { 0x9E3779B97F4A7C15 } else { seed })
    }

    fn from_time() -> Self {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(1);
        Self::from_seed(nanos)
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
}

fn pick_shuffle_id(
    candidates: &[TrackId],
    played: &HashSet<TrackId>,
    rng: &mut Xorshift64,
    excluded: Option<TrackId>,
) -> Option<TrackId> {
    let mut pool: Vec<TrackId> = Vec::new();
    let mut seen: HashSet<TrackId> = HashSet::new();
    for &tid in candidates {
        if played.contains(&tid) || excluded == Some(tid) { continue; }
        if seen.insert(tid) { pool.push(tid); }
    }
    if pool.is_empty() { return None; }
    let idx = (rng.next_u64() % pool.len() as u64) as usize;
    Some(pool[idx])
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

#[derive(Clone, Copy)]
enum TransportIcon {
    Prev,
    Play,
    Pause,
    Next,
    Stop,
}

fn paint_bar(painter: &egui::Painter, x0: f32, x1: f32, y: f32, fraction: f32, thickness: f32, handle: bool) {
    if x1 <= x0 { return; }
    let value = fraction.clamp(0.0, 1.0);
    let fill_x = x0 + (x1 - x0) * value;
    painter.line_segment([egui::pos2(x0, y), egui::pos2(x1, y)], egui::Stroke::new(thickness, theme::BORDER));
    if value > 0.0 {
        painter.line_segment([egui::pos2(x0, y), egui::pos2(fill_x, y)], egui::Stroke::new(thickness, theme::ACCENT));
    }
    if handle {
        painter.circle_filled(egui::pos2(fill_x, y), 5.0, theme::ACCENT);
    }
}

fn measure_label_width(ui: &egui::Ui, text: &str, font: egui::FontId) -> f32 {
    ui.painter().layout_no_wrap(text.to_owned(), font, theme::TEXT).rect.width()
}

fn draw_heartbeat(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
    let sx = rect.width() / 20.0;
    let sy = rect.height() / 20.0;
    let y = rect.center().y;
    let x = rect.min.x;
    let point = |px: f32, py: f32| egui::pos2(x + px * sx, y + py * sy);
    let pts = vec![
        point(0.0, 0.0),
        point(3.0, 0.0),
        point(5.5, -5.0),
        point(8.0, 4.0),
        point(10.5, 0.0),
        point(13.5, 0.0),
        point(16.0, -5.0),
        point(18.5, 4.0),
        point(20.0, 0.0),
    ];
    painter.line(pts.clone(), egui::epaint::PathStroke::new(1.6 * sy, color));
    painter.circle_filled(pts[0], 0.8 * sy, color);
    painter.circle_filled(*pts.last().unwrap(), 0.8 * sy, color);
}

fn draw_brand_mark(painter: &egui::Painter, rect: egui::Rect) {
    draw_heartbeat(painter, rect, theme::ACCENT);
}

fn draw_empty_state(ui: &mut egui::Ui, rect: egui::Rect, main: &str, sub: Option<&str>) {
    let painter = ui.painter_at(rect);
    let center_x = rect.center().x;
    let center_y = rect.min.y + rect.height() * 0.4;
    let icon_color = theme::BORDER.lerp_to_gamma(theme::TEXT, 0.45);
    let icon_rect = egui::Rect::from_center_size(
        egui::pos2(center_x, center_y - 24.0),
        egui::vec2(56.0, 36.0),
    );
    draw_heartbeat(&painter, icon_rect, icon_color);
    painter.text(
        egui::pos2(center_x, center_y + 10.0),
        egui::Align2::CENTER_CENTER,
        main,
        theme::semibold_font(16.0),
        theme::TEXT,
    );
    if let Some(sub) = sub {
        painter.text(
            egui::pos2(center_x, center_y + 32.0),
            egui::Align2::CENTER_CENTER,
            sub,
            egui::FontId::proportional(12.0),
            theme::MUTED,
        );
    }
}

fn draw_skeleton_rows(ui: &mut egui::Ui, rect: egui::Rect, row_h: f32) {
    let painter = ui.painter_at(rect);
    let widths = [0.42, 0.55, 0.34, 0.48, 0.30, 0.52, 0.38, 0.50, 0.33];
    let sub_widths = [0.24, 0.27, 0.21, 0.26, 0.20, 0.30, 0.23, 0.25, 0.22];
    let radius = egui::CornerRadius::same(4);
    let inset = 10.0;
    let number_w = 30.0;
    let duration_w = 38.0;
    let text_left = rect.min.x + inset + 44.0;
    let text_w = (rect.width() - inset * 2.0 - 44.0 - duration_w - 12.0).max(80.0);
    for i in 0..9 {
        let cy = rect.min.y + row_h * (i as f32) + row_h * 0.5;
        painter.rect_filled(
            egui::Rect::from_center_size(
                egui::pos2(rect.min.x + inset + number_w * 0.5, cy),
                egui::vec2(number_w, 8.0),
            ),
            radius,
            theme::SURFACE,
        );
        painter.rect_filled(
            egui::Rect::from_min_size(
                egui::pos2(text_left, cy - 10.0),
                egui::vec2(text_w * widths[i % widths.len()], 10.0),
            ),
            radius,
            theme::SURFACE,
        );
        painter.rect_filled(
            egui::Rect::from_min_size(
                egui::pos2(text_left, cy + 6.0),
                egui::vec2(text_w * sub_widths[i % sub_widths.len()], 8.0),
            ),
            radius,
            theme::SURFACE,
        );
        painter.rect_filled(
            egui::Rect::from_center_size(
                egui::pos2(rect.max.x - inset - duration_w * 0.5, cy),
                egui::vec2(duration_w, 8.0),
            ),
            radius,
            theme::SURFACE,
        );
    }
}

fn status_is_error(text: &str) -> bool {
    text.contains(FILTER_AUTOLOAD_ERROR_TEXT)
        || text.contains(PLAYER_CHANNEL_CLOSED_TEXT)
        || text.contains("error")
        || text.contains("unavailable")
}

fn draw_magnifier(painter: &egui::Painter, center: egui::Pos2) {
    let color = theme::MUTED;
    painter.circle_stroke(egui::pos2(center.x - 3.0, center.y - 3.0), 4.0, egui::Stroke::new(1.3, color));
    painter.line_segment(
        [egui::pos2(center.x + 0.5, center.y + 0.5), egui::pos2(center.x + 5.5, center.y + 5.5)],
        egui::Stroke::new(1.5, color),
    );
}

fn draw_clear_cross(painter: &egui::Painter, center: egui::Pos2) {
    let color = theme::MUTED;
    painter.line_segment(
        [egui::pos2(center.x - 4.0, center.y - 4.0), egui::pos2(center.x + 4.0, center.y + 4.0)],
        egui::Stroke::new(1.4, color),
    );
    painter.line_segment(
        [egui::pos2(center.x + 4.0, center.y - 4.0), egui::pos2(center.x - 4.0, center.y + 4.0)],
        egui::Stroke::new(1.4, color),
    );
}

fn draw_menu_dots(painter: &egui::Painter, center: egui::Pos2, hovered: bool) {
    let color = if hovered { theme::TEXT } else { theme::MUTED };
    for dx in [-6.0, 0.0, 6.0] {
        painter.circle_filled(egui::pos2(center.x + dx, center.y), 1.8, color);
    }
}

fn text_tab(ui: &mut egui::Ui, rect: egui::Rect, id: &str, label: &str, font: egui::FontId, active: bool) -> bool {
    let resp = ui.interact(rect, ui.make_persistent_id(id), egui::Sense::click());
    let hovered = resp.hovered();
    let color = if active || hovered { theme::TEXT } else { theme::MUTED };
    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, label, font, color);
    if active {
        let y = rect.max.y + 4.0;
        let stroke = egui::Stroke::new(2.0, theme::ACCENT);
        ui.painter().line_segment([egui::pos2(rect.min.x + 1.0, y), egui::pos2(rect.max.x - 1.0, y)], stroke);
        ui.painter().circle_filled(egui::pos2(rect.min.x, y), 1.0, theme::ACCENT);
        ui.painter().circle_filled(egui::pos2(rect.max.x, y), 1.0, theme::ACCENT);
    }
    if hovered {
        resp.clone().on_hover_cursor(egui::CursorIcon::PointingHand);
    }
    resp.clicked()
}

fn styled_search_field(ui: &mut egui::Ui, rect: egui::Rect, value: &mut String, id: egui::Id, hint: &'static str) -> egui::Response {
    let radius = egui::CornerRadius::same(8);
    ui.painter().rect_filled(rect, radius, theme::SURFACE);
    let text_rect = egui::Rect::from_min_max(
        egui::pos2(rect.min.x + 30.0, rect.min.y),
        egui::pos2(rect.max.x - 10.0, rect.max.y),
    );
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(text_rect).layout(egui::Layout::left_to_right(egui::Align::Center)));
    child.set_min_height(rect.height());
    let resp = child.add(
        egui::TextEdit::singleline(value)
            .id(id)
            .frame(egui::Frame::NONE)
            .desired_width(text_rect.width())
            .hint_text(egui::RichText::new(hint).color(theme::MUTED)),
    );
    let focused = resp.has_focus();
    ui.painter().rect_stroke(
        rect,
        radius,
        egui::Stroke::new(1.0, if focused { theme::ACCENT } else { theme::BORDER }),
        egui::StrokeKind::Middle,
    );
    draw_magnifier(ui.painter(), egui::pos2(rect.min.x + 16.0, rect.center().y));
    resp
}

fn ghost_pill_button(ui: &mut egui::Ui, rect: egui::Rect, id: &str, label: &str) -> bool {
    let resp = ui.interact(rect, ui.make_persistent_id(id), egui::Sense::click());
    let hovered = resp.hovered();
    let radius = egui::CornerRadius::same(16);
    let painter = ui.painter();
    painter.rect_filled(rect, radius, if hovered { theme::HOVER } else { theme::SURFACE });
    painter.rect_stroke(rect, radius, egui::Stroke::new(1.0, theme::BORDER), egui::StrokeKind::Middle);
    painter.text(rect.center(), egui::Align2::CENTER_CENTER, label, theme::semibold_font(12.0), if hovered { theme::TEXT } else { theme::MUTED });
    if hovered {
        resp.clone().on_hover_cursor(egui::CursorIcon::PointingHand);
    }
    resp.clicked()
}

fn primary_pill_button(ui: &mut egui::Ui, rect: egui::Rect, id: &str, label: &str) -> bool {
    let resp = ui.interact(rect, ui.make_persistent_id(id), egui::Sense::click());
    let hovered = resp.hovered();
    let radius = egui::CornerRadius::same(17);
    let painter = ui.painter();
    painter.rect_filled(rect, radius, if hovered { theme::ACCENT_DIM } else { theme::ACCENT });
    painter.text(rect.center(), egui::Align2::CENTER_CENTER, label, theme::semibold_font(13.0), theme::BG);
    if hovered {
        resp.clone().on_hover_cursor(egui::CursorIcon::PointingHand);
    }
    resp.clicked()
}

fn draw_step_circle(ui: &mut egui::Ui, rect: egui::Rect, number: usize) {
    let painter = ui.painter();
    let radius = rect.width() * 0.5;
    painter.circle_filled(rect.center(), radius, theme::ACCENT_DIM);
    painter.circle_stroke(rect.center(), radius, egui::Stroke::new(1.0, theme::ACCENT));
    painter.text(rect.center(), egui::Align2::CENTER_CENTER, number.to_string(), theme::mono_font(12.0), theme::ACCENT);
}

fn login_step_row(ui: &mut egui::Ui, number: usize, text: &str) {
    ui.horizontal(|ui| {
        let (circle_rect, _) = ui.allocate_exact_size(egui::vec2(LOGIN_STEP_CIRCLE, LOGIN_STEP_CIRCLE), egui::Sense::hover());
        draw_step_circle(ui, circle_rect, number);
        ui.add_space(LOGIN_STEP_GAP);
        ui.add(egui::Label::new(egui::RichText::new(text).color(theme::TEXT)).selectable(false).wrap());
    });
}

fn styled_login_field(ui: &mut egui::Ui, rect: egui::Rect, id: egui::Id, value: &mut String, hint: &'static str, password: bool) -> egui::Response {
    let radius = egui::CornerRadius::same(8);
    ui.painter().rect_filled(rect, radius, theme::SURFACE);
    let text_rect = rect.shrink2(egui::vec2(10.0, 0.0));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(text_rect).layout(egui::Layout::left_to_right(egui::Align::Center)));
    child.set_min_height(rect.height());
    let resp = child.scope(|ui| {
        ui.visuals_mut().widgets.hovered.bg_stroke = egui::Stroke::NONE;
        ui.add(
            egui::TextEdit::singleline(value)
                .id(id)
                .frame(egui::Frame::NONE)
                .desired_width(text_rect.width())
                .hint_text(egui::RichText::new(hint).color(theme::MUTED))
                .password(password),
        )
    }).inner;
    let focused = resp.has_focus();
    ui.painter().rect_stroke(
        rect,
        radius,
        egui::Stroke::new(1.0, if focused { theme::ACCENT } else { theme::BORDER }),
        egui::StrokeKind::Middle,
    );
    resp
}

fn draw_playing_marker(ui: &egui::Ui, rect: egui::Rect, paused: bool) {
    let painter = ui.painter();
    let center = rect.center();
    if paused {
        let bar_w = 3.0;
        let bar_h = 14.0;
        let gap = 6.0;
        for dx in [-gap * 0.5, gap * 0.5] {
            let bar = egui::Rect::from_center_size(
                egui::pos2(center.x + dx, center.y),
                egui::vec2(bar_w, bar_h),
            );
            painter.rect_filled(bar, egui::CornerRadius::same(1), theme::ACCENT_DIM);
        }
        return;
    }
    let bar_w = 3.0;
    let gap = 4.0;
    let heights = [9.0, 15.0, 11.0];
    let total_w = heights.len() as f32 * bar_w + (heights.len() as f32 - 1.0) * gap;
    let start_x = center.x - total_w * 0.5;
    for (i, h) in heights.iter().enumerate() {
        let bar = egui::Rect::from_center_size(
            egui::pos2(start_x + bar_w * 0.5 + i as f32 * (bar_w + gap), center.y),
            egui::vec2(bar_w, *h),
        );
        painter.rect_filled(bar, egui::CornerRadius::same(1), theme::ACCENT);
    }
}

fn draw_hover_play_marker(ui: &egui::Ui, rect: egui::Rect) {
    let tri = egui::Rect::from_center_size(rect.center(), egui::vec2(10.0, 12.0));
    ui.painter().add(egui::Shape::rotated_triangle(tri, -std::f32::consts::FRAC_PI_2, theme::MUTED));
}

fn transport_button(ui: &mut egui::Ui, rect: egui::Rect, id: &str, icon: TransportIcon, enabled: bool, tooltip: &str) -> bool {
    let resp = ui.interact(rect, ui.make_persistent_id(id), egui::Sense::click());
    let hovered = resp.hovered() && enabled;
    let painter = ui.painter();
    let center = rect.center();
    let is_primary = matches!(icon, TransportIcon::Play | TransportIcon::Pause);
    if is_primary {
        let fill = if enabled { theme::ACCENT } else { theme::ACCENT_DIM };
        painter.circle_filled(center, rect.width() * 0.5, fill);
    } else if hovered {
        painter.circle_filled(center, rect.width() * 0.5, theme::HOVER);
    }
    let color = if is_primary && enabled { theme::BG } else if enabled { theme::TEXT } else { theme::MUTED };
    match icon {
        TransportIcon::Prev => {
            let r = 6.0;
            painter.rect_filled(egui::Rect::from_min_size(egui::pos2(center.x - r - 2.0, center.y - r), egui::vec2(2.0, r * 2.0)), egui::CornerRadius::same(0), color);
            painter.add(egui::Shape::convex_polygon(vec![
                egui::pos2(center.x + r, center.y - r),
                egui::pos2(center.x + r, center.y + r),
                egui::pos2(center.x - r + 1.0, center.y),
            ], color, egui::Stroke::NONE));
        }
        TransportIcon::Next => {
            let r = 6.0;
            painter.add(egui::Shape::convex_polygon(vec![
                egui::pos2(center.x - r, center.y - r),
                egui::pos2(center.x - r, center.y + r),
                egui::pos2(center.x + r - 1.0, center.y),
            ], color, egui::Stroke::NONE));
            painter.rect_filled(egui::Rect::from_min_size(egui::pos2(center.x + r, center.y - r), egui::vec2(2.0, r * 2.0)), egui::CornerRadius::same(0), color);
        }
        TransportIcon::Play => {
            painter.add(egui::Shape::convex_polygon(vec![
                egui::pos2(center.x - 5.0, center.y - 7.0),
                egui::pos2(center.x - 5.0, center.y + 7.0),
                egui::pos2(center.x + 7.0, center.y),
            ], color, egui::Stroke::NONE));
        }
        TransportIcon::Pause => {
            painter.rect_filled(egui::Rect::from_center_size(egui::pos2(center.x - 4.0, center.y), egui::vec2(4.0, 16.0)), egui::CornerRadius::same(2), color);
            painter.rect_filled(egui::Rect::from_center_size(egui::pos2(center.x + 4.0, center.y), egui::vec2(4.0, 16.0)), egui::CornerRadius::same(2), color);
        }
        TransportIcon::Stop => {
            painter.rect_filled(egui::Rect::from_center_size(center, egui::vec2(14.0, 14.0)), egui::CornerRadius::same(3), color);
        }
    }
    let resp = resp.on_hover_text(tooltip);
    if hovered {
        resp.clone().on_hover_cursor(egui::CursorIcon::PointingHand);
    }
    resp.clicked() && enabled
}

fn pill_toggle(ui: &mut egui::Ui, rect: egui::Rect, id: &str, label: &str, active: bool, tooltip: &str) -> bool {
    let resp = ui.interact(rect, ui.make_persistent_id(id), egui::Sense::click());
    let fill = if active { theme::ACCENT_DIM } else { theme::SURFACE };
    let stroke = if active { theme::ACCENT } else { theme::BORDER };
    let text_color = if active { theme::ACCENT } else { theme::MUTED };
    let radius = egui::CornerRadius::same(11);
    let painter = ui.painter();
    painter.rect_filled(rect, radius, fill);
    painter.rect_stroke(rect, radius, egui::Stroke::new(1.0, stroke), egui::StrokeKind::Middle);
    painter.text(rect.center(), egui::Align2::CENTER_CENTER, label, theme::semibold_font(12.0), text_color);
    let resp = resp.on_hover_text(tooltip);
    if resp.hovered() {
        resp.clone().on_hover_cursor(egui::CursorIcon::PointingHand);
    }
    resp.clicked()
}

fn draw_speaker(painter: &egui::Painter, center: egui::Pos2, muted: bool) {
    let color = if muted { theme::MUTED } else { theme::TEXT };
    painter.rect_filled(egui::Rect::from_center_size(egui::pos2(center.x - 5.0, center.y), egui::vec2(5.0, 8.0)), egui::CornerRadius::same(0), color);
    painter.add(egui::Shape::convex_polygon(vec![
        egui::pos2(center.x - 2.0, center.y - 4.0),
        egui::pos2(center.x + 3.0, center.y - 8.0),
        egui::pos2(center.x + 3.0, center.y + 8.0),
        egui::pos2(center.x - 2.0, center.y + 4.0),
    ], color, egui::Stroke::NONE));
    if muted {
        painter.line_segment([egui::pos2(center.x + 5.0, center.y - 4.0), egui::pos2(center.x + 11.0, center.y + 4.0)], egui::Stroke::new(1.3, color));
        painter.line_segment([egui::pos2(center.x + 11.0, center.y - 4.0), egui::pos2(center.x + 5.0, center.y + 4.0)], egui::Stroke::new(1.3, color));
    } else {
        painter.line(vec![
            egui::pos2(center.x + 4.0, center.y - 5.0),
            egui::pos2(center.x + 7.0, center.y - 3.0),
            egui::pos2(center.x + 8.0, center.y),
            egui::pos2(center.x + 7.0, center.y + 3.0),
            egui::pos2(center.x + 4.0, center.y + 5.0),
        ], (1.3, color));
        painter.line(vec![
            egui::pos2(center.x + 4.0, center.y - 9.0),
            egui::pos2(center.x + 10.0, center.y - 5.0),
            egui::pos2(center.x + 12.0, center.y),
            egui::pos2(center.x + 10.0, center.y + 5.0),
            egui::pos2(center.x + 4.0, center.y + 9.0),
        ], (1.3, color));
    }
}

pub fn run_gui(env_token: Option<String>, env_ua: Option<String>, single_instance_listener: Option<std::net::TcpListener>) -> eframe::Result<()> {
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel::<PlayerCmd>();
    let (evt_tx, evt_rx) = std::sync::mpsc::channel::<PlayerEvent>();
    let (ui_evt_tx, ui_evt_rx) = std::sync::mpsc::channel::<UiEvent>();
    let window_icon = eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png"))
        .expect("embedded icon.png is valid");

    let mut options = eframe::NativeOptions::default();
    options.glow_options.vsync = true;
    options.viewport = options
        .viewport
        .with_title(theme::APP_NAME)
        .with_icon(window_icon)
        .with_inner_size([720.0, 640.0])
        .with_min_inner_size([640.0, 480.0]);
    eframe::run_native(
        theme::APP_NAME,
        options,
        Box::new(move |cc| {
            let ctx = cc.egui_ctx.clone();
            theme::apply(&ctx);
            ctx.set_fonts(theme::font_definitions(None));
            let (token, ua, from_saved) = resolve_startup_credentials(env_token, env_ua);
            let client_ua = ua.clone().unwrap_or_else(|| DEFAULT_ANDROID_UA.to_string());
            let client = api::build_client(&client_ua).unwrap_or_else(|_| reqwest::blocking::Client::new());
            crate::player::spawn_player_worker(client, cmd_rx, evt_tx, ctx.clone());
            let app = GuiAppImpl::new(
                cmd_tx, evt_rx, ui_evt_tx.clone(), ui_evt_rx, token, ua, from_saved, ctx.clone(),
            );
            if let Some(listener) = single_instance_listener {
                spawn_single_instance_listener(listener, ui_evt_tx, ctx);
            }
            Ok(Box::new(app))
        }),
    )
}

fn spawn_single_instance_listener(listener: std::net::TcpListener, ui_evt_tx: Sender<UiEvent>, ctx: egui::Context) {
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let mut buf = [0u8; 64];
            let n = std::io::Read::read(&mut stream, &mut buf).unwrap_or(0);
            let msg = String::from_utf8_lossy(&buf[..n]);
            if msg.trim() == "show" {
                let _ = ui_evt_tx.send(UiEvent::ShowWindow);
                ctx.request_repaint();
            }
        }
    });
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RepeatMode {
    Off,
    All,
    One,
}

impl RepeatMode {
    fn from_str(s: &str) -> Self {
        match s { "all" => Self::All, "one" => Self::One, _ => Self::Off }
    }
    fn as_str(self) -> &'static str {
        match self { Self::Off => "off", Self::All => "all", Self::One => "one" }
    }
    fn next(self) -> Self {
        match self { Self::Off => Self::All, Self::All => Self::One, Self::One => Self::Off }
    }
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
    TrayClick(MouseButton, MouseButtonState),
    TrayMenu(String),
    ShowWindow,
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
    my_user_id: Option<i64>,

    tracks: Vec<Track>,
    tracks_count: Option<u64>,
    loading_tracks: bool,
    next_offset: usize,
    filter_autoload_stopped: bool,
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
    rec_empty_retries: u8,

    queue: Vec<Track>,
    queue_source: QueueSource,
    queue_playlist: Option<(i64, i64)>,
    queue_search_query: Option<String>,

    status: String,
    now_playing: Option<NowPlaying>,
    volume: f32,
    volume_before_mute: Option<f32>,
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
    _tray_icon: Option<TrayIcon>,
    tray_menu_show_id: String,
    tray_menu_playpause_id: String,
    tray_menu_next_id: String,
    tray_menu_quit_id: String,
    window_hidden: bool,
    quit_requested: bool,
    player_channel_closed: bool,
    last_tray_tooltip: String,
    my_filter: String,
    repeat_mode: RepeatMode,
    shuffle: bool,
    shuffle_played: HashSet<TrackId>,
    shuffle_history: Vec<TrackId>,
    shuffle_rng: Xorshift64,
    shuffle_queue_key_state: Option<ShuffleQueueKey>,
    cjk_font_loaded: bool,
}

fn make_tray_icon() -> anyhow::Result<TrayIconImage> {
    let rgba = include_bytes!("../assets/icon_256.rgba").to_vec();
    Ok(TrayIconImage::from_rgba(rgba, 256, 256)?)
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
        let shuffle = storage::load_shuffle();
        let mut app = Self {
            cmd_tx,
            evt_rx,
            ui_evt_tx,
            ui_evt_rx,
            token: token.unwrap_or_default(),
            ua: ua_val,
            logged_in,
            login_input: String::new(),
            my_user_id: None,

            tracks: Vec::new(),
            tracks_count: None,
            loading_tracks: false,
            next_offset: 0,
            filter_autoload_stopped: false,
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
            rec_empty_retries: 0,

            queue: Vec::new(),
            queue_source: QueueSource::MyMusic,
            queue_playlist: None,
            queue_search_query: None,

            status: if logged_in { String::new() } else { LOGIN_REQUIRED_TEXT.to_string() },
            now_playing: None,
            volume,
            volume_before_mute: None,
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
            _tray_icon: None,
            tray_menu_show_id: String::new(),
            tray_menu_playpause_id: String::new(),
            tray_menu_next_id: String::new(),
            tray_menu_quit_id: String::new(),
            window_hidden: false,
            quit_requested: false,
            player_channel_closed: false,
            last_tray_tooltip: String::new(),
            my_filter: String::new(),
            repeat_mode: RepeatMode::from_str(&storage::load_repeat()),
            shuffle,
            shuffle_played: HashSet::new(),
            shuffle_history: Vec::new(),
            shuffle_rng: Xorshift64::from_time(),
            shuffle_queue_key_state: None,
            cjk_font_loaded: false,
        };
        let _ = app.cmd_tx.send(PlayerCmd::SetVolume(app.volume));
        app.register_media_hotkeys();
        app.build_tray();
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
        self.my_status = "\u{0437}\u{0430}\u{0433}\u{0440}\u{0443}\u{0437}\u{043A}\u{0430}...".to_string();
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
        self.playlists_status = "\u{0437}\u{0430}\u{0433}\u{0440}\u{0443}\u{0437}\u{043A}\u{0430}...".to_string();
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
        self.playlist_status = "\u{0437}\u{0430}\u{0433}\u{0440}\u{0443}\u{0437}\u{043A}\u{0430}...".to_string();
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
            self.search_status = "\u{043F}\u{0443}\u{0441}\u{0442}\u{043E}\u{0439} \u{0437}\u{0430}\u{043F}\u{0440}\u{043E}\u{0441}".to_string();
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
        self.search_status = "\u{0437}\u{0430}\u{0433}\u{0440}\u{0443}\u{0437}\u{043A}\u{0430}...".to_string();
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
        self.rec_status = "\u{0437}\u{0430}\u{0433}\u{0440}\u{0443}\u{0437}\u{043A}\u{0430}...".to_string();
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
        self.rec_status = "\u{0437}\u{0430}\u{0433}\u{0440}\u{0443}\u{0437}\u{043A}\u{0430}...".to_string();
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

    fn retry_recommendations_after_empty(&mut self) {
        self.rec_empty_retries = self.rec_empty_retries.saturating_add(1);
        if self.rec_empty_retries >= 3 {
            self.status = REC_NO_SUITABLE_TRACKS_TEXT.to_string();
            self.rec_status = REC_NO_SUITABLE_TRACKS_TEXT.to_string();
            self.now_playing = None;
            self.sync_tray_tooltip(theme::APP_NAME);
            return;
        }
        self.rec_waiting_next = true;
        self.request_recommendations_more();
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
        self.rec_tracks.clear(); self.rec_loaded_once = false; self.rec_loading = false; self.rec_status.clear(); self.rec_no_more = false; self.rec_request_id = 0; self.rec_waiting_next = false; self.rec_empty_retries = 0;

        self.queue.clear(); self.queue_source = QueueSource::MyMusic; self.queue_playlist = None; self.queue_search_query = None;
        self.clear_shuffle_round();
        self.shuffle_queue_key_state = None;
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
        self.status = message.unwrap_or_else(|| LOGIN_REQUIRED_TEXT.to_string());
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
            QueueSource::MyMusic => {
                if self.my_filter.trim().is_empty() {
                    self.set_queue_for_my_music();
                } else {
                    let filtered: Vec<Track> = self.my_music_filtered().into_iter().map(|(_, t)| t).collect();
                    self.queue = filtered;
                    self.queue_source = QueueSource::MyMusic;
                    self.queue_playlist = None;
                    self.queue_search_query = None;
                }
            }
            QueueSource::Playlist => self.set_queue_for_playlist(),
            QueueSource::Search => self.set_queue_for_search(),
            QueueSource::Recommendations => self.set_queue_for_recommendations(),
        }
        self.selected_index = Some(index);
        self.rec_empty_retries = 0;
        let tid = TrackId::from_track(&track);
        self.sync_shuffle_queue();
        self.mark_shuffle_played(tid);
        self.pending_play_track_id = Some(tid);
        let _ = self.cmd_tx.send(PlayerCmd::PlayTrack { track });
    }

    fn play_track_id(&mut self, track_id: TrackId) {
        let Some(pos) = self.queue.iter().position(|t| TrackId::from_track(t) == track_id) else { return; };
        let track = self.queue[pos].clone();
        if !track.is_playable() { return; }
        self.rec_empty_retries = 0;
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

    fn first_playable_in_queue(&self) -> Option<TrackId> {
        self.queue.iter().find(|t| t.is_playable()).map(TrackId::from_track)
    }

    fn shuffle_queue_key(&self) -> ShuffleQueueKey {
        ShuffleQueueKey {
            source: self.queue_source,
            playlist: self.queue_playlist,
            search_query: self.queue_search_query.clone(),
            filter: if self.queue_source == QueueSource::MyMusic { self.my_filter.clone() } else { String::new() },
        }
    }

    fn clear_shuffle_round(&mut self) {
        self.shuffle_played.clear();
        self.shuffle_history.clear();
    }

    fn sync_shuffle_queue(&mut self) {
        let key = self.shuffle_queue_key();
        if self.shuffle_queue_key_state.as_ref() != Some(&key) {
            self.clear_shuffle_round();
            self.shuffle_queue_key_state = Some(key);
        }
    }

    fn shuffle_candidates(&self) -> Vec<TrackId> {
        self.queue.iter().filter(|t| t.is_playable()).map(TrackId::from_track).collect()
    }

    fn mark_shuffle_played(&mut self, tid: TrackId) {
        if !self.shuffle { return; }
        self.shuffle_played.insert(tid);
        if self.shuffle_history.last() != Some(&tid) {
            self.shuffle_history.push(tid);
            if self.shuffle_history.len() > 100 { self.shuffle_history.remove(0); }
        }
    }

    fn shuffle_next_id(&mut self, cur: TrackId) -> Option<TrackId> {
        let candidates = self.shuffle_candidates();
        pick_shuffle_id(&candidates, &self.shuffle_played, &mut self.shuffle_rng, Some(cur))
    }

    fn shuffle_repeat_id(&mut self, cur: TrackId) -> Option<TrackId> {
        let candidates = self.shuffle_candidates();
        if let Some(next) = pick_shuffle_id(&candidates, &self.shuffle_played, &mut self.shuffle_rng, Some(cur)) {
            return Some(next);
        }
        if candidates.contains(&cur) { Some(cur) } else { None }
    }

    fn shuffle_prev_id(&self, cur: TrackId) -> Option<TrackId> {
        if self.shuffle_history.is_empty() { return None; }
        let Some(pos) = self.shuffle_history.iter().rposition(|&tid| tid == cur) else {
            return self.shuffle_history.last().copied();
        };
        if pos == 0 { return None; }
        let candidates: HashSet<TrackId> = self.shuffle_candidates().into_iter().collect();
        for &tid in self.shuffle_history[..pos].iter().rev() {
            if candidates.contains(&tid) { return Some(tid); }
        }
        None
    }

    fn start_shuffle_track(&mut self, tid: TrackId) {
        self.mark_shuffle_played(tid);
        self.play_track_id(tid);
    }

    fn toggle_shuffle(&mut self) {
        self.shuffle = !self.shuffle;
        storage::save_shuffle(self.shuffle);
        self.clear_shuffle_round();
        self.sync_shuffle_queue();
        if self.shuffle {
            if let Some(tid) = self.now_playing.as_ref().and_then(|np| np.track_id) {
                self.mark_shuffle_played(tid);
            }
        }
    }

    fn play_next(&mut self) {
        self.advance_next(false);
    }

    fn play_next_autoplay(&mut self) {
        self.advance_next(true);
    }

    fn advance_next(&mut self, autoplay: bool) {
        let Some(cur) = self.now_playing.as_ref().and_then(|np| np.track_id) else { return; };
        if autoplay && self.repeat_mode == RepeatMode::One {
            if self.queue.iter().any(|t| TrackId::from_track(t) == cur && t.is_playable()) {
                self.play_track_id(cur);
            } else {
                self.now_playing = None;
                self.sync_tray_tooltip(theme::APP_NAME);
            }
            return;
        }
        self.sync_shuffle_queue();
        if self.shuffle {
            if let Some(next) = self.shuffle_next_id(cur) {
                self.start_shuffle_track(next);
                return;
            }
        } else if let Some(next) = self.find_next_playable_id(cur, 1) {
            self.play_track_id(next);
            return;
        }
        if self.shuffle {
            if self.repeat_mode == RepeatMode::All {
                self.shuffle_played.clear();
                if let Some(next) = self.shuffle_repeat_id(cur) {
                    self.start_shuffle_track(next);
                    return;
                }
            } else if self.queue_source == QueueSource::Recommendations && !self.rec_no_more {
                self.rec_waiting_next = true;
                self.request_recommendations_more();
                return;
            }
        } else {
            if self.queue_source == QueueSource::Recommendations && !self.rec_no_more {
                self.rec_waiting_next = true;
                self.request_recommendations_more();
                return;
            }
            if self.repeat_mode == RepeatMode::All {
                if let Some(first) = self.first_playable_in_queue() {
                    self.play_track_id(first);
                    return;
                }
            }
        }
        self.now_playing = None;
        if autoplay { self.sync_tray_tooltip(theme::APP_NAME); }
    }

    fn play_prev(&mut self) {
        let Some(cur) = self.now_playing.as_ref().and_then(|np| np.track_id) else { return; };
        if self.shuffle {
            self.sync_shuffle_queue();
            if let Some(prev) = self.shuffle_prev_id(cur) {
                self.play_track_id(prev);
                return;
            }
        }
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

    fn sync_tray_tooltip(&mut self, tooltip: &str) {
        if self.last_tray_tooltip == tooltip { return; }
        if let Some(tray) = self._tray_icon.as_ref() {
            let _ = tray.set_tooltip(Some(tooltip));
            self.last_tray_tooltip = tooltip.to_string();
        }
    }

    fn tray_tooltip_for_track(artist: &str, title: &str) -> String {
        let mut s = format!("{} - {}", artist, title);
        if s.chars().count() > 60 {
            s = s.chars().take(60).collect();
        }
        s
    }

    fn my_music_filtered(&self) -> Vec<(usize, Track)> {
        let q = self.my_filter.trim().to_lowercase();
        self.tracks.iter().enumerate().filter(|(_, t)| {
            if q.is_empty() { return true; }
            let hay = format!("{} - {}", t.artist, t.title).to_lowercase();
            hay.contains(&q)
        }).map(|(i, t)| (i, t.clone())).collect()
    }

    fn build_tray(&mut self) {
        let icon = match make_tray_icon() {
            Ok(i) => i,
            Err(e) => { self.hotkey_status = format!("tray unavailable: {}", api::sanitize_output(&format!("{e:#}"))); return; }
        };
        let show = MenuItem::new("\u{041F}\u{043E}\u{043A}\u{0430}\u{0437}\u{0430}\u{0442}\u{044C}", true, None);
        let playpause = MenuItem::new("\u{041F}\u{0430}\u{0443}\u{0437}\u{0430} / \u{0418}\u{0433}\u{0440}\u{0430}\u{0442}\u{044C}", true, None);
        let next = MenuItem::new("\u{0421}\u{043B}\u{0435}\u{0434}\u{0443}\u{044E}\u{0449}\u{0438}\u{0439}", true, None);
        let quit = MenuItem::new("\u{0412}\u{044B}\u{0445}\u{043E}\u{0434}", true, None);
        self.tray_menu_show_id = show.id().0.clone();
        self.tray_menu_playpause_id = playpause.id().0.clone();
        self.tray_menu_next_id = next.id().0.clone();
        self.tray_menu_quit_id = quit.id().0.clone();
        let menu = Menu::new();
        let _ = menu.append(&show);
        let _ = menu.append(&playpause);
        let _ = menu.append(&next);
        let _ = menu.append(&quit);
        let tray = match TrayIconBuilder::new()
            .with_tooltip(theme::APP_NAME)
            .with_icon(icon)
            .with_menu(Box::new(menu))
            .with_menu_on_left_click(false)
            .build() {
            Ok(t) => t,
            Err(e) => { self.hotkey_status = format!("tray unavailable: {}", api::sanitize_output(&format!("{e:#}"))); return; }
        };

        let tx = self.ui_evt_tx.clone();
        let ctx = self.egui_ctx.clone();
        let _ = TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
            match event {
                TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } => {
                    let _ = tx.send(UiEvent::TrayClick(MouseButton::Left, MouseButtonState::Up));
                    ctx.request_repaint();
                }
                _ => {}
            }
        }));
        let tx = self.ui_evt_tx.clone();
        let ctx = self.egui_ctx.clone();
        let _ = MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            let _ = tx.send(UiEvent::TrayMenu(event.id().0.clone()));
            ctx.request_repaint();
        }));
        self._tray_icon = Some(tray);
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
        self.status = "\u{0437}\u{0430}\u{0433}\u{0440}\u{0443}\u{0437}\u{043A}\u{0430}...".to_string();
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

    fn toggle_play_pause(&mut self) {
        if self.now_playing.is_some() {
            let _ = self.cmd_tx.send(PlayerCmd::PauseResume);
        } else if self.restored_track.is_some() {
            self.resolve_restored_track();
        }
    }

    fn commit_seek(&mut self, target: u64) {
        if let Some(np) = self.now_playing.as_mut() {
            np.base_secs = target as f64;
            np.started_at = if np.paused { None } else { Some(Instant::now()) };
        }
        self.seek_drag = None;
        let _ = self.cmd_tx.send(PlayerCmd::Seek(target));
    }

    fn handle_media_hotkey(&mut self, kind: MediaHotkey) {
        match kind {
            MediaHotkey::PlayPause => {
                self.toggle_play_pause();
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
        let mut seek_delta = None;
        ctx.input_mut(|i| {
            if i.consume_key(egui::Modifiers::NONE, egui::Key::Enter) { play_selected = true; }
            if i.consume_key(egui::Modifiers::NONE, egui::Key::Space) { pause_resume = true; }
            if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowRight) { seek_delta = Some(5); }
            if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowLeft) { seek_delta = Some(-5); }
        });
        if play_selected {
            if let Some((src, idx)) = self.selected_or_playing_for_current_list() {
                self.play_from_list(src, idx);
            }
        }
        if pause_resume {
            self.toggle_play_pause();
        }
        if let Some(delta) = seek_delta {
            let target = self.now_playing.as_ref().and_then(|np| {
                np.duration_secs.filter(|&d| d > 0).map(|duration| {
                    let pos = np.position_secs();
                    (pos as i64 + delta).clamp(0, duration as i64) as u64
                })
            });
            if let Some(target) = target {
                self.commit_seek(target);
            }
        }
    }
    fn refresh_my_music_after_add(&mut self) {
        self.tracks.clear();
        self.tracks_count = None;
        self.next_offset = 0;
        self.loading_tracks = false;
        self.filter_autoload_stopped = false;
        self.my_status.clear();
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
            self.rec_status = "\u{0431}\u{043E}\u{043B}\u{044C}\u{0448}\u{0435} \u{043D}\u{0435}\u{0442}".to_string();
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
        let close_requested = ctx.input(|i| i.viewport().close_requested());
        if close_requested && !self.quit_requested {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            self.window_hidden = true;
        }
        while let Ok(evt) = self.ui_evt_rx.try_recv() {
            match evt {
                UiEvent::TracksLoaded { offset, count, items } => {
                    if offset == self.next_offset {
                        let old_offset = self.next_offset;
                        let no_progress = items.is_empty();
                        self.next_offset += items.len();
                        self.tracks.extend(items);
                        if let Some(c) = count { self.tracks_count = Some(c); }
                        self.loading_tracks = false;
                        self.my_status.clear();
                        if no_progress || self.next_offset == old_offset {
                            self.filter_autoload_stopped = true;
                        }
                        if !self.credentials_saved {
                            storage::save_credentials(&self.token, &self.ua);
                            self.credentials_saved = true;
                            self.using_saved_credentials = false;
                        }
                        if self.queue_source == QueueSource::MyMusic && self.my_filter.trim().is_empty() {
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
                            self.sync_shuffle_queue();
                            let next = if let Some(cur_id) = cur {
                                if self.shuffle {
                                    self.shuffle_next_id(cur_id)
                                } else {
                                    self.find_next_playable_id(cur_id, 1)
                                }
                            } else {
                                None
                            };
                            if let Some(next) = next {
                                if self.shuffle { self.start_shuffle_track(next); } else { self.play_track_id(next); }
                            } else if self.rec_no_more {
                                self.now_playing = None;
                            } else {
                                self.retry_recommendations_after_empty();
                            }
                        }
                    }
                }
                UiEvent::RecommendationsError { request_id, msg } => {
                    if request_id != self.rec_request_id { continue; }
                    self.rec_loading = false;
                    self.rec_waiting_next = false;
                    crate::log_status_error(&msg);
                    if self.using_saved_credentials && api::is_auth_error(&anyhow::anyhow!("{}", msg)) {
                        self.logout_to_login(Some("token expired, log in again".to_string()));
                    } else {
                        self.rec_status = msg;
                    }
                }
                                UiEvent::MediaKey(kind) => {
                    self.handle_media_hotkey(kind);
                }
                UiEvent::TrayClick(MouseButton::Left, MouseButtonState::Up) => {
                    let minimized = ctx.input(|i| i.viewport().minimized.unwrap_or(false));
                    if self.window_hidden || minimized {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                        self.window_hidden = false;
                    } else {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                        self.window_hidden = true;
                    }
                }
                UiEvent::TrayClick(_, _) => {}
                UiEvent::TrayMenu(id) => {
                    if id == self.tray_menu_show_id {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                        self.window_hidden = false;
                    } else if id == self.tray_menu_playpause_id {
                        self.handle_media_hotkey(MediaHotkey::PlayPause);
                    } else if id == self.tray_menu_next_id {
                        self.play_next();
                    } else if id == self.tray_menu_quit_id {
                        self.quit_requested = true;
                        let _ = self.cmd_tx.send(PlayerCmd::Stop);
                        self._tray_icon.take();
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                }
                UiEvent::ShowWindow => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                    self.window_hidden = false;
                }
                UiEvent::RestoredTrackResolved { track } => {
                    self.restored_track_pending = false;
                    self.status.clear();
                    self.restored_track = Some(track.clone());
                    self.set_queue_for_my_music();
                    self.sync_shuffle_queue();
                    self.mark_shuffle_played(TrackId::from_track(&track));
                    self.pending_play_track_id = Some(TrackId::from_track(&track));
                    let _ = self.cmd_tx.send(PlayerCmd::PlayTrack { track });
                }
                UiEvent::RestoredTrackUnavailable(msg) => {
                    self.restored_track_pending = false;
                    self.restored_track = None;
                    crate::log_status_error(&msg);
                    self.status = msg;
                }                UiEvent::AddDone { track_id } => {
                    self.add_states.insert(track_id, AddState::Done);
                    self.refresh_my_music_after_add();
                }
                UiEvent::AddError { track_id, msg } => {
                    crate::log_status_error(&msg);
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
                    crate::log_status_error(&msg);
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
                    crate::log_status_error(&msg);
                    let tracks_error = self.loading_tracks;
                    self.loading_tracks = false;
                    self.loading_playlists = false;
                    self.loading_playlist_tracks = false;
                    self.loading_search = false;
                    self.filter_autoload_stopped = true;
                    if self.using_saved_credentials && api::is_auth_error(&anyhow::anyhow!("{}", msg)) {
                        self.logout_to_login(Some("token expired, log in again".to_string()));
                    } else if tracks_error || !self.my_filter.trim().is_empty() {
                        self.status = FILTER_AUTOLOAD_ERROR_TEXT.to_string();
                        if self.active_tab == Tab::MyMusic { self.my_status = FILTER_AUTOLOAD_ERROR_TEXT.to_string(); }
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
        if !self.my_filter.trim().is_empty()
            && !self.loading_tracks
            && !self.filter_autoload_stopped
        {
            let need_more = self.tracks_count.map(|c| (self.next_offset as u64) < c).unwrap_or(false);
            if need_more { self.request_tracks_load(); }
        }
        loop {
            let evt = match self.evt_rx.try_recv() {
                Ok(evt) => evt,
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    if !self.quit_requested && !self.player_channel_closed {
                        self.player_channel_closed = true;
                        self.status = PLAYER_CHANNEL_CLOSED_TEXT.to_string();
                        crate::log_status_error("player channel closed");
                        ctx.request_repaint();
                    }
                    break;
                }
            };
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
                    let tooltip = Self::tray_tooltip_for_track(&artist, &title);
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
                    self.sync_tray_tooltip(&tooltip);
                }
                PlayerEvent::Paused { artist, title, position_secs, duration_secs } => {
                    let track_id = self.now_playing.as_ref().and_then(|np| np.track_id).or(self.pending_play_track_id);
                    self.now_playing = Some(NowPlaying { artist, title, base_secs: position_secs as f64, started_at: None, duration_secs, paused: true, track_id });
                    self.sync_tray_tooltip(theme::APP_NAME);
                }
                PlayerEvent::Resumed { artist, title, position_secs, duration_secs } => {
                    let track_id = self.now_playing.as_ref().and_then(|np| np.track_id).or(self.pending_play_track_id);
                    let tooltip = Self::tray_tooltip_for_track(&artist, &title);
                    self.now_playing = Some(NowPlaying { artist, title, base_secs: position_secs as f64, started_at: Some(Instant::now()), duration_secs, paused: false, track_id });
                    self.sync_tray_tooltip(&tooltip);
                }
                PlayerEvent::Stopped => {
                    self.now_playing = None;
                    self.pending_play_track_id = None;
                    self.current_gen = 0;
                    self.sync_tray_tooltip(theme::APP_NAME);
                }
                PlayerEvent::Ended { generation } => {
                    if generation == self.current_gen {
                        self.play_next_autoplay();
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
                    self.now_playing = None;
                    self.sync_tray_tooltip(theme::APP_NAME);
                    crate::log_status_error(&msg);
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
    fn login_now(&mut self) {
        if let Some(token) = api::extract_access_token(&self.login_input) {
            if self.ua.trim().is_empty() {
                self.ua = DEFAULT_ANDROID_UA.to_string();
            }
            self.token = token;
            self.logged_in = true;
            self.login_input.clear();
            self.status.clear();
            self.tracks.clear();
            self.tracks_count = None;
            self.next_offset = 0;
            self.filter_autoload_stopped = false;
            self.using_saved_credentials = false;
            self.credentials_saved = false;
            self.request_tracks_load();
        } else {
            self.status = "VK API error: access_token not found".to_string();
        }
    }

    fn login_ui(&mut self, ui: &mut egui::Ui) {
        let available = ui.available_rect_before_wrap();
        let card_w = 480.0f32.min(available.width() - 48.0).max(280.0);
        let inner_w = (card_w - 48.0).max(1.0);
        let top_gap = ((available.height() - LOGIN_CARD_ESTIMATE_H) * 0.5).max(0.0);

        egui::ScrollArea::vertical().hscroll(false).show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.set_width(card_w);
                ui.add_space(top_gap);
                egui::Frame::new()
                    .fill(theme::SURFACE)
                    .stroke(egui::Stroke::new(1.0, theme::BORDER))
                    .corner_radius(egui::CornerRadius::same(12))
                    .inner_margin(egui::Margin::symmetric(24, 18))
                    .show(ui, |ui| {
                        ui.vertical(|ui| {
                            ui.set_width(inner_w);

                            ui.horizontal(|ui| {
                                let (mark_rect, _) = ui.allocate_exact_size(egui::vec2(30.0, 20.0), egui::Sense::hover());
                                draw_brand_mark(ui.painter(), mark_rect);
                                ui.add_space(8.0);
                                let (name_rect, _) = ui.allocate_exact_size(egui::vec2(inner_w - 38.0, 20.0), egui::Sense::hover());
                                ui.painter().text(
                                    egui::pos2(name_rect.min.x, name_rect.center().y),
                                    egui::Align2::LEFT_CENTER,
                                    theme::APP_NAME,
                                    theme::semibold_font(18.0),
                                    theme::TEXT,
                                );
                            });
                            ui.add_space(14.0);

                            ui.add(egui::Label::new(
                                egui::RichText::new(LOGIN_HEADING).font(theme::semibold_font(22.0)).color(theme::TEXT),
                            ).selectable(false).wrap());
                            ui.add_space(4.0);
                            ui.add(egui::Label::new(
                                egui::RichText::new(LOGIN_SUBHEADING).size(12.0).color(theme::MUTED),
                            ).selectable(false).wrap());
                            ui.add_space(12.0);

                            login_step_row(ui, 1, LOGIN_STEP_ONE);
                            ui.add_space(LOGIN_STEP_ROW_GAP);
                            login_step_row(ui, 2, LOGIN_STEP_TWO);
                            ui.add_space(LOGIN_STEP_ROW_GAP);
                            login_step_row(ui, 3, LOGIN_STEP_THREE);
                            ui.add_space(10.0);

                            let button_gap = 10.0;
                            let button_w = 140.0f32.min(inner_w);
                            ui.horizontal_wrapped(|ui| {
                                let (open_rect, _) = ui.allocate_exact_size(egui::vec2(button_w, 34.0), egui::Sense::hover());
                                if primary_pill_button(ui, open_rect, "login.open", LOGIN_OPEN_LABEL) {
                                    if std::process::Command::new("rundll32")
                                        .args(["url.dll,FileProtocolHandler", api::AUTH_URL])
                                        .spawn()
                                        .is_err()
                                    {
                                        self.status = "browser open error".to_string();
                                    }
                                }
                                ui.add_space(button_gap);
                                let (copy_rect, _) = ui.allocate_exact_size(egui::vec2(button_w, 34.0), egui::Sense::hover());
                                if ghost_pill_button(ui, copy_rect, "login.copy", LOGIN_COPY_LABEL) {
                                    ui.ctx().copy_text(api::AUTH_URL.to_string());
                                }
                            });

                            ui.add_space(10.0);
                            let paste_w = LOGIN_PASTE_BUTTON_W.min(inner_w);
                            let link_w = (inner_w - paste_w - 8.0).max(1.0);
                            let mut submit_on_enter = false;
                            let mut paste_clicked = false;
                            ui.horizontal(|ui| {
                                let (link_rect, _) = ui.allocate_exact_size(egui::vec2(link_w, 34.0), egui::Sense::hover());
                                let link_resp = styled_login_field(
                                    ui,
                                    link_rect,
                                    ui.scope_id().with("login.link"),
                                    &mut self.login_input,
                                    LOGIN_LINK_HINT,
                                    true,
                                );
                                submit_on_enter = link_resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                                ui.add_space(8.0);
                                let (paste_rect, _) = ui.allocate_exact_size(egui::vec2(paste_w, 34.0), egui::Sense::hover());
                                paste_clicked = ghost_pill_button(ui, paste_rect, "login.paste", LOGIN_PASTE_LABEL);
                            });
                            if paste_clicked {
                                ui.ctx().send_viewport_cmd(egui::ViewportCommand::RequestPaste);
                            }

                            ui.add_space(8.0);
                            let (submit_rect, _) = ui.allocate_exact_size(egui::vec2(inner_w, 34.0), egui::Sense::hover());
                            let submit_clicked = primary_pill_button(ui, submit_rect, "login.submit", LOGIN_SUBMIT_LABEL);
                            if submit_clicked || submit_on_enter {
                                self.login_now();
                            }

                            ui.add_space(10.0);
                            ui.add(egui::Label::new(
                                egui::RichText::new(LOGIN_PRIVACY).size(11.0).color(theme::MUTED),
                            ).selectable(false).wrap());
                            ui.add_space(6.0);
                            ui.add(egui::Label::new(
                                egui::RichText::new(LOGIN_DISCLAIMER).size(11.0).color(theme::MUTED),
                            ).selectable(false).wrap());
                        });
                    });
            });
        });
    }

    fn ensure_cjk_font(&mut self, ctx: &egui::Context, text: &str) {
        if self.cjk_font_loaded || !theme::needs_cjk(text) {
            return;
        }
        self.cjk_font_loaded = true;
        if let Some(cjk) = theme::cjk_font_bytes() {
            ctx.set_fonts(theme::font_definitions(Some(cjk)));
        }
    }

    fn status_line(&mut self, ui: &mut egui::Ui, text: &str, is_error: bool) {
        if text.is_empty() {
            return;
        }
        self.ensure_cjk_font(ui.ctx(), text);
        let indent = if is_error { 9.0 } else { 0.0 };
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2((ui.available_width() - indent).max(1.0), 16.0),
            egui::Sense::hover(),
        );
        let color = if is_error { theme::WARN } else { theme::MUTED };
        if is_error {
            let bar = egui::Rect::from_min_size(rect.min, egui::vec2(3.0, rect.height()));
            ui.painter().rect_filled(bar, egui::CornerRadius::same(0), theme::WARN);
        }
        let text_rect = egui::Rect::from_min_max(
            egui::pos2(rect.min.x + indent, rect.min.y),
            rect.max,
        );
        let mut text_ui = ui.new_child(egui::UiBuilder::new().max_rect(text_rect).layout(egui::Layout::left_to_right(egui::Align::Center)));
        text_ui.set_min_height(rect.height());
        text_ui.add(egui::Label::new(egui::RichText::new(text.to_owned()).size(11.0).color(color)).selectable(false).truncate());
    }

    fn render_track_row(&mut self, ui: &mut egui::Ui, i: usize, track: &Track, width: f32, row_h: f32, source: QueueSource, playing_in_this_list: bool) {
        let is_playing = playing_in_this_list;
        let paused = self.now_playing.as_ref().map(|np| np.paused).unwrap_or(false);
        let playable = track.is_playable();
        let dur = track.duration_secs.map(api::format_mm_ss).unwrap_or_else(|| "--:--".to_string());
        let idx_text = format!("{:>2}", i + 1);
        let title_text = track.title.clone();
        let artist_text = track.artist.clone();
        self.ensure_cjk_font(ui.ctx(), &title_text);
        self.ensure_cjk_font(ui.ctx(), &artist_text);
        let tid = TrackId::from_track(track);
        let delete_confirm = source == QueueSource::MyMusic
            && matches!(self.delete_states.get(&tid), Some(DeleteState::Confirm));
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, row_h), egui::Sense::click());
        let pointer_over_row = ui.ctx().pointer_latest_pos().map_or(false, |pos| rect.contains(pos));
        let row_hovered = pointer_over_row && playable;
        if playable && pointer_over_row {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        let corner = egui::CornerRadius::same(6);
        if is_playing {
            ui.painter().rect_filled(rect, corner, theme::SELECTED);
            let bar = egui::Rect::from_min_size(rect.min, egui::vec2(3.0, row_h));
            ui.painter().rect_filled(bar, egui::CornerRadius::same(0), theme::ACCENT);
        } else if row_hovered {
            ui.painter().rect_filled(rect, corner, theme::HOVER);
        }

        let inner = rect.shrink2(egui::vec2(10.0, 0.0));
        let left_w = 44.0;
        let duration_w = 52.0;
        let action_w = if delete_confirm { 170.0 } else { 52.0 };
        let action_gap = 8.0;
        let text_gap = 6.0;
        let duration_rect = egui::Rect::from_min_size(
            egui::pos2(inner.max.x - duration_w, rect.min.y),
            egui::vec2(duration_w, row_h),
        );
        let action_right = duration_rect.min.x - action_gap;
        let action_rect = egui::Rect::from_min_size(
            egui::pos2(action_right - action_w, rect.min.y),
            egui::vec2(action_w, row_h),
        );
        let text_min_x = inner.min.x + left_w + text_gap;
        let text_w = (action_rect.min.x - text_gap - text_min_x).max(40.0);
        let left_rect = egui::Rect::from_min_size(inner.min, egui::vec2(left_w, row_h));
        let text_rect = egui::Rect::from_min_size(egui::pos2(text_min_x, rect.min.y), egui::vec2(text_w, row_h));

        let mut left_ui = ui.new_child(egui::UiBuilder::new().max_rect(left_rect).layout(egui::Layout::left_to_right(egui::Align::Center)));
        left_ui.set_min_height(row_h);
        if is_playing {
            draw_playing_marker(ui, left_rect, paused);
        } else if row_hovered {
            draw_hover_play_marker(ui, left_rect);
        } else {
            left_ui.add(egui::Label::new(egui::RichText::new(idx_text).font(theme::mono_font(13.0)).color(theme::MUTED)).selectable(false));
        }

        let dim = theme::MUTED.gamma_multiply(0.55);
        let title_color = if playable { theme::TEXT } else { dim };
        let artist_color = if playable { theme::MUTED } else { dim };
        let mut text_ui = ui.new_child(egui::UiBuilder::new().max_rect(text_rect).layout(egui::Layout::top_down(egui::Align::LEFT)));
        text_ui.set_min_height(row_h);
        text_ui.add(egui::Label::new(egui::RichText::new(title_text).font(theme::semibold_font(14.0)).color(title_color)).selectable(false).truncate());
        text_ui.add(egui::Label::new(egui::RichText::new(artist_text).size(12.0).color(artist_color)).selectable(false).truncate());

        let mut duration_ui = ui.new_child(egui::UiBuilder::new().max_rect(duration_rect).layout(egui::Layout::right_to_left(egui::Align::Center)));
        duration_ui.set_min_height(row_h);
        let time_color = if playable { theme::MUTED } else { dim };
        duration_ui.add(egui::Label::new(egui::RichText::new(dur).font(theme::mono_font(12.0)).color(time_color)).selectable(false));

        let mut action_ui = ui.new_child(egui::UiBuilder::new().max_rect(action_rect).layout(egui::Layout::left_to_right(egui::Align::Center)));
        action_ui.set_min_height(row_h);
        let action_clicked = self.render_track_action(&mut action_ui, track, source, &tid, &resp, pointer_over_row || delete_confirm);
        if !action_clicked && resp.clicked() {
            self.selected_index = Some(i);
            if playable {
                self.play_from_list(source, i);
            }
        }
    }

    fn render_track_action(&mut self, ui: &mut egui::Ui, track: &Track, source: QueueSource, tid: &TrackId, row_resp: &egui::Response, visible: bool) -> bool {
        match source {
            QueueSource::MyMusic => self.render_delete_action(ui, track, tid, row_resp, visible),
            QueueSource::Playlist | QueueSource::Search | QueueSource::Recommendations => self.render_add_action(ui, track, tid, visible),
        }
    }

    fn render_add_action(&mut self, ui: &mut egui::Ui, track: &Track, tid: &TrackId, visible: bool) -> bool {
        if !visible || self.track_is_mine(track) { return false; }
        let state = self.add_states.get(tid).cloned().unwrap_or(AddState::Idle);
        match state {
            AddState::Running => {
                ui.add(egui::Label::new(egui::RichText::new("...").font(theme::mono_font(12.0)).color(theme::MUTED)).selectable(false));
                false
            }
            AddState::Done => {
                ui.add(egui::Label::new(egui::RichText::new("ok").font(theme::mono_font(12.0)).color(theme::ACCENT)).selectable(false));
                false
            }
            AddState::Idle => {
                let (rect, resp) = ui.allocate_exact_size(egui::vec2(24.0, 18.0), egui::Sense::click());
                let color = if resp.hovered() { theme::ACCENT } else { theme::MUTED };
                let center = rect.center();
                let r = 5.0;
                ui.painter().line_segment([center - egui::vec2(r, 0.0), center + egui::vec2(r, 0.0)], egui::Stroke::new(1.5, color));
                ui.painter().line_segment([center - egui::vec2(0.0, r), center + egui::vec2(0.0, r)], egui::Stroke::new(1.5, color));
                let resp = resp.on_hover_text(ADD_TRACK_TOOLTIP);
                if resp.clicked() {
                    self.start_add_track(track.clone());
                    return true;
                }
                false
            }
            AddState::Error(msg) => {
                let (rect, resp) = ui.allocate_exact_size(egui::vec2(24.0, 18.0), egui::Sense::click());
                let color = if resp.hovered() { theme::ACCENT } else { theme::MUTED };
                let center = rect.center();
                let r = 5.0;
                ui.painter().line_segment([center - egui::vec2(r, 0.0), center + egui::vec2(r, 0.0)], egui::Stroke::new(1.5, color));
                ui.painter().line_segment([center - egui::vec2(0.0, r), center + egui::vec2(0.0, r)], egui::Stroke::new(1.5, color));
                let resp = resp.on_hover_text(format!("{}\n{}", ADD_TRACK_TOOLTIP, msg));
                if resp.clicked() {
                    self.start_add_track(track.clone());
                    return true;
                }
                false
            }
        }
    }

    fn render_delete_action(&mut self, ui: &mut egui::Ui, track: &Track, tid: &TrackId, row_resp: &egui::Response, visible: bool) -> bool {
        let state = self.delete_states.get(tid).cloned().unwrap_or(DeleteState::Idle);
        match state {
            DeleteState::Idle => {
                if !visible { return false; }
                let (rect, resp) = ui.allocate_exact_size(egui::vec2(24.0, 18.0), egui::Sense::click());
                let color = if resp.hovered() { theme::WARN } else { theme::MUTED };
                let center = rect.center();
                let r = 5.0;
                ui.painter().line_segment([center - egui::vec2(r, r), center + egui::vec2(r, r)], egui::Stroke::new(1.5, color));
                ui.painter().line_segment([center + egui::vec2(-r, r), center + egui::vec2(r, -r)], egui::Stroke::new(1.5, color));
                let resp = resp.on_hover_text(REMOVE_TRACK_TOOLTIP);
                if resp.clicked() {
                    self.delete_states.insert(*tid, DeleteState::Confirm);
                    return true;
                }
                false
            }
            DeleteState::Running => {
                if visible {
                    ui.add(egui::Label::new(egui::RichText::new("...").font(theme::mono_font(12.0)).color(theme::MUTED)).selectable(false));
                }
                false
            }
            DeleteState::Confirm => {
                let mut clicked_any = false;
                let yes = ui.small_button("\u{0414}\u{0430}");
                let no = ui.small_button("\u{041D}\u{0435}\u{0442}");
                ui.add(egui::Label::new(egui::RichText::new(format!("{}?", REMOVE_TRACK_TOOLTIP)).font(theme::semibold_font(13.0))).selectable(false));
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
                clicked_any
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
                crate::log_fps_line(&format!("passes: {}, frames: {}, logic: {}", self.fps_passes, self.fps_frames, logic_only));
                self.fps_passes = 0; self.fps_frames = 0; self.fps_logic = 0; self.fps_last = Instant::now();
            }
        }

        let panel_frame = egui::Frame::new()
            .fill(theme::PANEL)
            .stroke(egui::Stroke::NONE)
            .inner_margin(egui::Margin {
                left: PANEL_MARGIN_X as i8,
                right: PANEL_MARGIN_X as i8,
                top: 0,
                bottom: PANEL_BOTTOM_MARGIN as i8,
            });
        egui::Panel::bottom("bottom")
            .frame(panel_frame)
            .show_separator_line(false)
            .min_size(132.0)
            .max_size(132.0)
            .default_size(132.0)
            .show(ui, |ui| {
                let content = ui.available_rect_before_wrap();
                let top_y = content.min.y;
                ui.painter().line_segment(
                    [
                        egui::pos2(content.min.x - PANEL_MARGIN_X, top_y),
                        egui::pos2(content.max.x + PANEL_MARGIN_X, top_y),
                    ],
                    egui::Stroke::new(1.0, theme::BORDER),
                );

                let has_duration = self.now_playing.as_ref()
                    .and_then(|np| np.duration_secs)
                    .filter(|&d| d > 0)
                    .is_some();
                let pos = self.now_playing.as_ref().map(|np| np.position_secs()).unwrap_or(0);
                let dur = self.now_playing.as_ref().and_then(|np| np.duration_secs).unwrap_or(pos.max(1));
                let shown_pos = self.seek_drag.unwrap_or(pos).min(dur.max(1));
                let fraction = if has_duration { shown_pos as f32 / dur.max(1) as f32 } else { 0.0 };
                let progress_rect = egui::Rect::from_min_size(
                    egui::pos2(content.min.x, top_y),
                    egui::vec2(content.width(), PANEL_PROGRESS_HIT_H),
                );
                let seek_resp = ui.interact(progress_rect, ui.make_persistent_id("bottom.seek"), egui::Sense::click_and_drag());
                let seek_active = has_duration && (seek_resp.hovered() || seek_resp.dragged());
                paint_bar(
                    ui.painter(),
                    progress_rect.min.x,
                    progress_rect.max.x,
                    top_y + 6.0,
                    fraction,
                    if seek_active { 5.0 } else { 3.0 },
                    seek_active,
                );
                if seek_active {
                    seek_resp.clone().on_hover_cursor(egui::CursorIcon::PointingHand);
                }
                let seek_clicked = has_duration && seek_resp.clicked();
                let seek_dragged = has_duration && seek_resp.dragged();
                if seek_clicked || seek_dragged {
                    if let Some(pointer) = seek_resp.interact_pointer_pos() {
                        let t = ((pointer.x - progress_rect.min.x) / progress_rect.width()).clamp(0.0, 1.0);
                        self.seek_drag = Some((t * dur.max(1) as f32) as u64);
                    }
                }
                let commit_seek = seek_clicked || (has_duration && seek_resp.drag_stopped());
                if commit_seek {
                    let target = self.seek_drag.unwrap_or(pos).min(dur);
                    self.commit_seek(target);
                }

                let zones_top = top_y + PANEL_PROGRESS_HIT_H + 2.0;
                let max_left_w = content.width() - PANEL_CENTER_W - PANEL_RIGHT_W - 2.0 * PANEL_ZONE_GAP;
                let left_w = ((content.width() * 0.30).max(150.0)).min(max_left_w).max(150.0);
                let zones_rect = egui::Rect::from_min_size(
                    egui::pos2(content.min.x, zones_top),
                    egui::vec2(content.width(), PANEL_ZONES_H),
                );
                let left_rect = egui::Rect::from_min_size(
                    egui::pos2(zones_rect.min.x, zones_top),
                    egui::vec2(left_w, PANEL_ZONES_H),
                );
                let right_rect = egui::Rect::from_min_size(
                    egui::pos2(zones_rect.max.x - PANEL_RIGHT_W, zones_top),
                    egui::vec2(PANEL_RIGHT_W, PANEL_ZONES_H),
                );
                let between = right_rect.min.x - left_rect.max.x;
                let center_x = left_rect.max.x + ((between - PANEL_CENTER_W) * 0.5).max(PANEL_ZONE_GAP);
                let center_rect = egui::Rect::from_min_size(
                    egui::pos2(center_x, zones_top),
                    egui::vec2(PANEL_CENTER_W, PANEL_ZONES_H),
                );

                let (title, artist, active) = if let Some(np) = &self.now_playing {
                    (np.title.clone(), np.artist.clone(), true)
                } else if let Some(t) = &self.restored_track {
                    (
                        format!("{} - {} (\u{0433}\u{043E}\u{0442}\u{043E}\u{0432})", t.artist, t.title),
                        String::new(),
                        false,
                    )
                } else {
                    (NOTHING_PLAYING_TEXT.to_string(), String::new(), false)
                };
                self.ensure_cjk_font(ui.ctx(), &title);
                self.ensure_cjk_font(ui.ctx(), &artist);
                let mut left_ui = ui.new_child(egui::UiBuilder::new().max_rect(left_rect).layout(egui::Layout::top_down(egui::Align::LEFT)));
                left_ui.set_min_height(PANEL_ZONES_H);
                left_ui.add_space(1.0);
                left_ui.add(
                    egui::Label::new(
                        egui::RichText::new(title)
                            .font(theme::semibold_font(14.0))
                            .color(if active { theme::TEXT } else { theme::MUTED }),
                    )
                    .selectable(false)
                    .truncate(),
                );
                left_ui.add(
                    egui::Label::new(egui::RichText::new(artist).size(12.0).color(theme::MUTED))
                        .selectable(false)
                        .truncate(),
                );

                let paused = self.now_playing.as_ref().map(|np| np.paused).unwrap_or(false);
                let play_icon = if paused || self.now_playing.is_none() { TransportIcon::Play } else { TransportIcon::Pause };
                let play_tooltip = if matches!(play_icon, TransportIcon::Play) { PLAY_TOOLTIP } else { PAUSE_TOOLTIP };
                let play_enabled = self.now_playing.is_some() || self.restored_track.is_some();
                let side_enabled = self.now_playing.is_some();
                let controls_cy = zones_top + 22.0;
                let controls_start = center_rect.center().x - 75.0;
                let prev_rect = egui::Rect::from_center_size(egui::pos2(controls_start + 14.0, controls_cy), egui::vec2(28.0, 28.0));
                let play_rect = egui::Rect::from_center_size(egui::pos2(controls_start + 58.0, controls_cy), egui::vec2(40.0, 40.0));
                let next_rect = egui::Rect::from_center_size(egui::pos2(controls_start + 102.0, controls_cy), egui::vec2(28.0, 28.0));
                let stop_rect = egui::Rect::from_center_size(egui::pos2(controls_start + 138.0, controls_cy), egui::vec2(24.0, 24.0));
                if transport_button(ui, prev_rect, "bottom.prev", TransportIcon::Prev, side_enabled, PREV_TOOLTIP) {
                    self.play_prev();
                }
                if transport_button(ui, play_rect, "bottom.play", play_icon, play_enabled, play_tooltip) {
                    self.toggle_play_pause();
                }
                if transport_button(ui, next_rect, "bottom.next", TransportIcon::Next, side_enabled, NEXT_TOOLTIP) {
                    self.play_next();
                }
                if transport_button(ui, stop_rect, "bottom.stop", TransportIcon::Stop, side_enabled, STOP_TOOLTIP) {
                    let _ = self.cmd_tx.send(PlayerCmd::Stop);
                }

                let time_rect = egui::Rect::from_min_size(
                    egui::pos2(center_rect.min.x, zones_top + 50.0),
                    egui::vec2(PANEL_CENTER_W, 20.0),
                );
                let dur_text = if has_duration { api::format_mm_ss(dur) } else { "--:--".to_string() };
                ui.painter().text(
                    time_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    format!("{} / {}", api::format_mm_ss(shown_pos), dur_text),
                    theme::mono_font(12.0),
                    theme::MUTED,
                );

                let speaker_rect = egui::Rect::from_center_size(
                    egui::pos2(right_rect.min.x + 14.0, controls_cy),
                    egui::vec2(28.0, 28.0),
                );
                let muted = self.volume <= 0.0;
                draw_speaker(ui.painter(), speaker_rect.center(), muted);
                let speaker_resp = ui.interact(speaker_rect, ui.make_persistent_id("bottom.mute"), egui::Sense::click())
                    .on_hover_text(if muted { UNMUTE_TOOLTIP } else { MUTE_TOOLTIP });
                if speaker_resp.hovered() {
                    speaker_resp.clone().on_hover_cursor(egui::CursorIcon::PointingHand);
                }
                if speaker_resp.clicked() {
                    let mut save_fallback = false;
                    if muted {
                        if let Some(previous) = self.volume_before_mute.take().filter(|&v| v > 0.0) {
                            self.volume = previous.clamp(0.0, 1.0);
                        } else {
                            self.volume = 0.5;
                            save_fallback = true;
                        }
                    } else {
                        self.volume_before_mute = Some(self.volume.clamp(0.0, 1.0)).filter(|&v| v > 0.0);
                        self.volume = 0.0;
                    }
                    let _ = self.cmd_tx.send(PlayerCmd::SetVolume(self.volume));
                    if save_fallback {
                        storage::save_volume(self.volume);
                    }
                }

                let vol_bar_rect = egui::Rect::from_min_size(
                    egui::pos2(right_rect.min.x + 34.0, zones_top + 18.0),
                    egui::vec2(130.0, 8.0),
                );
                let vol_resp = ui.interact(vol_bar_rect, ui.make_persistent_id("bottom.volume"), egui::Sense::click_and_drag());
                paint_bar(
                    ui.painter(),
                    vol_bar_rect.min.x,
                    vol_bar_rect.max.x,
                    zones_top + 22.0,
                    self.volume,
                    3.0,
                    vol_resp.hovered(),
                );
                if vol_resp.hovered() {
                    vol_resp.clone().on_hover_cursor(egui::CursorIcon::PointingHand);
                }
                if vol_resp.clicked() || vol_resp.dragged() {
                    if let Some(pointer) = vol_resp.interact_pointer_pos() {
                        let t = ((pointer.x - vol_bar_rect.min.x) / vol_bar_rect.width()).clamp(0.0, 1.0);
                        self.volume = t.clamp(0.0, 1.0);
                        self.volume_before_mute = None;
                        let _ = self.cmd_tx.send(PlayerCmd::SetVolume(self.volume));
                        storage::save_volume(self.volume);
                    }
                }
                let pct = (self.volume * 100.0).round().clamp(0.0, 100.0) as i32;
                let pct_rect = egui::Rect::from_min_size(
                    egui::pos2(right_rect.min.x + 170.0, zones_top + 10.0),
                    egui::vec2(46.0, 24.0),
                );
                ui.painter().text(
                    pct_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    format!("{pct}%"),
                    theme::mono_font(12.0),
                    theme::MUTED,
                );

                let pill_y = zones_top + 54.0;
                let pill_h = 22.0;
                let repeat_w = measure_label_width(ui, REPEAT_ALL_LABEL, theme::semibold_font(12.0)) + 20.0;
                let shuffle_w = measure_label_width(ui, SHUFFLE_LABEL, theme::semibold_font(12.0)) + 20.0;
                let pill_gap = 6.0;
                let pills_w = repeat_w + shuffle_w + pill_gap;
                let pills_start = right_rect.center().x - pills_w * 0.5;
                let repeat_rect = egui::Rect::from_min_size(
                    egui::pos2(pills_start, pill_y),
                    egui::vec2(repeat_w, pill_h),
                );
                let shuffle_rect = egui::Rect::from_min_size(
                    egui::pos2(pills_start + repeat_w + pill_gap, pill_y),
                    egui::vec2(shuffle_w, pill_h),
                );
                let repeat_label = match self.repeat_mode {
                    RepeatMode::Off => REPEAT_LABEL,
                    RepeatMode::All => REPEAT_ALL_LABEL,
                    RepeatMode::One => REPEAT_ONE_LABEL,
                };
                if pill_toggle(
                    ui,
                    repeat_rect,
                    "bottom.repeat",
                    repeat_label,
                    self.repeat_mode != RepeatMode::Off,
                    REPEAT_TOOLTIP,
                ) {
                    self.repeat_mode = self.repeat_mode.next();
                    storage::save_repeat(self.repeat_mode.as_str());
                }
                if pill_toggle(
                    ui,
                    shuffle_rect,
                    "bottom.shuffle",
                    SHUFFLE_LABEL,
                    self.shuffle,
                    SHUFFLE_TOOLTIP,
                ) {
                    self.toggle_shuffle();
                }

                let mut status_parts: Vec<String> = Vec::new();
                if !self.hotkey_status.is_empty() {
                    status_parts.push(self.hotkey_status.clone());
                }
                if !self.status.is_empty() {
                    let status = self.status.clone();
                    self.ensure_cjk_font(ui.ctx(), &status);
                    status_parts.push(status);
                }
                let status = status_parts.join(" | ");
                self.status_line(ui, &status, status_is_error(&status));
            });

        egui::CentralPanel::default().show(ui, |ui| {
            if !self.logged_in {
                self.login_ui(ui);
                return;
            }

            self.ensure_my_user_id();

            let (top_rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), TOP_BAR_H), egui::Sense::hover());
            ui.painter().rect_filled(top_rect, egui::CornerRadius::same(0), theme::PANEL);
            ui.painter().line_segment(
                [egui::pos2(top_rect.min.x, top_rect.max.y), egui::pos2(top_rect.max.x, top_rect.max.y)],
                egui::Stroke::new(1.0, theme::BORDER),
            );
            draw_brand_mark(
                ui.painter(),
                egui::Rect::from_min_size(
                    egui::pos2(top_rect.min.x + 12.0, top_rect.center().y - 10.0),
                    egui::vec2(20.0, 20.0),
                ),
            );

            let tab_font = theme::semibold_font(13.0);
            let tab_y = top_rect.min.y + 4.0;
            let tab_h = TOP_BAR_H - 12.0;
            let mut tab_x = top_rect.min.x + 46.0;
            let my_label = "\u{041C}\u{043E}\u{044F} \u{043C}\u{0443}\u{0437}\u{044B}\u{043A}\u{0430}";
            let playlists_label = "\u{041F}\u{043B}\u{0435}\u{0439}\u{043B}\u{0438}\u{0441}\u{0442}\u{044B}";
            let search_label = "\u{041F}\u{043E}\u{0438}\u{0441}\u{043A}";
            let rec_label = "\u{0420}\u{0435}\u{043A}\u{043E}\u{043C}\u{0435}\u{043D}\u{0434}\u{0430}\u{0446}\u{0438}\u{0438}";
            let labels = [my_label, playlists_label, search_label, rec_label];
            let tabs = [Tab::MyMusic, Tab::Playlists, Tab::Search, Tab::Recommendations];
            for (label, tab) in labels.iter().zip(tabs.iter()) {
                let w = measure_label_width(ui, label, tab_font.clone()) + 12.0;
                let rect = egui::Rect::from_min_size(egui::pos2(tab_x, tab_y), egui::vec2(w, tab_h));
                if text_tab(ui, rect, label, label, tab_font.clone(), self.active_tab == *tab) {
                    self.active_tab = *tab;
                }
                tab_x += w + TOP_TAB_GAP;
            }

            let mut logout_clicked = false;
            let menu_rect = egui::Rect::from_center_size(
                egui::pos2(top_rect.max.x - 20.0, top_rect.center().y),
                egui::vec2(TOP_MENU_SIZE, TOP_MENU_SIZE),
            );
            let menu_resp = ui.interact(menu_rect, ui.make_persistent_id("top.menu"), egui::Sense::click());
            draw_menu_dots(ui.painter(), menu_rect.center(), menu_resp.hovered());
            let menu_resp = menu_resp.on_hover_text(MENU_TOOLTIP);
            if menu_resp.hovered() {
                menu_resp.clone().on_hover_cursor(egui::CursorIcon::PointingHand);
            }
            egui::Popup::menu(&menu_resp)
                .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                .show(|ui| {
                    if ui.button(LOGOUT_LABEL).clicked() {
                        ui.close();
                        logout_clicked = true;
                    }
                });
            if logout_clicked {
                self.logout_to_login(None);
            }

            let row_h = TRACK_ROW_HEIGHT;
            match self.active_tab {
                Tab::MyMusic => {
                    let my_status_error = status_is_error(&self.my_status);
                    let my_status = self.my_status.clone();
                    self.status_line(ui, &my_status, my_status_error);
                    let filter_before = self.my_filter.clone();
                    let mut filter_rect = egui::Rect::NOTHING;
                    ui.horizontal(|ui| {
                        let field_w = (ui.available_width() - 120.0).max(160.0);
                        let (rect, _) = ui.allocate_exact_size(egui::vec2(field_w, 28.0), egui::Sense::hover());
                        filter_rect = rect;
                        let resp = styled_search_field(ui, rect, &mut self.my_filter, ui.scope_id().with("vkmusic.filter"), FILTER_HINT);
                        if !self.my_filter.is_empty() {
                            let clear_rect = egui::Rect::from_center_size(
                                egui::pos2(rect.max.x + 18.0, rect.center().y),
                                egui::vec2(24.0, 24.0),
                            );
                            let clear_resp = ui.interact(clear_rect, ui.make_persistent_id("filter.clear"), egui::Sense::click());
                            draw_clear_cross(ui.painter(), clear_rect.center());
                            if clear_resp.hovered() {
                                clear_resp.clone().on_hover_cursor(egui::CursorIcon::PointingHand);
                            }
                            if clear_resp.clicked() {
                                self.my_filter.clear();
                                resp.request_focus();
                            }
                        }
                    });
                    if self.my_filter != filter_before {
                        self.filter_autoload_stopped = false;
                        self.my_status.clear();
                        self.sync_shuffle_queue();
                    }
                    let filter_active = !self.my_filter.trim().is_empty();
                    let filtered = self.my_music_filtered();
                    let total = if filter_active { filtered.len() } else { self.tracks_count.unwrap_or(self.tracks.len() as u64) as usize };
                    let count_text = format!("{}{}", total, TRACKS_SUFFIX);
                    let count_w = measure_label_width(ui, &count_text, theme::mono_font(12.0));
                    let count_x = ui.available_rect_before_wrap().max.x - 12.0 - count_w;
                    ui.painter().text(
                        egui::pos2(count_x, filter_rect.center().y),
                        egui::Align2::LEFT_CENTER,
                        count_text,
                        theme::mono_font(12.0),
                        theme::MUTED,
                    );
                    let playing_tid = self.now_playing.as_ref().and_then(|np| np.track_id);
                    let list_rect = ui.available_rect_before_wrap();
                    let first_page = self.tracks.is_empty() && (self.loading_tracks || self.tracks_count.is_none());
                    let filter_no_matches = filter_active && filtered.is_empty();
                    let library_empty = !filter_active && self.tracks.is_empty();
                    if filter_no_matches && !self.loading_tracks && !my_status_error {
                        draw_empty_state(ui, list_rect, EMPTY_FILTER_MAIN, Some(EMPTY_FILTER_SUB));
                    } else if first_page && !my_status_error {
                        if !filter_active && !self.loading_tracks && total > self.tracks.len() { self.request_tracks_load(); }
                        draw_skeleton_rows(ui, list_rect, row_h);
                    } else if library_empty && !self.loading_tracks && !my_status_error {
                        draw_empty_state(ui, list_rect, EMPTY_MY_MUSIC_MAIN, Some(EMPTY_MY_MUSIC_SUB));
                    } else {
                        egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(ui, row_h, total, |ui, range| {
                            let width = ui.available_width();
                            for row in range.clone() {
                                let Some((i, track)) = (if filter_active { filtered.get(row).cloned() } else { self.tracks.get(row).cloned().map(|t| (row, t)) }) else {
                                    if !filter_active && !self.loading_tracks { self.request_tracks_load(); }
                                    continue;
                                };
                                let playing_here = playing_tid == Some(TrackId::from_track(&track));
                                self.render_track_row(ui, i, &track, width, row_h, QueueSource::MyMusic, playing_here);
                            }
                            if !filter_active && range.end + 10 >= self.tracks.len() && !self.loading_tracks { self.request_tracks_load(); }
                        });
                    }
                    if !self.tracks.is_empty() && self.loading_tracks {
                        self.status_line(ui, LOADING_MORE_TEXT, false);
                    }
                }
                Tab::Playlists => {
                    if let Some(pidx) = self.selected_playlist {
                        let pl_title = self.playlists.get(pidx).map(|p| p.title.clone()).unwrap_or_default();
                        ui.horizontal(|ui| {
                            if ui.button("\u{041D}\u{0430}\u{0437}\u{0430}\u{0434}").clicked() {
                                self.selected_playlist = None;
                                self.selected_index = None;
                                return;
                            }
                            ui.add(egui::Label::new(egui::RichText::new(pl_title).strong()).selectable(false));
                        });
                        if self.selected_playlist.is_some() {
                            let playlist_status_error = status_is_error(&self.playlist_status);
                            let playlist_status = self.playlist_status.clone();
                            self.status_line(ui, &playlist_status, playlist_status_error);
                            let total = self.playlist_tracks_count.unwrap_or(self.playlist_tracks.len() as u64) as usize;
                            let playing_tid = self.now_playing.as_ref().and_then(|np| np.track_id);
                            let list_rect = ui.available_rect_before_wrap();
                            let first_page = self.playlist_tracks.is_empty()
                                && (self.loading_playlist_tracks || self.playlist_tracks_count.is_none());
                            if first_page && !playlist_status_error {
                                draw_skeleton_rows(ui, list_rect, row_h);
                            } else {
                                egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(ui, row_h, total, |ui, range| {
                                    let width = ui.available_width();
                                    for i in range.clone() {
                                        if i >= self.playlist_tracks.len() {
                                            if !self.loading_playlist_tracks { self.request_playlist_tracks_load(); }
                                            continue;
                                        }
                                        let track = self.playlist_tracks[i].clone();
                                        let playing_here = playing_tid == Some(TrackId::from_track(&track));
                                        self.render_track_row(ui, i, &track, width, row_h, QueueSource::Playlist, playing_here);
                                    }
                                    if range.end + 10 >= self.playlist_tracks.len() && !self.loading_playlist_tracks { self.request_playlist_tracks_load(); }
                                });
                            }
                            if !self.playlist_tracks.is_empty() && self.loading_playlist_tracks {
                                self.status_line(ui, LOADING_MORE_TEXT, false);
                            }
                        }
                    } else {
                        if self.playlists.is_empty() && !self.loading_playlists && self.playlists_count.is_none() {
                            self.request_playlists_load();
                        }
                        let playlists_status_error = status_is_error(&self.playlists_status);
                        let playlists_status = self.playlists_status.clone();
                        self.status_line(ui, &playlists_status, playlists_status_error);
                        let total = self.playlists_count.unwrap_or(self.playlists.len() as u64) as usize;
                        let list_rect = ui.available_rect_before_wrap();
                        let first_page = self.playlists.is_empty()
                            && (self.loading_playlists || self.playlists_count.is_none());
                        if first_page && !playlists_status_error {
                            draw_skeleton_rows(ui, list_rect, row_h);
                        } else if self.playlists.is_empty() && !self.loading_playlists && !playlists_status_error {
                            draw_empty_state(ui, list_rect, EMPTY_PLAYLISTS_MAIN, Some(EMPTY_PLAYLISTS_SUB));
                        } else {
                            egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(ui, row_h, total, |ui, range| {
                                let width = ui.available_width();
                                for i in range.clone() {
                                    if i >= self.playlists.len() {
                                        if !self.loading_playlists { self.request_playlists_load(); }
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
                        if !self.playlists.is_empty() && self.loading_playlists {
                            self.status_line(ui, LOADING_MORE_TEXT, false);
                        }
                    }
                }
                Tab::Search => {
                    ui.horizontal(|ui| {
                        let button_w = 84.0;
                        let field_w = (ui.available_width() - button_w - 12.0).max(160.0);
                        let (field_rect, _) = ui.allocate_exact_size(egui::vec2(field_w, 30.0), egui::Sense::hover());
                        let resp = styled_search_field(ui, field_rect, &mut self.search_input, ui.scope_id().with("vkmusic.search"), FILTER_HINT);
                        let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                        let (button_rect, _) = ui.allocate_exact_size(egui::vec2(button_w, 30.0), egui::Sense::hover());
                        let clicked = ghost_pill_button(ui, button_rect, "search.find", "\u{041D}\u{0430}\u{0439}\u{0442}\u{0438}");
                        if clicked || enter {
                            self.start_search();
                        }
                    });
                    let search_status_error = status_is_error(&self.search_status);
                    let search_status = self.search_status.clone();
                    self.status_line(ui, &search_status, search_status_error);
                    let total = self.search_count.unwrap_or(self.search_tracks.len() as u64) as usize;
                    let playing_tid = self.now_playing.as_ref().and_then(|np| np.track_id);
                    let list_rect = ui.available_rect_before_wrap();
                    let query_empty = self.search_input.trim().is_empty() && self.search_query.trim().is_empty();
                    let first_page = !query_empty && self.search_tracks.is_empty()
                        && (self.loading_search || self.search_count.is_none());
                    let no_results = !query_empty && self.search_tracks.is_empty() && !self.loading_search
                        && self.search_count.is_some() && !search_status_error;
                    if query_empty && self.search_tracks.is_empty() && !self.loading_search && !search_status_error {
                        draw_empty_state(ui, list_rect, EMPTY_SEARCH_IDLE_MAIN, Some(EMPTY_SEARCH_IDLE_SUB));
                    } else if first_page && !search_status_error {
                        if !self.loading_search && !self.search_query.trim().is_empty() && total > self.search_tracks.len() { self.request_search_load(); }
                        draw_skeleton_rows(ui, list_rect, row_h);
                    } else if no_results {
                        draw_empty_state(ui, list_rect, EMPTY_SEARCH_NONE_MAIN, Some(EMPTY_SEARCH_NONE_SUB));
                    } else {
                        egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(ui, row_h, total, |ui, range| {
                            let width = ui.available_width();
                            for i in range.clone() {
                                if i >= self.search_tracks.len() {
                                    if !self.loading_search && !self.search_query.trim().is_empty() { self.request_search_load(); }
                                    continue;
                                }
                                let track = self.search_tracks[i].clone();
                                let playing_here = playing_tid == Some(TrackId::from_track(&track));
                                self.render_track_row(ui, i, &track, width, row_h, QueueSource::Search, playing_here);
                            }
                            if range.end + 10 >= self.search_tracks.len() && !self.loading_search && !self.search_query.trim().is_empty() { self.request_search_load(); }
                        });
                    }
                    if !self.search_tracks.is_empty() && self.loading_search {
                        self.status_line(ui, LOADING_MORE_TEXT, false);
                    }
                }
                Tab::Recommendations => {
                    if !self.rec_loaded_once && !self.rec_loading {
                        self.request_recommendations(false);
                    }
                    ui.horizontal(|ui| {
                        let (button_rect, _) = ui.allocate_exact_size(egui::vec2(84.0, 30.0), egui::Sense::hover());
                        let clicked = ghost_pill_button(ui, button_rect, "rec.refresh", "\u{041E}\u{0431}\u{043D}\u{043E}\u{0432}\u{0438}\u{0442}\u{044C}");
                        if clicked && !self.rec_loading {
                            self.request_recommendations(true);
                        }
                    });
                    let rec_status_error = status_is_error(&self.rec_status);
                    let rec_no_more_empty = self.rec_no_more
                        && self.rec_tracks.is_empty()
                        && !self.rec_loading
                        && !rec_status_error;
                    if !rec_no_more_empty {
                        let rec_status = self.rec_status.clone();
                        self.status_line(ui, &rec_status, rec_status_error);
                    }
                    let playing_tid = self.now_playing.as_ref().and_then(|np| np.track_id);
                    let list_rect = ui.available_rect_before_wrap();
                    let first_page = !self.rec_loaded_once && self.rec_tracks.is_empty();
                    if first_page && !rec_status_error {
                        draw_skeleton_rows(ui, list_rect, row_h);
                    } else if rec_no_more_empty {
                        draw_empty_state(ui, list_rect, EMPTY_REC_NO_MORE_MAIN, Some(EMPTY_REC_NO_MORE_SUB));
                    } else {
                        let total = self.rec_tracks.len();
                        egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(ui, row_h, total, |ui, range| {
                            let width = ui.available_width();
                            for i in range.clone() {
                                let track = self.rec_tracks[i].clone();
                                let playing_here = playing_tid == Some(TrackId::from_track(&track));
                                self.render_track_row(ui, i, &track, width, row_h, QueueSource::Recommendations, playing_here);
                            }
                            if self.rec_loaded_once && !self.rec_no_more && !self.rec_loading && range.end + 5 >= self.rec_tracks.len() {
                                self.request_recommendations_more();
                            }
                        });
                    }
                    if !self.rec_tracks.is_empty() && self.rec_loading {
                        self.status_line(ui, LOADING_MORE_TEXT, false);
                    }
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tid(id: i64) -> TrackId {
        TrackId { owner_id: 1, id }
    }

    #[test]
    fn never_picks_played_track() {
        let candidates = [tid(1), tid(2), tid(3)];
        let played: HashSet<TrackId> = [tid(1), tid(2)].into_iter().collect();
        let mut rng = Xorshift64::from_seed(0x1234_5678_9ABC_DEF0);
        for _ in 0..32 {
            assert_eq!(pick_shuffle_id(&candidates, &played, &mut rng, None), Some(tid(3)));
        }
    }

    #[test]
    fn returns_none_when_all_played() {
        let candidates = [tid(1), tid(2)];
        let played: HashSet<TrackId> = candidates.iter().copied().collect();
        let mut rng = Xorshift64::from_seed(0x1234_5678_9ABC_DEF0);
        assert_eq!(pick_shuffle_id(&candidates, &played, &mut rng, None), None);
    }

    #[test]
    fn skips_excluded_current_when_another_exists() {
        let candidates = [tid(1), tid(2)];
        let played = HashSet::new();
        let mut rng = Xorshift64::from_seed(0x1234_5678_9ABC_DEF0);
        for _ in 0..32 {
            let picked = pick_shuffle_id(&candidates, &played, &mut rng, Some(tid(1)));
            assert!(picked.is_some());
            assert_ne!(picked, Some(tid(1)));
        }
    }

    #[test]
    fn works_with_single_track() {
        let candidates = [tid(7)];
        let played = HashSet::new();
        let mut rng = Xorshift64::from_seed(0x1234_5678_9ABC_DEF0);
        assert_eq!(pick_shuffle_id(&candidates, &played, &mut rng, None), Some(tid(7)));
        assert_eq!(pick_shuffle_id(&candidates, &played, &mut rng, Some(tid(7))), None);
    }
}
