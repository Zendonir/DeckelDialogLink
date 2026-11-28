//! Visual design: fonts (Inter, JetBrains Mono, Material icons), dark and light colour schemes
//! and reusable building blocks (cards, badges, buttons, notices).
//!
//! Simulation is always yellow, real operation always dark red - in both themes.

use std::sync::Arc;

use egui::epaint::Shadow;
use egui::{
    Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, RichText, Stroke, TextStyle, Theme,
    ThemePreference, Ui, Vec2, Visuals,
};
/// Material icons. Note: Inter has own glyphs at a few private-use codepoints (e.g. FOLDER,
/// PLAY_ARROW, SEND, STOP, USB) - those icons would render as Inter glyphs and are not used.
pub use egui_material_icons::icons;

pub const FONT_SEMIBOLD: &str = "inter-semibold";

/// Simulation (yellow) and real operation (dark red) - identical in both themes.
pub const SIM_FILL: Color32 = Color32::from_rgb(0xf2, 0xc2, 0x30);
pub const SIM_TEXT: Color32 = Color32::from_rgb(0x1a, 0x14, 0x00);
pub const REAL_FILL: Color32 = Color32::from_rgb(0xb3, 0x1b, 0x1b);
pub const REAL_TEXT: Color32 = Color32::WHITE;

/// Colours of the active theme.
#[derive(Debug, Clone, Copy)]
pub struct Palette {
    pub bg: Color32,
    pub sidebar: Color32,
    pub header: Color32,
    pub card: Color32,
    pub border: Color32,
    pub input: Color32,
    pub text: Color32,
    pub weak: Color32,
    pub accent: Color32,
    pub accent_text: Color32,
    pub ok: Color32,
    pub warn: Color32,
    pub error: Color32,
    pub info: Color32,
}

pub const DARK: Palette = Palette {
    bg: Color32::from_rgb(0x10, 0x14, 0x1a),
    sidebar: Color32::from_rgb(0x0c, 0x10, 0x15),
    header: Color32::from_rgb(0x14, 0x19, 0x20),
    card: Color32::from_rgb(0x18, 0x1e, 0x26),
    border: Color32::from_rgb(0x27, 0x30, 0x3c),
    input: Color32::from_rgb(0x0c, 0x10, 0x15),
    text: Color32::from_rgb(0xe3, 0xe8, 0xee),
    weak: Color32::from_rgb(0x8c, 0x98, 0xa6),
    accent: Color32::from_rgb(0x3b, 0x8e, 0xea),
    accent_text: Color32::WHITE,
    ok: Color32::from_rgb(0x3f, 0xb9, 0x50),
    warn: Color32::from_rgb(0xe3, 0xa0, 0x08),
    error: Color32::from_rgb(0xf0, 0x54, 0x4c),
    info: Color32::from_rgb(0x58, 0xa6, 0xff),
};

pub const LIGHT: Palette = Palette {
    bg: Color32::from_rgb(0xf1, 0xf3, 0xf6),
    sidebar: Color32::from_rgb(0xe6, 0xea, 0xef),
    header: Color32::from_rgb(0xff, 0xff, 0xff),
    card: Color32::from_rgb(0xff, 0xff, 0xff),
    border: Color32::from_rgb(0xd6, 0xdc, 0xe4),
    input: Color32::from_rgb(0xf6, 0xf8, 0xfa),
    text: Color32::from_rgb(0x1b, 0x23, 0x2d),
    weak: Color32::from_rgb(0x5d, 0x69, 0x77),
    accent: Color32::from_rgb(0x1f, 0x6f, 0xd1),
    accent_text: Color32::WHITE,
    ok: Color32::from_rgb(0x1a, 0x7f, 0x37),
    warn: Color32::from_rgb(0x9a, 0x67, 0x00),
    error: Color32::from_rgb(0xc6, 0x28, 0x28),
    info: Color32::from_rgb(0x1f, 0x6f, 0xd1),
};

