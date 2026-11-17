//! Choosing the serial port, with an automatic search.

use std::sync::mpsc::{self, Receiver};

use eframe::egui;

use crate::device::{find_device, list_ports, DeviceKind};
use crate::sweep::PortChoice;
use crate::ui::state::AppState;
use crate::ui::text::t;
use crate::ui::Action;

#[derive(Default)]
pub struct PortDialog {
    pub open: bool,
    ports: Vec<String>,
    search: Option<Receiver<Option<String>>>,
    message: String,
}

impl PortDialog {
    pub fn open(&mut self) {
        self.open = true;
        self.ports = list_ports();
        self.message.clear();
    }

    pub fn show(&mut self, ctx: &egui::Context, state: &AppState, current: &PortChoice, actions: &mut Vec<Action>) {
        if !self.open {
            return;
        }
        if let Some(found) = self.search.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.search = None;
            match found {
                Some(port) => {
                    self.message = format!("{} {} {port}", state.device().name(), t("found on"));
                    actions.push(Action::SetPort(PortChoice::Serial(port)));
                }
                None => self.message = format!("{} {}", state.device().name(), t("not found !")),
            }
        }
        let mut open = self.open;
        egui::Window::new(t("Device and Com Port")).open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
            let busy = state.busy || self.search.is_some();
            ui.add_enabled_ui(!busy && !state.device_held, |ui| {
                ui.label(t("Device :"));
                for kind in [DeviceKind::MiniVna, DeviceKind::Nwt] {
                    if ui.radio(state.device() == kind, kind.name()).clicked() && state.device() != kind {
                        actions.push(Action::SetDevice(kind));
                    }
                }
                if state.device() == DeviceKind::Nwt {
                    ui.label(t("Transmission loss and generator only (no phase)."));
                }
                ui.separator();
                ui.label(t("Port :"));
                for port in &self.ports {
                    let selected = *current == PortChoice::Serial(port.clone());
                    if ui.selectable_label(selected, port).clicked() {
                        actions.push(Action::SetPort(PortChoice::Serial(port.clone())));
                    }
                }
                if self.ports.is_empty() {
                    ui.label(t("No serial port on this computer."));
                }
                if ui.selectable_label(*current == PortChoice::Simulated, t("Simulated device")).clicked() {
                    actions.push(Action::SetPort(PortChoice::Simulated));
                }
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button(t("Automatic search")).clicked() {
                        let (tx, rx) = mpsc::channel();
                        let repaint = ctx.clone();
                        let kind = state.device();
                        std::thread::spawn(move || {
                            let _ = tx.send(find_device(kind));
                            repaint.request_repaint();
                        });
                        self.search = Some(rx);
                        self.message = t("look for current...").to_string();
                    }
                    if ui.button(t("Refresh")).clicked() {
                        self.ports = list_ports();
                    }
                });
            });
            if !self.message.is_empty() {
                ui.label(&self.message);
            }
        });
        self.open = open;
    }
}
