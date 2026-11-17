//! Everything the window knows and every decision it makes, with no drawing code.
//! The drawing modules only read this state and call these methods.

use std::path::Path;

use super::text::t;
use crate::device::RequestError;

use crate::cal::{CalError, Calibration, CAL_POINTS};
use crate::calc::tools::{
    bandwidth_q, cable_length_m, cable_loss, quarter_wave_crossings, refine_to_90, swr_min_index, velocity_from_length,
};
use crate::calc::Point;
use crate::device::{Mode, SweepRequest, F_MAX_HZ, F_MIN_HZ};
use crate::store::Settings;
use crate::calc::tools::{lc_resonance, max_rl_index, min_rl_index};
use crate::store::sweep_file::{SweepFile, ViewState};
use crate::sweep::{build_result, Event, SweepResult};

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

impl UiMode {
    /// The name used in the settings file.
    pub fn key(self) -> &'static str {
        match self {
            UiMode::Antenna => "antenna",
            UiMode::CableLoss => "cable-loss",
            UiMode::CableLength => "cable-length",
            UiMode::Transmission => "transmission",
            UiMode::Generator => "generator",
        }
    }

    /// The mode to start in: the saved one, except that the generator never starts by itself.
    pub fn from_key(key: &str) -> UiMode {
        [UiMode::CableLoss, UiMode::CableLength, UiMode::Transmission].into_iter().find(|m| m.key() == key).unwrap_or(UiMode::Antenna)
    }

    pub fn name(self) -> &'static str {
        match self {
            UiMode::Antenna => "Antenna",
            UiMode::CableLoss => "Cable Loss",
            UiMode::CableLength => "Cable Length",
            UiMode::Transmission => "Transmission",
            UiMode::Generator => "Generator",
        }
    }
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
    /// A value being typed in the small entry window, and the text so far.
    pub entry: Option<(Entry, String)>,
    pub view: View,
    /// The generator or the oscillator calibration is using the device: no sweeps, no mode change.
    pub device_held: bool,
    /// Size of the graph on screen in pixels, for saving it as a picture.
    pub graph_size: [u32; 2],
    /// The row shown as a large banner on the graph (the original's "Visual" boxes).
    pub big: Option<Big>,
    /// Put the markers on the band's default positions when the next sweep arrives.
    band_markers_pending: bool,
    first_result_seen: bool,
    /// Which pass of the automatic cable-length search is running (0 = none).
    auto_pass: u8,
}

impl AppState {
    pub fn new(settings: Settings, calibration: Calibration) -> Self {
        let start_hz = (settings.start_mhz * 1e6).round() as u64;
        let stop_hz = (settings.stop_mhz * 1e6).round() as u64;
        let samples = settings.samples;
        let mode = UiMode::from_key(&settings.last_mode);
        AppState {
            settings,
            calibration,
            mode,
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
            big: None,
            graph_size: [0, 0],
            view: View::Graph,
            device_held: false,
            band_markers_pending: false,
            first_result_seen: false,
            auto_pass: 0,
        }
    }

    fn limits_hz(&self) -> (u64, u64) {
        (F_MIN_HZ as u64 * self.extender as u64, F_MAX_HZ as u64 * self.extender as u64)
    }

    fn range_is_valid(&self, start_hz: u64, stop_hz: u64, samples: u16) -> bool {
        self.range_error(start_hz, stop_hz, samples).is_none()
    }

    /// Why a range cannot be swept, if it cannot.
    fn range_error(&self, start_hz: u64, stop_hz: u64, samples: u16) -> Option<RequestError> {
        SweepRequest { start_hz, stop_hz, samples, ..self.request() }.validate().err()
    }

