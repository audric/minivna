//! Everything the window knows and every decision it makes, with no drawing code.
//! The drawing modules only read this state and call these methods.

use crate::cal::Calibration;
use crate::calc::tools::{bandwidth_q, swr_min_index};
use crate::calc::Point;
use crate::device::{Mode, SweepRequest, F_MAX_HZ, F_MIN_HZ};
use crate::store::Settings;
use crate::sweep::{Event, SweepResult};

/// What the user is doing. Decides which panels are shown and how the device sweeps.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UiMode {
    Antenna,
    CableLoss,
    CableLength,
    Transmission,
    Generator,
}

/// The six curves, in the order of the `show` array.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Curve {
    Swr,
    Z,
    Rl,
    Phase,
    Rs,
    Xs,
}

impl Curve {
    pub const ALL: [Curve; 6] = [Curve::Swr, Curve::Z, Curve::Rl, Curve::Phase, Curve::Rs, Curve::Xs];
}

/// A frequency typed by the user, in MHz. Accepts a decimal point or a decimal comma.
/// Only plain positive numbers are accepted.
pub fn parse_mhz(text: &str) -> Option<f64> {
    let text = text.trim().replace(',', ".");
    let plain = !text.is_empty()
        && text.chars().all(|c| c.is_ascii_digit() || c == '.')
        && text.chars().filter(|c| *c == '.').count() <= 1;
    if !plain {
        return None;
    }
    text.parse::<f64>().ok().filter(|v| v.is_finite() && *v > 0.0)
}

