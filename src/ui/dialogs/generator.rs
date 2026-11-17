//! Generator mode: a fixed frequency on the DUT port.

use eframe::egui;

use crate::sweep::Command;
use crate::ui::state::{generator_freq_step, AppState, UiMode};
use crate::ui::text::t;
use crate::ui::Action;

/// One column of up/down buttons per digit, from 100 MHz down to 1 Hz.
const STEPS: [i64; 9] = [100_000_000, 10_000_000, 1_000_000, 100_000, 10_000, 1_000, 100, 10, 1];

pub struct GeneratorDialog {
    pub open: bool,
    freq_hz: u64,
}

impl Default for GeneratorDialog {
    fn default() -> Self {
        GeneratorDialog { open: false, freq_hz: 10_000_000 }
    }
}

impl GeneratorDialog {
    pub fn open(&mut self, state: &AppState, actions: &mut Vec<Action>) {
        self.open = true;
        self.freq_hz = generator_freq_step(self.freq_hz, 0, state.extender);
        self.send(state, actions);
    }

    fn send(&self, state: &AppState, actions: &mut Vec<Action>) {
        actions.push(Action::Send(Command::Generator {
            freq_hz: self.freq_hz,
            extender: state.extender,
            dds_factor: state.settings.dds_factor,
        }));
    }

    pub fn show(&mut self, ctx: &egui::Context, state: &AppState, actions: &mut Vec<Action>) {
        if !self.open {
            return;
        }
        let mut close = false;
        egui::Window::new(t("Generator :")).collapsible(false).resizable(false).show(ctx, |ui| {
            let digits = format!("{:010}", self.freq_hz);
            let mut delta = 0i64;
            egui::Grid::new("generator digits").spacing([2.0, 2.0]).show(ui, |ui| {
                for step in STEPS {
                    if ui.small_button("+").clicked() {
                        delta = step;
                    }
                }
                ui.end_row();
                for (i, _) in STEPS.iter().enumerate() {
                    ui.label(egui::RichText::new(&digits[i + 1..i + 2]).monospace().size(22.0));
                }
                ui.label("Hz");
                ui.end_row();
                for step in STEPS {
                    if ui.small_button("-").clicked() {
                        delta = -step;
                    }
                }
                ui.end_row();
            });
            if delta != 0 {
                self.freq_hz = generator_freq_step(self.freq_hz, delta, state.extender);
                self.send(state, actions);
            }
            ui.label(format!("{:.6} MHz", self.freq_hz as f64 / 1e6));
            close = ui.button(t("Exit")).clicked();
        });
        if close {
            self.open = false;
            actions.push(Action::Send(Command::Park));
            actions.push(Action::SetMode(UiMode::Antenna));
        }
    }
}