    /// Apply the three text boxes. Anything invalid or out of range puts the previous
    /// value back in its box. Returns true if the range or the samples changed.
    pub fn commit_range(&mut self) -> bool {
        let before = (self.start_hz, self.stop_hz, self.samples);
        let (low, high) = self.limits_hz();
        let typed = |text: &str| {
            parse_mhz(text).map(|mhz| (mhz * 1e6).round() as u64).filter(|hz| (low..=high).contains(hz))
        };
        let (typed_start, typed_stop) = (typed(&self.start_text), typed(&self.stop_text));
        let start = typed_start.unwrap_or(self.start_hz);
        let stop = typed_stop.unwrap_or(self.stop_hz);
        let typed_samples = self.samples_text.trim().parse::<u32>().ok();
        let samples = typed_samples.map_or(self.samples, |n| n.clamp(10, 1000) as u16);
        // Anything refused goes back to the old value, and the status bar says why.
        if let Some(why) = self.range_error(start, stop, samples) {
            self.status = format!("!!! {why}");
            if self.range_is_valid(self.start_hz, self.stop_hz, samples) {
                self.samples = samples;
            }
        } else {
            (self.start_hz, self.stop_hz, self.samples) = (start, stop, samples);
            if typed_start.is_none() || typed_stop.is_none() || typed_samples.is_none() {
                self.status = format!("!!! {} {} - {} MHz", t("Not a usable value. Frequencies:"), low as f64 / 1e6, high as f64 / 1e6);
            }
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
                self.status.clear();
                // Only the band's own sweep: one of the old range may still have been under way.
                let own = result.request.start_hz == self.start_hz && result.request.stop_hz == self.stop_hz;
                self.result = Some(result);
                if own && std::mem::take(&mut self.band_markers_pending) {
                    self.marker_to_band_default(Marker::M1);
                    self.marker_to_band_default(Marker::M2);
                }
                if !std::mem::replace(&mut self.first_result_seen, true)
                    && self.settings.m1_at_start
                    && self.m1.is_none()
                {
                    self.toggle_marker(Marker::M1);
                }
            }
            Event::CalTable(..) => {}
            Event::Error(text) => {
                self.status = text;
                self.busy = false;
                self.continuous = false;
                self.auto_pass = 0;
            }
        }
    }

    /// In transmission mode only loss and phase mean anything.
    pub fn curve_available(&self, curve: Curve) -> bool {
        self.mode != UiMode::Transmission || matches!(curve, Curve::Rl | Curve::Phase)
    }

    pub fn step_text(&self) -> String {
        format!("(Step : {} Hz)", self.request().step_hz())
    }
}

/// What asking for a sweep leads to right now.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SweepKind {
    Once,
    /// A continuous sweep is running: restart it with the present settings.
    Continuous,
}

/// What the central area shows.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum View {
    /// Curves over frequency.
    #[default]
    Graph,
    Smith,
}

/// Which read-out row is shown large on the graph.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Big {
    M1,
    M2,
    SwrMin,
}

/// What the small entry window is asking for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Entry {
    /// A marker's frequency in MHz.
    Freq(Marker),
    /// A known cable length in metres, to work out the velocity factor.
    Length,
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
    pub fn points(&self) -> &[Point] {
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
        if let Some(why) = self.range_error(low, high, self.samples) {
            self.status = format!("!!! {why}");
            return false;
        }
        self.set_range(low, high);
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

    /// The large banner: which row it belongs to and its text, "frequency -> SWR".
    pub fn big_text(&self) -> Option<(Big, String)> {
        let big = self.big?;
        let index = match big {
            Big::M1 => self.m1,
            Big::M2 => self.m2,
            Big::SwrMin => self.swr_min(),
        }?;
        let p = self.points().get(index)?;
        Some((big, format!("{:.6} -> {}", p.freq_hz / 1e6, field(p.swr))))
    }

    /// The point with the lowest SWR.
    pub fn swr_min(&self) -> Option<usize> {
        swr_min_index(self.points())
    }
}

/// What the automatic cable-length search wants next.
pub enum AutoLength {
    NextSweep(SweepRequest),
    Done,
    Failed,
}

impl AppState {
    /// Change mode. As in the original, both markers go to their home positions.
    /// Refused (with a message) while the generator holds the device, and the generator
    /// cannot start while a sweep is running.
    pub fn set_mode(&mut self, mode: UiMode) -> bool {
        if self.device_held {
            self.status = t("Close the generator window first.").to_string();
            return false;
        }
        if mode == UiMode::Generator && self.busy {
            self.status = t("Stop the sweep before using the generator.").to_string();
            return false;
        }
        self.mode = mode;
        self.settings.last_mode = mode.key().to_string();
        if let Some(last) = self.points().len().checked_sub(1) {
            self.m1 = Some((last as f64 / 3.0).round() as usize);
            self.m2 = Some((last as f64 * 0.66).round() as usize);
        }
        true
    }

    /// What a request for a sweep should do now; `None` means it has to wait.
    pub fn sweep_kind(&self) -> Option<SweepKind> {
        match (self.device_held, self.busy, self.continuous) {
            (true, ..) => None,
            (false, false, _) => Some(SweepKind::Once),
            (false, true, true) => Some(SweepKind::Continuous),
            (false, true, false) => None,
        }
    }

    /// A new range that does not come from zooming: forget the zoom history.
    fn new_range(&mut self, start_hz: u64, stop_hz: u64) -> bool {
        if !self.set_range(start_hz, stop_hz) {
            return false;
        }
        self.zoom_stack.clear();
        self.current_band = None;
        true
    }

    /// Whether a band lies within what the device can do with the present extender.
    pub fn band_reachable(&self, index: usize) -> bool {
        let (low, high) = self.limits_hz();
        self.settings.bands.get(index).is_some_and(|b| b.start_mhz * 1e6 >= low as f64 && b.stop_mhz * 1e6 <= high as f64)
    }

