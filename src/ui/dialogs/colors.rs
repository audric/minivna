//! Colours of the graph, on screen and in saved pictures.

use eframe::egui;

use crate::store::{Colors, Rgb};
use crate::ui::state::AppState;
use crate::ui::text::t;

#[derive(Default)]
pub struct ColorsDialog {
    pub open: bool,
}

fn slots(c: &mut Colors) -> [(&'static str, &mut Rgb); 12] {
    [
        ("SWR", &mut c.swr),
        ("| Z |", &mut c.z),
        ("R.L.", &mut c.rl),
        ("Phase", &mut c.phase),
        ("Rs", &mut c.rs),
        ("| Xs |", &mut c.xs),
        ("Background", &mut c.background),
        ("Frame", &mut c.frame),
        ("50 ohms line", &mut c.ohms),
        ("Marker 1", &mut c.m1),
        ("Marker 2", &mut c.m2),
        ("SWR Mini.", &mut c.swr_min),
    ]
}

fn picker(ui: &mut egui::Ui, color: &mut Rgb) {
    let mut rgb = [color.0, color.1, color.2];
    if ui.color_edit_button_srgb(&mut rgb).changed() {
        *color = Rgb(rgb[0], rgb[1], rgb[2]);
    }
}

impl ColorsDialog {
    pub fn show(&mut self, ctx: &egui::Context, state: &mut AppState) {
        egui::Window::new(t("Colors")).open(&mut self.open).collapsible(false).resizable(false).show(ctx, |ui| {
            let settings = &mut state.settings;
            egui::Grid::new("colors").num_columns(3).show(ui, |ui| {
                ui.label("");
                ui.label(t("Screen"));
                ui.label(t("PNG"));
                ui.end_row();
                for ((name, screen), (_, png)) in slots(&mut settings.screen).into_iter().zip(slots(&mut settings.png)) {
                    ui.label(t(name));
                    picker(ui, screen);
                    picker(ui, png);
                    ui.end_row();
                }
            });
            if ui.button(t("Default colors")).clicked() {
                let fresh = crate::store::Settings::default();
                (settings.screen, settings.png) = (fresh.screen, fresh.png);
            }
        });
    }
}
