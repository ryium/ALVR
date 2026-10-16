use egui::{self, Color32, Context, CornerRadius, Stroke, TextStyle, ThemePreference, Visuals};

pub const ACCENT: Color32 = Color32::from_rgb(39, 146, 229);
pub const ACCENT_LIGHT: Color32 = Color32::from_rgb(91, 188, 255);
pub const ACCENT_BG: Color32 = Color32::from_rgb(35, 58, 79);
pub const BG: Color32 = Color32::from_rgb(32, 33, 39);
pub const LIGHTER_BG: Color32 = Color32::from_rgb(25, 26, 31);
pub const SECTION_BG: Color32 = Color32::from_rgb(40, 42, 49);
pub const CARD_HOVER_BG: Color32 = Color32::from_rgb(49, 54, 64);
pub const DARKER_BG: Color32 = Color32::from_rgb(23, 24, 29);
pub const SEPARATOR_BG: Color32 = Color32::from_rgb(58, 61, 71);
pub const FG: Color32 = Color32::from_rgb(239, 241, 246);
pub const SUBTEXT: Color32 = Color32::from_rgb(147, 153, 168);
pub const SCROLLBAR_DOT_DIAMETER: f32 = 20.0;
pub const SWITCH_DOT_DIAMETER: f32 = SCROLLBAR_DOT_DIAMETER;
pub const FRAME_PADDING: f32 = 12.0;
pub const CORNER_RADIUS: u8 = 10;
pub const FRAME_TEXT_SPACING: f32 = 6.0;

pub const OK_GREEN: Color32 = Color32::from_rgb(78, 203, 153);
pub const KO_RED: Color32 = Color32::from_rgb(255, 77, 85);
pub const WARN_YELLOW: Color32 = Color32::from_rgb(255, 184, 0);

pub mod log_colors {
    use egui::epaint::Color32;

    pub const ERROR_LIGHT: Color32 = Color32::from_rgb(255, 80, 80);
    pub const WARNING_LIGHT: Color32 = Color32::from_rgb(255, 184, 0);
    pub const INFO_LIGHT: Color32 = Color32::from_rgb(0, 163, 255);
    pub const DEBUG_LIGHT: Color32 = Color32::from_rgb(140, 158, 182);
    pub const EVENT_LIGHT: Color32 = Color32::from_rgb(100, 116, 139);
}

// Graph colors
pub mod graph_colors {
    use egui::Color32;

    pub const RENDER_EXTERNAL: Color32 = Color32::from_rgb(50, 65, 85);
    pub const RENDER_EXTERNAL_LABEL: Color32 = Color32::from_rgb(140, 158, 182);
    pub const RENDER: Color32 = Color32::from_rgb(255, 77, 85);
    pub const IDLE: Color32 = Color32::from_rgb(255, 184, 0);
    pub const TRANSCODE: Color32 = Color32::from_rgb(0, 230, 153);
    pub const NETWORK: Color32 = Color32::from_rgb(0, 163, 255);

    pub const SERVER_FPS: Color32 = Color32::from_rgb(56, 189, 248);
    pub const CLIENT_FPS: Color32 = Color32::from_rgb(0, 230, 153);

    pub const INITIAL_CALCULATED_THROUGHPUT: Color32 = Color32::GRAY;
    pub const ENCODER_DECODER_LATENCY_LIMITER: Color32 = TRANSCODE;
    pub const NETWORK_LATENCY_LIMITER: Color32 = NETWORK;
    pub const MIN_MAX_LATENCY_THROUGHPUT: Color32 = Color32::from_rgb(255, 77, 85);
    pub const REQUESTED_BITRATE: Color32 = Color32::from_rgb(0, 230, 153);
    pub const RECORDED_THROUGHPUT: Color32 = Color32::from_rgb(255, 184, 0);
    pub const RECORDED_BITRATE: Color32 = super::FG;
}

pub fn set_theme(ctx: &Context) {
    ctx.set_theme(ThemePreference::Dark);

    let mut style = (*ctx.global_style()).clone();
    style.spacing.slider_width = 220_f32; // slider width can only be set globally
    style.spacing.interact_size.x = 36.0;
    style.spacing.interact_size.y = 36.0;

    style.spacing.item_spacing = egui::vec2(12.0, 12.0);
    style.spacing.button_padding = egui::vec2(12.0, 8.0);
    style.spacing.window_margin = egui::Margin::from(FRAME_PADDING);

    style.text_styles.get_mut(&TextStyle::Body).unwrap().size = 14.0;
    style.interaction.tooltip_delay = 0.0;

    ctx.set_global_style(style);

    let mut visuals = Visuals::dark();

    let corner_radius = CornerRadius::same(CORNER_RADIUS);

    visuals.override_text_color = Some(FG);

    // Active (pressed)
    visuals.widgets.active.bg_fill = ACCENT;
    visuals.widgets.active.fg_stroke = Stroke::new(1.0, Color32::WHITE);
    visuals.widgets.active.bg_stroke = Stroke::new(1.0, ACCENT_LIGHT);
    visuals.widgets.active.corner_radius = corner_radius;

    // Inactive (normal)
    visuals.widgets.inactive.bg_fill = SECTION_BG;
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, FG);
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, SEPARATOR_BG);
    visuals.widgets.inactive.corner_radius = corner_radius;

    // Hovered
    visuals.widgets.hovered.bg_fill = CARD_HOVER_BG;
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, Color32::WHITE);
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, ACCENT);
    visuals.widgets.hovered.corner_radius = corner_radius;

    // Open dropdowns/menus
    visuals.widgets.open.bg_fill = SECTION_BG;
    visuals.widgets.open.bg_stroke = Stroke::new(1.0, ACCENT);
    visuals.widgets.open.corner_radius = corner_radius;

    // Selection
    visuals.selection.bg_fill = ACCENT_BG;
    visuals.selection.stroke = Stroke::new(1.5, ACCENT);

    visuals.faint_bg_color = DARKER_BG;

    visuals.widgets.noninteractive.bg_fill = BG;
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, FG);
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, SEPARATOR_BG);
    visuals.widgets.noninteractive.corner_radius = CornerRadius::same(CORNER_RADIUS + 2);

    ctx.set_visuals(visuals);
}