    /// Select a band preset. False if the band does not exist or cannot be reached
    /// (for example 70 cm without a frequency extender).
    pub fn apply_band(&mut self, index: usize) -> bool {
        let Some(band) = self.settings.bands.get(index) else { return false };
        let (start, stop) = ((band.start_mhz * 1e6).round() as u64, (band.stop_mhz * 1e6).round() as u64);
        if !self.new_range(start, stop) {
            return false;
        }
        self.current_band = Some(index);
        self.band_markers_pending = self.settings.markers_from_band;
        true
    }

    /// The whole range of the device: 1 to 180 MHz, times the extender.
    pub fn apply_full_range(&mut self) {
        let mult = self.extender as u64;
        self.new_range(1_000_000 * mult, 180_000_000 * mult);
    }

    /// "160 - 10 m".
    pub fn apply_hf_range(&mut self) {
        self.new_range(1_700_000, 29_800_000);
    }

    /// Select a frequency extender (1 = none, 4, 8 or 16). The range becomes the full range.
    pub fn set_extender(&mut self, mult: u8) {
        if [1, 4, 8, 16].contains(&mult) && mult != self.extender {
            self.extender = mult;
            self.apply_full_range();
        }
    }

    fn marker_points(&self) -> Option<(&Point, &Point)> {
        let points = self.points();
        Some((points.get(self.m1?)?, points.get(self.m2?)?))
    }

    /// Cable loss at the two markers: minimum, average, maximum, in dB.
    pub fn cable_loss_fields(&self) -> Option<[String; 3]> {
        let (a, b) = self.marker_points()?;
        let loss = cable_loss(a.rl_db, b.rl_db);
        Some([field(loss.min), field(loss.avg), field(loss.max)])
    }

    /// Cable length in metres from the two markers and the velocity factor.
    pub fn cable_length_text(&self) -> Option<String> {
        let (a, b) = self.marker_points()?;
        cable_length_m(a.freq_hz, b.freq_hz, self.settings.velocity).map(field)
    }

    /// Work out the velocity factor from a known cable length and the two markers.
    /// False if the markers are not set or the result is not a possible factor.
    pub fn set_velocity_from_length(&mut self, length_m: f64) -> bool {
        let Some((a, b)) = self.marker_points() else { return false };
        match velocity_from_length(a.freq_hz, b.freq_hz, length_m) {
            Some(v) if v > 0.0 && v <= 1.0 => {
                self.settings.velocity = (v * 100.0).round() / 100.0;
                true
            }
            _ => false,
        }
    }

    pub fn auto_length_running(&self) -> bool {
        self.auto_pass != 0
    }

    /// Start the automatic cable-length search: a full sweep with the markers off.
    pub fn auto_length_start(&mut self) -> SweepRequest {
        self.apply_full_range();
        self.samples = 500;
        self.refresh_texts();
        (self.m1, self.m2) = (None, None);
        self.auto_pass = 1;
        self.request()
    }

    /// Call after each sweep while the search runs. The first pass zooms onto one full phase
    /// period (first to third 90 degree crossing); the second puts the markers on its ends.
    pub fn auto_length_on_result(&mut self) -> AutoLength {
        let pass = std::mem::take(&mut self.auto_pass);
        let points = self.points();
        let Some((first, third)) = quarter_wave_crossings(points).filter(|_| pass != 0) else {
            return AutoLength::Failed;
        };
        if pass == 1 {
            let (low, high) = self.limits_hz();
            let start = (points[first].freq_hz as u64).saturating_sub(1_000_000).max(low);
            let stop = (points[third].freq_hz as u64 + 1_000_000).min(high);
            if !self.new_range(start, stop) {
                return AutoLength::Failed;
            }
            self.auto_pass = 2;
            AutoLength::NextSweep(self.request())
        } else {
            let (m1, m2) = (refine_to_90(points, first), refine_to_90(points, third));
            (self.m1, self.m2) = (Some(m1), Some(m2));
            AutoLength::Done
        }
    }
}

/// A plain number typed by hand: a point or a comma as the decimal mark, may be negative.
pub fn parse_decimal(text: &str) -> Option<f64> {
    text.trim().replace(',', ".").parse::<f64>().ok().filter(|v| v.is_finite())
}

/// Adjust the trial Hz-to-device factor of the DDS calibration, within sane limits.
pub fn nudge_dds(trial: f64, step: f64) -> f64 {
    (trial + step).clamp(10.0, 11.5)
}

/// Change the generator frequency by `delta` Hz, staying within what the device can do.
pub fn generator_freq_step(freq_hz: u64, delta: i64, extender: u8) -> u64 {
    let mult = extender.max(1) as i64;
    let (low, high) = (F_MIN_HZ as i64 * mult, F_MAX_HZ as i64 * mult);
    (freq_hz as i64 + delta).clamp(low, high) as u64
}

