//! The table of usual velocity factors.

use eframe::egui;

use crate::ui::state::AppState;
use crate::ui::text::t;

#[derive(Default)]
pub struct VelocityDialog {
    pub open: bool,
}

impl VelocityDialog {
    pub fn show(&mut self, ctx: &egui::Context, state: &mut AppState) {
        egui::Window::new(t("Usual Velocity Factors")).open(&mut self.open).collapsible(false).show(ctx, |ui| {
            ui.label(format!("{} {:.2}", t("Velocity Factor"), state.settings.velocity));
            let mut chosen = None;
            egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
                egui::Grid::new("velocities").num_columns(3).striped(true).show(ui, |ui| {
                    for row in &mut state.settings.velocities {
                        if ui.button(t("Use")).clicked() {
                            chosen = Some(row.factor);
                        }
                        ui.add(egui::DragValue::new(&mut row.factor).range(0.1..=1.0).speed(0.005).fixed_decimals(2));
                        ui.add(egui::TextEdit::singleline(&mut row.note).desired_width(260.0));
                        ui.end_row();
                    }
                });
            });
            if let Some(factor) = chosen {
                state.settings.velocity = factor;
            }
        });
    }
}
