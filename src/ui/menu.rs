//! The menu bar and the keyboard shortcuts that go with it.

use eframe::egui;

use super::state::{AppState, UiMode};
use crate::device::{DeviceKind, Mode};
use super::text::t;
use super::{Action, Dialog, FileAction};

const MODES: [(UiMode, egui::Key); 5] = [
    (UiMode::Antenna, egui::Key::A),
    (UiMode::CableLoss, egui::Key::P),
    (UiMode::CableLength, egui::Key::L),
    (UiMode::Transmission, egui::Key::T),
    (UiMode::Generator, egui::Key::G),
];

const F_KEYS: [egui::Key; 10] = [
    egui::Key::F1,
    egui::Key::F2,
    egui::Key::F3,
    egui::Key::F4,
    egui::Key::F5,
    egui::Key::F6,
    egui::Key::F7,
    egui::Key::F8,
    egui::Key::F9,
    egui::Key::F10,
];

/// The shortcut shown next to a band: F1..F10 for the first ten, Ctrl+F1.. for the rest.
fn band_shortcut(index: usize) -> String {
    match index {
        0..=9 => format!("F{}", index + 1),
        10..=18 => format!("Ctrl+F{}", index - 9),
        _ => String::new(),
    }
}

fn item(ui: &mut egui::Ui, text: &str, shortcut: &str) -> bool {
    let clicked = ui.add(egui::Button::new(text).shortcut_text(shortcut.to_owned())).clicked();
    if clicked {
        ui.close();
    }
    clicked
}

pub fn show(ui: &mut egui::Ui, state: &mut AppState, actions: &mut Vec<Action>) {
    egui::MenuBar::new().ui(ui, |ui| {
        ui.menu_button(t("File"), |ui| {
            for (name, shortcut, action) in [
                ("Open...", "Ctrl+O", FileAction::Open),
                ("Save...", "Ctrl+S", FileAction::Save),
                ("Save as CSV (ZPlots)...", "Ctrl+C", FileAction::SaveCsv),
                ("Save graph as PNG...", "Ctrl+N", FileAction::SavePng),
            ] {
                if item(ui, t(name), shortcut) {
                    actions.push(Action::File(action));
                }
            }
            ui.separator();
            if item(ui, t("Exit"), "Ctrl+Q") {
                actions.push(Action::Quit);
            }
        });

        ui.menu_button(t("Modes"), |ui| {
            for (mode, key) in MODES {
                let shortcut = format!("Ctrl+{}", key.name());
                let button = egui::Button::new(t(mode.name())).selected(state.mode == mode).shortcut_text(shortcut);
                if ui.add_enabled(state.mode_available(mode), button).clicked() {
                    actions.push(Action::SetMode(mode));
                    ui.close();
                }
            }
        });

        ui.menu_button(t("Bands predefined"), |ui| {
            let minivna = state.device() == DeviceKind::MiniVna;
            if ui.add_enabled(minivna, egui::Button::new("160 - 10 m").shortcut_text("Ctrl+D")).clicked() {
                actions.push(Action::HfRange);
                ui.close();
            }
            if item(ui, if minivna { "1 - 180 MHz" } else { "35 - 4400 MHz" }, "Ctrl+F") {
                actions.push(Action::FullRange);
            }
            ui.separator();
            for (index, band) in state.settings.bands.iter().enumerate() {
                let reachable = state.band_reachable(index);
                let button = egui::Button::new(band.name.as_str())
                    .selected(state.current_band == Some(index))
                    .shortcut_text(band_shortcut(index));
                if ui.add_enabled(reachable, button).clicked() {
                    actions.push(Action::Band(index));
                    ui.close();
                }
            }
        });

        if state.settings.extender_menu && state.device() == DeviceKind::MiniVna {
            ui.menu_button(t("Frequency Extender"), |ui| {
                for (mult, name) in [(1u8, t("Disabled")), (4, "x 4"), (8, "x 8"), (16, "x 16")] {
                    if ui.add(egui::Button::new(name).selected(state.extender == mult)).clicked() {
                        actions.push(Action::Extender(mult));
                        ui.close();
                    }
                }
            });
        }

        ui.menu_button(t("Trace options"), |ui| {
            ui.checkbox(&mut state.settings.line_50_ohms, t("Draw 50 ohms line (for Z, Rs and Xs)"));
            ui.checkbox(&mut state.settings.vertical_lines, t("Draw vertical frequency lines"));
            ui.checkbox(&mut state.settings.m1_at_start, t("Activate Marker 1 at program start"));
            ui.checkbox(&mut state.settings.markers_from_band, t("Position M1 and M2 by the band's defaults"));
            ui.separator();
            let mut signed = state.settings.signed_phase;
            let has_phase = state.device() == DeviceKind::MiniVna;
            if ui.add_enabled(has_phase, egui::Checkbox::new(&mut signed, t("Estimate sign of phase and Xs"))).changed() {
                actions.push(Action::SetSigned(signed));
            }
        });

        ui.menu_button(t("Configure"), |ui| {
            let entries: &[(&str, Dialog)] = match state.device() {
                DeviceKind::MiniVna => &[
                    (t("Device and Com Port"), Dialog::Port),
                    (t("Calibrate DDS frequency"), Dialog::Dds),
                    (t("Calibrate Reflection"), Dialog::Calibrate(Mode::Reflection)),
                    (t("Calibrate Transmission"), Dialog::Calibrate(Mode::Transmission)),
                ],
                DeviceKind::Nwt => &[(t("Device and Com Port"), Dialog::Port), (t("Calibrate Reference (0 dB)"), Dialog::CalibrateNwt)],
            };
            for (name, dialog) in entries {
                if item(ui, name, "") {
                    actions.push(Action::Open(*dialog));
                }
            }
            ui.separator();
            for (name, dialog) in [(t("Bands predefined"), Dialog::Bands), (t("Colors"), Dialog::Colors), (t("Usual Velocity Factors"), Dialog::Velocity)] {
                if item(ui, name, "") {
                    actions.push(Action::Open(dialog));
                }
            }
            ui.separator();
            ui.checkbox(&mut state.settings.step_bug, t("Compensate for firmware Step bug"));
            // Hiding the menu also switches the extender off (nothing happens if it was off).
            if ui.checkbox(&mut state.settings.extender_menu, t("Enable menu of Frequency Extender")).changed()
                && !state.settings.extender_menu
            {
                actions.push(Action::Extender(1));
            }
        });

        if ui.button(t("LC calculator")).clicked() {
            actions.push(Action::Open(Dialog::Lc));
        }
        if ui.button(t("About")).clicked() {
            actions.push(Action::Open(Dialog::About));
        }
    });
}

