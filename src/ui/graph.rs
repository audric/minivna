//! Paints the shapes from `plot` and turns mouse input on the graph into state changes.

use eframe::egui;

use super::plot::{self, Anchor, Prim};
use super::state::AppState;
use super::{color32, Action};

pub fn show(ui: &mut egui::Ui, state: &mut AppState, _actions: &mut Vec<Action>) {
    let (response, painter) = ui.allocate_painter(ui.available_size(), egui::Sense::click());
    let origin = response.rect.min;
    let prims = plot::build(state, &state.settings.screen, response.rect.width(), response.rect.height());
    paint(&painter, origin, &prims);
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