impl Palette {
    pub fn of(ui: &Ui) -> Self {
        Self::for_dark(ui.visuals().dark_mode)
    }

    pub fn for_dark(dark: bool) -> Self {
        if dark {
            DARK
        } else {
            LIGHT
        }
    }
}

/// Theme setting: "dunkel", "hell" or "system".
pub fn preference(setting: &str) -> ThemePreference {
    match setting {
        "hell" => ThemePreference::Light,
        "system" => ThemePreference::System,
        _ => ThemePreference::Dark,
    }
}

fn fonts() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    fonts
        .font_data
        .insert("inter".into(), Arc::new(FontData::from_static(include_bytes!("../assets/fonts/Inter-Regular.otf"))));
    fonts.font_data.insert(
        FONT_SEMIBOLD.into(),
        Arc::new(FontData::from_static(include_bytes!("../assets/fonts/Inter-SemiBold.otf"))),
    );
    fonts.font_data.insert(
        "jetbrains-mono".into(),
        Arc::new(FontData::from_static(include_bytes!("../assets/fonts/JetBrainsMono-Regular.ttf"))),
    );
    // Material icons live in the private use area, where egui's emoji font also has glyphs:
    // the icon font therefore comes right after Inter and before the default fallback fonts.
    let icons = egui_material_icons::font_insert();
    let icon_font = icons.name.clone();
    fonts.font_data.insert(icon_font.clone(), Arc::new(icons.data));
    fonts.families.insert(FontFamily::Name(egui_material_icons::FONT_FAMILY.into()), vec![icon_font.clone()]);
    if let Some(family) = fonts.families.get_mut(&FontFamily::Proportional) {
        family.insert(0, "inter".into());
        family.insert(1, icon_font.clone());
    }
    if let Some(family) = fonts.families.get_mut(&FontFamily::Monospace) {
        family.insert(0, "jetbrains-mono".into());
        family.push(icon_font.clone());
    }
    let fallback: Vec<String> =
        fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default().into_iter().skip(1).collect();
    let mut semibold = vec![FONT_SEMIBOLD.to_string()];
    semibold.extend(fallback);
    fonts.families.insert(FontFamily::Name(FONT_SEMIBOLD.into()), semibold);
    fonts
}

fn visuals(palette: &Palette, dark: bool) -> Visuals {
    let mut v = if dark { Visuals::dark() } else { Visuals::light() };
    let radius = CornerRadius::same(7);
    v.panel_fill = palette.bg;
    v.window_fill = palette.card;
    v.window_stroke = Stroke::new(1.0, palette.border);
    v.window_corner_radius = CornerRadius::same(12);
    v.menu_corner_radius = CornerRadius::same(8);
    v.window_shadow =
        Shadow { offset: [0, 8], blur: 28, spread: 0, color: Color32::from_black_alpha(if dark { 140 } else { 50 }) };
    v.popup_shadow =
        Shadow { offset: [0, 4], blur: 14, spread: 0, color: Color32::from_black_alpha(if dark { 120 } else { 40 }) };
    v.extreme_bg_color = palette.input;
    v.faint_bg_color = if dark { Color32::from_rgb(0x1c, 0x23, 0x2c) } else { Color32::from_rgb(0xf4, 0xf6, 0xf9) };
    v.code_bg_color = palette.input;
    v.hyperlink_color = palette.accent;
    v.selection.bg_fill = palette.accent.gamma_multiply(if dark { 0.45 } else { 0.25 });
    v.selection.stroke = Stroke::new(1.0, palette.text);
    v.striped = false;
    let button = if dark { Color32::from_rgb(0x24, 0x2d, 0x38) } else { Color32::from_rgb(0xe9, 0xed, 0xf2) };
    let hover = if dark { Color32::from_rgb(0x2d, 0x38, 0x46) } else { Color32::from_rgb(0xdd, 0xe3, 0xea) };
    let press = if dark { Color32::from_rgb(0x36, 0x43, 0x53) } else { Color32::from_rgb(0xcf, 0xd7, 0xe1) };
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = palette.card;
    w.noninteractive.weak_bg_fill = palette.card;
    w.noninteractive.bg_stroke = Stroke::new(1.0, palette.border);
    w.noninteractive.fg_stroke = Stroke::new(1.0, palette.text);
    w.noninteractive.corner_radius = radius;
    for (state, fill) in
        [(&mut w.inactive, button), (&mut w.hovered, hover), (&mut w.active, press), (&mut w.open, hover)]
    {
        state.bg_fill = fill;
        state.weak_bg_fill = fill;
        state.corner_radius = radius;
        state.fg_stroke = Stroke::new(1.0, palette.text);
    }
    w.inactive.bg_stroke = Stroke::new(1.0, palette.border);
    w.hovered.bg_stroke = Stroke::new(1.0, palette.accent.gamma_multiply(0.8));
    w.active.bg_stroke = Stroke::new(1.0, palette.accent);
    w.open.bg_stroke = Stroke::new(1.0, palette.accent.gamma_multiply(0.8));
    w.hovered.expansion = 0.0;
    w.active.expansion = 0.0;
    v
}

