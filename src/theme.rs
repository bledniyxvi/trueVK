use eframe::egui;
use std::path::Path;
use std::sync::Arc;

pub const APP_NAME: &str = "vkmusic";

pub const BG: egui::Color32 = egui::Color32::from_rgb(0x0B, 0x0B, 0x0F);
pub const PANEL: egui::Color32 = egui::Color32::from_rgb(0x10, 0x10, 0x15);
pub const SURFACE: egui::Color32 = egui::Color32::from_rgb(0x16, 0x16, 0x1D);
pub const HOVER: egui::Color32 = egui::Color32::from_rgb(0x1C, 0x1C, 0x25);
pub const SELECTED: egui::Color32 = egui::Color32::from_rgb(0x24, 0x12, 0x19);
pub const BORDER: egui::Color32 = egui::Color32::from_rgb(0x23, 0x23, 0x2D);
pub const TEXT: egui::Color32 = egui::Color32::from_rgb(0xEC, 0xE9, 0xE6);
pub const MUTED: egui::Color32 = egui::Color32::from_rgb(0x7C, 0x7C, 0x88);
pub const ACCENT: egui::Color32 = egui::Color32::from_rgb(0xFF, 0x33, 0x55);
pub const ACCENT_DIM: egui::Color32 = egui::Color32::from_rgb(0x5A, 0x16, 0x26);
pub const WARN: egui::Color32 = egui::Color32::from_rgb(0xFF, 0xB0, 0x20);

pub fn apply(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.dark_mode = true;
    visuals.override_text_color = None;
    visuals.weak_text_color = Some(MUTED);
    visuals.hyperlink_color = ACCENT;
    visuals.warn_fg_color = WARN;
    visuals.error_fg_color = WARN;
    visuals.faint_bg_color = SURFACE;
    visuals.extreme_bg_color = SURFACE;
    visuals.text_edit_bg_color = Some(SURFACE);
    visuals.code_bg_color = SURFACE;
    visuals.window_fill = BG;
    visuals.window_stroke = egui::Stroke::new(1.0, BORDER);
    visuals.window_corner_radius = egui::CornerRadius::same(6);
    visuals.menu_corner_radius = egui::CornerRadius::same(6);
    visuals.panel_fill = PANEL;
    visuals.selection.bg_fill = SELECTED;
    visuals.selection.stroke = egui::Stroke::new(1.0, ACCENT);
    let text_stroke = egui::Stroke::new(1.0, TEXT);
    visuals.widgets.noninteractive.weak_bg_fill = SURFACE;
    visuals.widgets.noninteractive.bg_fill = PANEL;
    visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, BORDER);
    visuals.widgets.noninteractive.fg_stroke = text_stroke;
    visuals.widgets.noninteractive.corner_radius = egui::CornerRadius::same(6);
    visuals.widgets.inactive.weak_bg_fill = SURFACE;
    visuals.widgets.inactive.bg_fill = HOVER;
    visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, BORDER);
    visuals.widgets.inactive.fg_stroke = text_stroke;
    visuals.widgets.inactive.corner_radius = egui::CornerRadius::same(6);
    visuals.widgets.hovered.weak_bg_fill = HOVER;
    visuals.widgets.hovered.bg_fill = HOVER;
    visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, ACCENT_DIM);
    visuals.widgets.hovered.fg_stroke = text_stroke;
    visuals.widgets.hovered.corner_radius = egui::CornerRadius::same(6);
    visuals.widgets.active.weak_bg_fill = ACCENT_DIM;
    visuals.widgets.active.bg_fill = ACCENT_DIM;
    visuals.widgets.active.bg_stroke = egui::Stroke::new(1.0, ACCENT);
    visuals.widgets.active.fg_stroke = text_stroke;
    visuals.widgets.active.corner_radius = egui::CornerRadius::same(6);
    visuals.widgets.open.weak_bg_fill = SURFACE;
    visuals.widgets.open.bg_fill = SURFACE;
    visuals.widgets.open.bg_stroke = egui::Stroke::new(1.0, BORDER);
    visuals.widgets.open.fg_stroke = text_stroke;
    visuals.widgets.open.corner_radius = egui::CornerRadius::same(6);
    ctx.set_visuals_of(egui::Theme::Dark, visuals);
    ctx.set_theme(egui::ThemePreference::Dark);
    ctx.all_styles_mut(|style| {
        style.spacing.item_spacing = egui::vec2(10.0, 5.0);
        style.spacing.window_margin = egui::Margin::same(10);
        style.spacing.button_padding = egui::vec2(10.0, 4.0);
        let mut scroll = egui::style::ScrollStyle::thin();
        scroll.bar_width = 6.0;
        scroll.floating_width = 4.0;
        scroll.floating_allocated_width = 4.0;
        scroll.foreground_color = false;
        style.spacing.scroll = scroll;
    });
}

pub fn font_definitions(cjk: Option<(Vec<u8>, u32)>) -> egui::FontDefinitions {
    let mut fonts = egui::FontDefinitions::default();
    if let Ok(bytes) = std::fs::read(r"C:\Windows\Fonts\segoeui.ttf") {
        fonts.font_data.insert("SegoeUI".to_owned(), Arc::new(egui::FontData::from_owned(bytes)));
        for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
            if let Some(names) = fonts.families.get_mut(&family) {
                names.retain(|name| name != "SegoeUI");
                names.insert(0, "SegoeUI".to_owned());
            }
        }
    }
    if let Some((bytes, index)) = cjk {
        fonts.font_data.insert("CJK".to_owned(), Arc::new(egui::FontData::from_blob(Arc::new(bytes), index)));
        if let Some(names) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
            names.retain(|name| name != "CJK");
            names.push("CJK".to_owned());
        }
    }
    fonts
}

pub fn needs_cjk(text: &str) -> bool {
    text.chars().any(|c| {
        matches!(
            c,
            '\u{3040}'..='\u{30FF}' | '\u{3400}'..='\u{9FFF}' | '\u{AC00}'..='\u{D7AF}'
        )
    })
}

pub fn cjk_font_bytes() -> Option<(Vec<u8>, u32)> {
    for name in ["msyh.ttc", "YuGothM.ttc", "malgun.ttf"] {
        let path = Path::new(r"C:\Windows\Fonts").join(name);
        if let Ok(bytes) = std::fs::read(path) {
            return Some((bytes, 0));
        }
    }
    None
}
