//! Drawing of the DIALOG 11 simulator page (inside the dark screen).
//!
//! Arrangement of the Python simulator window: mode keys and auxiliary keys on top,
//! program listing left, tool path graphic in the middle, displays right, machine panel
//! keys (cycle start/stop, single block) below; menu softkeys F1-F8 come from
//! [`crate::ui::softkeys`]. Everything here is simulation only (yellow banner).

use egui::{Color32, Pos2, Rect, RichText, ScrollArea, Sense, Shape, Stroke, Ui, Vec2};

use deckellink_core::sim::geometry::Vec3;
use deckellink_core::sim::menus::{self, aux_mode};
use deckellink_core::sim::model::{MoveKind, Severity};
use deckellink_core::sim::py_g;
use deckellink_core::sim::reference::FUNCTION_REFERENCE;

use crate::app::{Action, DeckelApp};
use crate::sim_app::{
    cutting_data, SimAction, SimDialog, SimPage, SimTab, View, ViewMode, AUX_CODES, JOG_STEPS, MODE_NAMES, SPEEDS,
};
use crate::theme::{
    bold, hardware_key, mode_line, mono_bold, screen_box, screen_key, CYAN, DIM, GREEN, ORANGE, RED, SCREEN,
    SCREEN_FIELD, SCREEN_LINE, SIM_FILL, SIM_TEXT, TEXT, YELLOW,
};

const RAPID: Color32 = Color32::from_rgb(0xf5, 0x9f, 0x00);
const FEED: Color32 = Color32::from_rgb(0x74, 0xc0, 0xfc);
const DONE: Color32 = Color32::from_rgb(0x69, 0xdb, 0x7c);
const PROGRAMMED: Color32 = Color32::from_rgb(0xad, 0xb5, 0xbd);
const GRID: Color32 = Color32::from_rgb(0x22, 0x34, 0x4f);
const MANUAL: Color32 = Color32::from_rgb(0xe5, 0x99, 0xf7);
const AXIS_COLORS: [Color32; 3] =
    [Color32::from_rgb(0xff, 0x6b, 0x6b), Color32::from_rgb(0x51, 0xcf, 0x66), Color32::from_rgb(0x4d, 0xab, 0xf7)];
/// Upper limit of drawn points per frame (very long programs are thinned out for display only).
const MAX_DRAWN_POINTS: usize = 250_000;

fn sim(app: &mut DeckelApp, action: SimAction) {
    app.push(Action::Sim(action));
}

/// Whole simulator page.
pub fn page(app: &mut DeckelApp, ui: &mut Ui) {
    let flags = app.sim.mode_flags().join("  ·  ");
    mode_line(ui, &app.sim.mode_path(), &flags);
    ui.add_space(6.0);
    mode_keys(app, ui);
    ui.add_space(6.0);
    let panel_height = 52.0;
    let height = (ui.available_height() - panel_height - 16.0).max(220.0);
    let width = ui.available_width() - 2.0;
    let gap = 10.0;
    let graphics = app.sim.graphics_on;
    let (left_w, mid_w) = if graphics { (width * 0.27, width * 0.43) } else { (width * 0.6, 0.0) };
    let right_w = (width - left_w - mid_w - gap * if graphics { 2.0 } else { 1.0 }).max(200.0);
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        let column = egui::Layout::top_down(egui::Align::Min);
        ui.allocate_ui_with_layout(Vec2::new(left_w, height), column, |ui| {
            ui.set_min_size(Vec2::new(left_w, height));
            ui.set_max_size(Vec2::new(left_w, height));
            listing(app, ui);
        });
        if graphics {
            ui.allocate_ui_with_layout(Vec2::new(mid_w, height), column, |ui| {
                graphic(app, ui, Vec2::new(mid_w, height))
            });
        }
        ui.allocate_ui_with_layout(Vec2::new(right_w, height), column, |ui| {
            ui.set_min_size(Vec2::new(right_w, height));
            ui.set_max_size(Vec2::new(right_w, height));
            displays(app, ui);
        });
    });
    ui.add_space(8.0);
    machine_panel(app, ui);
    dialog(app, ui.ctx());
}

fn mode_keys(app: &mut DeckelApp, ui: &mut Ui) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        for (index, name) in MODE_NAMES.iter().enumerate() {
            let mode = index + 1;
            if hardware_key(ui, Vec2::new(124.0, 30.0), "", name, app.sim.mode == mode, true, None).clicked() {
                sim(app, SimAction::SetMode(mode));
            }
        }
        ui.add_space(10.0);
        for code in AUX_CODES {
            let open = app.sim.menu().is_some_and(|m| m.code == code);
            let response = hardware_key(ui, Vec2::new(58.0, 30.0), "", code, open, true, None);
            let response = match aux_mode(code) {
                Some(item) => response.on_hover_text(item.description),
                None => response,
            };
            if response.clicked() {
                sim(app, SimAction::Aux(code));
            }
        }
        if hardware_key(ui, Vec2::new(86.0, 30.0), "", "ZURÜCK", false, app.sim.menu_stack.len() > 1, None).clicked() {
            sim(app, SimAction::Back);
        }
        if ui.available_width() < 250.0 {
            return;
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            egui::Frame::new().fill(SIM_FILL).inner_margin(egui::Margin::symmetric(8, 3)).show(ui, |ui| {
                ui.label(bold("SIMULATION · offline, keine Maschine", 12.0).color(SIM_TEXT));
            });
        });
    });
}

