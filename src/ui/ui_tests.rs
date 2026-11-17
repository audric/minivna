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
