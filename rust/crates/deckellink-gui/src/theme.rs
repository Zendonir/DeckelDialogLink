//! Visual design modelled on the operating concept of the DIALOG 11 control and on the
//! arrangement of the Python version:
//!
//! * housing: light grey panel, dark header bar, numbered mode keys on the left,
//! * screen: dark display with the yellow mode line ("Betriebsartenanzeige") at the top,
//!   boxed areas and monospace text,
//! * softkeys F1-F8 below the screen carry the actions of the current page.
//!
//! The look is DeckelLink's own; it only follows the layout principles of the control.
//! Simulation is always yellow, real operation always dark red (as in the Python version).

use std::sync::Arc;

use egui::{
    Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, RichText, Stroke, TextStyle, Ui,
    Vec2, Visuals,
};

pub const FONT_BOLD: &str = "inter-semibold";
pub const FONT_MONO_BOLD: &str = "mono-bold";

// ---- housing (Python version: #eceff1 window, #263238 header, #37474f navigation)
pub const HOUSING: Color32 = Color32::from_rgb(0xdf, 0xe3, 0xe6);
pub const HOUSING_DARK: Color32 = Color32::from_rgb(0xc5, 0xcc, 0xd1);
pub const HEADER: Color32 = Color32::from_rgb(0x26, 0x32, 0x38);
pub const HEADER_TEXT: Color32 = Color32::from_rgb(0xec, 0xef, 0xf1);
pub const HEADER_DIM: Color32 = Color32::from_rgb(0xb0, 0xbe, 0xc5);
pub const NAV: Color32 = Color32::from_rgb(0x37, 0x47, 0x4f);
pub const KEY: Color32 = Color32::from_rgb(0xe9, 0xec, 0xee);
pub const KEY_TEXT: Color32 = Color32::from_rgb(0x1b, 0x1f, 0x23);
pub const KEY_EDGE: Color32 = Color32::from_rgb(0x6b, 0x77, 0x7f);
pub const KEY_LIT: Color32 = Color32::from_rgb(0xff, 0xd4, 0x3b);

// ---- screen (Python simulator: #0f1b2d background, #ffd43b mode line, #69db7c values)
pub const SCREEN: Color32 = Color32::from_rgb(0x0f, 0x1b, 0x2d);
pub const SCREEN_FIELD: Color32 = Color32::from_rgb(0x16, 0x26, 0x3d);
pub const SCREEN_LINE: Color32 = Color32::from_rgb(0x3a, 0x4d, 0x6a);
pub const TEXT: Color32 = Color32::from_rgb(0xe9, 0xec, 0xef);
pub const DIM: Color32 = Color32::from_rgb(0x91, 0xa4, 0xbd);
pub const YELLOW: Color32 = Color32::from_rgb(0xff, 0xd4, 0x3b);
pub const GREEN: Color32 = Color32::from_rgb(0x69, 0xdb, 0x7c);
pub const RED: Color32 = Color32::from_rgb(0xff, 0x6b, 0x6b);
pub const ORANGE: Color32 = Color32::from_rgb(0xff, 0xa9, 0x4d);
pub const CYAN: Color32 = Color32::from_rgb(0x74, 0xc0, 0xfc);

// ---- operating mode (Python: SIMULATION_BANNER_STYLE / REAL_BANNER_STYLE)
pub const SIM_FILL: Color32 = Color32::from_rgb(0xff, 0xcc, 0x00);
pub const SIM_TEXT: Color32 = Color32::BLACK;
pub const REAL_FILL: Color32 = Color32::from_rgb(0x8b, 0x00, 0x00);
pub const REAL_TEXT: Color32 = Color32::WHITE;