/// Fonts, text sizes, spacing and both colour schemes.
pub fn install(ctx: &egui::Context, setting: &str) {
    ctx.set_fonts(fonts());
    for (theme, palette, dark) in [(Theme::Dark, DARK, true), (Theme::Light, LIGHT, false)] {
        ctx.set_visuals_of(theme, visuals(&palette, dark));
        ctx.style_mut_of(theme, |style| {
            style.text_styles = [
                (TextStyle::Small, FontId::proportional(12.0)),
                (TextStyle::Body, FontId::proportional(14.5)),
                (TextStyle::Button, FontId::proportional(14.5)),
                (TextStyle::Heading, FontId::new(22.0, FontFamily::Name(FONT_SEMIBOLD.into()))),
                (TextStyle::Monospace, FontId::monospace(13.5)),
            ]
            .into();
            let spacing = &mut style.spacing;
            spacing.item_spacing = Vec2::new(10.0, 8.0);
            spacing.button_padding = Vec2::new(12.0, 6.0);
            spacing.interact_size = Vec2::new(40.0, 28.0);
            spacing.window_margin = Margin::same(18);
            spacing.menu_margin = Margin::same(8);
            spacing.combo_width = 220.0;
            spacing.icon_spacing = 6.0;
        });
    }
    ctx.set_theme(preference(setting));
}

// ------------------------------------------------------------------ building blocks

/// Icon followed by text, e.g. `icon_text(icons::ICON_UPLOAD, "Senden")`.
pub fn icon_text(icon: egui_material_icons::MaterialIcon, text: &str) -> String {
    format!("{}  {text}", icon.codepoint)
}

pub fn semibold(text: impl Into<String>, size: f32) -> RichText {
    RichText::new(text).font(FontId::new(size, FontFamily::Name(FONT_SEMIBOLD.into())))
}

/// Page title with icon and one line of explanation.
pub fn page_header(ui: &mut Ui, icon: egui_material_icons::MaterialIcon, title: &str, subtitle: &str) {
    let p = Palette::of(ui);
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new(icon.codepoint).size(26.0).color(p.accent));
        ui.label(semibold(title, 22.0).color(p.text));
    });
    if !subtitle.is_empty() {
        ui.label(RichText::new(subtitle).color(p.weak));
    }
    ui.add_space(8.0);
}

/// Content card with an optional title row.
pub fn card<R>(
    ui: &mut Ui,
    icon: Option<egui_material_icons::MaterialIcon>,
    title: &str,
    add_contents: impl FnOnce(&mut Ui) -> R,
) -> R {
    let p = Palette::of(ui);
    let inner = egui::Frame::new()
        .fill(p.card)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::same(16))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            if !title.is_empty() {
                ui.horizontal(|ui| {
                    if let Some(icon) = icon {
                        ui.label(RichText::new(icon.codepoint).size(18.0).color(p.accent));
                    }
                    ui.label(semibold(title, 15.5).color(p.text));
                });
                ui.add_space(6.0);
            }
            add_contents(ui)
        })
        .inner;
    ui.add_space(10.0);
    inner
}

