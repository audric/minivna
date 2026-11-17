//! Tests that drive the real widgets without a window: egui runs headless with made-up
//! key and mouse events.

use eframe::egui::{self, Event, Key, Modifiers, PointerButton, RawInput};

use super::dialogs::bands::BandsDialog;
use super::state::tests::{state, swept};
use super::state::{AppState, UiMode};
use super::{menu, side_panel, Action, FileAction};

struct Harness {
    ctx: egui::Context,
    time: f64,
}

const CTRL: Modifiers = Modifiers { alt: false, ctrl: true, shift: false, mac_cmd: false, command: true };
const SHIFT: Modifiers = Modifiers { alt: false, ctrl: false, shift: true, mac_cmd: false, command: false };

fn key(key: Key, modifiers: Modifiers) -> Event {
    Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers }
}

impl Harness {
    fn new() -> Self {
        Harness { ctx: egui::Context::default(), time: 0.0 }
    }

    fn frame(&mut self, mut events: Vec<Event>, modifiers: Modifiers, mut draw: impl FnMut(&mut egui::Ui)) {
        self.time += 0.05;
        events.insert(0, Event::ModifiersChanged(modifiers));
        let raw = RawInput {
            events,
            time: Some(self.time),
            focused: true,
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 900.0))),
            ..Default::default()
        };
        self.ctx.run_ui(raw, |ui| draw(ui)).drop_without_applying_deltas();
    }

    fn focused_rect(&self) -> Option<egui::Rect> {
        let id = self.ctx.memory(|m| m.focused())?;
        self.ctx.read_response(id).map(|r| r.rect)
    }

    fn click(&mut self, pos: egui::Pos2, mut draw: impl FnMut(&mut egui::Ui)) {
        let button = |pressed| Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
        self.frame(vec![Event::PointerMoved(pos)], Modifiers::NONE, &mut draw);
        self.frame(vec![button(true)], Modifiers::NONE, &mut draw);
        self.frame(vec![button(false)], Modifiers::NONE, &mut draw);
        for _ in 0..3 {
            self.frame(vec![], Modifiers::NONE, &mut draw);
        }
    }

    /// Press Tab until `count` text boxes have had the focus; returns their rectangles.
    fn tab_through_text_boxes(&mut self, count: usize, mut draw: impl FnMut(&mut egui::Ui)) -> Vec<egui::Rect> {
        let mut rects = Vec::new();
        for _ in 0..200 {
            if rects.len() == count {
                break;
            }
            self.frame(vec![key(Key::Tab, Modifiers::NONE)], Modifiers::NONE, &mut draw);
            self.frame(vec![], Modifiers::NONE, &mut draw);
            if self.ctx.text_edit_focused() {
                rects.push(self.focused_rect().expect("a focused widget"));
            }
        }
        assert_eq!(rects.len(), count, "not enough text boxes");
        rects
    }
}

/// The side panel with the menu shortcuts, as the window runs them.
fn panel(h: &mut Harness, s: &mut AppState, actions: &mut Vec<Action>, events: Vec<Event>, modifiers: Modifiers) {
    h.frame(events, modifiers, |ui| {
        side_panel::show(ui, s, actions);
        menu::shortcuts(ui, s, actions);
    });
}

fn has(actions: &[Action], wanted: impl Fn(&Action) -> bool) -> bool {
    actions.iter().any(wanted)
}

#[test]
fn shortcuts_do_nothing_while_typing() {
    let (mut h, mut s, mut actions) = (Harness::new(), swept(), Vec::new());
    for _ in 0..3 {
        panel(&mut h, &mut s, &mut actions, vec![], Modifiers::NONE);
    }
    // Not typing: Ctrl+P changes the mode and F6 picks a band.
    panel(&mut h, &mut s, &mut actions, vec![key(Key::P, CTRL), key(Key::F6, Modifiers::NONE)], CTRL);
    assert!(has(&actions, |a| matches!(a, Action::SetMode(UiMode::CableLoss))));
    assert!(has(&actions, |a| matches!(a, Action::Band(5))));
    actions.clear();

    // Put the cursor in the first range box: Ctrl+A is "select all" there, nothing else.
    h.tab_through_text_boxes(1, |ui| {
        side_panel::show(ui, &mut s, &mut actions);
        menu::shortcuts(ui, &s, &mut actions);
    });
    actions.clear();
    let events = vec![key(Key::A, CTRL), key(Key::F6, CTRL), key(Key::O, CTRL), Event::Copy, key(Key::N, CTRL)];
    panel(&mut h, &mut s, &mut actions, events, CTRL);
    assert!(actions.is_empty(), "{} actions while typing", actions.len());
}