fn fonts() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    let mut add = |name: &str, bytes: &'static [u8]| {
        fonts.font_data.insert(name.into(), Arc::new(FontData::from_static(bytes)));
    };
    add("inter", include_bytes!("../assets/fonts/Inter-Regular.otf"));
    add(FONT_BOLD, include_bytes!("../assets/fonts/Inter-SemiBold.otf"));
    add("mono", include_bytes!("../assets/fonts/JetBrainsMono-Regular.ttf"));
    add(FONT_MONO_BOLD, include_bytes!("../assets/fonts/JetBrainsMono-Bold.ttf"));
    let defaults = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    let with = |first: &str| -> Vec<String> {
        let mut list = vec![first.to_string()];
        list.extend(defaults.iter().cloned());
        list
    };
    fonts.families.insert(FontFamily::Proportional, with("inter"));
    fonts.families.insert(FontFamily::Monospace, with("mono"));
    fonts.families.insert(FontFamily::Name(FONT_BOLD.into()), with(FONT_BOLD));
    fonts.families.insert(FontFamily::Name(FONT_MONO_BOLD.into()), with(FONT_MONO_BOLD));
    fonts
}

/// Widgets live on the dark screen; frames are rectangular like the control's display.
fn visuals() -> Visuals {
    let mut v = Visuals::dark();
    let square = CornerRadius::same(2);
    v.panel_fill = HOUSING;
    v.window_fill = SCREEN;
    v.window_stroke = Stroke::new(2.0, SCREEN_LINE);
    v.window_corner_radius = CornerRadius::same(3);
    v.menu_corner_radius = square;
    v.extreme_bg_color = SCREEN_FIELD;
    v.faint_bg_color = Color32::from_rgb(0x13, 0x22, 0x37);
    v.code_bg_color = SCREEN_FIELD;
    v.hyperlink_color = CYAN;
    v.selection.bg_fill = Color32::from_rgb(0x5c, 0x4a, 0x00);
    v.selection.stroke = Stroke::new(1.0, YELLOW);
    v.striped = false;
    v.window_shadow =
        egui::epaint::Shadow { offset: [0, 6], blur: 18, spread: 0, color: Color32::from_black_alpha(150) };
    v.popup_shadow = egui::epaint::Shadow { offset: [0, 3], blur: 8, spread: 0, color: Color32::from_black_alpha(120) };
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = SCREEN;
    w.noninteractive.weak_bg_fill = SCREEN;
    w.noninteractive.bg_stroke = Stroke::new(1.0, SCREEN_LINE);
    w.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
    w.noninteractive.corner_radius = square;
    let states = [
        (&mut w.inactive, Color32::from_rgb(0x1c, 0x2e, 0x48), SCREEN_LINE),
        (&mut w.hovered, Color32::from_rgb(0x25, 0x3b, 0x5c), YELLOW),
        (&mut w.active, Color32::from_rgb(0x2e, 0x48, 0x70), YELLOW),
        (&mut w.open, Color32::from_rgb(0x25, 0x3b, 0x5c), YELLOW),
    ];
    for (state, fill, edge) in states {
        state.bg_fill = fill;
        state.weak_bg_fill = fill;
        state.bg_stroke = Stroke::new(1.0, edge);
        state.fg_stroke = Stroke::new(1.0, TEXT);
        state.corner_radius = square;
        state.expansion = 0.0;
    }
    v
}

/// Fonts, text sizes and the fixed colour scheme (like the Python version, independent of the OS).
pub fn install(ctx: &egui::Context) {
    ctx.set_fonts(fonts());
    ctx.set_theme(egui::Theme::Dark);
    ctx.set_visuals_of(egui::Theme::Dark, visuals());
    ctx.style_mut_of(egui::Theme::Dark, |style| {
        style.text_styles = [
            (TextStyle::Small, FontId::monospace(11.5)),
            (TextStyle::Body, FontId::monospace(13.5)),
            (TextStyle::Button, FontId::proportional(13.5)),
            (TextStyle::Heading, FontId::new(15.0, FontFamily::Name(FONT_MONO_BOLD.into()))),
            (TextStyle::Monospace, FontId::monospace(13.5)),
        ]
        .into();
        let spacing = &mut style.spacing;
        spacing.item_spacing = Vec2::new(10.0, 6.0);
        spacing.button_padding = Vec2::new(10.0, 5.0);
        spacing.interact_size = Vec2::new(40.0, 26.0);
        spacing.window_margin = Margin::same(14);
        spacing.menu_margin = Margin::same(6);
        spacing.combo_width = 220.0;
    });
}

