//! The Smith chart: the reflection coefficient of every point of the sweep, drawn inside
//! the unit circle over a grid of constant resistance and constant reactance (50 ohm chart).
//! Like `plot`, it only makes a list of shapes; the screen and the PNG export draw them.

use super::plot::{Anchor, Prim};
use super::state::{field, AppState, UiMode};
use crate::calc::tools::swr_min_index;
use crate::calc::Point;
use crate::device::Mode;
use crate::store::{Colors, Rgb};

/// Reference impedance of the chart.
const Z0: f64 = 50.0;
/// Grid lines, as impedance divided by 50 ohms.
const GRID: [f64; 5] = [0.2, 0.5, 1.0, 2.0, 5.0];
/// Points outside the chart (a reading stronger than the calibration) are pulled in to this.
const MAX_GAMMA: f64 = 1.08;
/// How close to the trace a click has to be, in pixels.
const PICK_RADIUS: f32 = 25.0;

/// Reflection coefficient of the impedance (r + jx) x 50 ohms.
pub fn gamma_of(r: f64, x: f64) -> (f64, f64) {
    let d = (r + 1.0) * (r + 1.0) + x * x;
    ((r * r + x * x - 1.0) / d, 2.0 * x / d)
}

/// The circle of the chart inside a picture of a given size.
#[derive(Clone, Copy, Debug)]
pub struct Disc {
    pub cx: f32,
    pub cy: f32,
    pub radius: f32,
}

impl Disc {
    pub fn new(width: f32, height: f32) -> Self {
        let (w, h) = (width.max(0.0), height.max(0.0));
        Disc { cx: w / 2.0, cy: h / 2.0, radius: (w.min(h) / 2.0 - 28.0).max(1.0) }
    }

    /// Pixel position of a reflection coefficient. Inductive (positive) is up.
    pub fn xy(&self, re: f64, im: f64) -> [f32; 2] {
        [self.cx + self.radius * re as f32, self.cy - self.radius * im as f32]
    }

    pub fn gamma_at(&self, x: f32, y: f32) -> (f64, f64) {
        (((x - self.cx) / self.radius) as f64, ((self.cy - y) / self.radius) as f64)
    }

    /// Where a measured point is drawn: `None` for a reading that is not a number.
    fn place(&self, p: &Point) -> Option<[f32; 2]> {
        if !(p.re.is_finite() && p.im.is_finite()) {
            return None;
        }
        let size = p.re.hypot(p.im);
        let shrink = if size > MAX_GAMMA { MAX_GAMMA / size } else { 1.0 };
        Some(self.xy(p.re * shrink, p.im * shrink))
    }
}

fn mix(a: Rgb, b: Rgb, share_of_a: f32) -> Rgb {
    let m = |x: u8, y: u8| (x as f32 * share_of_a + y as f32 * (1.0 - share_of_a)).round() as u8;
    Rgb(m(a.0, b.0), m(a.1, b.1), m(a.2, b.2))
}

fn is_transmission(state: &AppState) -> bool {
    state.mode == UiMode::Transmission || state.result.as_ref().is_some_and(|r| r.request.mode == Mode::Transmission)
}

/// The point of the trace nearest to a pixel position, if one is close enough.
pub fn nearest_index(state: &AppState, width: f32, height: f32, x: f32, y: f32) -> Option<usize> {
    if is_transmission(state) {
        return None;
    }
    let disc = Disc::new(width, height);
    state
        .points()
        .iter()
        .enumerate()
        .filter_map(|(i, p)| disc.place(p).map(|[px, py]| ((px - x).hypot(py - y), i)))
        .filter(|(d, _)| *d <= PICK_RADIUS)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, i)| i)
}

