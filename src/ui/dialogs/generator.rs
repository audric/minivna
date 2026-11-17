//! Generator mode: a fixed frequency on the DUT port.

use eframe::egui;

use crate::sweep::Command;
use crate::ui::state::{generator_freq_step_for, AppState};
use crate::ui::text::t;
use crate::ui::Action;

/// One column of up/down buttons per digit, from 1 GHz (reached with an extender) down to 1 Hz.
const STEPS: [i64; 10] = [1_000_000_000, 100_000_000, 10_000_000, 1_000_000, 100_000, 10_000, 1_000, 100, 10, 1];

/// The frequency as ten digits, one per column of buttons.
fn digits(freq_hz: u64) -> String {
    format!("{:010}", freq_hz.min(9_999_999_999))
}

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
        self.freq_hz = generator_freq_step_for(state.device(), self.freq_hz, 0, state.extender);
        self.send(state, actions);
    }

    fn send(&self, state: &AppState, actions: &mut Vec<Action>) {
        actions.push(Action::Send(Command::Generator {
            kind: state.settings.device,
            freq_hz: self.freq_hz,
            extender: state.extender,
            dds_factor: state.settings.dds_factor,
        }));
    }

    pub fn freq_hz(&self) -> u64 {
        self.freq_hz
    }

    pub fn show(&mut self, ctx: &egui::Context, state: &AppState, actions: &mut Vec<Action>) {
        if !self.open {
            return;
        }
        let mut close = false;
        egui::Window::new(t("Generator :")).collapsible(false).resizable(false).show(ctx, |ui| {
            let digits = digits(self.freq_hz);
            let mut delta = 0i64;
            egui::Grid::new("generator digits").spacing([2.0, 2.0]).show(ui, |ui| {
                for step in STEPS {
                    if ui.small_button("+").clicked() {
                        delta = step;
                    }
                }
                ui.end_row();
                for (i, _) in STEPS.iter().enumerate() {
                    ui.label(egui::RichText::new(&digits[i..i + 1]).monospace().size(22.0));
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
                self.freq_hz = generator_freq_step_for(state.device(), self.freq_hz, delta, state.extender);
                self.send(state, actions);
            }
            ui.label(format!("{:.6} MHz", self.freq_hz as f64 / 1e6));
            close = ui.button(t("Exit")).clicked();
        });
        if close {
            self.open = false;
            actions.push(Action::Send(Command::Park));
            actions.push(Action::SetMode(state.idle_mode()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_digit_up_to_the_gigahertz_is_shown() {
        assert_eq!(digits(1_440_000_000), "1440000000");
        assert_eq!(digits(10_000_000), "0010000000");
        assert_eq!(STEPS.len(), 10);
        assert_eq!(STEPS[0], 1_000_000_000);
    }
}
