//! Paints the shapes from `plot` and turns mouse input on the graph into state changes.

use eframe::egui;

use super::plot::{self, Anchor, Prim};
use super::smith;
use super::state::{AppState, Marker, View};
use super::{color32, Action};
use crate::store::Rgb;

pub fn show(ui: &mut egui::Ui, state: &mut AppState, _actions: &mut Vec<Action>) {
    let (response, painter) = ui.allocate_painter(ui.available_size(), egui::Sense::click());
    let origin = response.rect.min;
    state.graph_size = [response.rect.width().round() as u32, response.rect.height().round() as u32];
    let prims = plot::build_view(state, &state.settings.screen, response.rect.width(), response.rect.height());
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
    if state.view == View::Smith {
        // On the Smith chart a click takes the nearest point of the trace: left M1, right M2.
        let (x, y) = (pos.x - response.rect.min.x, pos.y - response.rect.min.y);
        let found = smith::nearest_index(state, response.rect.width(), response.rect.height(), x, y);
        if let (Some(index), false) = (found, middle) {
            let freq = state.points()[index].freq_hz;
            state.place_marker(if left { Marker::M1 } else { Marker::M2 }, freq);
        }
        return;
    }
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
            Prim::Banner { rect, fill, text, color } => {
                let rect = egui::Rect::from_min_size(at([rect[0], rect[1]]), egui::vec2(rect[2], rect[3]));
                painter.rect_filled(rect, 0.0, color32(*fill));
                painter.text(rect.center(), egui::Align2::CENTER_CENTER, text, egui::FontId::proportional(BANNER_PX), color32(*color));
            }
        }
    }
}

/// The graph as a picture: `width` x `height` pixels, 4 bytes each (red, green, blue,
/// alpha), drawn with the PNG colour set. Needs no window.
pub fn render_png(state: &AppState, width: u32, height: u32) -> Vec<u8> {
    let mut canvas = Canvas { width: width as i32, height: height as i32, pixels: vec![255; width as usize * height as usize * 4] };
    for prim in plot::build_view(state, &state.settings.png, width as f32, height as f32) {
        match prim {
            Prim::Fill { rect, color } => canvas.fill(rect, color),
            Prim::Line { points, color, width, dashed } => canvas.line(&points, color, width, dashed),
            Prim::Text { pos, text, color, anchor } => canvas.text(pos, &text, color, anchor, TEXT_PX),
            Prim::Banner { rect, fill, text, color } => {
                canvas.fill(rect, fill);
                let top = rect[1] + (rect[3] - BANNER_PX) / 2.0 - 2.0;
                canvas.text([rect[0] + rect[2] / 2.0, top], &text, color, Anchor::Center, BANNER_PX);
            }
        }
    }
    canvas.pixels
}

struct Canvas {
    width: i32,
    height: i32,
    pixels: Vec<u8>,
}

const DASH_ON: f32 = 6.0;
const DASH_PERIOD: f32 = 10.0;
const TEXT_PX: f32 = 13.0;
const BANNER_PX: f32 = 26.0;

impl Canvas {
    /// Mix a colour into a pixel; `alpha` 1 replaces it.
    fn blend(&mut self, x: i32, y: i32, color: Rgb, alpha: f32) {
        if x < 0 || y < 0 || x >= self.width || y >= self.height {
            return;
        }
        let i = ((y * self.width + x) * 4) as usize;
        let a = alpha.clamp(0.0, 1.0);
        for (slot, value) in self.pixels[i..i + 3].iter_mut().zip([color.0, color.1, color.2]) {
            *slot = (*slot as f32 * (1.0 - a) + value as f32 * a).round() as u8;
        }
        self.pixels[i + 3] = 255;
    }

    fn fill(&mut self, rect: [f32; 4], color: Rgb) {
        let (x0, y0) = (rect[0].floor() as i32, rect[1].floor() as i32);
        let (x1, y1) = ((rect[0] + rect[2]).ceil() as i32, (rect[1] + rect[3]).ceil() as i32);
        for y in y0.max(0)..y1.min(self.height) {
            for x in x0.max(0)..x1.min(self.width) {
                self.blend(x, y, color, 1.0);
            }
        }
    }