#[test]
fn csv_and_png_have_their_keys() {
    let (mut h, mut s, mut actions) = (Harness::new(), swept(), Vec::new());
    panel(&mut h, &mut s, &mut actions, vec![], Modifiers::NONE);
    // The toolkit reports Ctrl+C as a "copy" event, not as a key.
    panel(&mut h, &mut s, &mut actions, vec![Event::Copy], CTRL);
    assert!(has(&actions, |a| matches!(a, Action::File(FileAction::SaveCsv))));
    panel(&mut h, &mut s, &mut actions, vec![key(Key::N, CTRL)], CTRL);
    assert!(has(&actions, |a| matches!(a, Action::File(FileAction::SavePng))));
}

#[test]
fn range_boxes_are_wide_enough_for_a_zoomed_frequency() {
    let (mut h, mut s, mut actions) = (Harness::new(), state(), Vec::new());
    let rects = h.tab_through_text_boxes(3, |ui| {
        side_panel::show(ui, &mut s, &mut actions);
    });
    for rect in rects {
        assert!(rect.width() >= 80.0, "range box only {} px wide", rect.width());
    }
}

/// The lowest widget of the side panel that takes the keyboard focus: the offset field.
fn offset_field(h: &mut Harness, s: &mut AppState) -> egui::Rect {
    let mut actions = Vec::new();
    let mut lowest: Option<egui::Rect> = None;
    for _ in 0..60 {
        h.frame(vec![key(Key::Tab, Modifiers::NONE)], Modifiers::NONE, |ui| side_panel::show(ui, s, &mut actions));
        if let Some(rect) = h.focused_rect() {
            if lowest.is_none_or(|l| rect.top() > l.top()) {
                lowest = Some(rect);
            }
        }
    }
    lowest.expect("nothing takes the focus")
}

fn loss_curve(s: &AppState) -> Vec<[f32; 2]> {
    use super::plot::{build, Prim};
    let colors = s.settings.screen.clone();
    build(s, &colors, 800.0, 500.0)
        .into_iter()
        .find_map(|p| match p {
            Prim::Line { points, color, dashed: false, .. } if color == colors.rl && points.len() > 2 => Some(points),
            _ => None,
        })
        .expect("no loss curve")
}

#[test]
fn the_loss_offset_moves_the_curve_when_typed_or_dragged() {
    let (mut h, mut s, mut actions) = (Harness::new(), swept(), Vec::new());
    let before = loss_curve(&s);
    let field = offset_field(&mut h, &mut s);

    // Typed: click the field, type, Enter.
    h.click(field.center(), |ui| side_panel::show(ui, &mut s, &mut actions));
    let typed = vec![key(Key::A, CTRL), Event::Text("10".into())];
    h.frame(typed, Modifiers::NONE, |ui| side_panel::show(ui, &mut s, &mut actions));
    h.frame(vec![key(Key::Enter, Modifiers::NONE)], Modifiers::NONE, |ui| side_panel::show(ui, &mut s, &mut actions));
    h.frame(vec![], Modifiers::NONE, |ui| side_panel::show(ui, &mut s, &mut actions));
    assert_eq!(s.rl_offset_db, 10.0, "typing 10 in the offset field");
    let after = loss_curve(&s);
    let moved = before.iter().zip(&after).filter(|(a, b)| (a[1] - b[1]).abs() > 1.0).count();
    assert!(moved > before.len() / 2, "only {moved} of {} points moved", before.len());

    // Dragged to the right: the value goes up.
    let button = |pos, pressed| Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
    let (from, to) = (field.center(), field.center() + egui::vec2(40.0, 0.0));
    let mut draw = |h: &mut Harness, events: Vec<Event>, s: &mut AppState| {
        let mut actions = Vec::new();
        h.frame(events, Modifiers::NONE, |ui| side_panel::show(ui, s, &mut actions));
    };
    draw(&mut h, vec![key(Key::Escape, Modifiers::NONE)], &mut s);
    draw(&mut h, vec![Event::PointerMoved(from)], &mut s);
    draw(&mut h, vec![button(from, true)], &mut s);
    for step in 1..=8 {
        draw(&mut h, vec![Event::PointerMoved(from + egui::vec2(5.0 * step as f32, 0.0))], &mut s);
    }
    draw(&mut h, vec![button(to, false)], &mut s);
    assert!(s.rl_offset_db > 10.0, "dragging right left the offset at {}", s.rl_offset_db);
}