/// Small rounded badge with tinted background.
pub fn pill(ui: &mut Ui, text: &str, color: Color32) -> egui::Response {
    egui::Frame::new()
        .fill(color.gamma_multiply(0.16))
        .stroke(Stroke::new(1.0, color.gamma_multiply(0.55)))
        .corner_radius(CornerRadius::same(255))
        .inner_margin(Margin::symmetric(10, 3))
        .show(ui, |ui| {
            ui.add(egui::Label::new(RichText::new(text).size(12.5).color(color)).wrap_mode(egui::TextWrapMode::Extend));
        })
        .response
}

pub const BADGE_WIDTH: f32 = 300.0;

pub fn solid_badge(
    ui: &mut Ui,
    icon: egui_material_icons::MaterialIcon,
    title: &str,
    detail: &str,
    fill: Color32,
    color: Color32,
) {
    egui::Frame::new().fill(fill).corner_radius(CornerRadius::same(9)).inner_margin(Margin::symmetric(14, 7)).show(
        ui,
        |ui| {
            ui.set_width(BADGE_WIDTH - 28.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new(icon.codepoint).size(24.0).color(color));
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    ui.label(semibold(title, 14.5).color(color));
                    ui.label(RichText::new(detail).size(11.5).color(color));
                });
            });
        },
    );
}

/// Operating mode badge: yellow "SIMULATION" or red "REALBETRIEB".
pub fn mode_badge(ui: &mut Ui, simulation: bool, port: &str) {
    if simulation {
        solid_badge(ui, icons::ICON_SCIENCE, "SIMULATION", "keine Verbindung zur Maschine", SIM_FILL, SIM_TEXT);
    } else {
        let port = if port.is_empty() { "kein Port gewählt" } else { port };
        solid_badge(ui, icons::ICON_BOLT, "REALBETRIEB", &format!("Schnittstelle {port}"), REAL_FILL, REAL_TEXT);
    }
}

/// Main call to action in the accent colour.
pub fn primary_button(ui: &Ui, text: &str) -> egui::Button<'static> {
    let p = Palette::of(ui);
    egui::Button::new(semibold(text.to_string(), 14.5).color(p.accent_text))
        .fill(p.accent)
        .min_size(Vec2::new(0.0, 34.0))
}

/// Start button in the colour of the operating mode.
pub fn mode_button(text: &str, simulation: bool) -> egui::Button<'static> {
    let (fill, color) = if simulation { (SIM_FILL, SIM_TEXT) } else { (REAL_FILL, REAL_TEXT) };
    egui::Button::new(semibold(text.to_string(), 14.5).color(color)).fill(fill).min_size(Vec2::new(0.0, 36.0))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Info,
    Ok,
    Warn,
    Error,
}

impl Tone {
    pub fn color(self, p: &Palette) -> Color32 {
        match self {
            Tone::Info => p.info,
            Tone::Ok => p.ok,
            Tone::Warn => p.warn,
            Tone::Error => p.error,
        }
    }

    pub fn icon(self) -> egui_material_icons::MaterialIcon {
        match self {
            Tone::Info => icons::ICON_INFO,
            Tone::Ok => icons::ICON_CHECK_CIRCLE,
            Tone::Warn => icons::ICON_WARNING,
            Tone::Error => icons::ICON_ERROR,
        }
    }
}

