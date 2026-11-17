//! The read-outs above the graph: marker rows, inductance/capacitance, bandwidth and Q.

use eframe::egui;

use super::state::{parse_decimal, parse_mhz, AppState, Big, Entry, Marker, UiMode};
use super::text::t;
use super::{color32, Action};

const FIELD_WIDTH: f32 = 72.0;
/// Wide enough for "2880.000000" (a frequency with the x16 extender).
const FREQ_WIDTH: f32 = 94.0;

pub fn show(ui: &mut egui::Ui, state: &mut AppState, actions: &mut Vec<Action>) {
    ui.add_space(2.0);
    ui.horizontal(|ui| {
        rows(ui, state);
        ui.separator();
        match state.mode {
            UiMode::CableLoss => cable_loss(ui, state),
            UiMode::CableLength => cable_length(ui, state, actions),
            _ => between_markers(ui, state),
        }
    });
    ui.add_space(2.0);
}

/// A read-out field: a framed box in the field colour, so it stands out from the panel.
fn value(ui: &mut egui::Ui, text: &str) -> egui::Response {
    value_wide(ui, text, FIELD_WIDTH)
}

/// A read-out field of at least `min_width`; it grows rather than cut its text.
fn value_wide(ui: &mut egui::Ui, text: &str, min_width: f32) -> egui::Response {
    let color = ui.visuals().strong_text_color();
    let galley = ui.painter().layout_no_wrap(text.to_owned(), egui::FontId::monospace(12.0), color);
    let width = min_width.max(galley.size().x + 10.0);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 18.0), egui::Sense::click());
    let visuals = ui.visuals();
    ui.painter().rect(rect, 2.0, visuals.extreme_bg_color, visuals.widgets.noninteractive.bg_stroke, egui::StrokeKind::Inside);
    let pos = egui::pos2(rect.right() - 5.0 - galley.size().x, rect.center().y - galley.size().y / 2.0);
    ui.painter().galley(pos, galley, color);
    response
}

fn swatch(ui: &mut egui::Ui, color: egui::Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, 0.0, color);
}

fn rows(ui: &mut egui::Ui, state: &mut AppState) {
    let colors = state.settings.screen.clone();
    let reactance = state.reactance_fields();
    egui::Grid::new("marker rows").num_columns(12).spacing([6.0, 2.0]).show(ui, |ui| {
        let xs_header = match &reactance {
            Some((true, ..)) => "| Xs | +j",
            Some((false, ..)) => "| Xs | -j",
            None => "| Xs |",
        };
        let last_header = match &reactance {
            Some((true, ..)) => "XL \u{b5}H",
            Some((false, ..)) => "Xc pF",
            None => "---",
        };
        let loss_header = if state.mode == UiMode::Transmission { "T.L." } else { "R.L." };
        for header in ["", "", "", "Frequency", "SWR", "| Z |", loss_header, "Phase", "Rs", xs_header, last_header, "Visual."] {
            ui.label(t_dynamic(header));
        }
        ui.end_row();

        for (marker, name, color) in [(Marker::M1, "Marker 1", colors.m1), (Marker::M2, "Marker 2", colors.m2)] {
            let index = match marker {
                Marker::M1 => state.m1,
                Marker::M2 => state.m2,
            };
            let mut on = index.is_some();
            if ui.checkbox(&mut on, t_dynamic(name)).changed() {
                state.toggle_marker(marker);
            }
            swatch(ui, color32(color));
            ui.horizontal(|ui| {
                if ui.small_button("-").clicked() {
                    state.step_marker(marker, -1);
                }
                if ui.small_button("+").clicked() {
                    state.step_marker(marker, 1);
                }
            });
            let row = index.map(|i| state.marker_row(i)).unwrap_or_default();
            if value_wide(ui, &row[0], FREQ_WIDTH).on_hover_text(t("Double-click to type the frequency")).double_clicked() && index.is_some() {
                state.entry = Some((Entry::Freq(marker), row[0].clone()));
            }
            for text in &row[1..] {
                value(ui, text);
            }
            let extra = reactance.as_ref().map(|(_, a, b)| if marker == Marker::M1 { a.as_str() } else { b.as_str() });
            value(ui, extra.unwrap_or(""));
            big_box(ui, state, if marker == Marker::M1 { Big::M1 } else { Big::M2 });
            ui.end_row();
        }

        ui.checkbox(&mut state.show_swr_min, t("SWR Mini."));
        swatch(ui, color32(colors.swr_min));
        ui.label("");
        let row = if state.show_swr_min { state.swr_min().map(|i| state.marker_row(i)) } else { None }.unwrap_or_default();
        value_wide(ui, &row[0], FREQ_WIDTH);
        for text in &row[1..] {
            value(ui, text);
        }
        ui.label("");
        big_box(ui, state, Big::SwrMin);
        ui.end_row();
    });
}