// ------------------------------------------------------------------ listing / editor

fn listing(app: &mut DeckelApp, ui: &mut Ui) {
    screen_box(ui, if app.sim.mode == 3 { "Programm bearbeiten" } else { "Programm" }, |ui| {
        ui.label(RichText::new(app.sim.mode_hint()).size(11.5).color(DIM));
        ui.label(RichText::new(&app.sim.source_name).monospace().color(CYAN));
        ui.horizontal_wrapped(|ui| {
            if ui.add(screen_key("Datei öffnen")).clicked() {
                sim(app, SimAction::OpenFile);
            }
            if ui.add(screen_key("Als neue Datei")).clicked() {
                sim(app, SimAction::SaveAs);
            }
            if ui.add(screen_key("Demo")).clicked() {
                sim(app, SimAction::LoadDemo);
            }
        });
        ui.add_space(4.0);
        if app.sim.mode == 3 {
            editor(app, ui);
        } else {
            read_only_listing(app, ui);
        }
    });
}

fn editor(app: &mut DeckelApp, ui: &mut Ui) {
    let id = egui::Id::unique("sim-editor");
    ScrollArea::both().id_salt("sim-editor-scroll").auto_shrink([false, false]).show(ui, |ui| {
        let output = egui::TextEdit::multiline(&mut app.sim.text)
            .id(id)
            .font(egui::TextStyle::Monospace)
            .code_editor()
            .desired_width(f32::INFINITY)
            .desired_rows(30)
            .show(ui);
        if output.response.changed() {
            app.sim.text_changed();
        }
        if let Some((start, end)) = app.sim.pending_cursor.take() {
            let mut state = output.state.clone();
            state.cursor.set_char_range(Some(egui::text::CCursorRange::two(
                egui::text::CCursor::new(start),
                egui::text::CCursor::new(end),
            )));
            state.store(ui.ctx(), id);
            output.response.request_focus();
        }
        if let Some(range) = output.cursor_range {
            let index: usize = range.primary.index.into();
            let line = app.sim.text.chars().take(index).filter(|c| *c == '\n').count() + 1;
            app.sim.cursor_line = line;
        }
    });
}

fn read_only_listing(app: &mut DeckelApp, ui: &mut Ui) {
    let current = app.sim.frame().map_or(0, |f| f.line);
    let follow = app.sim.running;
    let mut clicked = None;
    ScrollArea::both().id_salt("sim-listing").auto_shrink([false, false]).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        for (index, line) in app.sim.text.lines().enumerate() {
            let number = index + 1;
            let active = number == current;
            let text = format!("{number:>4}  {line}");
            let rich = if active {
                RichText::new(text).monospace().color(Color32::BLACK).background_color(YELLOW)
            } else {
                RichText::new(text).monospace().color(TEXT)
            };
            let response = ui.add(egui::Label::new(rich).sense(Sense::click()).wrap_mode(egui::TextWrapMode::Extend));
            if response.clicked() {
                clicked = Some(number);
            }
            if active && follow {
                response.scroll_to_me(Some(egui::Align::Center));
            }
        }
    });
    if let Some(line) = clicked {
        sim(app, SimAction::SelectLine(line));
    }
}

// ------------------------------------------------------------------ graphic

struct Projection {
    scale: f64,
    origin: Pos2,
    view: View,
}

impl Projection {
    fn to_screen(&self, p: Vec3) -> Pos2 {
        let (u, v) = self.view.project(p);
        #[allow(clippy::cast_possible_truncation)]
        Pos2::new(self.origin.x + (u * self.scale) as f32, self.origin.y - (v * self.scale) as f32)
    }
}