impl AppState {
    /// Keep a freshly measured calibration table and write the calibration file.
    pub fn store_cal_table(&mut self, mode: Mode, table: Vec<i16>, path: &Path) -> Result<(), CalError> {
        if table.len() != CAL_POINTS {
            return Err(CalError::Format(format!("{} entries, expected {CAL_POINTS}", table.len())));
        }
        match mode {
            Mode::Reflection => self.calibration.reflection = Some(table),
            Mode::Transmission => self.calibration.transmission = Some(table),
        }
        self.calibration.save(path)
    }
}

/// Where the LC calculator's buttons can send marker 1.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum M1Target {
    MinSwr,
    MaxRl,
    MinRl,
}

impl AppState {
    /// What "Save" writes: the last sweep and how it is being looked at.
    pub fn to_sweep_file(&self) -> Option<SweepFile> {
        let result = self.result.as_ref()?;
        Some(SweepFile {
            request: result.request,
            raw: result.raw.clone(),
            cal: result.cal.clone(),
            view: ViewState {
                m1: self.m1,
                m2: self.m2,
                show: self.show,
                scale_60db: self.scale_60db,
                rl_offset_db: self.rl_offset_db,
            },
        })
    }

    /// Show a saved sweep: range, mode, markers and view come from the file and the
    /// values are recomputed from its raw readings.
    pub fn load_sweep_file(&mut self, file: SweepFile) {
        let SweepFile { request, raw, cal, view } = file;
        self.extender = request.extender;
        self.mode = match request.mode {
            Mode::Transmission => UiMode::Transmission,
            Mode::Reflection if self.mode == UiMode::Transmission || self.mode == UiMode::Generator => UiMode::Antenna,
            Mode::Reflection => self.mode,
        };
        (self.start_hz, self.stop_hz, self.samples) = (request.start_hz, request.stop_hz, request.samples);
        self.refresh_texts();
        self.zoom_stack.clear();
        self.current_band = None;
        self.band_markers_pending = false;
        self.auto_pass = 0;
        self.result = Some(build_result(request, raw, cal, self.settings.signed_phase));
        self.first_result_seen = true;
        (self.m1, self.m2) = (view.m1, view.m2);
        self.show = view.show;
        self.scale_60db = view.scale_60db;
        self.rl_offset_db = if view.rl_offset_db.is_finite() { view.rl_offset_db.clamp(-60.0, 60.0) } else { 0.0 };
        // A sweep made with an extender switches it on: its menu must be there to switch it off.
        if self.extender > 1 {
            self.settings.extender_menu = true;
        }
        self.status.clear();
    }

    /// The LC calculator: the capacitance in pF that resonates with `value` µH at marker 1's
    /// frequency, or the inductance in µH for `value` pF. The formula is the same both ways.
    pub fn lc_result(&self, value: f64) -> Option<f64> {
        let point = self.points().get(self.m1?)?;
        (value > 0.0 && value.is_finite()).then(|| lc_resonance(point.freq_hz / 1e6, value))
    }

    pub fn move_m1_to(&mut self, target: M1Target) {
        let found = match target {
            M1Target::MinSwr => swr_min_index(self.points()),
            M1Target::MaxRl => max_rl_index(self.points()),
            M1Target::MinRl => min_rl_index(self.points()),
        };
        if found.is_some() {
            self.m1 = found;
        }
    }
}

/// A numeric column of the band table.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BandField {
    Start,
    Stop,
    M1,
    M2,
}