/// Keyboard shortcuts of the menus: function keys alone, letters with Ctrl.
/// None of them act while the cursor is in a text box, where Ctrl+A, Ctrl+C and the
/// like belong to the text.
pub fn shortcuts(ui: &egui::Ui, state: &AppState, actions: &mut Vec<Action>) {
    if ui.ctx().text_edit_focused() {
        return;
    }
    let ctrl = egui::Modifiers::COMMAND;
    ui.input_mut(|input| {
        // The toolkit turns Ctrl+C into a "copy" event instead of a key.
        if input.events.iter().any(|event| matches!(event, egui::Event::Copy)) {
            actions.push(Action::File(FileAction::SaveCsv));
        }
        if input.consume_key(ctrl, egui::Key::N) {
            actions.push(Action::File(FileAction::SavePng));
        }
        for (index, key) in F_KEYS.into_iter().enumerate() {
            if index + 10 < state.settings.bands.len() && index < 9 && input.consume_key(ctrl, key) {
                actions.push(Action::Band(index + 10));
            } else if input.consume_key(egui::Modifiers::NONE, key) {
                actions.push(Action::Band(index));
            }
        }
        for (mode, key) in MODES {
            if input.consume_key(ctrl, key) {
                actions.push(Action::SetMode(mode));
            }
        }
        if input.consume_key(ctrl, egui::Key::D) {
            actions.push(Action::HfRange);
        }
        if input.consume_key(ctrl, egui::Key::F) {
            actions.push(Action::FullRange);
        }
        if input.consume_key(ctrl, egui::Key::O) {
            actions.push(Action::File(FileAction::Open));
        }
        if input.consume_key(ctrl, egui::Key::S) {
            actions.push(Action::File(FileAction::Save));
        }
        if input.consume_key(ctrl, egui::Key::Q) {
            actions.push(Action::Quit);
        }
    });
}

/// Space = one sweep, F12 = continuous, Esc = stop.
/// Space is left alone while anything has the keyboard (a text box takes it as a blank, a
/// focused button as a press), and Esc while the small entry window is open, which it closes.
pub fn sweep_keys(ui: &egui::Ui, entry_open: bool, actions: &mut Vec<Action>) {
    let something_focused = ui.ctx().memory(|m| m.focused().is_some());
    ui.input(|input| {
        if input.key_pressed(egui::Key::Escape) && !entry_open {
            actions.push(Action::Stop);
        }
        if input.key_pressed(egui::Key::F12) {
            actions.push(Action::SweepContinuous);
        }
        if input.key_pressed(egui::Key::Space) && !something_focused {
            actions.push(Action::SweepOnce);
        }
    });
}

pub fn about(ctx: &egui::Context, open: &mut bool) {
    egui::Window::new(t("About")).open(open).collapsible(false).resizable(false).show(ctx, |ui| {
        ui.heading(format!("minivna {}", env!("CARGO_PKG_VERSION")));
        ui.label(t("For the mRS miniVNA antenna analyser."));
        ui.label(t("A rewrite by IW1GEU (Audric) of IG_miniVNA by F4CLB (Jean-Louis), whose measurement methods it follows."));
    });
}