fn projection(page: &SimPage, rect: Rect) -> Projection {
    let (lo, hi) = page.result().and_then(|r| r.bounds()).unwrap_or(([-50.0, -50.0, -10.0], [50.0, 50.0, 50.0]));
    let mut xs = Vec::new();
    let mut ys = Vec::new();
    for x in [lo[0], hi[0]] {
        for y in [lo[1], hi[1]] {
            for z in [lo[2], hi[2]] {
                let (u, v) = page.view.project([x, y, z]);
                xs.push(u);
                ys.push(v);
            }
        }
    }
    let min = |v: &[f64]| v.iter().copied().fold(f64::INFINITY, f64::min);
    let max = |v: &[f64]| v.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let (x0, y0) = (min(&xs), min(&ys));
    let width = (max(&xs) - x0).max(1.0);
    let height = (max(&ys) - y0).max(1.0);
    let margin = 30.0;
    let avail_w = f64::from(rect.width() - 2.0 * margin).max(10.0);
    let avail_h = f64::from(rect.height() - 2.0 * margin).max(10.0);
    let scale = (avail_w / width).min(avail_h / height) * page.view.zoom;
    let (cx, cy) = (x0 + width / 2.0, y0 + height / 2.0);
    #[allow(clippy::cast_possible_truncation)]
    let origin = Pos2::new(
        rect.center().x - (cx * scale) as f32 + page.view.pan.0,
        rect.center().y + (cy * scale) as f32 + page.view.pan.1,
    );
    Projection { scale, origin, view: page.view.clone() }
}

fn nice_step(raw: f64) -> f64 {
    if raw <= 0.0 || !raw.is_finite() {
        return 1.0;
    }
    let exponent = raw.log10().floor();
    let base = raw / 10f64.powf(exponent);
    let nice = if base < 1.5 {
        1.0
    } else if base < 3.5 {
        2.0
    } else if base < 7.5 {
        5.0
    } else {
        10.0
    };
    nice * 10f64.powf(exponent)
}

