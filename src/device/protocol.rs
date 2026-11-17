//! The sweep command and its reply, as plain data. No input or output happens here.

use super::{Mode, F_MAX_HZ, F_MIN_HZ};
use crate::calc::RawPoint;

/// Everything that defines one sweep. Frequencies are as displayed, extender included.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SweepRequest {
    pub mode: Mode,
    pub start_hz: u64,
    pub stop_hz: u64,
    pub samples: u16,
    /// Frequency extender multiplier: 1 (none), 4, 8 or 16.
    pub extender: u8,
    pub dds_factor: f64,
    /// Work around the firmware's bad first point.
    pub step_bug: bool,
}

#[derive(thiserror::Error, Debug, PartialEq, Clone, Copy)]
pub enum RequestError {
    #[error("the frequency range is not valid")]
    Range,
    #[error("the range is too narrow for this number of samples")]
    StepZero,
    #[error("the number of samples must be between 10 and 1000")]
    Samples,
}

pub const SAMPLES_MIN: u16 = 10;
pub const SAMPLES_MAX: u16 = 1000;

/// Parking command: output near 1 Hz, one point, no step.
pub const PARK_PARTS: [&str; 4] = ["0\r", "10\r", "1\r", "0\r"];

fn word(hz: f64, extender: u8, dds_factor: f64) -> i64 {
    (hz / extender as f64 * dds_factor).round_ties_even() as i64
}

impl SweepRequest {
    pub fn validate(&self) -> Result<(), RequestError> {
        if ![1, 4, 8, 16].contains(&self.extender) {
            return Err(RequestError::Range);
        }
        let low = F_MIN_HZ as u64 * self.extender as u64;
        let high = F_MAX_HZ as u64 * self.extender as u64;
        if self.start_hz < low || self.stop_hz > high || self.start_hz >= self.stop_hz {
            return Err(RequestError::Range);
        }
        if !(SAMPLES_MIN..=SAMPLES_MAX).contains(&self.samples) {
            return Err(RequestError::Samples);
        }
        if self.step_hz() == 0 {
            return Err(RequestError::StepZero);
        }
        Ok(())
    }

    /// Whole Hz between points, as displayed.
    pub fn step_hz(&self) -> u64 {
        let span = self.stop_hz.saturating_sub(self.start_hz) as f64;
        (span / self.samples.max(1) as f64).round_ties_even() as u64
    }

    /// True when the sweep starts one step low so that the bad first point can be dropped.
    /// The start is never lowered below zero (the original sends a negative number there).
    pub fn lowers_start(&self) -> bool {
        self.step_bug && self.start_hz >= self.step_hz()
    }

    /// The four parts of the command, each ending in a carriage return.
    pub fn command_parts(&self) -> [String; 4] {
        let step = self.step_hz();
        let start = if self.lowers_start() { self.start_hz - step } else { self.start_hz };
        [
            match self.mode {
                Mode::Reflection => "0\r".to_string(),
                Mode::Transmission => "1\r".to_string(),
            },
            format!("{}\r", word(start as f64, self.extender, self.dds_factor)),
            format!("{}\r", self.samples as u32 + 2),
            format!("{}\r", word(step as f64, self.extender, self.dds_factor)),
        ]
    }

    /// Bytes the device sends back: two more points than samples, 4 bytes each.
    pub fn reply_len(&self) -> usize {
        (self.samples as usize + 2) * 4
    }

    /// Turn the reply into points 0..=samples. A short reply gives what it can.
    pub fn parse_reply(&self, reply: &[u8]) -> Vec<RawPoint> {
        let device = parse_points(reply);
        let skip = usize::from(self.lowers_start());
        let mut points: Vec<RawPoint> =
            device.into_iter().skip(skip).take(self.samples as usize + 1).collect();
        if self.step_bug && points.len() > 1 {
            points[0] = points[1];
        }
        points
    }

    /// Displayed frequency of points 0..=samples.
    pub fn freqs_hz(&self) -> Vec<f64> {
        let step = self.step_hz();
        (0..=self.samples as u64).map(|i| (self.start_hz + i * step) as f64).collect()
    }

    /// The frequencies the device itself produced: displayed frequency divided by the extender.
    pub fn device_freqs_hz(&self) -> Vec<f64> {
        self.freqs_hz().into_iter().map(|f| f / self.extender.max(1) as f64).collect()
    }
}

/// 4 bytes per point: phase then level, each low byte first. Trailing partial points are ignored.
pub fn parse_points(bytes: &[u8]) -> Vec<RawPoint> {
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| RawPoint { phase: u16::from_le_bytes([c[0], c[1]]), level: u16::from_le_bytes([c[2], c[3]]) })
        .collect()
}