fn bands(h: &mut Harness, d: &mut BandsDialog, s: &mut AppState) -> Vec<egui::Rect> {
    d.open = true;
    for _ in 0..4 {
        h.frame(vec![], Modifiers::NONE, |ui| d.show(ui.ctx(), s));
    }
    // First row: name, begin, end, mark 1, mark 2.
    h.tab_through_text_boxes(5, |ui| d.show(ui.ctx(), s))
}

#[test]
fn band_cells_are_wide_enough() {
    let (mut h, mut d, mut s) = (Harness::new(), BandsDialog::default(), state());
    let rects = bands(&mut h, &mut d, &mut s);
    assert!(rects[0].width() >= 100.0, "name cell only {} px", rects[0].width());
    assert!(rects[1..].iter().all(|r| r.width() >= 64.0), "number cells: {rects:?}");
}

#[test]
fn a_band_edit_is_kept_when_another_cell_is_clicked() {
    for (edited, clicked) in [(3, 4), (4, 3)] {
        let (mut h, mut d, mut s) = (Harness::new(), BandsDialog::default(), state());
        let rects = bands(&mut h, &mut d, &mut s); // focus is now on mark 2 (cell 4)
        if edited == 3 {
            h.frame(vec![key(Key::Tab, SHIFT)], SHIFT, |ui| d.show(ui.ctx(), &mut s));
            h.frame(vec![], Modifiers::NONE, |ui| d.show(ui.ctx(), &mut s));
        }
        assert_eq!(h.focused_rect(), Some(rects[edited]));
        h.frame(vec![key(Key::A, CTRL)], CTRL, |ui| d.show(ui.ctx(), &mut s));
        h.frame(vec![Event::Text("1.9".into())], Modifiers::NONE, |ui| d.show(ui.ctx(), &mut s));
        h.click(rects[clicked].center(), |ui| d.show(ui.ctx(), &mut s));
        let band = &s.settings.bands[0];
        let value = if edited == 3 { band.m1_mhz } else { band.m2_mhz };
        assert_eq!(value, 1.9, "edit of cell {edited} lost when clicking cell {clicked}");
    }
}

// ---- minor findings of the final review ----

fn sweeps(actions: &[Action]) -> usize {
    actions.iter().filter(|a| matches!(a, Action::SweepOnce | Action::SweepContinuous)).count()
}

#[test]
fn space_on_a_focused_button_presses_only_that_button() {
    let (mut h, mut s, mut actions) = (Harness::new(), state(), Vec::new());
    let draw = |ui: &mut egui::Ui, s: &mut AppState, actions: &mut Vec<Action>| {
        side_panel::show(ui, s, actions);
        menu::sweep_keys(ui, false, actions);
    };
    h.frame(vec![], Modifiers::NONE, |ui| draw(ui, &mut s, &mut actions));
    // No focus anywhere: Space is "one sweep".
    h.frame(vec![key(Key::Space, Modifiers::NONE)], Modifiers::NONE, |ui| draw(ui, &mut s, &mut actions));
    assert_eq!(sweeps(&actions), 1);
    actions.clear();
    // Tab puts the focus on the first button ("x Sweep"); Space now presses it, nothing more.
    h.frame(vec![key(Key::Tab, Modifiers::NONE)], Modifiers::NONE, |ui| draw(ui, &mut s, &mut actions));
    h.frame(vec![], Modifiers::NONE, |ui| draw(ui, &mut s, &mut actions));
    let space_up = Event::Key { key: Key::Space, physical_key: None, pressed: false, repeat: false, modifiers: Modifiers::NONE };
    h.frame(vec![key(Key::Space, Modifiers::NONE)], Modifiers::NONE, |ui| draw(ui, &mut s, &mut actions));
    h.frame(vec![space_up], Modifiers::NONE, |ui| draw(ui, &mut s, &mut actions));
    h.frame(vec![], Modifiers::NONE, |ui| draw(ui, &mut s, &mut actions));
    assert_eq!(sweeps(&actions), 1, "Space on a focused button");
}