fn graphic(app: &mut DeckelApp, ui: &mut Ui, size: Vec2) {
    let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
    // interaction: wheel = zoom, drag = pan, right drag (3D) = rotate, double click = fit
    if response.hovered() {
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll.abs() > 0.0 {
            let factor = if scroll > 0.0 { 1.1 } else { 1.0 / 1.1 };
            app.sim.view.zoom = (app.sim.view.zoom * factor).clamp(0.05, 200.0);
        }
    }
    if response.dragged_by(egui::PointerButton::Secondary) && app.sim.view.mode == ViewMode::Iso {
        let delta = response.drag_delta();
        app.sim.view.yaw += f64::from(delta.x) * 0.5;
        app.sim.view.pitch = (app.sim.view.pitch - f64::from(delta.y) * 0.5).clamp(-89.0, 89.0);
    } else if response.dragged() {
        let delta = response.drag_delta();
        app.sim.view.pan.0 += delta.x;
        app.sim.view.pan.1 += delta.y;
    }
    if response.double_clicked() {
        app.sim.view.fit();
    }
    let response = response.on_hover_text(
        "Mausrad: Zoom, linke Maustaste: Verschieben, rechte Maustaste (3D): Drehen, Doppelklick: Einpassen",
    );
    let page = &app.sim;
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 2.0, SCREEN);
    painter.rect_stroke(rect, 2.0, Stroke::new(1.0, SCREEN_LINE), egui::StrokeKind::Inside);
    let projection = projection(page, rect);
    let font = egui::FontId::monospace(11.0);
    // grid
    if page.view.mode != ViewMode::Iso && projection.scale > 0.0 {
        let step = nice_step(60.0 / projection.scale);
        let origin = projection.origin;
        let left = f64::from(rect.left() - origin.x) / projection.scale;
        let right = f64::from(rect.right() - origin.x) / projection.scale;
        let top = f64::from(origin.y - rect.top()) / projection.scale;
        let bottom = f64::from(origin.y - rect.bottom()) / projection.scale;
        let mut x = (left / step).floor() * step;
        let mut guard = 0;
        while x <= right && guard < 400 {
            #[allow(clippy::cast_possible_truncation)]
            let sx = origin.x + (x * projection.scale) as f32;
            painter.line_segment([Pos2::new(sx, rect.top()), Pos2::new(sx, rect.bottom())], Stroke::new(1.0, GRID));
            x += step;
            guard += 1;
        }
        let mut y = (bottom / step).floor() * step;
        guard = 0;
        while y <= top && guard < 400 {
            #[allow(clippy::cast_possible_truncation)]
            let sy = origin.y - (y * projection.scale) as f32;
            painter.line_segment([Pos2::new(rect.left(), sy), Pos2::new(rect.right(), sy)], Stroke::new(1.0, GRID));
            y += step;
            guard += 1;
        }
        painter.text(
            rect.left_bottom() + Vec2::new(8.0, -8.0),
            egui::Align2::LEFT_BOTTOM,
            format!("Raster {} mm", py_g(step)),
            font.clone(),
            TEXT,
        );
    }
    // axes
    let zero = projection.to_screen([0.0; 3]);
    let length = 40.0 / projection.scale.max(1e-9);
    for (index, color) in AXIS_COLORS.iter().enumerate() {
        let mut end = [0.0; 3];
        if let Some(slot) = end.get_mut(index) {
            *slot = length;
        }
        let tip = projection.to_screen(end);
        if (tip - zero).length() < 1.0 {
            continue;
        }
        painter.line_segment([zero, tip], Stroke::new(2.0, *color));
        painter.text(
            tip + Vec2::new(4.0, -4.0),
            egui::Align2::LEFT_BOTTOM,
            ["X", "Y", "Z"].get(index).copied().unwrap_or(""),
            font.clone(),
            *color,
        );
    }
    let frame = page.frame();
    if let Some(result) = page.result() {
        let total_points: usize = result.moves.iter().map(|m| m.points.len()).sum();
        let stride = (total_points / MAX_DRAWN_POINTS).max(1);
        let polyline = |points: &[Vec3]| -> Vec<Pos2> {
            let last = points.len().saturating_sub(1);
            points
                .iter()
                .enumerate()
                .filter(|(i, _)| i % stride == 0 || *i == last)
                .map(|(_, p)| projection.to_screen(*p))
                .collect()
        };
        if page.view.mode != ViewMode::Iso {
            for entry in result.programmed.iter().filter(|m| m.kind == MoveKind::Feed) {
                painter.add(Shape::dotted_line(&polyline(&entry.points), PROGRAMMED, 4.0, 0.8));
            }
        }
        let done = match &frame {
            Some(f) if f.finished => result.moves.len(),
            Some(f) => f.move_index.unwrap_or(0),
            None => 0,
        };
        for (index, entry) in result.moves.iter().enumerate() {
            let points = polyline(&entry.points);
            match entry.kind {
                MoveKind::Rapid if page.show_rapid => {
                    painter.add(Shape::dashed_line(&points, Stroke::new(1.2, RAPID), 6.0, 4.0));
                }
                MoveKind::Rapid => {}
                MoveKind::Feed if index < done => {
                    painter.add(Shape::line(points, Stroke::new(2.4, DONE)));
                }
                MoveKind::Feed => {
                    painter.add(Shape::line(points, Stroke::new(1.4, FEED)));
                }
            }
        }
        if page.mode != 1 {
            if let (Some(frame), Some(current)) =
                (&frame, frame.as_ref().and_then(|f| f.move_index).and_then(|i| result.moves.get(i)))
            {
                if current.kind == MoveKind::Feed && !frame.finished {
                    painter.line_segment(
                        [projection.to_screen(current.start()), projection.to_screen(frame.position)],
                        Stroke::new(2.4, DONE),
                    );
                }
            }
        }
    }
    if page.mode == 1 && page.manual_trace.len() > 1 {
        let points: Vec<Pos2> = page.manual_trace.iter().map(|p| projection.to_screen(*p)).collect();
        painter.add(Shape::line(points, Stroke::new(1.6, MANUAL)));
    }
    // tool
    let tool = if page.mode == 1 {
        Some((page.manual_position, 0.0))
    } else {
        frame.as_ref().map(|f| (f.position, f.snapshot.tool_radius))
    };
    if let Some((position, radius)) = tool {
        let center = projection.to_screen(position);
        let stroke = Stroke::new(2.0, Color32::WHITE);
        #[allow(clippy::cast_possible_truncation)]
        let r = (radius * projection.scale) as f32;
        if page.show_tool && radius > 0.0 {
            match page.view.mode {
                ViewMode::Xy => {
                    painter.circle_stroke(center, r, stroke);
                }
                ViewMode::Xz | ViewMode::Yz => {
                    painter.rect_stroke(
                        Rect::from_min_max(
                            Pos2::new(center.x - r, center.y - 3.0 * r),
                            Pos2::new(center.x + r, center.y),
                        ),
                        0.0,
                        stroke,
                        egui::StrokeKind::Middle,
                    );
                }
                ViewMode::Iso => {}
            }
        }
        painter.line_segment([center - Vec2::new(8.0, 0.0), center + Vec2::new(8.0, 0.0)], stroke);
        painter.line_segment([center - Vec2::new(0.0, 8.0), center + Vec2::new(0.0, 8.0)], stroke);
    }
    // legend
    painter.text(
        rect.left_top() + Vec2::new(8.0, 6.0),
        egui::Align2::LEFT_TOP,
        page.view.mode.label(),
        font.clone(),
        TEXT,
    );
    let mut x = rect.left() + 8.0;
    for (text, color) in [("Vorschub", FEED), ("Eilgang", RAPID), ("bearbeitet", DONE)] {
        painter.rect_filled(Rect::from_min_size(Pos2::new(x, rect.top() + 26.0), Vec2::new(14.0, 4.0)), 0.0, color);
        painter.text(Pos2::new(x + 18.0, rect.top() + 28.0), egui::Align2::LEFT_CENTER, text, font.clone(), TEXT);
        x += 100.0;
    }
    if page.busy {
        painter.text(rect.right_top() + Vec2::new(-8.0, 6.0), egui::Align2::RIGHT_TOP, "berechne …", font, YELLOW);
    }
    // view keys inside the graphic (bottom right)
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(Rect::from_min_max(
                rect.right_bottom() - Vec2::new(330.0, 30.0),
                rect.right_bottom() - Vec2::new(6.0, 4.0),
            ))
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
    );
    let rapid_label = if app.sim.show_rapid { "Eilgang aus" } else { "Eilgang ein" };
    if child.add(screen_key(rapid_label)).clicked() {
        sim(app, SimAction::ToggleRapid);
    }
    for mode in ViewMode::ALL.iter().rev() {
        let short = match mode {
            ViewMode::Xy => "XY",
            ViewMode::Xz => "XZ",
            ViewMode::Yz => "YZ",
            ViewMode::Iso => "3D",
        };
        let selected = app.sim.view.mode == *mode;
        let button =
            egui::Button::new(RichText::new(short).monospace().color(if selected { Color32::BLACK } else { TEXT }))
                .fill(if selected { YELLOW } else { SCREEN_FIELD });
        if child.add(button).on_hover_text(mode.label()).clicked() {
            sim(app, SimAction::View(*mode));
        }
    }
    drop(response);
}