/// A frequency as the edit boxes show it: MHz without trailing zeros.
pub fn mhz_text(hz: u64) -> String {
    let text = format!("{:.6}", hz as f64 / 1e6);
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

pub struct AppState {
    pub settings: Settings,
    pub calibration: Calibration,
    pub mode: UiMode,
    /// Frequency extender multiplier: 1, 4, 8 or 16.
    pub extender: u8,
    /// What is typed in the three boxes; applied by `commit_range`.
    pub start_text: String,
    pub stop_text: String,
    pub samples_text: String,
    pub start_hz: u64,
    pub stop_hz: u64,
    pub samples: u16,
    /// Which curves are drawn: SWR, |Z|, RL, Phase, Rs, |Xs|.
    pub show: [bool; 6],
    pub scale_60db: bool,
    /// Shifts the plotted loss curve and its axis labels only.
    pub rl_offset_db: f64,
    pub result: Option<SweepResult>,
    pub busy: bool,
    pub progress: f32,
    pub continuous: bool,
    /// Shown in the status bar: the last error or notice.
    pub status: String,
    pub m1: Option<usize>,
    pub m2: Option<usize>,
    pub show_swr_min: bool,
    /// Height of the memo line as a fraction of the graph, if shown.
    pub memo_y: Option<f64>,
    /// Ranges to go back to with "unZoom".
    pub zoom_stack: Vec<(u64, u64)>,
    /// The band preset last chosen, if the range still comes from it.
    pub current_band: Option<usize>,
    /// The marker whose frequency is being typed, and the text so far.
    pub entry: Option<(Marker, String)>,
}

impl AppState {
    pub fn new(settings: Settings, calibration: Calibration) -> Self {
        let start_hz = (settings.start_mhz * 1e6).round() as u64;
        let stop_hz = (settings.stop_mhz * 1e6).round() as u64;
        let samples = settings.samples;
        AppState {
            settings,
            calibration,
            mode: UiMode::Antenna,
            extender: 1,
            start_text: mhz_text(start_hz),
            stop_text: mhz_text(stop_hz),
            samples_text: samples.to_string(),
            start_hz,
            stop_hz,
            samples,
            show: [true, false, true, false, false, false],
            scale_60db: false,
            rl_offset_db: 0.0,
            result: None,
            busy: false,
            progress: 0.0,
            continuous: false,
            status: String::new(),
            m1: None,
            m2: None,
            show_swr_min: false,
            memo_y: None,
            zoom_stack: Vec::new(),
            current_band: None,
            entry: None,
        }
    }

    fn limits_hz(&self) -> (u64, u64) {
        (F_MIN_HZ as u64 * self.extender as u64, F_MAX_HZ as u64 * self.extender as u64)
    }

    fn range_is_valid(&self, start_hz: u64, stop_hz: u64, samples: u16) -> bool {
        SweepRequest { start_hz, stop_hz, samples, ..self.request() }.validate().is_ok()
    }

    /// Apply the three text boxes. Anything invalid or out of range puts the previous
    /// value back in its box. Returns true if the range or the samples changed.
    pub fn commit_range(&mut self) -> bool {
        let before = (self.start_hz, self.stop_hz, self.samples);
        let (low, high) = self.limits_hz();
        let typed = |text: &str| {
            parse_mhz(text).map(|mhz| (mhz * 1e6).round() as u64).filter(|hz| (low..=high).contains(hz))
        };
        let start = typed(&self.start_text).unwrap_or(self.start_hz);
        let stop = typed(&self.stop_text).unwrap_or(self.stop_hz);
        let samples = match self.samples_text.trim().parse::<u32>() {
            Ok(n) => n.clamp(10, 1000) as u16,
            Err(_) => self.samples,
        };
        if self.range_is_valid(start, stop, samples) {
            (self.start_hz, self.stop_hz, self.samples) = (start, stop, samples);
        } else if self.range_is_valid(self.start_hz, self.stop_hz, samples) {
            self.samples = samples;
        }
        self.refresh_texts();
        (self.start_hz, self.stop_hz, self.samples) != before
    }

    /// Set the sweep range directly. Returns false and changes nothing if it is not valid.
    pub fn set_range(&mut self, start_hz: u64, stop_hz: u64) -> bool {
        if !self.range_is_valid(start_hz, stop_hz, self.samples) {
            return false;
        }
        self.start_hz = start_hz;
        self.stop_hz = stop_hz;
        self.refresh_texts();
        true
    }

    fn refresh_texts(&mut self) {
        self.start_text = mhz_text(self.start_hz);
        self.stop_text = mhz_text(self.stop_hz);
        self.samples_text = self.samples.to_string();
    }

    /// How the device sweeps in the current mode.
    pub fn sweep_mode(&self) -> Mode {
        match self.mode {
            UiMode::Transmission => Mode::Transmission,
            _ => Mode::Reflection,
        }
    }

    /// The sweep the current state asks for.
    pub fn request(&self) -> SweepRequest {
        SweepRequest {
            mode: self.sweep_mode(),
            start_hz: self.start_hz,
            stop_hz: self.stop_hz,
            samples: self.samples,
            extender: self.extender,
            dds_factor: self.settings.dds_factor,
            step_bug: self.settings.step_bug,
        }
    }

    /// Take in what the sweep worker reports.
    pub fn on_event(&mut self, event: Event) {
        match event {
            Event::Busy(busy) => {
                self.busy = busy;
                if !busy {
                    self.continuous = false;
                    self.progress = 0.0;
                }
            }
            Event::Progress(fraction) => self.progress = fraction,
            Event::Sweep(result) => {
                let last = result.points.len().saturating_sub(1);
                self.m1 = self.m1.map(|m| m.min(last));
                self.m2 = self.m2.map(|m| m.min(last));
                self.result = Some(result);
                self.status.clear();
            }
            Event::CalTable(..) => {}
            Event::Error(text) => {
                self.status = text;
                self.busy = false;
                self.continuous = false;
            }
        }
    }

    pub fn step_text(&self) -> String {
        format!("(Step : {} Hz)", self.request().step_hz())
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Marker {
    M1,
    M2,
}

/// A value as the read-out fields show it: two decimals, never "NaN" or a huge number.
pub fn field(value: f64) -> String {
    if value.is_nan() {
        String::new()
    } else if value.is_infinite() {
        "\u{221e}".to_string()
    } else if value.abs() > 9999.0 {
        if value > 0.0 { ">9999" } else { "<-9999" }.to_string()
    } else {
        format!("{value:.2}")
    }
}

impl AppState {
    fn points(&self) -> &[Point] {
        self.result.as_ref().map_or(&[], |r| &r.points)
    }

    fn marker_mut(&mut self, m: Marker) -> &mut Option<usize> {
        match m {
            Marker::M1 => &mut self.m1,
            Marker::M2 => &mut self.m2,
        }
    }

    /// The point nearest to a frequency. `None` without a sweep.
    pub fn index_at(&self, freq_hz: f64) -> Option<usize> {
        let points = self.points();
        let (first, last) = (points.first()?, points.last()?);
        if points.len() < 2 {
            return Some(0);
        }
        let step = (last.freq_hz - first.freq_hz) / (points.len() - 1) as f64;
        let index = ((freq_hz - first.freq_hz) / step).round();
        Some(index.clamp(0.0, (points.len() - 1) as f64) as usize)
    }

    pub fn place_marker(&mut self, m: Marker, freq_hz: f64) {
        if let Some(index) = self.index_at(freq_hz) {
            *self.marker_mut(m) = Some(index);
        }
    }

    /// Move a marker by a number of points, staying on the sweep.
    pub fn step_marker(&mut self, m: Marker, delta: i32) {
        let Some(last) = self.points().len().checked_sub(1) else { return };
        if let Some(index) = self.marker_mut(m) {
            *index = (*index as i64 + delta as i64).clamp(0, last as i64) as usize;
        }
    }

    /// Switch a marker on (M1 at one third of the sweep, M2 at two thirds) or off.
    pub fn toggle_marker(&mut self, m: Marker) {
        let Some(last) = self.points().len().checked_sub(1) else { return };
        let home = match m {
            Marker::M1 => (last as f64 / 3.0).round() as usize,
            Marker::M2 => (last as f64 * 0.66).round() as usize,
        };
        let slot = self.marker_mut(m);
        *slot = if slot.is_some() { None } else { Some(home) };
    }

    /// Put a marker on the default frequency of the current band preset.
    pub fn marker_to_band_default(&mut self, m: Marker) {
        let Some(band) = self.current_band.and_then(|i| self.settings.bands.get(i)) else { return };
        let mhz = match m {
            Marker::M1 => band.m1_mhz,
            Marker::M2 => band.m2_mhz,
        };
        self.place_marker(m, mhz * 1e6);
    }

    /// Make the two markers the new sweep range. The old range can be restored with `unzoom`.
    pub fn zoom_to_markers(&mut self) -> bool {
        let (Some(a), Some(b)) = (self.m1, self.m2) else { return false };
        let points = self.points();
        let (Some(pa), Some(pb)) = (points.get(a), points.get(b)) else { return false };
        let (low, high) = (pa.freq_hz.min(pb.freq_hz) as u64, pa.freq_hz.max(pb.freq_hz) as u64);
        let before = (self.start_hz, self.stop_hz);
        if !self.set_range(low, high) {
            return false;
        }
        self.zoom_stack.push(before);
        self.current_band = None;
        true
    }

    pub fn unzoom(&mut self) -> bool {
        match self.zoom_stack.pop() {
            Some((start, stop)) => self.set_range(start, stop),
            None => false,
        }
    }

    /// Show the memo line at this height (0 bottom, 1 top), or remove it if it is shown.
    pub fn toggle_memo(&mut self, y: f64) {
        self.memo_y = if self.memo_y.is_some() { None } else { Some(y) };
    }

    /// Frequency in MHz, SWR, |Z|, loss, phase, Rs and Xs at a point, as text.
    pub fn marker_row(&self, idx: usize) -> [String; 7] {
        match self.points().get(idx) {
            Some(p) => [
                format!("{:.6}", p.freq_hz / 1e6),
                field(p.swr),
                field(p.z_mag),
                field(p.rl_db),
                field(p.phase_deg),
                field(p.rs),
                field(p.xs),
            ],
            None => Default::default(),
        }
    }

    /// With both markers set: whether the load looks inductive, and the inductance in µH
    /// (or the capacitance in pF) at M1 and M2.
    pub fn reactance_fields(&self) -> Option<(bool, String, String)> {
        let points = self.points();
        let (a, b) = (points.get(self.m1?)?, points.get(self.m2?)?);
        let inductive = a.c_pf > b.c_pf;
        Some(if inductive {
            (true, field(a.l_uh), field(b.l_uh))
        } else {
            (false, field(a.c_pf), field(b.c_pf))
        })
    }

    /// With both markers set on different points: bandwidth in Hz and Q.
    pub fn bandwidth_fields(&self) -> Option<(String, String)> {
        let points = self.points();
        let (a, b) = (points.get(self.m1?)?, points.get(self.m2?)?);
        let (bw, q) = bandwidth_q(a.freq_hz, b.freq_hz)?;
        Some((format!("{bw:.0}"), field(q)))
    }

    /// The point with the lowest SWR.
    pub fn swr_min(&self) -> Option<usize> {
        swr_min_index(self.points())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::calc::RawPoint;
    use crate::device::DDS_FACTOR_DEFAULT;
    use crate::sweep::build_result;

    pub fn state() -> AppState {
        AppState::new(Settings::default(), Calibration::default())
    }

    fn type_range(s: &mut AppState, start: &str, stop: &str, samples: &str) -> bool {
        s.start_text = start.into();
        s.stop_text = stop.into();
        s.samples_text = samples.into();
        s.commit_range()
    }

    #[test]
    fn parse_mhz_accepts_comma_and_point() {
        assert_eq!(parse_mhz("3.5"), Some(3.5));
        assert_eq!(parse_mhz(" 3,5 "), Some(3.5));
        assert_eq!(parse_mhz("180"), Some(180.0));
        for bad in ["", "abc", "1.2.3", "-5", "1e3x", "NaN", "inf", "1,2,3", "0"] {
            assert_eq!(parse_mhz(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn starts_from_the_settings() {
        let s = state();
        assert_eq!((s.start_hz, s.stop_hz, s.samples), (1_000_000, 180_000_000, 500));
        assert_eq!((s.start_text.as_str(), s.stop_text.as_str(), s.samples_text.as_str()), ("1", "180", "500"));
        assert_eq!(s.show, [true, false, true, false, false, false]);
    }

    #[test]
    fn commit_range_accepts_valid_text() {
        let mut s = state();
        assert!(type_range(&mut s, "7", "7,2", "100"));
        assert_eq!((s.start_hz, s.stop_hz, s.samples), (7_000_000, 7_200_000, 100));
        assert_eq!(s.stop_text, "7.2");
        assert!(!s.commit_range());
    }

    #[test]
    fn commit_range_restores_invalid_text() {
        let mut s = state();
        for (start, stop, samples) in [("200", "180", "500"), ("1", "0.5", "500"), ("abc", "180", "500"), ("", "180", "500"), ("1", "", "500"), ("0.05", "180", "500"), ("1", "181", "500"), ("50", "50", "500")] {
            assert!(!type_range(&mut s, start, stop, samples), "{start:?} {stop:?}");
            assert_eq!((s.start_text.as_str(), s.stop_text.as_str()), ("1", "180"));
            assert_eq!((s.start_hz, s.stop_hz), (1_000_000, 180_000_000));
        }
        type_range(&mut s, "1", "180", "5");
        assert_eq!((s.samples, s.samples_text.as_str()), (10, "10"));
        type_range(&mut s, "1", "180", "5000");
        assert_eq!((s.samples, s.samples_text.as_str()), (1000, "1000"));
        type_range(&mut s, "1", "180", "");
        assert_eq!((s.samples, s.samples_text.as_str()), (1000, "1000"));
        type_range(&mut s, "1", "180", "12x");
        assert_eq!(s.samples, 1000);
    }

    #[test]
    fn a_range_too_narrow_for_the_samples_is_refused() {
        let mut s = state();
        assert!(!type_range(&mut s, "7", "7.000004", "500"));
        assert_eq!((s.start_hz, s.stop_hz), (1_000_000, 180_000_000));
        assert!(s.request().validate().is_ok());
    }

    #[test]
    fn set_range_refuses_reversed_or_out_of_limit() {
        let mut s = state();
        assert!(!s.set_range(9_000_000, 7_000_000));
        assert!(!s.set_range(7_000_000, 7_000_000));
        assert!(!s.set_range(50_000, 7_000_000));
        assert!(!s.set_range(1_000_000, 181_000_000));
        assert_eq!((s.start_hz, s.stop_hz), (1_000_000, 180_000_000));
        assert!(s.set_range(13_900_000, 14_500_000));
        assert_eq!((s.start_text.as_str(), s.stop_text.as_str()), ("13.9", "14.5"));
    }

    #[test]
    fn extender_scales_the_limits() {
        let mut s = state();
        assert!(!s.set_range(430_000_000, 440_000_000));
        s.extender = 4;
        assert!(s.set_range(430_000_000, 440_000_000));
    }

    #[test]
    fn request_reflects_state() {
        let mut s = state();
        s.settings.step_bug = false;
        s.settings.dds_factor = 10.74;
        s.mode = UiMode::Transmission;
        let r = s.request();
        assert_eq!(r.mode, Mode::Transmission);
        assert!(!r.step_bug);
        assert_eq!(r.dds_factor, 10.74);
        assert_eq!((r.start_hz, r.stop_hz, r.samples, r.extender), (1_000_000, 180_000_000, 500, 1));
        s.mode = UiMode::CableLength;
        assert_eq!(s.request().mode, Mode::Reflection);
    }

    pub fn fake_result(start: u64, stop: u64, samples: u16) -> SweepResult {
        let request = SweepRequest {
            mode: Mode::Reflection,
            start_hz: start,
            stop_hz: stop,
            samples,
            extender: 1,
            dds_factor: DDS_FACTOR_DEFAULT,
            step_bug: true,
        };
        let n = samples as usize + 1;
        build_result(request, vec![RawPoint { phase: 100, level: 500 }; n], vec![0; n], false)
    }

    #[test]
    fn events_update_state() {
        let mut s = state();
        s.continuous = true;
        s.on_event(Event::Busy(true));
        assert!(s.busy);
        s.on_event(Event::Progress(0.5));
        assert_eq!(s.progress, 0.5);
        s.status = "old".into();
        s.on_event(Event::Sweep(fake_result(1_000_000, 30_000_000, 290)));
        assert_eq!(s.result.as_ref().unwrap().points.len(), 291);
        assert_eq!(s.status, "");
        s.on_event(Event::Error("x".into()));
        assert_eq!(s.status, "x");
        assert!(!s.busy && !s.continuous);
        s.continuous = true;
        s.on_event(Event::Busy(false));
        assert!(!s.continuous);
    }

    #[test]
    fn step_text() {
        assert_eq!(state().step_text(), "(Step : 358000 Hz)");
    }

    // ---- markers (Task 10) ----

    /// A state holding a 1-30 MHz sweep of 290 samples: points are 100 kHz apart.
    pub fn swept() -> AppState {
        let mut s = state();
        s.set_range(1_000_000, 30_000_000);
        s.samples = 290;
        s.on_event(Event::Sweep(fake_result(1_000_000, 30_000_000, 290)));
        s
    }

    #[test]
    fn place_and_step() {
        let mut s = swept();
        s.place_marker(Marker::M1, 14.12e6);
        assert_eq!(s.m1, Some(131));
        s.step_marker(Marker::M1, 1);
        assert_eq!(s.m1, Some(132));
        s.step_marker(Marker::M1, -500);
        assert_eq!(s.m1, Some(0));
        s.step_marker(Marker::M1, 5000);
        assert_eq!(s.m1, Some(290));
        s.place_marker(Marker::M2, 999e6);
        assert_eq!(s.m2, Some(290));
        s.m2 = None;
        s.step_marker(Marker::M2, 1);
        assert_eq!(s.m2, None);
    }

    #[test]
    fn no_result_means_no_markers() {
        let mut s = state();
        s.place_marker(Marker::M1, 14e6);
        s.toggle_marker(Marker::M2);
        assert_eq!((s.m1, s.m2), (None, None));
        assert_eq!(s.index_at(14e6), None);
        assert!(!s.zoom_to_markers());
        assert!(s.reactance_fields().is_none() && s.bandwidth_fields().is_none() && s.swr_min().is_none());
    }

    #[test]
    fn toggle_places_at_thirds() {
        let mut s = swept();
        s.toggle_marker(Marker::M1);
        s.toggle_marker(Marker::M2);
        assert_eq!((s.m1, s.m2), (Some(97), Some(191)));
        s.toggle_marker(Marker::M1);
        assert_eq!(s.m1, None);
    }

    #[test]
    fn markers_clamp_when_samples_shrink() {
        let mut s = swept();
        s.m1 = Some(280);
        s.m2 = Some(50);
        s.on_event(Event::Sweep(fake_result(1_000_000, 30_000_000, 100)));
        assert_eq!((s.m1, s.m2), (Some(100), Some(50)));
    }

    #[test]
    fn zoom_and_unzoom() {
        let mut s = swept();
        s.place_marker(Marker::M1, 20e6);
        s.place_marker(Marker::M2, 10e6);
        assert!(s.zoom_to_markers());
        assert_eq!((s.start_hz, s.stop_hz), (10_000_000, 20_000_000));
        assert_eq!((s.start_text.as_str(), s.stop_text.as_str()), ("10", "20"));
        assert!(s.unzoom());
        assert_eq!((s.start_hz, s.stop_hz), (1_000_000, 30_000_000));
        assert!(!s.unzoom());
        s.m2 = s.m1;
        assert!(!s.zoom_to_markers());
        s.m2 = None;
        assert!(!s.zoom_to_markers());
    }

    #[test]
    fn marker_row_formats() {
        let mut s = swept();
        let p = &mut s.result.as_mut().unwrap().points[131];
        (p.swr, p.z_mag, p.rl_db, p.phase_deg, p.rs, p.xs) = (1.23456, 51.0, 20.129, 17.5, f64::INFINITY, 12345.6);
        assert_eq!(s.marker_row(131), ["14.100000", "1.23", "51.00", "20.13", "17.50", "\u{221e}", ">9999"]);
        assert_eq!(s.marker_row(5000), ["", "", "", "", "", "", ""]);
    }

    #[test]
    fn reactance_fields_need_both_markers() {
        let mut s = swept();
        {
            let points = &mut s.result.as_mut().unwrap().points;
            (points[10].l_uh, points[10].c_pf) = (1.5, 300.0);
            (points[20].l_uh, points[20].c_pf) = (2.5, 100.0);
        }
        s.m1 = Some(10);
        assert!(s.reactance_fields().is_none());
        s.m2 = Some(20);
        assert_eq!(s.reactance_fields(), Some((true, "1.50".to_string(), "2.50".to_string())));
        (s.m1, s.m2) = (Some(20), Some(10));
        assert_eq!(s.reactance_fields(), Some((false, "100.00".to_string(), "300.00".to_string())));
    }

    #[test]
    fn bandwidth_fields() {
        let mut s = swept();
        s.place_marker(Marker::M1, 7.0e6);
        s.place_marker(Marker::M2, 7.2e6);
        assert_eq!(s.bandwidth_fields(), Some(("200000".to_string(), "35.50".to_string())));
        s.m2 = s.m1;
        assert_eq!(s.bandwidth_fields(), None);
    }

    #[test]
    fn memo_toggles() {
        let mut s = state();
        s.toggle_memo(0.4);
        assert_eq!(s.memo_y, Some(0.4));
        s.toggle_memo(0.9);
        assert_eq!(s.memo_y, None);
    }

    #[test]
    fn swr_min_follows_the_result() {
        let mut s = swept();
        s.result.as_mut().unwrap().points[77].swr = 1.05;
        assert_eq!(s.swr_min(), Some(77));
    }
}
