//! Calibrating the frequency of the device's oscillator against a frequency counter.

use eframe::egui;

use crate::sweep::Command;
use crate::ui::state::{generator_freq_step, nudge_dds, parse_mhz, AppState};
use crate::ui::text::t;
use crate::ui::Action;

const NUDGES: [f64; 4] = [0.001, 0.0001, 0.00001, 0.000001];

#[derive(Default)]
pub struct DdsDialog {
    pub open: bool,
    trial: f64,
    freq_text: String,
    freq_hz: u64,
}

impl DdsDialog {
    pub fn open(&mut self, state: &AppState, actions: &mut Vec<Action>) {
        self.open = true;
        self.trial = state.settings.dds_factor;
        self.freq_hz = 10_000_000;
        self.freq_text = "10".to_string();
        self.send(actions);
    }

    fn send(&self, actions: &mut Vec<Action>) {
        actions.push(Action::Send(Command::Generator { kind: crate::device::DeviceKind::MiniVna, freq_hz: self.freq_hz, extender: 1, dds_factor: self.trial }));
    }

    pub fn show(&mut self, ctx: &egui::Context, state: &mut AppState, actions: &mut Vec<Action>) {
        if !self.open {
            return;
        }
        let mut done = None;
        egui::Window::new(t("Calibrate DDS :")).collapsible(false).resizable(false).show(ctx, |ui| {
            ui.label(t("Connect a frequency counter to DUT and adjust until it reads the frequency below."));
            ui.horizontal(|ui| {
                ui.label(t("Frequency"));
                if ui.add(egui::TextEdit::singleline(&mut self.freq_text).desired_width(80.0)).lost_focus() {
                    if let Some(mhz) = parse_mhz(&self.freq_text) {
                        self.freq_hz = generator_freq_step((mhz * 1e6).round() as u64, 0, 1);
                    }
                    self.freq_text = crate::ui::state::mhz_text(self.freq_hz);
                    self.send(actions);
                }
                ui.label("MHz");
            });
            ui.horizontal(|ui| {
                ui.label(t("Correction"));
                ui.monospace(format!("{:.8}", self.trial));
            });
            egui::Grid::new("dds nudges").num_columns(2).show(ui, |ui| {
                for step in NUDGES {
                    for signed in [-step, step] {
                        if ui.button(format!("{signed:+}")).clicked() {
                            self.trial = nudge_dds(self.trial, signed);
                            self.send(actions);
                        }
                    }
                    ui.end_row();
                }
            });
            ui.horizontal(|ui| {
                if ui.button(t("Ok")).clicked() {
                    done = Some(true);
                }
                if ui.button(t("Cancel")).clicked() {
                    done = Some(false);
                }
            });
        });
        if let Some(keep) = done {
            if keep {
                state.settings.dds_factor = self.trial;
            }
            actions.push(Action::Send(Command::Park));
            self.open = false;
        }
    }
}