// ------------------------------------------------------------------ displays

fn displays(app: &mut DeckelApp, ui: &mut Ui) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = Vec2::new(4.0, 4.0);
        for tab in SimTab::ALL {
            let selected = app.sim.tab == tab;
            let mut label = tab.label().to_string();
            if tab == SimTab::Messages {
                let count = app.sim.result().map_or(0, |r| r.errors().len() + r.warnings().len());
                if count > 0 {
                    label = format!("{label} ({count})");
                }
            }
            let button = egui::Button::new(RichText::new(label).monospace().size(11.5).color(if selected {
                Color32::BLACK
            } else {
                TEXT
            }))
            .fill(if selected { YELLOW } else { SCREEN_FIELD });
            if ui.add(button).clicked() {
                sim(app, SimAction::Tab(tab));
            }
        }
    });
    ui.add_space(6.0);
    ScrollArea::both().id_salt("sim-displays").auto_shrink([false, false]).show(ui, |ui| match app.sim.tab {
        SimTab::Display => display_tab(app, ui),
        SimTab::Messages => messages_tab(app, ui),
        SimTab::Setup => setup_tab(app, ui),
        SimTab::Reference => reference_tab(ui),
        SimTab::Notes => {
            ui.label(RichText::new("Notizen (nur in dieser Sitzung, werden nicht gespeichert)").size(11.5).color(DIM));
            ui.add(egui::TextEdit::multiline(&mut app.sim.notes).desired_width(f32::INFINITY).desired_rows(16));
        }
        SimTab::Screen => {
            let text =
                if app.sim.screen_text.is_empty() { "(keine Bildschirmausgabe)" } else { app.sim.screen_text.as_str() };
            ui.add(egui::Label::new(RichText::new(text).monospace().color(TEXT)).wrap_mode(egui::TextWrapMode::Wrap));
        }
    });
}

fn display_tab(app: &mut DeckelApp, ui: &mut Ui) {
    let Some(frame) = app.sim.frame() else {
        ui.label(
            RichText::new(if app.sim.busy { "Simulation läuft …" } else { "Noch keine Simulation." })
                .monospace()
                .color(DIM),
        );
        return;
    };
    let manual = app.sim.mode == 1;
    let position = if manual { app.sim.display_position(app.sim.manual_position) } else { frame.position };
    let remaining = if manual { [0.0; 3] } else { frame.remaining };
    screen_box(ui, if manual { "Istwert (Simulator, PRESET)" } else { "Istwert (Werkstück)" }, |ui| {
        egui::Grid::new("sim-dro").num_columns(3).spacing([14.0, 2.0]).show(ui, |ui| {
            ui.label(RichText::new("").monospace());
            ui.label(RichText::new("Istwert").monospace().color(DIM));
            ui.label(RichText::new("Restweg").monospace().color(DIM));
            ui.end_row();
            for (index, name) in ["X", "Y", "Z"].iter().enumerate() {
                ui.label(mono_bold(*name, 20.0).color(TEXT));
                ui.label(mono_bold(format!("{:10.3}", position.get(index).copied().unwrap_or(0.0)), 20.0).color(GREEN));
                ui.label(
                    RichText::new(format!("{:9.3}", remaining.get(index).copied().unwrap_or(0.0)))
                        .monospace()
                        .color(DIM),
                );
                ui.end_row();
            }
            ui.label(mono_bold("C", 20.0).color(TEXT));
            ui.label(mono_bold(format!("{:10.3}", frame.snapshot.c_axis), 20.0).color(GREEN));
            ui.end_row();
        });
    });
    let snap = &frame.snapshot;
    ui.label(
        RichText::new(format!(
            "F {:8.1}   S {:7.0}   {} (R {})   {}",
            snap.feed,
            snap.speed,
            snap.tool,
            py_g(snap.tool_radius),
            snap.spindle
        ))
        .monospace()
        .color(TEXT),
    );
    let block = if frame.line == 0 {
        "-".to_string()
    } else {
        let text = app.sim.text.lines().nth(frame.line - 1).unwrap_or("").trim();
        format!("Zeile {}: {text}", frame.line)
    };
    egui::Frame::new().fill(SCREEN_FIELD).inner_margin(egui::Margin::same(4)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.add(
            egui::Label::new(RichText::new(format!("Satz: {block}")).monospace().color(YELLOW))
                .wrap_mode(egui::TextWrapMode::Wrap),
        );
    });
    ui.add_space(6.0);
    screen_box(ui, "Modalliste (wirksame Befehle)", |ui| {
        egui::Grid::new("sim-modal").num_columns(2).spacing([12.0, 2.0]).show(ui, |ui| {
            for (group, value) in &snap.modal {
                ui.label(RichText::new(group).monospace().color(DIM));
                ui.label(RichText::new(value).monospace().color(TEXT));
                ui.end_row();
            }
        });
    });
    ui.add(
        egui::Label::new(RichText::new(app.sim.summary()).size(12.0).color(TEXT)).wrap_mode(egui::TextWrapMode::Wrap),
    );
}