pub fn build(state: &AppState, colors: &Colors, width: f32, height: f32) -> Vec<Prim> {
    let mut out = Vec::new();
    let disc = Disc::new(width, height);
    let grid = mix(colors.frame, colors.background, 0.3);
    let label = mix(colors.frame, colors.background, 0.65);
    out.push(Prim::Fill { rect: [0.0, 0.0, width.max(0.0), height.max(0.0)], color: colors.background });

    let line = |points: Vec<[f32; 2]>, color: Rgb, width: f32| Prim::Line { points, color, width, dashed: false };

    // Constant resistance: full circles through the right edge.
    for r in GRID {
        let (centre, radius) = (r / (1.0 + r), 1.0 / (1.0 + r));
        let circle = (0..=96).map(|i| {
            let a = i as f64 / 96.0 * std::f64::consts::TAU;
            disc.xy(centre + radius * a.cos(), radius * a.sin())
        });
        out.push(line(circle.collect(), if r == 1.0 { label } else { grid }, 1.0));
        let [x, y] = disc.xy(gamma_of(r, 0.0).0, 0.0);
        out.push(Prim::Text { pos: [x + 2.0, y + 1.0], text: format!("{}", r * Z0), color: label, anchor: Anchor::Left });
    }
    // Constant reactance: arcs from the rim to the right edge, inductive above, capacitive below.
    for x in GRID {
        for sign in [1.0, -1.0] {
            let arc = (0..=64).map(|i| {
                // Resistance from 0 to very large, spaced so that the arc is evenly drawn.
                let t = i as f64 / 64.0;
                let (re, im) = gamma_of(t * t * 60.0, sign * x);
                disc.xy(re, im)
            });
            out.push(line(arc.collect(), grid, 1.0));
            let (re, im) = gamma_of(0.0, sign * x);
            let [px, py] = disc.xy(re * 1.07, im * 1.07);
            let text = format!("{}j{}", if sign > 0.0 { "+" } else { "-" }, x * Z0);
            out.push(Prim::Text { pos: [px, py - 7.0], text, color: label, anchor: Anchor::Center });
        }
    }
    // The real axis and the rim.
    out.push(line(vec![disc.xy(-1.0, 0.0), disc.xy(1.0, 0.0)], grid, 1.0));
    let rim = (0..=180).map(|i| {
        let a = i as f64 / 180.0 * std::f64::consts::TAU;
        disc.xy(a.cos(), a.sin())
    });
    out.push(line(rim.collect(), colors.frame, 1.5));

    out.push(Prim::Text { pos: [6.0, 4.0], text: "Smith chart, 50 ohms".into(), color: colors.frame, anchor: Anchor::Left });
    let note_at = [6.0, 20.0];
    if is_transmission(state) {
        out.push(Prim::Text { pos: note_at, text: "The Smith chart needs a reflection sweep.".into(), color: colors.frame, anchor: Anchor::Left });
        return out;
    }
    let signed = state.settings.signed_phase;
    if !signed {
        let text = "Reactance sign not measured: upper half only (see Trace options).";
        out.push(Prim::Text { pos: note_at, text: text.into(), color: colors.frame, anchor: Anchor::Left });
    }

    let points = state.points();
    let trace: Vec<[f32; 2]> = points.iter().filter_map(|p| disc.place(p)).collect();
    if trace.len() > 1 {
        out.push(line(trace, colors.swr, if state.settings.thick_lines { 2.5 } else { 1.2 }));
    }

    // Each label has its own corner, so that markers on the same point stay readable.
    let mut marker = |index: Option<usize>, color: Rgb, name: &str, (dx, dy, anchor): (f32, f32, Anchor)| {
        let Some(p) = index.and_then(|i| points.get(i)) else { return };
        let Some([x, y]) = disc.place(p) else { return };
        let arm = 6.0;
        out.push(line(vec![[x - arm, y], [x + arm, y]], color, 2.0));
        out.push(line(vec![[x, y - arm], [x, y + arm]], color, 2.0));
        let sign = match (signed, p.xs < 0.0) {
            (false, _) => "±",
            (true, false) => "+",
            (true, true) => "-",
        };
        let text = format!("{name}  {:.3} MHz  {} {sign}j{}", p.freq_hz / 1e6, field(p.rs), field(p.xs.abs()));
        out.push(Prim::Text { pos: [x + dx, y + dy], text, color, anchor });
    };
    marker(state.m1, colors.m1, "M1", (8.0, -17.0, Anchor::Left));
    marker(state.m2, colors.m2, "M2", (8.0, 4.0, Anchor::Left));
    if state.show_swr_min {
        marker(swr_min_index(points), colors.swr_min, "min", (-8.0, -17.0, Anchor::Right));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::Mode;
    use crate::ui::plot::{build_view, Prim};
    use crate::ui::state::tests::{fake_result, state, swept};
    use crate::ui::state::{UiMode, View};

    fn close(a: f64, b: f64) {
        assert!((a - b).abs() < 1e-9, "{a} vs {b}");
    }

    fn texts(prims: &[Prim]) -> Vec<String> {
        prims.iter().filter_map(|p| if let Prim::Text { text, .. } = p { Some(text.clone()) } else { None }).collect()
    }

    #[test]
    fn impedance_to_reflection_coefficient() {
        let (re, im) = gamma_of(1.0, 0.0); // 50 ohms: the centre
        close(re, 0.0);
        close(im, 0.0);
        let (re, im) = gamma_of(0.0, 0.0); // short: left edge
        close(re, -1.0);
        close(im, 0.0);
        let (re, im) = gamma_of(1.0, 1.0); // 50 + j50
        close(re, 0.2);
        close(im, 0.4);
        let (re, im) = gamma_of(2.0, -1.0);
        close(re, 0.4);
        close(im, -0.2);
    }

    #[test]
    fn the_disc_is_centred_and_inductive_is_up() {
        let disc = Disc::new(800.0, 500.0);
        assert!(disc.radius > 150.0 && disc.radius * 2.0 < 500.0);
        assert_eq!(disc.xy(0.0, 0.0), [disc.cx, disc.cy]);
        assert_eq!(disc.xy(1.0, 0.0), [disc.cx + disc.radius, disc.cy]);
        assert!(disc.xy(0.0, 0.5)[1] < disc.cy);
        let [x, y] = disc.xy(0.3, -0.6);
        let (re, im) = disc.gamma_at(x, y);
        assert!((re - 0.3).abs() < 1e-5 && (im + 0.6).abs() < 1e-5);
    }

    #[test]
    fn the_trace_has_one_vertex_per_point_and_stays_in_the_disc() {
        let mut s = swept();
        for (i, p) in s.result.as_mut().unwrap().points.iter_mut().enumerate() {
            let a = i as f64 * 0.02;
            (p.re, p.im) = (0.7 * a.cos(), 0.7 * a.sin());
        }
        let prims = build(&s, &s.settings.screen, 800.0, 500.0);
        let disc = Disc::new(800.0, 500.0);
        let trace = prims
            .iter()
            .find_map(|p| match p {
                Prim::Line { points, color, .. } if color == &s.settings.screen.swr && points.len() == 291 => Some(points.clone()),
                _ => None,
            })
            .expect("a trace");
        for [x, y] in trace {
            let d = ((x - disc.cx).powi(2) + (y - disc.cy).powi(2)).sqrt();
            assert!((d - 0.7 * disc.radius).abs() < 0.01, "{d}");
        }
    }

    #[test]
    fn the_grid_is_there_without_a_sweep_and_every_number_is_finite() {
        let mut s = state();
        let empty = build(&s, &s.settings.screen, 800.0, 500.0);
        assert!(empty.iter().filter(|p| matches!(p, Prim::Line { .. })).count() >= 12);
        assert!(texts(&empty).contains(&"50".to_string()));

        s.result = Some(fake_result(1_000_000, 30_000_000, 290));
        (s.m1, s.m2) = (Some(3), Some(4));
        let points = &mut s.result.as_mut().unwrap().points;
        (points[3].re, points[3].im) = (f64::NAN, 0.1);
        (points[9].re, points[9].im) = (f64::INFINITY, f64::NEG_INFINITY);
        (points[12].re, points[12].im) = (40.0, -40.0); // far outside: bad calibration
        for size in [(800.0, 500.0), (0.0, 0.0), (1.0, 1.0), (30.0, 7.0)] {
            for prim in build(&s, &s.settings.screen, size.0, size.1) {
                let numbers: Vec<f32> = match prim {
                    Prim::Fill { rect, .. } | Prim::Banner { rect, .. } => rect.to_vec(),
                    Prim::Line { points, .. } => points.into_iter().flatten().collect(),
                    Prim::Text { pos, .. } => pos.to_vec(),
                };
                assert!(numbers.iter().all(|v| v.is_finite()));
            }
        }
    }

    #[test]
    fn markers_are_labelled_with_their_impedance() {
        let mut s = swept();
        s.m1 = Some(10);
        {
            let p = &mut s.result.as_mut().unwrap().points[10];
            (p.re, p.im, p.rs, p.xs) = (0.2, 0.4, 50.0, 50.0);
        }
        let all = texts(&build(&s, &s.settings.screen, 800.0, 500.0));
        assert!(all.iter().any(|t| t.starts_with("M1") && t.contains("50.00") && t.contains("j")), "{all:?}");
        assert!(!all.iter().any(|t| t.starts_with("M2")));
    }

    #[test]
    fn unsigned_data_and_transmission_are_explained() {
        let mut s = swept();
        let note = |s: &crate::ui::state::AppState| texts(&build(s, &s.settings.screen, 800.0, 500.0)).join("|");
        assert!(note(&s).contains("sign"), "{}", note(&s));
        s.settings.signed_phase = true;
        assert!(!note(&s).contains("sign"));
        s.mode = UiMode::Transmission;
        s.result.as_mut().unwrap().request.mode = Mode::Transmission;
        assert!(note(&s).contains("reflection"));
        let lines = build(&s, &s.settings.screen, 800.0, 500.0);
        assert!(!lines.iter().any(|p| matches!(p, Prim::Line { points, .. } if points.len() == 291)));
    }

    #[test]
    fn a_click_finds_the_nearest_point_of_the_trace() {
        let mut s = swept();
        for (i, p) in s.result.as_mut().unwrap().points.iter_mut().enumerate() {
            (p.re, p.im) = (-0.9 + i as f64 * 0.006, 0.0); // along the real axis
        }
        let disc = Disc::new(800.0, 500.0);
        let [x, y] = disc.xy(-0.9 + 100.0 * 0.006, 0.0);
        assert_eq!(nearest_index(&s, 800.0, 500.0, x, y - 6.0), Some(100));
        assert_eq!(nearest_index(&s, 800.0, 500.0, 2.0, 2.0), None); // far from the trace
        assert_eq!(nearest_index(&state(), 800.0, 500.0, x, y), None);
    }

    #[test]
    fn the_view_setting_chooses_the_drawing() {
        let mut s = swept();
        assert_eq!(build_view(&s, &s.settings.screen, 800.0, 500.0), crate::ui::plot::build(&s, &s.settings.screen, 800.0, 500.0));
        s.view = View::Smith;
        assert_eq!(build_view(&s, &s.settings.screen, 800.0, 500.0), build(&s, &s.settings.screen, 800.0, 500.0));
    }
}
