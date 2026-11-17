//! The window. Logic lives in `state`; the other modules only draw and report clicks.

use eframe::egui;

use crate::cal::Calibration;
use crate::store::{self, Rgb, Settings};
use crate::sweep::{self, Command, PortChoice, Worker};

pub mod graph;
pub mod marker_panel;
pub mod plot;
pub mod side_panel;
pub mod state;
pub mod text;

use state::AppState;

/// Something the user asked for that needs the sweep worker.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    SweepOnce,
    SweepContinuous,
    Stop,
    /// Sweep the range between the markers.
    Zoom,
    /// Go back to the range before the last zoom.
    Unzoom,
}

pub fn color32(c: Rgb) -> egui::Color32 {
    egui::Color32::from_rgb(c.0, c.1, c.2)
}

pub struct App {
    pub state: AppState,
    worker: Worker,
    port: PortChoice,
    /// Run one sweep as soon as the window is up (`--sweep` on the command line).
    sweep_at_start: bool,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>, port: PortChoice, settings: Settings, notice: Option<String>, sweep_at_start: bool) -> Self {
        let (calibration, cal_notice) = match Calibration::load(&store::calibration_path()) {
            Ok(cal) => (cal, None),
            Err(e) => (Calibration::default(), Some(e.to_string())),
        };
        let ctx = cc.egui_ctx.clone();
        let worker = sweep::spawn(port.clone(), calibration.clone(), Box::new(move || ctx.request_repaint()));
        let mut state = AppState::new(settings, calibration);
        let _ = worker.tx.send(Command::SetSigned(state.settings.signed_phase));
        state.status = notice.or(cal_notice).unwrap_or_default();
        App { state, worker, port, sweep_at_start }
    }

    fn send(&self, command: Command) {
        let _ = self.worker.tx.send(command);
    }

    fn port_name(&self) -> String {
        match &self.port {
            PortChoice::Simulated => text::t("Simulated device").to_string(),
            PortChoice::Serial(name) => name.clone(),
        }
    }

    fn apply(&mut self, action: Action) {
        match action {
            Action::SweepOnce | Action::SweepContinuous if self.state.busy => {}
            Action::SweepOnce => {
                self.state.commit_range();
                self.send(Command::Single(self.state.request()));
            }
            Action::SweepContinuous => {
                self.state.commit_range();
                self.state.continuous = true;
                self.send(Command::Continuous(self.state.request()));
            }
            Action::Stop => self.send(Command::Stop),
            Action::Zoom => {
                if self.state.zoom_to_markers() {
                    self.apply(Action::SweepOnce);
                }
            }
            Action::Unzoom => {
                if self.state.unzoom() {
                    self.apply(Action::SweepOnce);
                }
            }
        }
    }
}

impl App {
    /// Space = one sweep, F12 = continuous, Esc = stop. Ignored while a text box has the keyboard.
    fn keys(&self, ui: &egui::Ui, actions: &mut Vec<Action>) {
        let typing = ui.ctx().text_edit_focused();
        ui.input(|input| {
            if input.key_pressed(egui::Key::Escape) {
                actions.push(Action::Stop);
            }
            if input.key_pressed(egui::Key::F12) {
                actions.push(Action::SweepContinuous);
            }
            if input.key_pressed(egui::Key::Space) && !typing {
                actions.push(Action::SweepOnce);
            }
        });
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        while let Ok(event) = self.worker.rx.try_recv() {
            self.state.on_event(event);
        }
        let mut actions = Vec::new();
        if std::mem::take(&mut self.sweep_at_start) {
            actions.push(Action::SweepOnce);
        }

        egui::Panel::bottom("status").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(self.port_name());
                ui.separator();
                ui.label(&self.state.status);
            });
        });
        egui::Panel::top("markers").show(ui, |ui| {
            marker_panel::show(ui, &mut self.state, &mut actions);
        });
        egui::Panel::left("controls").resizable(false).show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                side_panel::show(ui, &mut self.state, &mut actions);
            });
        });
        egui::CentralPanel::default().show(ui, |ui| {
            graph::show(ui, &mut self.state, &mut actions);
        });

        marker_panel::entry_dialog(ui.ctx(), &mut self.state);
        self.keys(ui, &mut actions);

        for action in actions {
            self.apply(action);
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.send(Command::Shutdown);
        self.state.settings.start_mhz = self.state.start_hz as f64 / 1e6 / self.state.extender as f64;
        self.state.settings.stop_mhz = self.state.stop_hz as f64 / 1e6 / self.state.extender as f64;
        self.state.settings.samples = self.state.samples;
        self.state.settings.repair();
        let _ = self.state.settings.save(&store::settings_path());
    }
}

/// Open the window and run until it is closed.
pub fn run(port: PortChoice, settings: Settings, notice: Option<String>, sweep_at_start: bool) -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("minivna")
            .with_inner_size([1100.0, 760.0])
            .with_min_inner_size([760.0, 480.0]),
        // OpenGL runs on older graphics hardware where the default renderer draws wrongly.
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };
    eframe::run_native(
        "minivna",
        options,
        Box::new(move |cc| Ok(Box::new(App::new(cc, port, settings, notice, sweep_at_start)))),
    )
}