fn messages_tab(app: &mut DeckelApp, ui: &mut Ui) {
    let Some(result) = app.sim.result() else { return };
    if result.messages.is_empty() {
        ui.label(RichText::new("Keine Meldungen.").monospace().color(GREEN));
        return;
    }
    let mut clicked = None;
    for message in &result.messages {
        let color = match message.severity {
            Severity::Error => RED,
            Severity::Warning => ORANGE,
            Severity::Info => CYAN,
        };
        let response = ui.add(
            egui::Label::new(RichText::new(message.to_string()).monospace().size(12.0).color(color))
                .sense(Sense::click())
                .wrap_mode(egui::TextWrapMode::Wrap),
        );
        if response.on_hover_text("Klick: Zeile im Programm anzeigen").clicked() && message.line > 0 {
            clicked = Some(message.line);
        }
    }
    if let Some(line) = clicked {
        sim(app, SimAction::SelectLine(line));
    }
}

fn number_cell(ui: &mut Ui, value: &mut String, width: f32) {
    ui.add(egui::TextEdit::singleline(value).desired_width(width).font(egui::TextStyle::Monospace));
}

fn setup_tab(app: &mut DeckelApp, ui: &mut Ui) {
    screen_box(ui, "Werkzeugtabelle des Simulators", |ui| {
        ui.label(
            RichText::new("Radius für Bahnkorrektur und Zyklen; leere Zeilen werden ignoriert.").size(11.5).color(DIM),
        );
        egui::Grid::new("sim-tools").num_columns(3).spacing([6.0, 2.0]).show(ui, |ui| {
            for title in ["T-Nr.", "Radius mm", "Länge mm"] {
                ui.label(RichText::new(title).monospace().color(DIM));
            }
            ui.end_row();
            for row in &mut app.sim.tools {
                for cell in row.iter_mut() {
                    number_cell(ui, cell, 70.0);
                }
                ui.end_row();
            }
        });
    });
    screen_box(ui, "Nullpunktspeicher G51.n (relativ zu G51.0)", |ui| {
        egui::Grid::new("sim-zeros").num_columns(4).spacing([6.0, 2.0]).show(ui, |ui| {
            for title in ["n", "X", "Y", "Z"] {
                ui.label(RichText::new(title).monospace().color(DIM));
            }
            ui.end_row();
            for row in &mut app.sim.zeros {
                for cell in row.iter_mut() {
                    number_cell(ui, cell, 52.0);
                }
                ui.end_row();
            }
        });
    });
    screen_box(ui, "Parametertabelle", |ui| {
        ui.label(RichText::new("Je Zeile z. B. P1=10, PP2=(P1+5), P3=%*1").size(11.5).color(DIM));
        ui.add(
            egui::TextEdit::multiline(&mut app.sim.parameter_text)
                .font(egui::TextStyle::Monospace)
                .desired_width(f32::INFINITY)
                .desired_rows(4),
        );
    });
    screen_box(ui, "Annahmen", |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Eilgang (Annahme):").monospace().color(DIM));
            ui.add(egui::DragValue::new(&mut app.sim.rapid_feed).range(100.0..=60_000.0).suffix(" mm/min"))
                .on_hover_text("Annahme für die Laufzeitschätzung - Eilgang der FP2A nicht hinterlegt.");
        });
        ui.horizontal(|ui| {
            ui.label(RichText::new("Startposition X/Y/Z:").monospace().color(DIM));
            for value in &mut app.sim.start {
                ui.add(egui::DragValue::new(value).range(-5000.0..=5000.0).speed(1.0));
            }
        });
    });
    if ui.add(screen_key("Übernehmen und neu simulieren")).clicked() {
        sim(app, SimAction::Resimulate);
    }
}