// ------------------------------------------------------------------ text helpers

pub fn bold(text: impl Into<String>, size: f32) -> RichText {
    RichText::new(text).font(FontId::new(size, FontFamily::Name(FONT_BOLD.into())))
}

pub fn mono_bold(text: impl Into<String>, size: f32) -> RichText {
    RichText::new(text).font(FontId::new(size, FontFamily::Name(FONT_MONO_BOLD.into())))
}

// ------------------------------------------------------------------ screen elements

/// Betriebsartenanzeige: yellow mode line at the top of the screen.
pub fn mode_line(ui: &mut Ui, path: &str, right: &str) {
    egui::Frame::new().fill(SCREEN).stroke(Stroke::new(1.0, SCREEN_LINE)).inner_margin(Margin::symmetric(12, 7)).show(
        ui,
        |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(mono_bold(path, 15.0).color(YELLOW));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new(right).monospace().color(DIM));
                });
            });
        },
    );
}

/// Boxed screen area with an inverse title at the top left (as the control groups its displays).
pub fn screen_box<R>(ui: &mut Ui, title: &str, add_contents: impl FnOnce(&mut Ui) -> R) -> R {
    let inner = egui::Frame::new()
        .stroke(Stroke::new(1.0, SCREEN_LINE))
        .inner_margin(Margin { left: 12, right: 12, top: 8, bottom: 10 })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            if !title.is_empty() {
                egui::Frame::new().fill(SCREEN_LINE).inner_margin(Margin::symmetric(6, 1)).show(ui, |ui| {
                    ui.label(mono_bold(title.to_uppercase(), 12.5).color(TEXT));
                });
                ui.add_space(4.0);
            }
            add_contents(ui)
        })
        .inner;
    ui.add_space(8.0);
    inner
}

/// Key/value lines in screen style (labels dim, values bright).
pub fn fields(ui: &mut Ui, id: &str, rows: &[(&str, String)]) {
    let value_width = (ui.available_width() - 190.0).max(120.0);
    egui::Grid::new(id).num_columns(2).spacing([16.0, 3.0]).show(ui, |ui| {
        for (key, value) in rows {
            ui.label(RichText::new(format!("{key}:")).monospace().color(DIM));
            ui.scope(|ui| {
                ui.set_max_width(value_width);
                ui.add(
                    egui::Label::new(RichText::new(value).monospace().color(TEXT)).wrap_mode(egui::TextWrapMode::Wrap),
                );
            });
            ui.end_row();
        }
    });
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Info,
    Ok,
    Warn,
    Error,
}

impl Tone {
    pub fn color(self) -> Color32 {
        match self {
            Tone::Info => CYAN,
            Tone::Ok => GREEN,
            Tone::Warn => ORANGE,
            Tone::Error => RED,
        }
    }

    pub fn tag(self) -> &'static str {
        match self {
            Tone::Info => "HINWEIS",
            Tone::Ok => "OK",
            Tone::Warn => "WARNUNG",
            Tone::Error => "FEHLER",
        }
    }
}

/// Message block: coloured frame with a tag like a control message line.
pub fn message(ui: &mut Ui, tone: Tone, title: &str, add_contents: impl FnOnce(&mut Ui)) {
    let color = tone.color();
    egui::Frame::new()
        .fill(color.gamma_multiply(0.08))
        .stroke(Stroke::new(1.0, color))
        .inner_margin(Margin::symmetric(10, 7))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                egui::Frame::new().fill(color).inner_margin(Margin::symmetric(5, 0)).show(ui, |ui| {
                    ui.label(mono_bold(tone.tag(), 12.0).color(SCREEN));
                });
                ui.label(RichText::new(title).monospace().color(color));
            });
            add_contents(ui);
        });
    ui.add_space(6.0);
}

