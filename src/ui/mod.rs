//! The window. Logic lives in `state`; the other modules only draw and report clicks.

use eframe::egui;

use crate::cal::Calibration;
use crate::device::Mode;
use crate::store::{self, Rgb, Settings};
use crate::sweep::{self, Command, PortChoice, Worker};

pub mod dialogs;
pub mod files;
pub mod graph;
pub mod marker_panel;
pub mod menu;
pub mod plot;
pub mod side_panel;
pub mod smith;
pub mod state;
pub mod text;
#[cfg(test)]
mod ui_tests;

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
    File(FileAction),
    Quit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileAction {
    Open,
    Save,
    SaveCsv,
    SavePng,
}

/// The windows that can be opened from the menus.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dialog {
    About,
    Velocity,
    Bands,
    Colors,
    Lc,
    Port,
    Calibrate(Mode),
    Dds,
}

pub fn color32(c: Rgb) -> egui::Color32 {
    egui::Color32::from_rgb(c.0, c.1, c.2)
}

fn saved(path: std::path::PathBuf) -> String {
    format!("{} {}", text::t("Saved"), path.display())
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
    cal_path: std::path::PathBuf,
    /// Where the window was last seen: x, y, width, height.
    window: Option<[f32; 4]>,
}

impl App {
    /// `cal_path` is where the calibration is read from and written to.
    fn new(
        ctx: &egui::Context,
        port: PortChoice,
        settings: Settings,
        notice: Option<String>,
        sweep_at_start: bool,
        cal_path: std::path::PathBuf,
    ) -> Self {
        let (calibration, cal_notice) = match Calibration::load(&cal_path) {
            Ok(cal) => (cal, None),
            Err(e) => (Calibration::default(), Some(e.to_string())),
        };
        // A light window like the original's: entry and read-out fields are white boxes with
        // a border, clearly apart from the grey panels. Not tied to the desktop's dark mode.
        ctx.set_theme(egui::Theme::Light);
        ctx.all_styles_mut(|style| {
            let border = egui::Stroke::new(1.0, egui::Color32::from_gray(140));
            style.visuals.widgets.inactive.bg_stroke = border;
            style.visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, egui::Color32::from_gray(170));
            style.visuals.extreme_bg_color = egui::Color32::WHITE;
            style.visuals.panel_fill = egui::Color32::from_gray(228);
            style.visuals.window_fill = egui::Color32::from_gray(236);
        });
        let ctx = ctx.clone();
        let worker = sweep::spawn(port.clone(), calibration.clone(), Box::new(move || ctx.request_repaint()));
        let mut state = AppState::new(settings, calibration);
        // `--smith` starts on the Smith chart.
        if std::env::args().any(|a| a == "--smith") {
            state.view = state::View::Smith;
        }
        let _ = worker.tx.send(Command::SetSigned(state.settings.signed_phase));
        let uncalibrated = state.calibration.reflection.is_none().then(|| text::t("Please calibrate your miniVNA first (Configure menu).").to_string());
        state.status = notice.or(cal_notice).or(uncalibrated).unwrap_or_default();
        App { state, worker, port, sweep_at_start, about_open: false, title: String::new(), quit: false, dialogs: Default::default(), cal_path, window: None }
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
        self.sync_device_held();
        match action {
            // A running continuous sweep takes over new settings (range, band, mode);
            // a single sweep in progress, or the generator, makes the request wait.
            Action::SweepOnce | Action::SweepContinuous => {
                let Some(kind) = self.state.sweep_kind() else { return };
                self.state.commit_range();
                if kind == state::SweepKind::Continuous || matches!(action, Action::SweepContinuous) {
                    self.state.continuous = true;
                    self.send(Command::Continuous(self.state.request()));
                } else {
                    self.send(Command::Single(self.state.request()));
                }
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
                self.sync_device_held();
                if !self.state.set_mode(mode) {
                    return;
                }
                if mode == UiMode::Generator {
                    let mut more = Vec::new();
                    self.dialogs.generator.open(&self.state, &mut more);
                    more.into_iter().for_each(|a| self.apply(a));
                } else if self.state.continuous {
                    // Reflection or transmission may have changed under a running sweep.
                    self.apply(Action::SweepOnce);
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
            Action::Extender(mult) => {
                self.state.set_extender(mult);
                if self.state.continuous {
                    self.apply(Action::SweepOnce);
                }
            }
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
            Action::Open(Dialog::Velocity) => self.dialogs.velocity.open = true,
            Action::Open(Dialog::Bands) => self.dialogs.bands.open = true,
            Action::Open(Dialog::Colors) => self.dialogs.colors.open = true,
            Action::Open(Dialog::Lc) => self.dialogs.lc.open = true,
            Action::File(action) => self.file(action),
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
    /// The generator and the oscillator calibration keep the device to themselves.
    fn sync_device_held(&mut self) {
        self.state.device_held = self.dialogs.generator.open || self.dialogs.dds.open;
    }

    /// Ask for a file name and do what the File menu item says. Problems go to the status bar.
    fn file(&mut self, action: FileAction) {
        let dialog = rfd::FileDialog::new();
        let [width, height] = self.state.graph_size;
        let outcome = match action {
            FileAction::Open => {
                if self.state.busy || self.state.continuous {
                    Some(Err(text::t("Stop the sweep before opening a file.").to_string()))
                } else {
                    dialog.add_filter("miniVNA sweep", &["mvna"]).pick_file().map(|path| {
                        files::open_sweep(&mut self.state, &path).map(|()| format!("{} {}", text::t("Opened"), path.display()))
                    })
                }
            }
            FileAction::Save if self.state.result.is_none() => Some(Err(text::t("Nothing to save: make a sweep first.").to_string())),
            FileAction::SaveCsv if self.state.result.is_none() => Some(Err(text::t("Nothing to save: make a sweep first.").to_string())),
            FileAction::Save => dialog.add_filter("miniVNA sweep", &["mvna"]).set_file_name("sweep.mvna").save_file().map(|p| files::save_sweep(&self.state, &p).map(saved)),
            FileAction::SaveCsv => dialog.add_filter("CSV", &["csv"]).set_file_name("sweep.csv").save_file().map(|p| files::save_csv(&self.state, &p).map(saved)),
            FileAction::SavePng => dialog.add_filter("PNG", &["png"]).set_file_name("sweep.png").save_file().map(|p| files::save_png(&self.state, &p, width, height).map(saved)),
        };
        match outcome {
            Some(Ok(message)) => self.state.status = message,
            Some(Err(why)) => self.state.status = format!("!!! {why}"),
            None => {}
        }
    }

    /// The title bar names the mode and the port, as the original does.
    fn update_title(&mut self, ctx: &egui::Context) {
        let title = format!("minivna - Mode : {} ( {} )", self.state.mode.name(), self.port_name());
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }

}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.pump();
        self.remember_window(ui.ctx());
        self.frame(ui);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        // Wait for the worker to park the device, so that the output is not left running.
        self.worker.shutdown(std::time::Duration::from_secs(3));
        self.state.settings.start_mhz = self.state.start_hz as f64 / 1e6 / self.state.extender as f64;
        self.state.settings.stop_mhz = self.state.stop_hz as f64 / 1e6 / self.state.extender as f64;
        self.state.settings.samples = self.state.samples;
        self.state.settings.repair();
        if self.window.is_some() {
            self.state.settings.window = self.window;
        }
        if let Err(e) = self.state.settings.save(&store::settings_path()) {
            eprintln!("minivna: the settings could not be saved: {e}");
        }
    }
}

impl App {
    /// Take in what the sweep worker has reported since the last call.
    fn pump(&mut self) {
        while let Ok(event) = self.worker.rx.try_recv() {
            let is_sweep = matches!(event, sweep::Event::Sweep(_));
            let failed = matches!(event, sweep::Event::Error(_));
            let event = match event {
                sweep::Event::CalTable(mode, table) => {
                    let stored = self.state.store_cal_table(mode, table, &self.cal_path);
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
    }

    /// The window position and size, for the next start.
    fn remember_window(&mut self, ctx: &egui::Context) {
        let (outer, inner) = ctx.input(|i| (i.viewport().outer_rect, i.viewport().inner_rect));
        if let (Some(outer), Some(inner)) = (outer, inner) {
            self.window = Some([outer.min.x, outer.min.y, inner.width(), inner.height()]);
        }
    }

    /// Draw one frame and carry out what the user asked for in it.
    fn frame(&mut self, ui: &mut egui::Ui) {
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

        let entry_open = self.state.entry.is_some();
        marker_panel::entry_dialog(ui.ctx(), &mut self.state);
        menu::about(ui.ctx(), &mut self.about_open);
        self.dialogs.port.show(ui.ctx(), &self.state, &self.port, &mut actions);
        self.dialogs.calibrate.show(ui.ctx(), &self.state, &mut actions);
        self.dialogs.dds.show(ui.ctx(), &mut self.state, &mut actions);
        self.dialogs.generator.show(ui.ctx(), &self.state, &mut actions);
        self.dialogs.bands.show(ui.ctx(), &mut self.state);
        self.dialogs.colors.show(ui.ctx(), &mut self.state);
        self.dialogs.velocity.show(ui.ctx(), &mut self.state);
        self.dialogs.lc.show(ui.ctx(), &mut self.state);
        menu::shortcuts(ui, &self.state, &mut actions);
        menu::sweep_keys(ui, entry_open, &mut actions);

        self.sync_device_held();
        for action in actions {
            self.apply(action);
        }
    }

}

/// Open the window and run until it is closed.
pub fn run(port: PortChoice, settings: Settings, notice: Option<String>, sweep_at_start: bool) -> eframe::Result<()> {
    // The saved window, if it is sane; some desktops (Wayland) ignore the position.
    let saved = settings.window.filter(|w| w.iter().all(|v| v.is_finite()) && w[2] >= 760.0 && w[3] >= 480.0 && w[2] <= 10_000.0 && w[3] <= 10_000.0);
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("minivna")
        .with_inner_size(saved.map_or([1100.0, 760.0], |w| [w[2], w[3]]))
        .with_min_inner_size([760.0, 480.0]);
    if let Some(w) = saved {
        viewport = viewport.with_position([w[0].max(0.0), w[1].max(0.0)]);
    }
    let options = eframe::NativeOptions {
        viewport,
        // OpenGL runs on older graphics hardware where the default renderer draws wrongly.
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };
    eframe::run_native(
        "minivna",
        options,
        Box::new(move |cc| Ok(Box::new(App::new(&cc.egui_ctx, port, settings, notice, sweep_at_start, store::calibration_path())))),
    )
}