fn reference_tab(ui: &mut Ui) {
    ui.label(
        RichText::new(
            "Übersicht der vom Simulator ausgewerteten Befehle (eigene Kurzbeschreibungen nach den \
             DECKEL-Trainingsunterlagen). Die Simulation ersetzt keine Prüfung an der Maschine.",
        )
        .size(11.5)
        .color(DIM),
    );
    ui.add_space(4.0);
    egui::Grid::new("sim-reference").num_columns(3).striped(true).spacing([10.0, 3.0]).show(ui, |ui| {
        for title in ["Befehl", "Bedeutung", "Simulator"] {
            ui.label(RichText::new(title).monospace().color(YELLOW));
        }
        ui.end_row();
        for (code, meaning, support) in FUNCTION_REFERENCE {
            ui.label(mono_bold(*code, 12.0).color(TEXT));
            ui.add(
                egui::Label::new(RichText::new(*meaning).size(12.0).color(TEXT)).wrap_mode(egui::TextWrapMode::Wrap),
            );
            ui.label(RichText::new(*support).size(12.0).color(DIM));
            ui.end_row();
        }
    });
}

// ------------------------------------------------------------------ machine panel

/// (label, action, enabled, fill) of a machine panel key.
type PanelKey = (&'static str, SimAction, bool, Option<(Color32, Color32)>);

fn machine_panel(app: &mut DeckelApp, ui: &mut Ui) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let playing = app.sim.playback.is_some() && app.sim.mode == 2;
        let keys: [PanelKey; 5] = [
            (
                "ZYKLUS-\nSTART",
                SimAction::Start,
                playing && !app.sim.running,
                Some((Color32::from_rgb(0x2f, 0x9e, 0x44), Color32::WHITE)),
            ),
            (
                "ZYKLUS-\nSTOP",
                SimAction::Stop,
                app.sim.running,
                Some((Color32::from_rgb(0xb3, 0x26, 0x1e), Color32::WHITE)),
            ),
            ("EINZEL-\nSATZ", SimAction::SingleBlock, playing && !app.sim.running, None),
            ("ANFANG", SimAction::Rewind, playing, None),
            ("ENDE", SimAction::ToEnd, playing, None),
        ];
        let key_width = if ui.available_width() > 1250.0 { 112.0 } else { 96.0 };
        for (label, action, enabled, fill) in keys {
            if hardware_key(ui, Vec2::new(key_width, 36.0), "", label, false, enabled, fill).clicked() {
                sim(app, action);
            }
        }
        ui.add_space(8.0);
        let total = app.sim.total();
        let mut fraction = if total > 0.0 { app.sim.time / total } else { 0.0 };
        let reserve = if app.sim.mode == 1 { 700.0 } else { 400.0 };
        let slider_width = (ui.available_width() - reserve).max(50.0);
        ui.spacing_mut().slider_width = slider_width;
        if ui
            .add_enabled(app.sim.playback.is_some(), egui::Slider::new(&mut fraction, 0.0..=1.0).show_value(false))
            .changed()
        {
            sim(app, SimAction::Seek(fraction));
        }
        ui.label(RichText::new(app.sim.time_text()).monospace().color(TEXT));
        if app.sim.mode != 1 {
            ui.label(RichText::new("Tempo").monospace().color(DIM));
            let current = SPEEDS.get(app.sim.speed).map_or("", |s| s.0);
            egui::ComboBox::from_id_salt("sim-speed").selected_text(current).width(96.0).show_ui(ui, |ui| {
                for (index, (label, _)) in SPEEDS.iter().enumerate() {
                    if ui.selectable_label(app.sim.speed == index, *label).clicked() {
                        sim(app, SimAction::Speed(index));
                    }
                }
            });
        }
        if app.sim.mode != 1 {
            return;
        }
        ui.scope(|ui| {
            ui.label(RichText::new("Schritt").monospace().color(DIM));
            egui::ComboBox::from_id_salt("sim-jog")
                .selected_text(format!("{} mm", py_g(app.sim.jog_step())))
                .width(80.0)
                .show_ui(ui, |ui| {
                    for (index, step) in JOG_STEPS.iter().enumerate() {
                        if ui.selectable_label(app.sim.jog == index, format!("{} mm", py_g(*step))).clicked() {
                            sim(app, SimAction::JogStep(index));
                        }
                    }
                });
        });
    });
}

// ------------------------------------------------------------------ dialogs