/// Inline status tag like "[ VOLLSTÄNDIG ]".
pub fn tag(ui: &mut Ui, text: &str, color: Color32) -> egui::Response {
    ui.add(egui::Label::new(mono_bold(format!("[{text}]"), 12.5).color(color)).wrap_mode(egui::TextWrapMode::Extend))
}

/// Status line with a lamp (filled square) like an indicator on the control panel.
pub fn lamp(ui: &mut Ui, color: Color32, text: &str) {
    ui.horizontal_wrapped(|ui| {
        let (rect, _) = ui.allocate_exact_size(Vec2::new(12.0, 14.0), egui::Sense::hover());
        ui.painter().rect_filled(
            egui::Rect::from_center_size(rect.center(), Vec2::splat(9.0)),
            CornerRadius::same(1),
            color,
        );
        ui.label(RichText::new(text).monospace().color(TEXT));
    });
}

/// Small key inside the screen (row actions).
pub fn screen_key(text: &str) -> egui::Button<'static> {
    egui::Button::new(RichText::new(text.to_uppercase()).monospace().color(TEXT)).min_size(Vec2::new(0.0, 24.0))
}

// ------------------------------------------------------------------ housing elements

pub const BANNER_WIDTH: f32 = 260.0;

/// Mode banner exactly like the Python version (yellow with dashed black border / dark red with white border).
pub fn mode_banner(ui: &mut Ui, simulation: bool, port: &str) {
    let (fill, color, title, detail) = if simulation {
        (SIM_FILL, SIM_TEXT, "SIMULATIONSMODUS".to_string(), "keine Verbindung zur Maschine".to_string())
    } else {
        let port = if port.is_empty() { "(kein Port)" } else { port };
        (REAL_FILL, REAL_TEXT, "REALBETRIEB".to_string(), format!("Schnittstelle {port}"))
    };
    let response = egui::Frame::new().fill(fill).inner_margin(Margin::symmetric(14, 6)).show(ui, |ui| {
        ui.set_width(BANNER_WIDTH - 28.0);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.label(bold(title, 14.0).color(color));
            ui.label(RichText::new(detail).size(12.0).color(color).font(FontId::proportional(12.0)));
        });
    });
    let rect = response.response.rect.shrink(1.5);
    let painter = ui.painter();
    if simulation {
        // dashed black border
        let stroke = Stroke::new(3.0, Color32::BLACK);
        let corners = [rect.left_top(), rect.right_top(), rect.right_bottom(), rect.left_bottom(), rect.left_top()];
        for pair in corners.windows(2) {
            if let [a, b] = pair {
                painter.add(egui::Shape::dashed_line(&[*a, *b], stroke, 8.0, 5.0));
            }
        }
    } else {
        painter.rect_stroke(rect, CornerRadius::ZERO, Stroke::new(3.0, Color32::WHITE), egui::StrokeKind::Inside);
    }
}

