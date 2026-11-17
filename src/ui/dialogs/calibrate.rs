//! The calibration steps, with the wording of the original program.

use eframe::egui;

use crate::device::Mode;
use crate::sweep::Command;
use crate::ui::state::AppState;
use crate::ui::text::t;
use crate::ui::Action;

#[derive(Default, PartialEq)]
enum Step {
    #[default]
    Closed,
    Instruction,
    Measuring,
    Done,
    Failed(String),
}

#[derive(Default)]
pub struct CalDialog {
    step: Step,
    mode: Option<Mode>,
}

impl CalDialog {
    pub fn open(&mut self, mode: Mode) {
        self.mode = Some(mode);
        self.step = Step::Instruction;
    }

    pub fn is_measuring(&self) -> bool {
        self.step == Step::Measuring
    }

    /// The worker delivered the table and it has been stored (or storing failed).
    pub fn finished(&mut self, outcome: Result<(), String>) {
        if self.step == Step::Measuring {
            self.step = match outcome {
                Ok(()) => Step::Done,
                Err(why) => Step::Failed(why),
            };
        }
    }

    pub fn show(&mut self, ctx: &egui::Context, state: &AppState, actions: &mut Vec<Action>) {
        let Some(mode) = self.mode else { return };
        if self.step == Step::Closed {
            return;
        }
        let (title, instruction) = match mode {
            Mode::Transmission => (t("Transmission Calibrate :"), t("Connect DUT and DET with the shortest possible link,")),
            Mode::Reflection => (t("Reflection Calibrate :"), t("Remove any connection between DUT and DET,")),
        };
        let mut close = false;
        egui::Window::new(title).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            match &self.step {
                Step::Instruction => {
                    ui.label(instruction);
                    ui.label(t("then click on Ok."));
                    ui.horizontal(|ui| {
                        if ui.add_enabled(!state.busy, egui::Button::new(t("Ok"))).clicked() {
                            actions.push(Action::Send(Command::Calibrate(mode, state.settings.step_bug, state.settings.dds_factor)));
                            self.step = Step::Measuring;
                        }
                        close = ui.button(t("Cancel")).clicked();
                    });
                }
                Step::Measuring => {
                    ui.label(t("Measuring..."));
                    ui.add(egui::ProgressBar::new(state.progress).show_percentage());
                }
                Step::Done => {
                    ui.label(t("Ended operations."));
                    ui.label(t("Your MiniVNA is calibrated."));
                    close = ui.button(t("Ok")).clicked();
                }
                Step::Failed(why) => {
                    ui.label(t("The calibration did not work:"));
                    ui.label(why);
                    close = ui.button(t("Ok")).clicked();
                }
                Step::Closed => {}
            }
        });
        if close {
            self.step = Step::Closed;
        }
    }
}