/// Tinted notice with icon; `add_contents` may add more widgets below the text.
pub fn notice(ui: &mut Ui, tone: Tone, title: &str, add_contents: impl FnOnce(&mut Ui)) {
    let p = Palette::of(ui);
    let color = tone.color(&p);
    egui::Frame::new()
        .fill(color.gamma_multiply(if ui.visuals().dark_mode { 0.10 } else { 0.08 }))
        .stroke(Stroke::new(1.0, color.gamma_multiply(0.6)))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::same(12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(tone.icon().codepoint).size(18.0).color(color));
                ui.label(semibold(title, 14.5).color(color));
            });
            add_contents(ui);
        });
    ui.add_space(6.0);
}

/// Status line with a coloured dot.
pub fn status_dot(ui: &mut Ui, color: Color32, text: &str) {
    ui.horizontal_wrapped(|ui| {
        let (rect, _) = ui.allocate_exact_size(Vec2::new(12.0, 16.0), egui::Sense::hover());
        ui.painter().circle_filled(rect.center(), 5.0, color);
        ui.painter().circle_stroke(rect.center(), 7.5, Stroke::new(1.5, color.gamma_multiply(0.35)));
        ui.label(text);
    });
}

/// Two-column key/value table.
pub fn key_values(ui: &mut Ui, id: &str, rows: &[(&str, String)]) {
    let p = Palette::of(ui);
    let value_width = (ui.available_width() - 150.0).max(120.0);
    egui::Grid::new(id).num_columns(2).spacing([18.0, 6.0]).striped(false).show(ui, |ui| {
        for (key, value) in rows {
            ui.label(RichText::new(*key).color(p.weak));
            ui.scope(|ui| {
                ui.set_max_width(value_width);
                ui.add(egui::Label::new(RichText::new(value).color(p.text)).wrap_mode(egui::TextWrapMode::Wrap));
            });
            ui.end_row();
        }
    });
}

/// Uppercase group label (sidebar).
pub fn group_label(ui: &mut Ui, text: &str) {
    let p = Palette::of(ui);
    ui.add_space(10.0);
    ui.label(RichText::new(text.to_uppercase()).size(11.0).color(p.weak).extra_letter_spacing(1.2));
    ui.add_space(2.0);
}

/// Navigation entry: icon + label, accent bar and tint when selected.
pub fn nav_item(ui: &mut Ui, icon: egui_material_icons::MaterialIcon, label: &str, selected: bool) -> egui::Response {
    let p = Palette::of(ui);
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 36.0), egui::Sense::click());
    let painter = ui.painter();
    if selected {
        painter.rect_filled(rect, CornerRadius::same(8), p.accent.gamma_multiply(0.18));
        let bar = egui::Rect::from_min_size(rect.min + Vec2::new(0.0, 7.0), Vec2::new(3.0, rect.height() - 14.0));
        painter.rect_filled(bar, CornerRadius::same(2), p.accent);
    } else if response.hovered() {
        painter.rect_filled(rect, CornerRadius::same(8), p.border.gamma_multiply(0.6));
    }
    let color = if selected { p.text } else { p.weak };
    painter.text(
        rect.left_center() + Vec2::new(16.0, 0.0),
        egui::Align2::LEFT_CENTER,
        icon.codepoint,
        FontId::proportional(19.0),
        if selected { p.accent } else { p.weak },
    );
    painter.text(
        rect.left_center() + Vec2::new(46.0, 0.0),
        egui::Align2::LEFT_CENTER,
        label,
        if selected { FontId::new(14.5, FontFamily::Name(FONT_SEMIBOLD.into())) } else { FontId::proportional(14.5) },
        color,
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dark_after_frame(setting: &str) -> bool {
        let ctx = egui::Context::default();
        install(&ctx, setting);
        let mut dark = None;
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| dark = Some(ui.visuals().dark_mode));
        output.textures_delta.clear();
        dark.unwrap_or(true)
    }

    #[test]
    fn theme_setting_applies() {
        assert!(dark_after_frame("dunkel"));
        assert!(!dark_after_frame("hell"));
        assert_eq!(preference("unbekannt"), ThemePreference::Dark);
    }
}
