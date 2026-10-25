//! Visual identity: palette, type scale and egui style, modelled on macOS 26 in dark mode.
//!
//! Charcoal layers rather than pure black, Apple's system colours for meaning, one accent (Elite
//! orange, which is also Apple's system orange), hairline separators instead of boxes, and a clear
//! type scale set in Inter (bundled; the open typeface closest to Apple's SF Pro).

use egui::{Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Shadow, Stroke, TextStyle};
use std::sync::Arc;

pub mod pal {
    use egui::Color32;

    // Surfaces, back to front.
    /// Window background (content area).
    pub const WINDOW: Color32 = Color32::from_rgb(28, 28, 30);
    /// The floating sidebar.
    pub const SIDEBAR: Color32 = Color32::from_rgb(40, 40, 43);
    /// Cards, grouped lists, popovers.
    pub const CARD: Color32 = Color32::from_rgb(44, 44, 46);
    /// Controls on a card (buttons, segmented control track), hover.
    pub const CARD_HI: Color32 = Color32::from_rgb(58, 58, 60);
    /// Pressed controls, the selected segment.
    pub const CONTROL: Color32 = Color32::from_rgb(78, 78, 82);
    /// Text fields and wells.
    pub const FIELD: Color32 = Color32::from_rgb(33, 33, 36);
    /// Hairlines between rows and around fields.
    pub const SEPARATOR: Color32 = Color32::from_rgb(60, 60, 63);

    // Older names, kept so every page shares one palette.
    pub const BORDER: Color32 = SEPARATOR;

    // Labels: primary, secondary, tertiary.
    pub const TEXT: Color32 = Color32::from_rgb(245, 245, 247);
    pub const DIM: Color32 = Color32::from_rgb(152, 152, 157);
    pub const FAINT: Color32 = Color32::from_rgb(110, 110, 115);

    // Accent: Apple's system orange, close to Elite's HUD orange.
    pub const ACCENT: Color32 = Color32::from_rgb(255, 159, 10);
    pub const ACCENT_SOFT: Color32 = Color32::from_rgb(255, 179, 64);
    pub const ACCENT_BG: Color32 = Color32::from_rgb(74, 52, 20);
    /// Text on an accent fill.
    pub const ON_ACCENT: Color32 = Color32::from_rgb(32, 20, 2);

    // Apple system colours (dark appearance).
    pub const BLUE: Color32 = Color32::from_rgb(10, 132, 255);
    pub const CYAN: Color32 = Color32::from_rgb(100, 210, 255);
    pub const TEAL: Color32 = Color32::from_rgb(64, 200, 224);
    pub const MINT: Color32 = Color32::from_rgb(102, 212, 207);
    pub const GOOD: Color32 = Color32::from_rgb(48, 209, 88);
    pub const WARN: Color32 = Color32::from_rgb(255, 214, 10);
    pub const BAD: Color32 = Color32::from_rgb(255, 69, 58);
    pub const PINK: Color32 = Color32::from_rgb(255, 55, 95);
    pub const PURPLE: Color32 = Color32::from_rgb(191, 90, 242);
    pub const GOLD: Color32 = Color32::from_rgb(255, 204, 0);
    pub const GRAY: Color32 = Color32::from_rgb(142, 142, 147);
    /// Apple's indigo, in its lighter accessible tone: fleet carriers.
    pub const INDIGO: Color32 = Color32::from_rgb(125, 122, 255);

    // The overlay's island: pure black, white text, and opaque equivalents of Apple's translucent greys on black.
    pub const ISLAND: Color32 = Color32::BLACK;
    /// Secondary text on the island (62 % white).
    pub const ISLAND_DIM: Color32 = Color32::from_rgb(146, 146, 152);
    /// The hairline around the island, over whatever is behind it.
    pub const ISLAND_HAIR: Color32 = Color32::from_rgba_premultiplied(23, 23, 23, 23);
    /// A radar dial's face, its rings, and a progress ring's track.
    pub const ISLAND_DIAL: Color32 = Color32::from_rgb(28, 28, 30);
    pub const ISLAND_LINE: Color32 = Color32::from_rgb(58, 58, 60);
    pub const ISLAND_TRACK: Color32 = Color32::from_rgb(41, 41, 42);
}

/// Semibold: headings, numbers, emphasis.
pub fn display_family() -> FontFamily {
    FontFamily::Name("semibold".into())
}

/// Semibold text of `size` (headings, figures, emphasis).
pub fn display(size: f32) -> FontId {
    FontId::new(size, display_family())
}

/// Bold text of `size` (large titles).
pub fn bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("bold".into()))
}

/// Medium text of `size` (the overlay's secondary lines).
pub fn medium(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("medium".into()))
}

