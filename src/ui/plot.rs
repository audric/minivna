//! The graph as a list of simple shapes, with no dependency on the window toolkit.
//! The screen and the PNG export both draw from this list.

use super::state::{AppState, Big, Curve, View};
use crate::calc::Point;
use crate::store::{Colors, Rgb};

use crate::calc::tools::swr_min_index;

/// Where text sits relative to its position.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Anchor {
    Left,
    Center,
    Right,
}

/// One shape of the graph. Coordinates are pixels from the top-left corner.
#[derive(Clone, Debug, PartialEq)]
pub enum Prim {
    Fill { rect: [f32; 4], color: Rgb },
    Line { points: Vec<[f32; 2]>, color: Rgb, width: f32, dashed: bool },
    Text { pos: [f32; 2], text: String, color: Rgb, anchor: Anchor },
    /// A filled box with large centred text.
    Banner { rect: [f32; 4], fill: Rgb, text: String, color: Rgb },
}

/// The plotting rectangle inside a picture of a given size.
#[derive(Clone, Copy, Debug)]
pub struct Frame {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl Frame {
    pub fn new(width: f32, height: f32) -> Self {
        let left = 64.0f32.min(width);
        let top = 22.0f32.min(height);
        Frame { left, top, right: (width - 36.0).max(left + 1.0), bottom: (height - 24.0).max(top + 1.0) }
    }

    pub fn x_of(&self, freq_hz: f64, lo: f64, hi: f64) -> f32 {
        let t = if hi > lo { ((freq_hz - lo) / (hi - lo)).clamp(0.0, 1.0) } else { 0.0 };
        self.left + (self.right - self.left) * t as f32
    }

    pub fn freq_at(&self, x: f32, lo: f64, hi: f64) -> f64 {
        let t = ((x - self.left) / (self.right - self.left)).clamp(0.0, 1.0);
        lo + (hi - lo) * t as f64
    }

    /// Height 0 is the bottom edge, 1 the top edge.
    pub fn y_of(&self, height: f64) -> f32 {
        self.bottom - (self.bottom - self.top) * height.clamp(0.0, 1.0) as f32
    }

    pub fn height_at(&self, y: f32) -> f64 {
        ((self.bottom - y) / (self.bottom - self.top)).clamp(0.0, 1.0) as f64
    }

    pub fn contains(&self, x: f32, y: f32) -> bool {
        (self.left..=self.right).contains(&x) && (self.top..=self.bottom).contains(&y)
    }
}

pub const OHMS_FULL_SCALE: f64 = 500.0;

fn unit(v: f64) -> f64 {
    if v.is_nan() {
        0.0
    } else {
        v.clamp(0.0, 1.0)
    }
}

fn loss_scale_db(state: &AppState) -> f64 {
    if state.scale_60db {
        60.0
    } else {
        30.0
    }
}

/// Where a point of a curve sits vertically: 0 at the bottom of the graph, 1 at the top.
/// Each curve has its own scale, as in the original: SWR logarithmic from 1 to 10, loss
/// with 0 dB at the top, phase over 180 degrees, impedances over 500 ohms.
pub fn height(state: &AppState, curve: Curve, p: &Point) -> f64 {
    let signed = state.settings.signed_phase;
    unit(match curve {
        Curve::Swr => p.swr.max(1.0).log10(),
        Curve::Rl => 1.0 - (p.rl_db + state.rl_offset_db) / loss_scale_db(state),
        Curve::Phase if signed => (p.phase_deg + 180.0) / 360.0,
        Curve::Phase => p.phase_deg.abs() / 180.0,
        Curve::Z => p.z_mag / OHMS_FULL_SCALE,
        Curve::Rs => p.rs / OHMS_FULL_SCALE,
        Curve::Xs if signed => (p.xs + OHMS_FULL_SCALE) / (2.0 * OHMS_FULL_SCALE),
        Curve::Xs => p.xs.abs() / OHMS_FULL_SCALE,
    })
}

pub fn curve_color(colors: &Colors, curve: Curve) -> Rgb {
    match curve {
        Curve::Swr => colors.swr,
        Curve::Z => colors.z,
        Curve::Rl => colors.rl,
        Curve::Phase => colors.phase,
        Curve::Rs => colors.rs,
        Curve::Xs => colors.xs,
    }
}

pub fn curve_name(curve: Curve) -> &'static str {
    match curve {
        Curve::Swr => "SWR",
        Curve::Z => "| Z |",
        Curve::Rl => "R.L.",
        Curve::Phase => "Phase",
        Curve::Rs => "Rs",
        Curve::Xs => "| Xs |",
    }
}

