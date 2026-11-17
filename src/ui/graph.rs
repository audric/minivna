//! Paints the shapes from `plot` and turns mouse input on the graph into state changes.

use eframe::egui;

use super::plot::{self, Anchor, Prim};
use super::state::{AppState, Marker};
use super::{color32, Action};

pub fn show(ui: &mut egui::Ui, state: &mut AppState, _actions: &mut Vec<Action>) {
    let (response, painter) = ui.allocate_painter(ui.available_size(), egui::Sense::click());
    let origin = response.rect.min;
    let prims = plot::build(state, &state.settings.screen, response.rect.width(), response.rect.height());
    paint(&painter, origin, &prims);
    clicks(ui, &response, state);
}

/// Mouse on the graph, as in the original:
/// left / right click places M1 / M2; with Ctrl (M1) or Shift (M2) held, left and right
/// step that marker one point down or up; with Alt held, left / right click send M1 / M2
/// to the band preset's default position; the middle button toggles the memo line.
fn clicks(ui: &egui::Ui, response: &egui::Response, state: &mut AppState) {
    let left = response.clicked();
    let right = response.secondary_clicked();
    let middle = response.middle_clicked();
    if !(left || right || middle) {
        return;
    }
    let Some(pos) = response.interact_pointer_pos() else { return };
    let frame = plot::Frame::new(response.rect.width(), response.rect.height());
    let (x, y) = (pos.x - response.rect.min.x, pos.y - response.rect.min.y);
    if !frame.contains(x, y) {
        return;
    }
    if middle {
        state.toggle_memo(frame.height_at(y));
        return;
    }
    let modifiers = ui.input(|i| i.modifiers);
    let direction = if left { -1 } else { 1 };
    if modifiers.ctrl || modifiers.command {
        state.step_marker(Marker::M1, direction);
    } else if modifiers.shift {
        state.step_marker(Marker::M2, direction);
    } else if modifiers.alt {
        state.marker_to_band_default(if left { Marker::M1 } else { Marker::M2 });
    } else {
        let (lo, hi) = plot::x_range_hz(state);
        state.place_marker(if left { Marker::M1 } else { Marker::M2 }, frame.freq_at(x, lo, hi));
    }
}

/// Draw the shapes with their top-left corner at `origin`.
pub fn paint(painter: &egui::Painter, origin: egui::Pos2, prims: &[Prim]) {
    let at = |p: [f32; 2]| origin + egui::vec2(p[0], p[1]);
    for prim in prims {
        match prim {
            Prim::Fill { rect, color } => {
                let rect = egui::Rect::from_min_size(at([rect[0], rect[1]]), egui::vec2(rect[2], rect[3]));
                painter.rect_filled(rect, 0.0, color32(*color));
            }
            Prim::Line { points, color, width, dashed } => {
                let path: Vec<egui::Pos2> = points.iter().map(|p| at(*p)).collect();
                let stroke = egui::Stroke::new(*width, color32(*color));
                if *dashed {
                    painter.extend(egui::Shape::dashed_line(&path, stroke, 6.0, 4.0));
                } else {
                    painter.add(egui::Shape::line(path, stroke));
                }
            }
            Prim::Text { pos, text, color, anchor } => {
                let align = match anchor {
                    Anchor::Left => egui::Align2::LEFT_TOP,
                    Anchor::Center => egui::Align2::CENTER_TOP,
                    Anchor::Right => egui::Align2::RIGHT_TOP,
                };
                painter.text(at(*pos), align, text, egui::FontId::proportional(11.0), color32(*color));
            }
        }
    }
}