/// Phosphor's filled icons at `size` (the overlay's glyphs; `egui_phosphor::fill` names them).
pub fn icon_fill(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(ICON_FILL.into()))
}

/// The font family of Phosphor's filled icons.
const ICON_FILL: &str = "phosphor-fill";

/// Type scale (points), after Apple's text styles, sized for reading at arm's length on a desktop
/// monitor. Nothing in the app is smaller than `CAPTION`.
pub mod size {
    pub const LARGE_TITLE: f32 = 30.0;
    pub const TITLE: f32 = 24.0;
    pub const TITLE3: f32 = 19.0;
    pub const HEADLINE: f32 = 15.5;
    pub const BODY: f32 = 14.5;
    pub const CALLOUT: f32 = 14.0;
    pub const FOOTNOTE: f32 = 13.0;
    pub const CAPTION: f32 = 12.0;
}

/// The first font file found among `names`, in the user's or the system's font folder.
fn find_font(names: &[&str]) -> Option<Vec<u8>> {
    let mut dirs = Vec::new();
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        dirs.push(std::path::PathBuf::from(local).join("Microsoft").join("Windows").join("Fonts"));
    }
    if let Some(w) = std::env::var_os("WINDIR") {
        dirs.push(std::path::PathBuf::from(w).join("Fonts"));
    }
    names.iter().find_map(|n| dirs.iter().find_map(|d| std::fs::read(d.join(n)).ok()))
}

/// Inter (SIL OFL 1.1, data/fonts/inter): the closest open match to Apple's SF Pro, compiled in so
/// every PC gets the same type.
const INTER_REGULAR: &[u8] = include_bytes!("../../../data/fonts/inter/Inter-Regular.ttf");
const INTER_MEDIUM: &[u8] = include_bytes!("../../../data/fonts/inter/Inter-Medium.ttf");
const INTER_SEMIBOLD: &[u8] = include_bytes!("../../../data/fonts/inter/Inter-SemiBold.ttf");
const INTER_BOLD: &[u8] = include_bytes!("../../../data/fonts/inter/Inter-Bold.ttf");
/// Monospace from the system (Cascadia Mono on Windows 11, Consolas everywhere).
const MONO: &[&str] = &["CascadiaMono.ttf", "consola.ttf"];