impl AppState {
    /// Change one number of a band from typed text. False (and nothing changed) if the
    /// text is not a frequency or would leave the band without a valid range.
    pub fn set_band_field(&mut self, index: usize, field: BandField, text: &str) -> bool {
        let (Some(band), Some(mhz)) = (self.settings.bands.get_mut(index), parse_mhz(text)) else { return false };
        let (lowest, highest) = (F_MIN_HZ as f64 / 1e6, F_MAX_HZ as f64 / 1e6 * 16.0);
        let (mut start, mut stop, mut m1, mut m2) = (band.start_mhz, band.stop_mhz, band.m1_mhz, band.m2_mhz);
        match field {
            BandField::Start => start = mhz,
            BandField::Stop => stop = mhz,
            BandField::M1 => m1 = mhz,
            BandField::M2 => m2 = mhz,
        }
        if start < lowest || stop > highest || start >= stop {
            self.status = format!("!!! {} ({lowest} - {highest} MHz)", t("Begin must be below End"));
            return false;
        }
        (band.start_mhz, band.stop_mhz, band.m1_mhz, band.m2_mhz) = (start, stop, m1, m2);
        band.needs_extender = stop > F_MAX_HZ as f64 / 1e6;
        true
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

    // ---- modes, bands, extender, cable (Task 11) ----

    #[test]
    fn mode_change_places_markers() {
        let mut s = swept();
        s.set_mode(UiMode::CableLoss);
        assert_eq!(s.mode, UiMode::CableLoss);
        assert_eq!((s.m1, s.m2), (Some(97), Some(191)));
        let mut empty = state();
        empty.set_mode(UiMode::Transmission);
        assert_eq!((empty.m1, empty.m2), (None, None));
    }

    #[test]
    fn band_sets_range_and_optional_markers() {
        let mut s = swept();
        assert!(s.apply_band(5));
        assert_eq!((s.start_hz, s.stop_hz), (13_900_000, 14_500_000));
        assert_eq!(s.current_band, Some(5));
        s.on_event(Event::Sweep(fake_result(13_900_000, 14_500_000, 290)));
        assert_eq!((s.m1, s.m2), (None, None));

        s.settings.markers_from_band = true;
        assert!(s.apply_band(5));
        s.on_event(Event::Sweep(fake_result(13_900_000, 14_500_000, 290)));
        let freq = |i: Option<usize>| s.result.as_ref().unwrap().points[i.unwrap()].freq_hz;
        assert!((freq(s.m1) - 14.0e6).abs() < 2100.0);
        assert!((freq(s.m2) - 14.35e6).abs() < 2100.0);
        assert!(!s.apply_band(99));
    }

    #[test]
    fn extender_bands_need_an_extender() {
        let mut s = state();
        assert!(!s.apply_band(13));
        s.set_extender(4);
        assert!(s.apply_band(13));
        assert_eq!((s.start_hz, s.stop_hz), (429_000_000, 441_000_000));
    }

    #[test]
    fn extender_resets_range() {
        let mut s = state();
        s.set_extender(8);
        assert_eq!((s.extender, s.start_hz, s.stop_hz), (8, 8_000_000, 1_440_000_000));
        s.set_extender(1);
        assert_eq!((s.extender, s.start_hz, s.stop_hz), (1, 1_000_000, 180_000_000));
        s.set_extender(3);
        assert_eq!(s.extender, 1);
    }

    #[test]
    fn preset_ranges() {
        let mut s = state();
        s.apply_hf_range();
        assert_eq!((s.start_hz, s.stop_hz), (1_700_000, 29_800_000));
        s.apply_full_range();
        assert_eq!((s.start_hz, s.stop_hz), (1_000_000, 180_000_000));
    }

    #[test]
    fn cable_loss_fields() {
        let mut s = swept();
        assert!(s.cable_loss_fields().is_none());
        s.result.as_mut().unwrap().points[10].rl_db = 6.0;
        s.result.as_mut().unwrap().points[20].rl_db = 4.0;
        (s.m1, s.m2) = (Some(10), Some(20));
        assert_eq!(s.cable_loss_fields(), Some(["2.00".to_string(), "2.50".to_string(), "3.00".to_string()]));
    }

    #[test]
    fn cable_length_text() {
        let mut s = swept();
        s.settings.velocity = 0.67;
        s.place_marker(Marker::M1, 10e6);
        s.place_marker(Marker::M2, 21e6);
        assert_eq!(s.cable_length_text().as_deref(), Some("9.09"));
        s.m2 = s.m1;
        assert_eq!(s.cable_length_text(), None);
    }

    #[test]
    fn velocity_from_length() {
        let mut s = swept();
        s.place_marker(Marker::M1, 10e6);
        s.place_marker(Marker::M2, 20e6);
        assert!(s.set_velocity_from_length(10.0));
        assert_eq!(s.settings.velocity, 0.67);
        assert!(!s.set_velocity_from_length(1000.0)); // would need a factor above 1
        assert_eq!(s.settings.velocity, 0.67);
    }

    #[test]
    fn m1_can_be_switched_on_with_the_first_result() {
        let mut s = state();
        s.settings.m1_at_start = true;
        s.on_event(Event::Sweep(fake_result(1_000_000, 30_000_000, 290)));
        assert_eq!(s.m1, Some(97));
        s.m1 = None;
        s.on_event(Event::Sweep(fake_result(1_000_000, 30_000_000, 290)));
        assert_eq!(s.m1, None);
    }

    /// A cable-like result: phase is 90 degrees at 20, 40, 60 ... MHz.
    fn cable_result(start: u64, stop: u64) -> SweepResult {
        let mut r = fake_result(start, stop, 500);
        for p in &mut r.points {
            let t = ((p.freq_hz - 10e6) / 40e6).rem_euclid(1.0);
            p.phase_deg = 180.0 * (2.0 * t - 1.0).abs();
        }
        r
    }

    #[test]
    fn auto_length_two_passes() {
        let mut s = swept();
        (s.m1, s.m2) = (Some(3), Some(4));
        let first = s.auto_length_start();
        assert_eq!((first.start_hz, first.stop_hz, first.samples), (1_000_000, 180_000_000, 500));
        assert_eq!((s.m1, s.m2), (None, None));

        s.on_event(Event::Sweep(cable_result(1_000_000, 180_000_000)));
        let AutoLength::NextSweep(second) = s.auto_length_on_result() else { panic!("expected a second sweep") };
        assert!((18_000_000..19_600_000).contains(&second.start_hz), "{}", second.start_hz);
        assert!((60_000_000..61_600_000).contains(&second.stop_hz), "{}", second.stop_hz);

        s.on_event(Event::Sweep(cable_result(second.start_hz, second.stop_hz)));
        assert!(matches!(s.auto_length_on_result(), AutoLength::Done));
        let freq = |i: Option<usize>| s.result.as_ref().unwrap().points[i.unwrap()].freq_hz;
        assert!((freq(s.m1) - 20e6).abs() < 150_000.0, "{}", freq(s.m1));
        assert!((freq(s.m2) - 60e6).abs() < 150_000.0, "{}", freq(s.m2));
        assert!(!s.auto_length_running());
    }

    #[test]
    fn auto_length_fails_without_crossings() {
        let mut s = swept();
        s.auto_length_start();
        s.on_event(Event::Sweep(fake_result(1_000_000, 180_000_000, 500)));
        assert!(matches!(s.auto_length_on_result(), AutoLength::Failed));
        assert!(!s.auto_length_running());
        assert!(matches!(s.auto_length_on_result(), AutoLength::Failed));
    }

    // ---- device dialogs (Task 12) ----

    #[test]
    fn nudge_dds_stays_in_range() {
        assert!((nudge_dds(10.73741824, 0.001) - 10.73841824).abs() < 1e-12);
        assert!((nudge_dds(10.73741824, -0.000001) - 10.73741724).abs() < 1e-12);
        assert_eq!(nudge_dds(11.4999, 0.001), 11.5);
        assert_eq!(nudge_dds(10.0005, -0.001), 10.0);
    }

    #[test]
    fn generator_step_clamps() {
        assert_eq!(generator_freq_step(10_000_000, 1_000_000, 1), 11_000_000);
        assert_eq!(generator_freq_step(100_000, -1_000_000, 1), 100_000);
        assert_eq!(generator_freq_step(180_000_000, 1, 1), 180_000_000);
        assert_eq!(generator_freq_step(500_000, -1_000_000, 4), 400_000);
        assert_eq!(generator_freq_step(719_999_999, 10, 4), 720_000_000);
    }

    #[test]
    fn storing_a_table_marks_it_done() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("calibration.toml");
        let mut s = state();
        assert!(s.calibration.reflection.is_none());
        s.store_cal_table(Mode::Reflection, vec![-20; 1000], &path).unwrap();
        assert_eq!(s.calibration.reflection.as_deref(), Some(&[-20i16; 1000][..]));
        assert!(s.calibration.transmission.is_none());
        assert_eq!(Calibration::load(&path).unwrap(), s.calibration);
        s.store_cal_table(Mode::Transmission, vec![5; 1000], &path).unwrap();
        assert_eq!(Calibration::load(&path).unwrap(), s.calibration);
        assert!(s.store_cal_table(Mode::Reflection, vec![1; 7], &path).is_err());
        assert_eq!(s.calibration.reflection.as_deref(), Some(&[-20i16; 1000][..]));
    }