/// The frequency range of the horizontal axis: the last sweep's, or the requested one.
pub fn x_range_hz(state: &AppState) -> (f64, f64) {
    match &state.result {
        Some(r) if r.points.len() > 1 => (r.points[0].freq_hz, r.points[r.points.len() - 1].freq_hz),
        _ => (state.start_hz as f64, state.stop_hz as f64),
    }
}

const SWR_TICKS: [f64; 6] = [1.0, 1.5, 2.0, 3.0, 5.0, 10.0];
const LOSS_TICKS_30: [f64; 6] = [0.0, 3.0, 6.0, 10.0, 20.0, 30.0];
const LOSS_TICKS_60: [f64; 6] = [0.0, 6.0, 12.0, 20.0, 40.0, 60.0];
const FREQ_DIVISIONS: usize = 6;

/// The whole graph for a picture of `width` x `height` pixels.
/// The drawing of whichever view is chosen: the curves over frequency or the Smith chart.
pub fn build_view(state: &AppState, colors: &Colors, width: f32, height: f32) -> Vec<Prim> {
    match state.view {
        View::Graph => build(state, colors, width, height),
        View::Smith => super::smith::build(state, colors, width, height),
    }
}

pub fn build(state: &AppState, colors: &Colors, width: f32, height_px: f32) -> Vec<Prim> {
    let mut out = Vec::new();
    let frame = Frame::new(width, height_px);
    let (lo, hi) = x_range_hz(state);
    let across = |y: f32| vec![[frame.left, y], [frame.right, y]];
    let down = |x: f32| vec![[x, frame.top], [x, frame.bottom]];

    out.push(Prim::Fill { rect: [0.0, 0.0, width.max(0.0), height_px.max(0.0)], color: colors.background });

    // Vertical divisions and frequency labels.
    for i in 0..=FREQ_DIVISIONS {
        let f = lo + (hi - lo) * i as f64 / FREQ_DIVISIONS as f64;
        let x = frame.x_of(f, lo, hi);
        if state.settings.vertical_lines && i > 0 && i < FREQ_DIVISIONS {
            out.push(Prim::Line { points: down(x), color: colors.frame, width: 1.0, dashed: false });
        }
        out.push(Prim::Text {
            pos: [x, frame.bottom + 5.0],
            text: format!("{:.3}MHz", f / 1e6),
            color: colors.frame,
            anchor: Anchor::Center,
        });
    }

    // The SWR and ohm scales are for an instrument that measures impedance (the miniVNA).
    // SWR ticks: dashed lines across, labels just left of the frame.
    for tick in SWR_TICKS.into_iter().filter(|_| state.smith_available()) {
        let y = frame.y_of(tick.log10());
        if tick > 1.0 && tick < 10.0 {
            out.push(Prim::Line { points: across(y), color: colors.swr, width: 1.0, dashed: true });
        }
        out.push(Prim::Text { pos: [frame.left - 4.0, y - 6.0], text: format!("{tick}"), color: colors.swr, anchor: Anchor::Right });
    }

    // Loss ticks, further left. The offset shifts the labels with the curve.
    let scale = loss_scale_db(state);
    for tick in if state.scale_60db { LOSS_TICKS_60 } else { LOSS_TICKS_30 } {
        let y = frame.y_of(1.0 - tick / scale);
        if tick > 0.0 && tick < scale {
            out.push(Prim::Line { points: across(y), color: colors.rl, width: 1.0, dashed: true });
        }
        // Rounded to a tenth, without a trailing ".0".
        let label = ((tick - state.rl_offset_db) * 10.0).round() / 10.0 + 0.0;
        let text = if tick == 0.0 { format!("{label}dB") } else { format!("{label}") };
        out.push(Prim::Text { pos: [frame.left - 30.0, y - 6.0], text, color: colors.rl, anchor: Anchor::Right });
    }

    if state.settings.line_50_ohms && state.smith_available() {
        let y = frame.y_of(50.0 / OHMS_FULL_SCALE);
        out.push(Prim::Line { points: across(y), color: colors.ohms, width: 1.0, dashed: true });
        out.push(Prim::Text { pos: [frame.right - 2.0, y - 13.0], text: "50 ohms".into(), color: colors.ohms, anchor: Anchor::Right });
    }

    // Legend along the top, visible curves only.
    let mut x = frame.left + 4.0;
    for (curve, shown) in Curve::ALL.into_iter().zip(state.show) {
        if shown && state.curve_available(curve) {
            let name = if curve == Curve::Rl && !state.curve_available(Curve::Swr) { "T.L." } else { curve_name(curve) };
            out.push(Prim::Text { pos: [x, 4.0], text: name.into(), color: curve_color(colors, curve), anchor: Anchor::Left });
            x += 12.0 + 7.0 * name.len() as f32;
        }
    }

    if let Some(result) = &state.result {
        let line_width = if state.settings.thick_lines { 2.5 } else { 1.2 };
        for (curve, shown) in Curve::ALL.into_iter().zip(state.show) {
            if !shown || !state.curve_available(curve) {
                continue;
            }
            let points = result
                .points
                .iter()
                .map(|p| [frame.x_of(p.freq_hz, lo, hi), frame.y_of(height(state, curve, p))])
                .collect();
            out.push(Prim::Line { points, color: curve_color(colors, curve), width: line_width, dashed: false });
        }

        let mut marker = |index: Option<usize>, color: Rgb, label: &str| {
            if let Some(p) = index.and_then(|i| result.points.get(i)) {
                let x = frame.x_of(p.freq_hz, lo, hi);
                out.push(Prim::Line { points: down(x), color, width: 1.5, dashed: false });
                out.push(Prim::Text { pos: [x + 3.0, frame.top + 2.0], text: label.into(), color, anchor: Anchor::Left });
            }
        };
        marker(state.m1, colors.m1, "M1");
        marker(state.m2, colors.m2, "M2");
        if state.show_swr_min {
            marker(swr_min_index(&result.points), colors.swr_min, "min");
        }
    }

    if let Some((big, text)) = state.big_text() {
        let fill = match big {
            Big::M1 => Rgb(0, 255, 255),
            Big::M2 => Rgb(192, 192, 192),
            Big::SwrMin => colors.swr_min,
        };
        let width = (frame.right - frame.left).min(360.0);
        let rect = [(frame.left + frame.right - width) / 2.0, frame.top + 8.0, width, 40.0];
        out.push(Prim::Banner { rect, fill, text, color: colors.frame });
    }

    if let Some(memo) = state.memo_y {
        out.push(Prim::Line { points: across(frame.y_of(memo)), color: colors.frame, width: 1.0, dashed: true });
    }

    // The frame itself, drawn last so that it stays on top.
    out.push(Prim::Line {
        points: vec![
            [frame.left, frame.top],
            [frame.right, frame.top],
            [frame.right, frame.bottom],
            [frame.left, frame.bottom],
            [frame.left, frame.top],
        ],
        color: colors.frame,
        width: 1.5,
        dashed: false,
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::state::tests::{fake_result, state};

    fn point(swr: f64, rl: f64, phase: f64, ohms: f64) -> Point {
        Point { freq_hz: 1e6, phase_deg: phase, rl_db: rl, re: 0.0, im: 0.0, swr, z_mag: ohms, rs: ohms, xs: ohms, l_uh: 0.0, c_pf: 0.0 }
    }

    fn close(a: f64, b: f64) {
        assert!((a - b).abs() < 1e-6, "{a} vs {b}");
    }

    #[test]
    fn swr_is_logarithmic_from_1_to_10() {
        let s = state();
        close(height(&s, Curve::Swr, &point(1.0, 0.0, 0.0, 0.0)), 0.0);
        close(height(&s, Curve::Swr, &point(10.0, 0.0, 0.0, 0.0)), 1.0);
        close(height(&s, Curve::Swr, &point(2.0, 0.0, 0.0, 0.0)), 2f64.log10());
        close(height(&s, Curve::Swr, &point(f64::INFINITY, 0.0, 0.0, 0.0)), 1.0);
        close(height(&s, Curve::Swr, &point(5000.0, 0.0, 0.0, 0.0)), 1.0);
    }

    #[test]
    fn loss_has_zero_at_the_top_and_follows_scale_and_offset() {
        let mut s = state();
        close(height(&s, Curve::Rl, &point(1.0, 0.0, 0.0, 0.0)), 1.0);
        close(height(&s, Curve::Rl, &point(1.0, 30.0, 0.0, 0.0)), 0.0);
        close(height(&s, Curve::Rl, &point(1.0, 10.0, 0.0, 0.0)), 1.0 - 10.0 / 30.0);
        close(height(&s, Curve::Rl, &point(1.0, 45.0, 0.0, 0.0)), 0.0);
        s.scale_60db = true;
        close(height(&s, Curve::Rl, &point(1.0, 30.0, 0.0, 0.0)), 0.5);
        s.rl_offset_db = 6.0;
        close(height(&s, Curve::Rl, &point(1.0, 30.0, 0.0, 0.0)), 0.4);
    }

    #[test]
    fn phase_and_ohms_scales() {
        let mut s = state();
        close(height(&s, Curve::Phase, &point(1.0, 0.0, 90.0, 0.0)), 0.5);
        close(height(&s, Curve::Z, &point(1.0, 0.0, 0.0, 50.0)), 0.1);
        close(height(&s, Curve::Rs, &point(1.0, 0.0, 0.0, 250.0)), 0.5);
        close(height(&s, Curve::Xs, &point(1.0, 0.0, 0.0, 9999.0)), 1.0);
        close(height(&s, Curve::Rs, &point(1.0, 0.0, 0.0, f64::INFINITY)), 1.0);
        s.settings.signed_phase = true;
        close(height(&s, Curve::Phase, &point(1.0, 0.0, -90.0, 0.0)), 0.25);
        close(height(&s, Curve::Xs, &point(1.0, 0.0, 0.0, -250.0)), 0.25);
        close(height(&s, Curve::Rs, &point(1.0, 0.0, 0.0, 250.0)), 0.5);
    }

    #[test]
    fn nan_never_reaches_the_screen() {
        let s = state();
        for curve in Curve::ALL {
            let h = height(&s, curve, &point(f64::NAN, f64::NAN, f64::NAN, f64::NAN));
            assert!((0.0..=1.0).contains(&h));
        }
    }

    #[test]
    fn frequency_and_position_convert_both_ways() {
        let mut s = state();
        s.result = Some(fake_result(1_000_000, 30_000_000, 290));
        let frame = Frame::new(800.0, 500.0);
        let (lo, hi) = x_range_hz(&s);
        assert_eq!((lo, hi), (1e6, 30e6));
        let x = frame.x_of(15.5e6, lo, hi);
        assert!((frame.freq_at(x, lo, hi) - 15.5e6).abs() < 1.0);
        assert_eq!(frame.freq_at(-50.0, lo, hi), lo);
        assert_eq!(frame.freq_at(5000.0, lo, hi), hi);
    }

    #[test]
    fn without_a_result_the_axis_shows_the_requested_range() {
        assert_eq!(x_range_hz(&state()), (1e6, 180e6));
    }

    #[test]
    fn build_draws_one_line_per_visible_curve() {
        let mut s = state();
        s.result = Some(fake_result(1_000_000, 30_000_000, 290));
        let count = |s: &AppState| {
            build(s, &s.settings.screen, 800.0, 500.0)
                .iter()
                .filter(|p| matches!(p, Prim::Line { points, .. } if points.len() == 291))
                .count()
        };
        assert_eq!(count(&s), 2); // SWR and RL by default
        s.show = [true; 6];
        assert_eq!(count(&s), 6);
        s.show = [false; 6];
        assert_eq!(count(&s), 0);
    }

    #[test]
    fn the_big_banner_is_drawn_when_chosen() {
        let mut s = state();
        s.result = Some(fake_result(1_000_000, 30_000_000, 290));
        let banners = |s: &AppState| build(s, &s.settings.screen, 800.0, 500.0).iter().filter(|p| matches!(p, Prim::Banner { .. })).count();
        assert_eq!(banners(&s), 0);
        s.m2 = Some(10);
        s.big = Some(Big::M2);
        assert_eq!(banners(&s), 1);
    }

    #[test]
    fn build_survives_no_result_and_tiny_sizes() {
        let s = state();
        assert!(!build(&s, &s.settings.screen, 800.0, 500.0).is_empty());
        let _ = build(&s, &s.settings.screen, 0.0, 0.0);
        let _ = build(&s, &s.settings.screen, 10.0, 10.0);
    }

    #[test]
    fn every_coordinate_is_finite() {
        let mut s = state();
        s.show = [true; 6];
        s.result = Some(fake_result(1_000_000, 30_000_000, 290));
        for p in &mut s.result.as_mut().unwrap().points {
            p.swr = f64::INFINITY;
            p.rs = f64::INFINITY;
            p.z_mag = f64::NAN;
        }
        for prim in build(&s, &s.settings.screen, 800.0, 500.0) {
            if let Prim::Line { points, .. } = prim {
                assert!(points.iter().all(|p| p[0].is_finite() && p[1].is_finite()));
            }
        }
    }

    #[test]
    fn loss_labels_are_rounded() {
        let mut s = state();
        s.rl_offset_db = 3.3;
        let prims = build(&s, &s.settings.screen, 800.0, 500.0);
        let texts: Vec<&str> = prims.iter().filter_map(|p| if let Prim::Text { text, .. } = p { Some(text.as_str()) } else { None }).collect();
        assert!(texts.contains(&"-3.3dB"), "{texts:?}");
        assert!(texts.contains(&"-0.3"), "{texts:?}");
        assert!(texts.iter().all(|t| t.len() <= 12), "{texts:?}");
    }
}
