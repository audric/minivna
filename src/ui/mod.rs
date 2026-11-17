//! The window. Logic lives in `state`; the other modules only draw and report clicks.

use eframe::egui;

use crate::cal::Calibration;
use crate::device::Mode;
use crate::store::{self, Rgb, Settings};
use crate::sweep::{self, Command, PortChoice, Worker};

pub mod dialogs;
pub mod graph;
pub mod marker_panel;
pub mod menu;
pub mod plot;
pub mod side_panel;
pub mod state;
pub mod text;

use state::{AppState, AutoLength, UiMode};

/// Something the user asked for that the window has to carry out.
pub enum Action {
    SweepOnce,
    SweepContinuous,
    Stop,
    /// Sweep the range between the markers.
    Zoom,
    /// Go back to the range before the last zoom.
    Unzoom,
    SetMode(UiMode),
    Band(usize),
    FullRange,
    HfRange,
    Extender(u8),
    /// Switch the estimate of the sign of phase and reactance on or off.
    SetSigned(bool),
    /// Start the automatic cable-length search.
    AutoLength,
    Open(Dialog),
    /// Pass a command straight to the sweep worker.
    Send(Command),
    SetPort(PortChoice),
    Quit,
}

/// The windows that can be opened from the menus.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dialog {
    About,
    Velocity,
    Port,
    Calibrate(Mode),
    Dds,
}

pub fn color32(c: Rgb) -> egui::Color32 {
    egui::Color32::from_rgb(c.0, c.1, c.2)
}

/// Add advice to an error where there is something the user can do about it.
fn explain(why: String) -> String {
    if why.starts_with("no permission") {
        format!("{why}. {}", text::t("On Linux, add your user to the 'dialout' group and log in again."))
    } else {
        why
    }
}

pub struct App {
    pub state: AppState,
    worker: Worker,
    port: PortChoice,
    /// Run one sweep as soon as the window is up (`--sweep` on the command line).
    sweep_at_start: bool,
    about_open: bool,
    title: String,
    quit: bool,
    dialogs: dialogs::Dialogs,
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
        let uncalibrated = state.calibration.reflection.is_none().then(|| text::t("Please calibrate your miniVNA first (Configure menu).").to_string());
        state.status = notice.or(cal_notice).or(uncalibrated).unwrap_or_default();
        App { state, worker, port, sweep_at_start, about_open: false, title: String::new(), quit: false, dialogs: Default::default() }
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
            Action::SetMode(mode) => {
                self.state.set_mode(mode);
                if mode == UiMode::Generator && !self.state.busy {
                    let mut more = Vec::new();
                    self.dialogs.generator.open(&self.state, &mut more);
                    more.into_iter().for_each(|a| self.apply(a));
                }
            }
            Action::Band(index) => {
                if self.state.apply_band(index) {
                    self.apply(Action::SweepOnce);
                }
            }
            Action::FullRange => {
                self.state.apply_full_range();
                self.apply(Action::SweepOnce);
            }
            Action::HfRange => {
                self.state.apply_hf_range();
                self.apply(Action::SweepOnce);
            }
            Action::Extender(mult) => self.state.set_extender(mult),
            Action::SetSigned(signed) => {
                self.state.settings.signed_phase = signed;
                self.send(Command::SetSigned(signed));
                if let Some(r) = self.state.result.take() {
                    self.state.result = Some(sweep::build_result(r.request, r.raw, r.cal, signed));
                }
            }
            Action::AutoLength => {
                if !self.state.busy {
                    let request = self.state.auto_length_start();
                    self.send(Command::Single(request));
                }
            }
            Action::Open(Dialog::About) => self.about_open = true,
            Action::Open(Dialog::Velocity) => {}
            Action::Open(Dialog::Port) => self.dialogs.port.open(),
            Action::Open(Dialog::Calibrate(mode)) => self.dialogs.calibrate.open(mode),
            Action::Open(Dialog::Dds) => {
                if !self.state.busy {
                    let mut more = Vec::new();
                    self.dialogs.dds.open(&self.state, &mut more);
                    more.into_iter().for_each(|a| self.apply(a));
                }
            }
            Action::Send(command) => self.send(command),
            Action::SetPort(port) => {
                self.state.settings.port = match &port {
                    PortChoice::Serial(name) => Some(name.clone()),
                    PortChoice::Simulated => None,
                };
                self.send(Command::SetPort(port.clone()));
                self.port = port;
            }
            Action::Quit => self.quit = true,
        }
    }
}

impl App {
    /// The title bar names the mode and the port, as the original does.
    fn update_title(&mut self, ctx: &egui::Context) {
        let title = format!("minivna - Mode : {} ( {} )", self.state.mode.name(), self.port_name());
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }

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
            let is_sweep = matches!(event, sweep::Event::Sweep(_));
            let failed = matches!(event, sweep::Event::Error(_));
            let event = match event {
                sweep::Event::CalTable(mode, table) => {
                    let stored = self.state.store_cal_table(mode, table, &store::calibration_path());
                    self.send(Command::SetCalibration(self.state.calibration.clone()));
                    self.dialogs.calibrate.finished(stored.map_err(|e| e.to_string()));
                    continue;
                }
                sweep::Event::Error(why) => {
                    self.dialogs.calibrate.finished(Err(why.clone()));
                    sweep::Event::Error(explain(why))
                }
                other => other,
            };
            self.state.on_event(event);
            if self.state.auto_length_running() && (is_sweep || failed) {
                match self.state.auto_length_on_result() {
                    AutoLength::NextSweep(request) => self.send(Command::Single(request)),
                    AutoLength::Done => {}
                    AutoLength::Failed if failed => {}
                    AutoLength::Failed => self.state.status = text::t("!!! No cable found: the phase does not pass 90 degrees three times.").to_string(),
                }
            }
        }
        self.update_title(ui.ctx());
        if self.quit {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
        let mut actions = Vec::new();
        if std::mem::take(&mut self.sweep_at_start) {
            actions.push(Action::SweepOnce);
        }

        egui::Panel::top("menu").show(ui, |ui| {
            menu::show(ui, &mut self.state, &mut actions);
        });
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
        menu::about(ui.ctx(), &mut self.about_open);
        self.dialogs.port.show(ui.ctx(), &self.state, &self.port, &mut actions);
        self.dialogs.calibrate.show(ui.ctx(), &self.state, &mut actions);
        self.dialogs.dds.show(ui.ctx(), &mut self.state, &mut actions);
        self.dialogs.generator.show(ui.ctx(), &self.state, &mut actions);
        menu::shortcuts(ui, &self.state, &mut actions);
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