    // ---- files, LC calculator (Task 13) ----

    #[test]
    fn sweep_file_round_trip_through_state() {
        let mut s = swept();
        (s.m1, s.m2) = (Some(12), Some(200));
        s.show = [true, true, false, false, true, false];
        s.scale_60db = true;
        s.rl_offset_db = -3.0;
        let file = s.to_sweep_file().unwrap();

        let mut fresh = state();
        fresh.load_sweep_file(file);
        assert_eq!((fresh.start_hz, fresh.stop_hz, fresh.samples), (1_000_000, 30_000_000, 290));
        assert_eq!((fresh.start_text.as_str(), fresh.samples_text.as_str()), ("1", "290"));
        assert_eq!((fresh.m1, fresh.m2), (Some(12), Some(200)));
        assert_eq!(fresh.show, s.show);
        assert!(fresh.scale_60db);
        assert_eq!(fresh.rl_offset_db, -3.0);
        let (a, b) = (&s.result.as_ref().unwrap().points, &fresh.result.as_ref().unwrap().points);
        assert_eq!(a.len(), b.len());
        assert!(a.iter().zip(b).all(|(x, y)| x.rl_db == y.rl_db && x.freq_hz == y.freq_hz && x.swr == y.swr));
    }

    #[test]
    fn loading_a_transmission_file_switches_mode() {
        let mut s = swept();
        s.result.as_mut().unwrap().request.mode = Mode::Transmission;
        let file = s.to_sweep_file().unwrap();
        let mut fresh = state();
        fresh.load_sweep_file(file);
        assert_eq!(fresh.mode, UiMode::Transmission);
    }