fn dialog(app: &mut DeckelApp, ctx: &egui::Context) {
    let Some(mut dialog) = app.sim.dialog.clone() else { return };
    let mut ok = false;
    let mut cancel = false;
    let modal = egui::Modal::new(egui::Id::unique("sim-dialog")).show(ctx, |ui| {
        ui.set_max_width(560.0);
        let title = match &dialog {
            SimDialog::Goto { .. } => "13 FAHREN (Simulator)".to_string(),
            SimDialog::Dia { kind, .. } => format!("DIA - {}", menus::dia_form(kind).map_or("", |f| f.default_g)),
            SimDialog::Find { .. } => "EDIT › SUCHEN".to_string(),
            SimDialog::Replace { .. } => "EDIT › ERSETZEN".to_string(),
            SimDialog::CuttingData { .. } => "16 SCHNITTDATEN".to_string(),
        };
        crate::ui::dialog_title(ui, &title);
        let confirm = match &mut dialog {
            SimDialog::Goto { text } => {
                ui.label("Zielposition, z. B. X10 Y20 Z5 (nur Simulator, keine Maschine):");
                let response =
                    ui.add(egui::TextEdit::singleline(text).font(egui::TextStyle::Monospace).desired_width(300.0));
                response.request_focus();
                "FAHREN"
            }
            SimDialog::Dia { kind, values } => {
                ui.label("Nur gewünschte Adressen ausfüllen. Der Satz wird hinter der Cursorzeile eingefügt.");
                if let Some(form) = menus::dia_form(kind) {
                    egui::Grid::new("dia-form").num_columns(2).spacing([10.0, 3.0]).show(ui, |ui| {
                        for ((address, label), value) in form.fields.iter().zip(values.iter_mut()) {
                            ui.label(format!("{label}:"));
                            let hint = if *address == "G" { form.default_g } else { "" };
                            ui.add(
                                egui::TextEdit::singleline(value)
                                    .hint_text(hint)
                                    .font(egui::TextStyle::Monospace)
                                    .desired_width(160.0),
                            );
                            ui.end_row();
                        }
                    });
                    let block = SimPage::dia_block(kind, values, Some(app.sim.next_block_number()));
                    ui.add_space(6.0);
                    ui.label(mono_bold(block, 14.0));
                }
                "ABSCHLUSS (einfügen)"
            }
            SimDialog::Find { text } => {
                ui.label("Suchtext:");
                ui.add(egui::TextEdit::singleline(text).font(egui::TextStyle::Monospace).desired_width(300.0))
                    .request_focus();
                "SUCHEN"
            }
            SimDialog::Replace { old, new } => {
                ui.label("Suchen:");
                ui.add(egui::TextEdit::singleline(old).font(egui::TextStyle::Monospace).desired_width(300.0));
                ui.label("Ersetzen durch:");
                ui.add(egui::TextEdit::singleline(new).font(egui::TextStyle::Monospace).desired_width(300.0));
                "ALLE ERSETZEN"
            }
            SimDialog::CuttingData { diameter, vc, fz, flutes } => {
                egui::Grid::new("cutting-data").num_columns(2).spacing([10.0, 4.0]).show(ui, |ui| {
                    ui.label("Werkzeugdurchmesser D:");
                    ui.add(egui::DragValue::new(diameter).range(0.1..=500.0).speed(0.1).suffix(" mm"));
                    ui.end_row();
                    ui.label("Schnittgeschwindigkeit vc:");
                    ui.add(egui::DragValue::new(vc).range(1.0..=2000.0).suffix(" m/min"));
                    ui.end_row();
                    ui.label("Vorschub je Zahn fz:");
                    ui.add(egui::DragValue::new(fz).range(0.001..=5.0).speed(0.001).max_decimals(3).suffix(" mm"));
                    ui.end_row();
                    ui.label("Schneidenzahl z:");
                    ui.add(egui::DragValue::new(flutes).range(1..=50));
                    ui.end_row();
                });
                let (speed, feed) = cutting_data(*diameter, *vc, *fz, *flutes);
                ui.add_space(6.0);
                ui.label(bold(format!("Drehzahl S = {speed:.0} 1/min    Vorschub F = {feed:.0} mm/min"), 14.0));
                ""
            }
        };
        let (c, o) = crate::ui::dialog_keys(
            ui,
            if confirm.is_empty() { "SCHLIESSEN" } else { "ABBRUCH" },
            confirm,
            !confirm.is_empty(),
            None,
        );
        cancel = c;
        ok = o
            || (!confirm.is_empty()
                && ui.input(|i| i.key_pressed(egui::Key::Enter))
                && !matches!(dialog, SimDialog::Dia { .. }));
    });
    app.sim.dialog = Some(dialog);
    if ok {
        sim(app, SimAction::DialogOk);
    } else if cancel || modal.should_close() {
        sim(app, SimAction::CloseDialog);
    }
}