/// "Visual": show this row large on the graph. Only one row at a time.
fn big_box(ui: &mut egui::Ui, state: &mut AppState, row: Big) {
    let mut on = state.big == Some(row);
    if ui.checkbox(&mut on, "").changed() {
        state.big = on.then_some(row);
    }
}

fn between_markers(ui: &mut egui::Ui, state: &AppState) {
    let (bw, q) = state.bandwidth_fields().unwrap_or_default();
    ui.vertical(|ui| {
        ui.label(t("Marker 1 to 2"));
        egui::Grid::new("between").num_columns(3).show(ui, |ui| {
            ui.label(t("Band Width"));
            value(ui, &bw);
            ui.label("Hz");
            ui.end_row();
            ui.label("Q");
            value(ui, &q);
            ui.end_row();
        });
    });
}

fn cable_loss(ui: &mut egui::Ui, state: &AppState) {
    let [min, avg, max] = state.cable_loss_fields().unwrap_or_default();
    ui.vertical(|ui| {
        ui.label(t("Cable loss dB"));
        egui::Grid::new("cable loss").num_columns(2).show(ui, |ui| {
            for (name, text) in [(t("Min."), &min), (t("Avg."), &avg), (t("Max."), &max)] {
                ui.label(name);
                value(ui, text);
                ui.end_row();
            }
        });
    });
}

fn cable_length(ui: &mut egui::Ui, state: &mut AppState, actions: &mut Vec<Action>) {
    ui.vertical(|ui| {
        egui::Grid::new("cable length").num_columns(2).show(ui, |ui| {
            ui.label(t("Meters"));
            value(ui, &state.cable_length_text().unwrap_or_default());
            ui.end_row();
            ui.label(t("Velocity Factor"));
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(&mut state.settings.velocity).range(0.1..=1.0).speed(0.005).fixed_decimals(2).custom_parser(parse_decimal));
                if ui.small_button("?").on_hover_text(t("Usual Velocity Factors")).clicked() {
                    actions.push(Action::Open(super::Dialog::Velocity));
                }
            });
            ui.end_row();
        });
        ui.horizontal(|ui| {
            if ui.add_enabled(!state.busy, egui::Button::new(t("Find automatically"))).clicked() {
                actions.push(Action::AutoLength);
            }
            if ui.button(t("L (m) -> Velo.")).clicked() {
                state.entry = Some((Entry::Length, String::new()));
            }
        });
    });
}

/// Like `t`, for texts chosen at run time from a fixed set.
fn t_dynamic(text: &str) -> String {
    text.to_string()
}

/// The small window for typing a marker's frequency or a cable length.
pub fn entry_dialog(ctx: &egui::Context, state: &mut AppState) {
    let Some((kind, mut text)) = state.entry.take() else { return };
    let title = match kind {
        Entry::Freq(_) => t("Frequency in MHz"),
        Entry::Length => t("Cable length in meters"),
    };
    let mut keep = true;
    let mut apply = false;
    egui::Window::new(title)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            let edit = ui.text_edit_singleline(&mut text);
            edit.request_focus();
            let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
            ui.horizontal(|ui| {
                if ui.button(t("Ok")).clicked() || enter {
                    apply = true;
                }
                if ui.button(t("Cancel")).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    keep = false;
                }
            });
        });
    if apply {
        // parse_mhz accepts any plain positive number, with a point or a comma.
        match (kind, parse_mhz(&text)) {
            (Entry::Freq(marker), Some(mhz)) => state.place_marker(marker, mhz * 1e6),
            (Entry::Length, Some(metres)) => {
                if !state.set_velocity_from_length(metres) {
                    state.status = t("That length does not fit the two markers.").to_string();
                }
            }
            (_, None) => {}
        }
    } else if keep {
        state.entry = Some((kind, text));
    }
}