    fn line(&mut self, points: &[[f32; 2]], color: Rgb, width: f32, dashed: bool) {
        let thick = (width.round() as i32).max(1);
        let (before, after) = ((thick - 1) / 2, thick / 2);
        let mut travelled = 0.0f32;
        for pair in points.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
            let length = dx.hypot(dy);
            if !length.is_finite() {
                continue;
            }
            let steps = (length * 2.0).ceil().max(1.0) as i32;
            for step in 0..=steps {
                let t = step as f32 / steps as f32;
                if dashed && (travelled + length * t) % DASH_PERIOD >= DASH_ON {
                    continue;
                }
                let (x, y) = ((a[0] + dx * t).round() as i32, (a[1] + dy * t).round() as i32);
                for oy in -before..=after {
                    for ox in -before..=after {
                        self.blend(x + ox, y + oy, color, 1.0);
                    }
                }
            }
            travelled += length;
        }
    }

    fn text(&mut self, pos: [f32; 2], text: &str, color: Rgb, anchor: Anchor, size: f32) {
        use ab_glyph::{Font, ScaleFont};
        let Some(font) = png_font() else { return };
        let scaled = font.as_scaled(ab_glyph::PxScale::from(size));
        let width: f32 = text.chars().map(|c| scaled.h_advance(font.glyph_id(c))).sum();
        let mut x = match anchor {
            Anchor::Left => pos[0],
            Anchor::Center => pos[0] - width / 2.0,
            Anchor::Right => pos[0] - width,
        };
        let baseline = pos[1] + scaled.ascent();
        for c in text.chars() {
            let id = font.glyph_id(c);
            let glyph = id.with_scale_and_position(size, ab_glyph::point(x, baseline));
            x += scaled.h_advance(id);
            if let Some(outline) = font.outline_glyph(glyph) {
                let bounds = outline.px_bounds();
                outline.draw(|gx, gy, coverage| {
                    self.blend(bounds.min.x as i32 + gx as i32, bounds.min.y as i32 + gy as i32, color, coverage);
                });
            }
        }
    }
}

/// The interface's own font, so that pictures look like the screen.
fn png_font() -> Option<&'static ab_glyph::FontVec> {
    static FONT: std::sync::OnceLock<Option<ab_glyph::FontVec>> = std::sync::OnceLock::new();
    FONT.get_or_init(|| {
        let definitions = egui::FontDefinitions::default();
        let name = definitions.families.get(&egui::FontFamily::Proportional)?.first()?;
        let data = definitions.font_data.get(name)?;
        ab_glyph::FontVec::try_from_vec((*data.font).as_ref().to_vec()).ok()
    })
    .as_ref()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::state::tests::swept;

    fn pixel(buffer: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * width + x) * 4) as usize;
        [buffer[i], buffer[i + 1], buffer[i + 2], buffer[i + 3]]
    }

    #[test]
    fn render_png_has_the_requested_size_and_background() {
        let mut state = swept();
        state.settings.png.background = Rgb(250, 240, 230);
        let buffer = render_png(&state, 400, 300);
        assert_eq!(buffer.len(), 400 * 300 * 4);
        assert_eq!(pixel(&buffer, 400, 0, 0), [250, 240, 230, 255]);
    }

    #[test]
    fn render_png_draws_the_frame_curves_and_text() {
        let mut state = swept();
        state.settings.png.frame = Rgb(1, 2, 3);
        let buffer = render_png(&state, 400, 300);
        let frame = plot::Frame::new(400.0, 300.0);
        // The left edge of the frame, halfway down.
        let (x, y) = (frame.left.round() as u32, ((frame.top + frame.bottom) / 2.0) as u32);
        assert_eq!(pixel(&buffer, 400, x, y), [1, 2, 3, 255]);
        // Some pixel carries the SWR colour (curve or tick line) and some text was drawn below the frame.
        let has = |c: [u8; 3], y0: u32, y1: u32| {
            (y0..y1).any(|y| (0..400).any(|x| pixel(&buffer, 400, x, y)[..3] == c))
        };
        assert!(has([255, 0, 0], 0, 300));
        let below = frame.bottom as u32 + 3;
        assert!((below..300).any(|y| (0..400).any(|x| pixel(&buffer, 400, x, y)[..3] != [255, 255, 255])));
    }

    #[test]
    fn render_png_survives_odd_sizes() {
        let state = swept();
        assert_eq!(render_png(&state, 0, 0).len(), 0);
        assert_eq!(render_png(&state, 1, 1).len(), 4);
        assert_eq!(render_png(&state, 30, 7).len(), 30 * 7 * 4);
    }

    #[test]
    fn line_thickness_follows_the_width() {
        let thickness = |width: f32| {
            let mut canvas = Canvas { width: 20, height: 20, pixels: vec![255; 20 * 20 * 4] };
            canvas.line(&[[2.0, 10.0], [18.0, 10.0]], Rgb(0, 0, 0), width, false);
            (0..20).filter(|y| pixel(&canvas.pixels, 20, 10, *y)[0] == 0).count()
        };
        assert_eq!((thickness(1.0), thickness(1.5), thickness(3.0)), (1, 2, 3));
    }
}
