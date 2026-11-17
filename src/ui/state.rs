//! Everything the window knows and every decision it makes, with no drawing code.
//! The drawing modules only read this state and call these methods.

use crate::cal::Calibration;
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
}