    #[test]
    fn no_result_no_file() {
        assert!(state().to_sweep_file().is_none());
    }

    #[test]
    fn lc_uses_m1() {
        let mut s = swept();
        assert_eq!(s.lc_result(15.0), None);
        s.place_marker(Marker::M1, 7.1e6);
        assert!((s.lc_result(15.0).unwrap() - 33.497693).abs() < 1e-3);
        assert_eq!(s.lc_result(0.0), None);
        assert_eq!(s.lc_result(-1.0), None);
    }

    #[test]
    fn move_m1() {
        let mut s = swept();
        {
            let points = &mut s.result.as_mut().unwrap().points;
            points[40].swr = 1.1;
            points[60].rl_db = 55.0;
            points[80].rl_db = 0.5;
        }
        s.move_m1_to(M1Target::MinSwr);
        assert_eq!(s.m1, Some(40));
        s.move_m1_to(M1Target::MaxRl);
        assert_eq!(s.m1, Some(60));
        s.move_m1_to(M1Target::MinRl);
        assert_eq!(s.m1, Some(80));
        let mut empty = state();
        empty.move_m1_to(M1Target::MinSwr);
        assert_eq!(empty.m1, None);
    }

    #[test]
    fn big_banner_text() {
        let mut s = swept();
        assert_eq!(s.big_text(), None);
        s.big = Some(Big::M1);
        assert_eq!(s.big_text(), None); // marker 1 is off
        s.m1 = Some(131);
        s.result.as_mut().unwrap().points[131].swr = 1.5;
        assert_eq!(s.big_text(), Some((Big::M1, "14.100000 -> 1.50".to_string())));
    }

    #[test]
    fn band_cells_take_good_numbers_only() {
        let mut s = state();
        assert!(s.set_band_field(5, BandField::Start, "13,5"));
        assert_eq!(s.settings.bands[5].start_mhz, 13.5);
        assert!(!s.set_band_field(5, BandField::Start, "abc"));
        assert!(!s.set_band_field(5, BandField::Start, "20")); // not below the end
        assert!(!s.set_band_field(5, BandField::Stop, "5000")); // beyond any extender
        assert!(!s.set_band_field(99, BandField::Stop, "15"));
        assert_eq!((s.settings.bands[5].start_mhz, s.settings.bands[5].stop_mhz), (13.5, 14.5));
        assert!(s.set_band_field(5, BandField::M1, "14.1"));
        assert_eq!(s.settings.bands[5].m1_mhz, 14.1);
        assert!(s.set_band_field(5, BandField::Stop, "200"));
        assert!(s.settings.bands[5].needs_extender);
        assert!(s.set_band_field(5, BandField::Stop, "14.5"));
        assert!(!s.settings.bands[5].needs_extender);
    }

    // ---- findings of the final review ----

    #[test]
    fn a_refused_range_says_why() {
        let mut s = state();
        (s.start_text, s.stop_text, s.samples_text) = ("7".into(), "7.0001".into(), "500".into());
        assert!(!s.commit_range());
        assert!(s.status.contains("too narrow"), "{:?}", s.status);
        assert_eq!((s.start_hz, s.stop_hz), (1_000_000, 180_000_000));

        s.status.clear();
        (s.start_text, s.stop_text) = ("20".into(), "10".into());
        assert!(!s.commit_range());
        assert!(s.status.contains("not valid"), "{:?}", s.status);

        s.status.clear();
        s.start_text = "abc".into();
        assert!(!s.commit_range());
        assert!(!s.status.is_empty());

        s.status.clear();
        (s.start_text, s.stop_text) = ("3,5".into(), "4".into());
        assert!(s.commit_range());
        assert!(s.status.is_empty());
    }

    #[test]
    fn a_refused_zoom_says_why() {
        let mut s = swept();
        s.samples = 1000;
        {
            let points = &mut s.result.as_mut().unwrap().points;
            (points[10].freq_hz, points[11].freq_hz) = (7_000_000.0, 7_000_500.0);
        }
        (s.m1, s.m2) = (Some(10), Some(11));
        assert!(!s.zoom_to_markers());
        assert!(s.status.contains("too narrow"), "{:?}", s.status);
    }

    #[test]
    fn what_a_sweep_request_does_depends_on_the_device() {
        let mut s = state();
        assert_eq!(s.sweep_kind(), Some(SweepKind::Once));
        s.busy = true; // a single sweep is running: wait for it
        assert_eq!(s.sweep_kind(), None);
        s.continuous = true; // a continuous sweep follows the new settings
        assert_eq!(s.sweep_kind(), Some(SweepKind::Continuous));
        s.device_held = true; // the generator has the device
        assert_eq!(s.sweep_kind(), None);
        (s.busy, s.continuous) = (false, false);
        assert_eq!(s.sweep_kind(), None);
    }