#[test]
fn escape_stops_the_sweep_except_from_the_entry_window() {
    let (mut h, mut actions) = (Harness::new(), Vec::new());
    h.frame(vec![key(Key::Escape, Modifiers::NONE)], Modifiers::NONE, |ui| menu::sweep_keys(ui, true, &mut actions));
    assert!(actions.is_empty());
    h.frame(vec![key(Key::Escape, Modifiers::NONE)], Modifiers::NONE, |ui| menu::sweep_keys(ui, false, &mut actions));
    assert!(has(&actions, |a| matches!(a, Action::Stop)));
}

#[test]
fn ctrl_and_shift_clicks_step_the_markers_on_the_smith_chart_too() {
    let (mut h, mut s, mut actions) = (Harness::new(), swept(), Vec::new());
    s.view = super::state::View::Smith;
    (s.m1, s.m2) = (Some(10), Some(20));
    let pos = egui::pos2(600.0, 450.0);
    let mut click = |h: &mut Harness, s: &mut AppState, modifiers: Modifiers, button: PointerButton| {
        let press = |pressed| Event::PointerButton { pos, button, pressed, modifiers };
        for events in [vec![Event::PointerMoved(pos)], vec![press(true)], vec![press(false)], vec![]] {
            h.frame(events, modifiers, |ui| super::graph::show(ui, s, &mut actions));
        }
    };
    click(&mut h, &mut s, CTRL, PointerButton::Primary);
    assert_eq!(s.m1, Some(9));
    click(&mut h, &mut s, SHIFT, PointerButton::Secondary);
    assert_eq!(s.m2, Some(21));
}

mod app {
    use std::time::{Duration, Instant};

    use super::*;
    use crate::store::Settings;
    use crate::sweep::PortChoice;
    use crate::ui::App;

    fn app() -> (App, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let app = App::new(&ctx, PortChoice::Simulated, Settings::default(), None, false, dir.path().join("calibration.toml"));
        (app, dir)
    }

