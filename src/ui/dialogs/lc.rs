//! The LC calculator: which capacitor resonates with a coil at marker 1's frequency,
//! or which coil with a capacitor.

use eframe::egui;

use crate::ui::state::{field, parse_mhz, AppState, M1Target};
use crate::ui::text::t;

pub struct LcDialog {
    pub open: bool,
    text: String,
    input_is_uh: bool,
    answer: String,
}

impl Default for LcDialog {
    fn default() -> Self {
        LcDialog { open: false, text: "15".to_string(), input_is_uh: true, answer: String::new() }
    }
}

impl LcDialog {
    pub fn show(&mut self, ctx: &egui::Context, state: &mut AppState) {
        let mut open = self.open;
        egui::Window::new(t("LC calculator")).open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
            let frequency = state.m1.filter(|&i| i < state.points().len()).map(|i| state.marker_row(i)[0].clone());
            ui.label(format!("{} {} MHz", t("Marker 1 :"), frequency.as_deref().unwrap_or("-")));
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.text).desired_width(70.0));
                ui.radio_value(&mut self.input_is_uh, true, "µH");
                ui.radio_value(&mut self.input_is_uh, false, "pF");
                if ui.button("=").clicked() {
                    // parse_mhz accepts any plain positive number, with a point or a comma.
                    self.answer = match parse_mhz(&self.text).and_then(|value| state.lc_result(value)) {
                        Some(result) => format!("{} {}", field(result), if self.input_is_uh { "pF" } else { "µH" }),
                        None => t("Needs marker 1 and a positive value.").to_string(),
                    };
                }
                ui.label(&self.answer);
            });
            ui.separator();
            ui.horizontal(|ui| {
                for (name, target) in [
                    ("M1 -> SWR Mini.", M1Target::MinSwr),
                    ("M1 -> R.L. Max.", M1Target::MaxRl),
                    ("M1 -> R.L. Mini.", M1Target::MinRl),
                ] {
                    if ui.button(t(name)).clicked() {
                        state.move_m1_to(target);
                    }
                }
            });
        });
        self.open = open;
    }
}