pub fn install(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);

    fonts.font_data.insert("ui".into(), Arc::new(FontData::from_static(INTER_REGULAR)));
    if let Some(f) = fonts.families.get_mut(&FontFamily::Proportional) {
        // Inter first for text. Its private-use mappings were removed (tools/fonts), so icon code
        // points fall through to Phosphor.
        f.insert(0, "ui".into());
    }
    // Segoe UI as a fallback for glyphs Inter lacks, then emoji.
    if let Some(bytes) = find_font(&["segoeui.ttf"]) {
        fonts.font_data.insert("segoe".into(), Arc::new(FontData::from_owned(bytes)));
        if let Some(f) = fonts.families.get_mut(&FontFamily::Proportional) {
            f.push("segoe".into());
        }
    }
    if let Some(bytes) = find_font(&["seguiemj.ttf"]) {
        fonts.font_data.insert("emoji".into(), Arc::new(FontData::from_owned(bytes)));
        if let Some(f) = fonts.families.get_mut(&FontFamily::Proportional) {
            f.push("emoji".into());
        }
    }
    if let Some(bytes) = find_font(MONO) {
        fonts.font_data.insert("mono".into(), Arc::new(FontData::from_owned(bytes)));
        if let Some(f) = fonts.families.get_mut(&FontFamily::Monospace) {
            f.insert(0, "mono".into());
        }
    }
    // Each weight falls back to the regular stack (icons, emoji, missing glyphs).
    let proportional = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    for (family, bytes) in [("medium", INTER_MEDIUM), ("semibold", INTER_SEMIBOLD), ("bold", INTER_BOLD)] {
        fonts.font_data.insert(family.into(), Arc::new(FontData::from_static(bytes)));
        let mut stack = vec![family.to_string()];
        stack.extend(proportional.iter().cloned());
        fonts.families.insert(FontFamily::Name(family.into()), stack);
    }
    // The filled icons share the regular set's code points, so they get a family of their own instead of joining the
    // text's fallbacks (Inter first: it answers for letters, the icons fall through to Phosphor).
    egui_phosphor::add_font_bytes_as_family(&mut fonts, ICON_FILL, egui_phosphor::Variant::Fill.font_bytes());
    ctx.set_fonts(fonts);
    // A dark native title bar, so the window frame matches the app.
    ctx.send_viewport_cmd(egui::ViewportCommand::SetTheme(egui::SystemTheme::Dark));

    ctx.all_styles_mut(|style| {
        style.text_styles = [
            (TextStyle::Heading, display(size::TITLE3)),
            (TextStyle::Body, FontId::proportional(size::BODY)),
            (TextStyle::Button, FontId::proportional(size::CALLOUT)),
            (TextStyle::Small, FontId::proportional(size::FOOTNOTE)),
            (TextStyle::Monospace, FontId::monospace(13.0)),
        ]
        .into();
        let sp = &mut style.spacing;
        sp.item_spacing = egui::vec2(8.0, 8.0);
        sp.button_padding = egui::vec2(12.0, 5.0);
        sp.interact_size = egui::vec2(40.0, 28.0);
        sp.window_margin = Margin::same(16);
        sp.menu_margin = Margin::same(6);
        sp.indent = 18.0;
        sp.icon_width = 16.0;
        sp.icon_width_inner = 10.0;
        sp.icon_spacing = 8.0;
        sp.combo_height = 340.0;
        sp.slider_width = 160.0;
        sp.slider_rail_height = 4.0;
        sp.text_edit_width = 240.0;
        sp.scroll = egui::style::ScrollStyle::floating();
        sp.scroll.bar_width = 7.0;
        sp.scroll.floating_allocated_width = 0.0;

        let v = &mut style.visuals;
        *v = egui::Visuals::dark();
        v.dark_mode = true;
        v.panel_fill = pal::WINDOW;
        v.window_fill = Color32::from_rgb(40, 40, 43);
        v.extreme_bg_color = pal::FIELD;
        v.text_edit_bg_color = Some(pal::FIELD);
        v.faint_bg_color = Color32::from_white_alpha(5);
        v.code_bg_color = pal::FIELD;
        v.hyperlink_color = pal::BLUE;
        v.warn_fg_color = pal::WARN;
        v.error_fg_color = pal::BAD;
        v.window_corner_radius = CornerRadius::same(14);
        v.menu_corner_radius = CornerRadius::same(10);
        v.window_stroke = Stroke::new(1.0, Color32::from_white_alpha(18));
        v.window_shadow = Shadow { offset: [0, 14], blur: 44, spread: 0, color: Color32::from_black_alpha(140) };
        v.popup_shadow = Shadow { offset: [0, 10], blur: 30, spread: 0, color: Color32::from_black_alpha(130) };
        v.selection.bg_fill = Color32::from_rgb(110, 72, 16);
        v.selection.stroke = Stroke::new(1.5, pal::ACCENT);
        v.text_cursor.stroke = Stroke::new(2.0, pal::ACCENT);
        v.override_text_color = None;
        v.striped = false;
        v.slider_trailing_fill = true;
        v.handle_shape = egui::style::HandleShape::Circle;
        v.collapsing_header_frame = false;
        v.indent_has_left_vline = false;
        v.interact_cursor = None;

        let w = &mut v.widgets;
        let r = CornerRadius::same(8);
        // Static surfaces: separators and frames use this stroke, labels this text colour.
        w.noninteractive.bg_fill = pal::CARD;
        w.noninteractive.weak_bg_fill = pal::CARD;
        w.noninteractive.bg_stroke = Stroke::new(1.0, pal::SEPARATOR);
        w.noninteractive.fg_stroke = Stroke::new(1.0, pal::TEXT);
        w.noninteractive.corner_radius = r;
        // Buttons, combo boxes, slider rails, checkboxes.
        w.inactive.bg_fill = Color32::from_rgb(72, 72, 76);
        w.inactive.weak_bg_fill = pal::CARD_HI;
        w.inactive.bg_stroke = Stroke::NONE;
        w.inactive.fg_stroke = Stroke::new(1.0, pal::TEXT);
        w.inactive.corner_radius = r;
        w.inactive.expansion = 0.0;
        w.hovered.bg_fill = Color32::from_rgb(86, 86, 90);
        w.hovered.weak_bg_fill = Color32::from_rgb(68, 68, 71);
        w.hovered.bg_stroke = Stroke::NONE;
        w.hovered.fg_stroke = Stroke::new(1.0, Color32::WHITE);
        w.hovered.corner_radius = r;
        w.hovered.expansion = 0.0;
        w.active.bg_fill = Color32::from_rgb(96, 96, 100);
        w.active.weak_bg_fill = pal::CONTROL;
        w.active.bg_stroke = Stroke::NONE;
        w.active.fg_stroke = Stroke::new(1.0, Color32::WHITE);
        w.active.corner_radius = r;
        w.active.expansion = 0.0;
        w.open.bg_fill = pal::CARD_HI;
        w.open.weak_bg_fill = pal::CARD_HI;
        w.open.bg_stroke = Stroke::NONE;
        w.open.fg_stroke = Stroke::new(1.0, pal::TEXT);
        w.open.corner_radius = r;
    });
}