/// Raised hardware-style key (navigation keys, softkeys, dialog keys).
pub fn hardware_key(
    ui: &mut Ui,
    size: Vec2,
    number: &str,
    label: &str,
    lit: bool,
    enabled: bool,
    fill: Option<(Color32, Color32)>,
) -> egui::Response {
    let sense = if enabled { egui::Sense::click() } else { egui::Sense::hover() };
    let (rect, response) = ui.allocate_exact_size(size, sense);
    let painter = ui.painter();
    let pressed = response.is_pointer_button_down_on();
    let (base, text) = match (fill, lit) {
        (Some((f, t)), _) => (f, t),
        (None, true) => (KEY_LIT, KEY_TEXT),
        (None, false) => (KEY, KEY_TEXT),
    };
    let base = if !enabled {
        Color32::from_rgb(0xb8, 0xbf, 0xc5)
    } else if response.hovered() && !pressed {
        base.gamma_multiply(1.04).linear_multiply(1.0)
    } else {
        base
    };
    let body = if pressed { rect.translate(Vec2::new(0.0, 1.5)) } else { rect };
    // key shadow and body
    painter.rect_filled(rect.translate(Vec2::new(0.0, 2.0)), CornerRadius::same(4), Color32::from_black_alpha(90));
    painter.rect_filled(body, CornerRadius::same(4), base);
    painter.rect_stroke(body, CornerRadius::same(4), Stroke::new(1.0, KEY_EDGE), egui::StrokeKind::Inside);
    // top highlight
    let highlight = egui::Rect::from_min_size(body.min + Vec2::new(2.0, 2.0), Vec2::new(body.width() - 4.0, 2.0));
    painter.rect_filled(highlight, CornerRadius::same(1), Color32::from_white_alpha(if enabled { 90 } else { 30 }));
    let text_color = if enabled { text } else { Color32::from_rgb(0x6f, 0x78, 0x80) };
    if !number.is_empty() {
        painter.text(
            body.left_top() + Vec2::new(7.0, 5.0),
            egui::Align2::LEFT_TOP,
            number,
            FontId::new(11.0, FontFamily::Name(FONT_MONO_BOLD.into())),
            text_color.gamma_multiply(0.75),
        );
    }
    let font = FontId::new(12.5, FontFamily::Name(FONT_BOLD.into()));
    let lines: Vec<&str> = label.lines().collect();
    let line_height = 15.0;
    #[allow(clippy::cast_precision_loss)]
    let block = line_height * lines.len() as f32;
    let top = body.center().y - block / 2.0 + if number.is_empty() { 0.0 } else { 6.0 };
    for (index, line) in lines.iter().enumerate() {
        #[allow(clippy::cast_precision_loss)]
        let y = top + line_height * index as f32 + line_height / 2.0;
        painter.text(egui::pos2(body.center().x, y), egui::Align2::CENTER_CENTER, *line, font.clone(), text_color);
    }
    if enabled {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    }
}

/// Navigation key in the left key column: number field + label, lit when active.
pub fn nav_key(ui: &mut Ui, number: usize, label: &str, active: bool) -> egui::Response {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 40.0), egui::Sense::click());
    let painter = ui.painter();
    let fill = if active {
        Color32::from_rgb(0x54, 0x6e, 0x7a)
    } else if response.hovered() {
        Color32::from_rgb(0x45, 0x5a, 0x64)
    } else {
        NAV
    };
    painter.rect_filled(rect, CornerRadius::ZERO, fill);
    // number key
    let key = egui::Rect::from_min_size(rect.left_top() + Vec2::new(10.0, 7.0), Vec2::new(26.0, 26.0));
    painter.rect_filled(key.translate(Vec2::new(0.0, 1.5)), CornerRadius::same(3), Color32::from_black_alpha(110));
    painter.rect_filled(key, CornerRadius::same(3), if active { KEY_LIT } else { KEY });
    painter.text(
        key.center(),
        egui::Align2::CENTER_CENTER,
        number.to_string(),
        FontId::new(13.0, FontFamily::Name(FONT_MONO_BOLD.into())),
        KEY_TEXT,
    );
    painter.text(
        rect.left_center() + Vec2::new(48.0, 0.0),
        egui::Align2::LEFT_CENTER,
        label,
        FontId::new(13.5, FontFamily::Name(FONT_BOLD.into())),
        if active { Color32::WHITE } else { HEADER_TEXT },
    );
    if active {
        painter.rect_filled(
            egui::Rect::from_min_size(rect.right_top() - Vec2::new(4.0, 0.0), Vec2::new(4.0, rect.height())),
            CornerRadius::ZERO,
            KEY_LIT,
        );
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_sets_fixed_dark_screen_theme() {
        let ctx = egui::Context::default();
        install(&ctx);
        let mut dark = None;
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| dark = Some(ui.visuals().dark_mode));
        output.textures_delta.clear();
        assert_eq!(dark, Some(true));
    }
}
