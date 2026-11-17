//! The table of band presets.

use eframe::egui;

use crate::ui::state::{AppState, BandField};
use crate::ui::text::t;

const FIELDS: [BandField; 4] = [BandField::Start, BandField::Stop, BandField::M1, BandField::M2];

#[derive(Default)]
pub struct BandsDialog {
    pub open: bool,
    /// The cell being typed in: band, column, text so far.
    editing: Option<(usize, usize, String)>,
}

fn shown(state: &AppState, band: usize, column: usize) -> String {
    let b = &state.settings.bands[band];
    let mhz = [b.start_mhz, b.stop_mhz, b.m1_mhz, b.m2_mhz][column];
    format!("{mhz}")
}

impl BandsDialog {
    pub fn show(&mut self, ctx: &egui::Context, state: &mut AppState) {
        let mut open = self.open;
        egui::Window::new(t("Bands predefined")).open(&mut open).collapsible(false).show(ctx, |ui| {
            egui::ScrollArea::vertical().max_height(480.0).show(ui, |ui| {
                egui::Grid::new("bands").num_columns(5).striped(true).show(ui, |ui| {
                    for header in ["Name", "Begin", "End", "Mark 1", "Mark 2"] {
                        ui.label(t(header));
                    }
                    ui.end_row();
                    for band in 0..state.settings.bands.len() {
                        ui.add(egui::TextEdit::singleline(&mut state.settings.bands[band].name).desired_width(110.0));
                        for (column, field) in FIELDS.into_iter().enumerate() {
                            let mut text = match &self.editing {
                                Some((b, c, text)) if (*b, *c) == (band, column) => text.clone(),
                                _ => shown(state, band, column),
                            };
                            let response = ui.add(egui::TextEdit::singleline(&mut text).desired_width(70.0));
                            if response.lost_focus() {
                                // A cell that is not a usable frequency goes back to its old value.
                                state.set_band_field(band, field, &text);
                                self.editing = None;
                            } else if response.has_focus() {
                                self.editing = Some((band, column, text));
                            }
                        }
                        ui.end_row();
                    }
                });
            });
            ui.label(t("Frequencies in MHz. Bands above 180 MHz need a frequency extender."));
        });
        self.open = open;
        if !open {
            self.editing = None;
        }
    }
}