/// Fixed-frequency output: mode 0, the frequency, one point, no step.
pub fn generator_parts(freq_hz: u64, extender: u8, dds_factor: f64) -> [String; 4] {
    [
        "0\r".to_string(),
        format!("{}\r", word(freq_hz as f64, extender.max(1), dds_factor)),
        "1\r".to_string(),
        "0\r".to_string(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::DDS_FACTOR_DEFAULT;

    fn req(mode: Mode, start: u64, stop: u64, samples: u16) -> SweepRequest {
        SweepRequest {
            mode,
            start_hz: start,
            stop_hz: stop,
            samples,
            extender: 1,
            dds_factor: DDS_FACTOR_DEFAULT,
            step_bug: true,
        }
    }

    fn reply(points: &[(u16, u16)]) -> Vec<u8> {
        points
            .iter()
            .flat_map(|(a, b)| [a.to_le_bytes(), b.to_le_bytes()].concat())
            .collect()
    }

    fn pairs(points: &[RawPoint]) -> Vec<(u16, u16)> {
        points.iter().map(|p| (p.phase, p.level)).collect()
    }

    #[test]
    fn full_range_command() {
        let r = req(Mode::Reflection, 1_000_000, 180_000_000, 500);
        assert_eq!(r.step_hz(), 358_000);
        assert_eq!(r.command_parts(), ["0\r", "6893423\r", "502\r", "3843996\r"]);
        assert_eq!(r.reply_len(), 2008);
    }

    #[test]
    fn without_step_bug_the_start_is_not_lowered() {
        let r = SweepRequest { step_bug: false, ..req(Mode::Reflection, 1_000_000, 180_000_000, 500) };
        assert_eq!(r.command_parts(), ["0\r", "10737418\r", "502\r", "3843996\r"]);
    }

    #[test]
    fn transmission_narrow_sweep() {
        assert_eq!(
            req(Mode::Transmission, 7_000_000, 7_200_000, 100).command_parts(),
            ["1\r", "75140453\r", "102\r", "21475\r"]
        );
    }

    #[test]
    fn extender_divides_what_is_sent() {
        let r = SweepRequest { extender: 4, ..req(Mode::Reflection, 430_000_000, 440_000_000, 200) };
        assert_eq!(r.command_parts(), ["0\r", "1154138243\r", "202\r", "134218\r"]);
        assert_eq!(r.freqs_hz()[1], 430_050_000.0);
        assert_eq!(r.device_freqs_hz()[1], 107_512_500.0);
        assert_eq!(r.freqs_hz().len(), 201);
    }

    #[test]
    fn start_is_not_lowered_below_zero() {
        let r = req(Mode::Reflection, 100_000, 180_000_000, 1000);
        assert!(!r.lowers_start());
        assert_eq!(r.command_parts(), ["0\r", "1073742\r", "1002\r", "1931662\r"]);
    }

    #[test]
    fn validation() {
        assert_eq!(req(Mode::Reflection, 7_000_000, 7_000_004, 500).validate(), Err(RequestError::StepZero));
        assert_eq!(req(Mode::Reflection, 9_000_000, 7_000_000, 500).validate(), Err(RequestError::Range));
        assert_eq!(req(Mode::Reflection, 7_000_000, 7_000_000, 500).validate(), Err(RequestError::Range));
        assert_eq!(req(Mode::Reflection, 50_000, 7_000_000, 500).validate(), Err(RequestError::Range));
        assert_eq!(req(Mode::Reflection, 1_000_000, 181_000_000, 500).validate(), Err(RequestError::Range));
        assert_eq!(req(Mode::Reflection, 1_000_000, 2_000_000, 5).validate(), Err(RequestError::Samples));
        assert_eq!(req(Mode::Reflection, 1_000_000, 2_000_000, 1001).validate(), Err(RequestError::Samples));
        assert!(req(Mode::Reflection, 1_000_000, 180_000_000, 500).validate().is_ok());
        let high = SweepRequest { extender: 16, ..req(Mode::Reflection, 2_400_000_000, 2_500_000_000, 500) };
        assert!(high.validate().is_ok());
        let odd = SweepRequest { extender: 3, ..req(Mode::Reflection, 1_000_000, 2_000_000, 500) };
        assert_eq!(odd.validate(), Err(RequestError::Range));
    }

    #[test]
    fn reply_parsing_with_step_bug() {
        let bytes = reply(&[(1, 101), (2, 102), (3, 103), (4, 104), (5, 105)]);
        // Start lowered: the first device point is dropped, then point 0 becomes a copy of point 1.
        let lowered = req(Mode::Reflection, 10_000_000, 13_000_000, 3);
        assert!(lowered.lowers_start());
        assert_eq!(pairs(&lowered.parse_reply(&bytes)), [(3, 103), (3, 103), (4, 104), (5, 105)]);
        // Option off: points as the device returned them.
        let off = SweepRequest { step_bug: false, ..lowered };
        assert_eq!(pairs(&off.parse_reply(&bytes)), [(1, 101), (2, 102), (3, 103), (4, 104)]);
        // Option on but the start could not be lowered: nothing dropped, point 0 still overwritten.
        let low = req(Mode::Reflection, 100_000, 180_000_000, 3);
        assert!(!low.lowers_start());
        assert_eq!(pairs(&low.parse_reply(&bytes)), [(2, 102), (2, 102), (3, 103), (4, 104)]);
    }

    #[test]
    fn short_reply_gives_what_it_can() {
        let r = req(Mode::Reflection, 10_000_000, 13_000_000, 3);
        assert_eq!(r.parse_reply(&[]).len(), 0);
        assert_eq!(parse_points(&[1, 0, 2, 0, 3]).len(), 1);
    }

    #[test]
    fn recorded_device_reply_parses() {
        let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/open_capture.json")).unwrap();
        let json: serde_json::Value = serde_json::from_str(&text).unwrap();
        let hex = json["reflection_open"]["replies_hex"][0].as_str().unwrap();
        let bytes: Vec<u8> = (0..hex.len() / 2).map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).unwrap()).collect();
        let p = parse_points(&bytes);
        assert_eq!(p.len(), 500);
        assert_eq!((p[0].phase, p[0].level), (87, 366));
        assert_eq!((p[1].phase, p[1].level), (12, 365));
        assert_eq!((p[499].phase, p[499].level), (131, 380));
    }

    #[test]
    fn generator_and_park() {
        assert_eq!(generator_parts(10_000_000, 1, DDS_FACTOR_DEFAULT), ["0\r", "107374182\r", "1\r", "0\r"]);
        assert_eq!(generator_parts(432_000_000, 4, DDS_FACTOR_DEFAULT), ["0\r", "1159641170\r", "1\r", "0\r"]);
        assert_eq!(PARK_PARTS, ["0\r", "10\r", "1\r", "0\r"]);
    }
}
