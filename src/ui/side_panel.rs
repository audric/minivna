//! The left column: sweep buttons, frequency range, curve choices, loss scale.

use eframe::egui;

use super::plot::{curve_color, curve_name};
use super::state::{AppState, Curve};
use super::text::t;
use super::{color32, Action};

pub fn show(ui: &mut egui::Ui, state: &mut AppState, actions: &mut Vec<Action>) {
    ui.set_width(150.0);
    ui.add_space(4.0);

    ui.add_enabled_ui(!state.busy, |ui| {
        if ui.add_sized([140.0, 26.0], egui::Button::new(t("x Sweep"))).clicked() {
            actions.push(Action::SweepContinuous);
        }
        if ui.add_sized([140.0, 26.0], egui::Button::new(t("1 Sweep"))).clicked() {
            actions.push(Action::SweepOnce);
        }
    });
    ui.add_enabled_ui(state.busy, |ui| {
        if ui.add_sized([140.0, 22.0], egui::Button::new(t("Stop"))).clicked() {
            actions.push(Action::Stop);
        }
    });

    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
        let lamp = if state.busy { egui::Color32::from_rgb(0, 220, 0) } else { egui::Color32::DARK_GRAY };
        ui.painter().circle_filled(rect.center(), 6.0, lamp);
        ui.label(t("Read"));
        if state.busy {
            ui.label(format!("{:.0}%", state.progress * 100.0));
        }
    });

    ui.separator();
    ui.label(t("Frequency :"));
    let mut committed = false;
    egui::Grid::new("range").num_columns(3).show(ui, |ui| {
        ui.label(t("of"));
        committed |= edit(ui, &mut state.start_text);
        ui.label("MHz");
        ui.end_row();
        ui.label(t("to"));
        committed |= edit(ui, &mut state.stop_text);
        ui.label("MHz");
        ui.end_row();
        ui.label(t("Steps"));
        committed |= edit(ui, &mut state.samples_text);
        ui.end_row();
    });
    if committed {
        state.commit_range();
    }
    ui.small(state.step_text());

    ui.separator();
    ui.label(t("Graphic :"));
    let colors = state.settings.screen.clone();
    for (i, curve) in Curve::ALL.into_iter().enumerate() {
        ui.horizontal(|ui| {
            ui.checkbox(&mut state.show[i], curve_name(curve));
            let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
            ui.painter().rect_filled(rect, 0.0, color32(curve_color(&colors, curve)));
        });
    }
    ui.horizontal(|ui| {
        ui.radio_value(&mut state.settings.thick_lines, false, t("Thin"));
        ui.radio_value(&mut state.settings.thick_lines, true, t("Thick"));
    });

    ui.separator();
    ui.label(t("R.L. :"));
    ui.radio_value(&mut state.scale_60db, false, t("Scale 30dB"));
    ui.radio_value(&mut state.scale_60db, true, t("Scale 60dB"));
    ui.horizontal(|ui| {
        ui.label(t("Offset"));
        ui.add(egui::DragValue::new(&mut state.rl_offset_db).range(-60.0..=60.0).speed(0.5).suffix(" dB"));
    });
}

/// A small text box. True when the user finished editing it.
fn edit(ui: &mut egui::Ui, text: &mut String) -> bool {
    ui.add(egui::TextEdit::singleline(text).desired_width(62.0)).lost_focus()
}