    #[test]
    fn the_mode_cannot_change_under_the_generator() {
        let mut s = swept();
        s.device_held = true;
        assert!(!s.set_mode(UiMode::CableLoss));
        assert_eq!(s.mode, UiMode::Antenna);
        assert!(!s.status.is_empty());
        s.device_held = false;
        s.busy = true;
        assert!(!s.set_mode(UiMode::Generator)); // cannot take the device from a running sweep
        assert!(s.set_mode(UiMode::CableLoss));
    }

    #[test]
    fn band_markers_wait_for_the_bands_own_sweep() {
        let mut s = swept();
        s.settings.markers_from_band = true;
        assert!(s.apply_band(5));
        // A sweep of the old range that was still under way arrives first.
        s.on_event(Event::Sweep(fake_result(1_000_000, 30_000_000, 290)));
        assert_eq!((s.m1, s.m2), (None, None));
        s.on_event(Event::Sweep(fake_result(13_900_000, 14_500_000, 290)));
        let freq = |i: Option<usize>| s.result.as_ref().unwrap().points[i.unwrap()].freq_hz;
        assert!((freq(s.m1) - 14.0e6).abs() < 2100.0);
        assert!((freq(s.m2) - 14.35e6).abs() < 2100.0);
    }

    // ---- minor findings of the final review ----

    #[test]
    fn a_failed_sweep_ends_the_cable_length_search() {
        let mut s = swept();
        s.auto_length_start();
        s.on_event(Event::Error("unplugged".into()));
        assert!(!s.auto_length_running());
    }

    #[test]
    fn plain_numbers_take_a_point_or_a_comma() {
        assert_eq!(parse_decimal("0,66"), Some(0.66));
        assert_eq!(parse_decimal(" -3.5 "), Some(-3.5));
        assert_eq!(parse_decimal("1.2.3"), None);
        assert_eq!(parse_decimal(""), None);
        assert_eq!(parse_decimal("nan"), None);
        assert_eq!(parse_decimal("inf"), None);
    }

    #[test]
    fn the_last_mode_is_remembered() {
        let mut s = swept();
        assert!(s.set_mode(UiMode::CableLength));
        assert_eq!(s.settings.last_mode, "cable-length");
        let again = AppState::new(s.settings.clone(), Calibration::default());
        assert_eq!(again.mode, UiMode::CableLength);

        // The generator is never started by itself, and nonsense means Antenna.
        for text in ["generator", "what"] {
            let settings = Settings { last_mode: text.into(), ..Default::default() };
            assert_eq!(AppState::new(settings, Calibration::default()).mode, UiMode::Antenna);
        }
    }

    #[test]
    fn an_odd_sweep_file_cannot_spoil_the_view() {
        let mut file = swept().to_sweep_file().unwrap();
        file.view.rl_offset_db = f64::NAN;
        let mut s = state();
        s.load_sweep_file(file);
        assert_eq!(s.rl_offset_db, 0.0);
    }

    #[test]
    fn a_file_made_with_an_extender_shows_the_extender_menu() {
        let mut source = state();
        source.set_extender(4);
        source.on_event(Event::Sweep(SweepResult {
            request: SweepRequest { extender: 4, ..fake_result(4_000_000, 720_000_000, 290).request },
            ..fake_result(4_000_000, 720_000_000, 290)
        }));
        let file = source.to_sweep_file().unwrap();
        let mut s = state();
        assert!(!s.settings.extender_menu);
        s.load_sweep_file(file);
        assert_eq!(s.extender, 4);
        assert!(s.settings.extender_menu);
    }

    #[test]
    fn a_refused_band_cell_says_why_and_changes_nothing() {
        let mut s = state();
        let before = s.settings.bands[5].clone();
        assert!(!s.set_band_field(5, BandField::Start, "20"));
        assert!(s.status.contains("Begin"), "{:?}", s.status);
        assert_eq!(s.settings.bands[5], before);
    }

    #[test]
    fn a_band_is_reachable_only_with_enough_extender() {
        let mut s = state();
        let cm13 = s.settings.bands.iter().position(|b| b.stop_mhz == 2500.0).expect("the 13 cm band");
        let m2 = s.settings.bands.iter().position(|b| b.stop_mhz == 146.1).expect("the 2 m band");
        assert!(s.band_reachable(m2) && !s.band_reachable(cm13) && !s.band_reachable(999));
        s.set_extender(8);
        assert!(!s.band_reachable(cm13));
        s.set_extender(16);
        assert!(s.band_reachable(cm13));
    }

    #[test]
    fn choosing_the_extender_already_in_use_keeps_the_range() {
        let mut s = state();
        s.set_range(7_000_000, 7_200_000);
        s.set_extender(1);
        assert_eq!((s.start_hz, s.stop_hz), (7_000_000, 7_200_000));
    }
}