    /// Handle worker events until `done` holds; panics after three seconds.
    fn wait(app: &mut App, what: &str, done: impl Fn(&App) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            app.pump();
            if done(app) {
                return;
            }
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn a_continuous_sweep_follows_a_band_change() {
        let (mut app, _dir) = app();
        app.apply(Action::SweepContinuous);
        wait(&mut app, "the first sweep", |a| a.state.result.is_some());
        app.apply(Action::Band(5));
        wait(&mut app, "a sweep of the band", |a| a.state.result.as_ref().is_some_and(|r| r.request.start_hz == 13_900_000));
        assert!(app.state.continuous && app.state.busy);
        app.apply(Action::Stop);
        wait(&mut app, "the stop", |a| !a.state.busy);
        assert!(!app.state.continuous);
    }

    #[test]
    fn the_generator_keeps_sweeps_and_mode_changes_out() {
        let (mut app, _dir) = app();
        app.apply(Action::SetMode(UiMode::Generator));
        assert!(app.dialogs.generator.open);
        app.apply(Action::SweepOnce);
        app.apply(Action::SetMode(UiMode::CableLoss));
        std::thread::sleep(Duration::from_millis(300));
        app.pump();
        assert!(app.state.result.is_none() && !app.state.busy);
        assert_eq!(app.state.mode, UiMode::Generator);
    }

    #[test]
    fn stop_ends_a_calibration_without_storing_anything() {
        let (mut app, dir) = app();
        app.apply(Action::Send(crate::sweep::Command::Calibrate(crate::device::Mode::Reflection, true, crate::device::DDS_FACTOR_DEFAULT)));
        wait(&mut app, "busy", |a| a.state.busy);
        app.apply(Action::Stop);
        wait(&mut app, "idle", |a| !a.state.busy);
        assert!(app.state.calibration.reflection.is_none());
        assert!(!dir.path().join("calibration.toml").exists());
    }

    #[test]
    fn a_calibration_is_stored_in_the_given_file() {
        let (mut app, dir) = app();
        app.apply(Action::Send(crate::sweep::Command::Calibrate(crate::device::Mode::Reflection, true, crate::device::DDS_FACTOR_DEFAULT)));
        wait(&mut app, "the table", |a| a.state.calibration.reflection.is_some());
        assert!(dir.path().join("calibration.toml").exists());
    }

    #[test]
    fn the_nwt_sweeper_is_chosen_calibrated_and_swept() {
        use crate::device::DeviceKind;
        let (mut app, dir) = app();
        app.apply(Action::SetDevice(DeviceKind::Nwt));
        assert_eq!(app.state.device(), DeviceKind::Nwt);

        // Before the reference is measured, the loss is counted from a nominal reading.
        app.apply(Action::Band(app.state.settings.bands.iter().position(|b| b.name == "70cm").unwrap()));
        app.apply(Action::SweepOnce);
        wait(&mut app, "a sweep", |a| a.state.result.is_some() && !a.state.busy);
        let result = app.state.result.clone().unwrap();
        assert_eq!((result.request.kind, result.points.len()), (DeviceKind::Nwt, app.state.samples as usize + 1));
        assert!(result.cal.iter().all(|c| *c == crate::device::nwt::REFERENCE_DEFAULT));

        // The reference comes from a sweep over the whole range and is kept in the file.
        app.apply(Action::Open(crate::ui::Dialog::CalibrateNwt));
        app.apply(Action::Send(crate::sweep::Command::CalibrateNwt));
        wait(&mut app, "the reference", |a| a.state.nwt_calibrated() && !a.state.busy);
        let stored = crate::cal::Calibration::load(&dir.path().join("calibration.toml")).unwrap();
        assert_eq!(stored.nwt_reference, app.state.calibration.nwt_reference);

        // The worker uses it from the next sweep on: the simulated filter is its own
        // reference, so the loss is zero everywhere.
        app.apply(Action::SweepOnce);
        wait(&mut app, "a calibrated sweep", |a| !a.state.busy && a.state.result.as_ref().is_some_and(|r| r.cal[0] != crate::device::nwt::REFERENCE_DEFAULT));
        let worst = app.state.points().iter().map(|p| p.rl_db.abs()).fold(0.0, f64::max);
        assert!(worst < 2.7, "{worst}"); // the reference grid is 4.365 MHz wide, the filter drops 0.6 dB per MHz
    }

    #[test]
    fn the_device_is_not_changed_under_a_running_sweep() {
        use crate::device::DeviceKind;
        let (mut app, _dir) = app();
        app.apply(Action::SweepContinuous);
        wait(&mut app, "busy", |a| a.state.busy);
        app.apply(Action::SetDevice(DeviceKind::Nwt));
        assert_eq!(app.state.device(), DeviceKind::MiniVna);
        app.apply(Action::Stop);
        wait(&mut app, "idle", |a| !a.state.busy);
        app.apply(Action::SetDevice(DeviceKind::Nwt));
        assert_eq!(app.state.device(), DeviceKind::Nwt);
        // Things that belong to the miniVNA stay shut.
        app.apply(Action::Open(crate::ui::Dialog::Dds));
        app.apply(Action::Open(crate::ui::Dialog::Calibrate(crate::device::Mode::Reflection)));
        app.apply(Action::SetMode(UiMode::Antenna));
        assert!(!app.dialogs.dds.open && !app.dialogs.calibrate.is_open());
        assert_eq!(app.state.mode, UiMode::Transmission);
    }

    #[test]
    fn the_generator_drives_the_nwt_sweeper() {
        use crate::device::DeviceKind;
        let (mut app, _dir) = app();
        app.apply(Action::SetDevice(DeviceKind::Nwt));
        app.apply(Action::SetMode(UiMode::Generator));
        assert!(app.dialogs.generator.open);
        let freq = app.dialogs.generator.freq_hz();
        assert!((35_000_000..=4_400_000_000).contains(&freq), "{freq}");
        std::thread::sleep(Duration::from_millis(200));
        app.pump();
        assert!(!app.state.status.starts_with("!!!"), "{}", app.state.status);
    }
}
