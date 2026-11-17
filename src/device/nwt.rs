//! The "NWT" sweeper protocol, as spoken by the 35-4400 MHz sweep generator / detector
//! boxes (LTDZ family, firmware version byte 119). Scalar: one amplitude reading per point,
//! no phase. Everything here was confirmed on a real unit; see
//! docs/reverse-engineering/nwt-sweeper-findings.md.

use super::SweepRequest;
use crate::calc::{Point, RawPoint};

/// Every command starts with this byte.
const LEAD: u8 = 0x8f;
/// Asks for the firmware version; the answer is one byte (119 on the unit examined).
pub const VERSION_QUERY: [u8; 2] = [LEAD, b'v'];
/// Frequencies travel in units of 10 Hz.
pub const HZ_PER_UNIT: u64 = 10;
/// Typical slope of the detector, in dB per count, until it has been measured with an attenuator.
pub const DB_PER_COUNT_DEFAULT: f64 = 0.1;
/// Reading taken as 0 dB before the reference has been measured.
pub const REFERENCE_DEFAULT: i16 = 600;

/// One sweep: start (9 digits), step (8 digits), number of points (4 digits), all in
/// decimal, then two digits whose meaning is not known ("00" works; without them the
/// unit waits for ever).
pub fn sweep_command(req: &SweepRequest) -> Vec<u8> {
    let (start, step) = (req.start_hz / HZ_PER_UNIT, device_step_hz(req) / HZ_PER_UNIT);
    let mut out = vec![LEAD, b'a'];
    out.extend_from_slice(format!("{start:09}{step:08}{:04}00", device_points(req)).as_bytes());
    out
}

/// The largest step the unit honours. Of a larger one it ignores everything from
/// 10 MHz up: asked for 43.65 MHz it steps by 3.65 MHz (seen on hardware).
pub const MAX_STEP_HZ: u64 = 9_999_990;

/// How many device steps make one step of the sweep asked for. A sweep coarser than
/// `MAX_STEP_HZ` is made this many times finer and every such point kept.
pub fn thinning(req: &SweepRequest) -> u64 {
    let span = req.stop_hz.saturating_sub(req.start_hz) as f64;
    ((span / req.samples.max(1) as f64) / MAX_STEP_HZ as f64).ceil().max(1.0) as u64
}

/// The step sent to the unit, a whole number of tens of Hz.
pub fn device_step_hz(req: &SweepRequest) -> u64 {
    let span = req.stop_hz.saturating_sub(req.start_hz) as f64;
    let step = span / req.samples.max(1) as f64 / thinning(req) as f64;
    (step / HZ_PER_UNIT as f64).round_ties_even() as u64 * HZ_PER_UNIT
}

fn device_points(req: &SweepRequest) -> usize {
    req.samples as usize * thinning(req) as usize + 1
}

/// Bytes in the answer to `sweep_command`: four per point.
pub fn reply_len(req: &SweepRequest) -> usize {
    device_points(req) * 4
}

/// The points asked for, out of the answer to `sweep_command`.
pub fn parse_sweep(req: &SweepRequest, reply: &[u8]) -> Vec<RawPoint> {
    parse_reply(reply).into_iter().step_by(thinning(req) as usize).collect()
}

/// Each point is the detector reading as a 16-bit number, low byte first, sent twice
/// (the protocol has two channels; this unit has one input).
pub fn parse_reply(reply: &[u8]) -> Vec<RawPoint> {
    reply.as_chunks::<4>().0.iter().map(|c| RawPoint { phase: 0, level: u16::from_le_bytes([c[0], c[1]]) }).collect()
}

/// Switch the output to one frequency.
pub fn generator_command(freq_hz: u64) -> Vec<u8> {
    let mut out = vec![LEAD, b'f'];
    out.extend_from_slice(format!("{:09}", freq_hz / HZ_PER_UNIT).as_bytes());
    out
}

/// Loss in dB against the reference reading (output joined to input) at each point.
/// A scalar detector gives nothing else: SWR, phase and impedance are "not a number".
pub fn to_points(raw: &[RawPoint], reference: &[i16], db_per_count: f64, start_hz: f64, step_hz: f64) -> Vec<Point> {
    raw.iter()
        .enumerate()
        .map(|(i, r)| {
            let zero = reference.get(i).copied().unwrap_or(REFERENCE_DEFAULT) as f64;
            Point {
                freq_hz: start_hz + i as f64 * step_hz,
                phase_deg: f64::NAN,
                rl_db: (zero - r.level as f64) * db_per_count,
                re: 0.0,
                im: 0.0,
                swr: f64::NAN,
                z_mag: f64::NAN,
                rs: f64::NAN,
                xs: f64::NAN,
                l_uh: f64::NAN,
                c_pf: f64::NAN,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::{DeviceKind, Mode, RequestError, SweepRequest, DDS_FACTOR_DEFAULT};

    fn req(start_hz: u64, stop_hz: u64, samples: u16) -> SweepRequest {
        SweepRequest {
            kind: DeviceKind::Nwt,
            mode: Mode::Transmission,
            start_hz,
            stop_hz,
            samples,
            extender: 1,
            dds_factor: DDS_FACTOR_DEFAULT,
            step_bug: false,
        }
    }

    #[test]
    fn sweep_command_is_what_the_real_unit_accepted() {
        // 100 to 110 MHz in 1 MHz steps: 11 points. Frequencies go out in units of 10 Hz.
        assert_eq!(sweep_command(&req(100_000_000, 110_000_000, 10)), b"\x8fa01000000000100000001100".to_vec());
        assert_eq!(reply_len(&req(100_000_000, 110_000_000, 10)), 44);
    }

    #[test]
    fn reply_is_one_reading_per_point_sent_twice() {
        let points = parse_reply(&[56, 0, 56, 0, 58, 0, 58, 0, 0x2c, 0x01, 0x2c, 0x01]);
        let levels: Vec<u16> = points.iter().map(|p| p.level).collect();
        assert_eq!(levels, vec![56, 58, 300]);
        assert!(points.iter().all(|p| p.phase == 0));
        assert_eq!(parse_reply(&[1, 0, 1]).len(), 0); // an incomplete point is dropped
    }

    #[test]
    fn generator_and_version_commands() {
        assert_eq!(generator_command(145_000_000), b"\x8ff014500000".to_vec());
        assert_eq!(VERSION_QUERY, [0x8f, b'v']);
    }

    #[test]
    fn the_range_is_35_to_4400_mhz_and_the_step_at_least_10_hz() {
        assert_eq!(req(35_000_000, 4_400_000_000, 500).validate(), Ok(()));
        assert_eq!(req(34_000_000, 100_000_000, 500).validate(), Err(RequestError::Range));
        assert_eq!(req(100_000_000, 4_500_000_000, 500).validate(), Err(RequestError::Range));
        assert_eq!(req(200_000_000, 100_000_000, 500).validate(), Err(RequestError::Range));
        assert_eq!(req(100_000_000, 100_001_000, 500).validate(), Err(RequestError::StepZero));
        // The miniVNA's limits are untouched.
        let vna = SweepRequest { kind: DeviceKind::MiniVna, mode: Mode::Reflection, ..req(100_000_000, 400_000_000, 500) };
        assert_eq!(vna.validate(), Err(RequestError::Range));
    }

    #[test]
    fn frequencies_are_whole_tens_of_hertz() {
        let r = req(100_000_004, 100_100_004, 3);
        assert_eq!(r.step_hz(), 33_330);
        assert_eq!(r.freqs_hz(), vec![100_000_000.0, 100_033_330.0, 100_066_660.0, 100_099_990.0]);
    }

    #[test]
    fn readings_become_loss_against_the_reference() {
        let raw: Vec<crate::calc::RawPoint> = [600u16, 500, 650, 0].into_iter().map(|level| crate::calc::RawPoint { phase: 0, level }).collect();
        let points = to_points(&raw, &[600, 600, 600, 600], 0.1, 100e6, 1e6);
        let loss: Vec<f64> = points.iter().map(|p| p.rl_db).collect();
        assert!((loss[0] - 0.0).abs() < 1e-9 && (loss[1] - 10.0).abs() < 1e-9);
        assert!((loss[2] + 5.0).abs() < 1e-9, "gain is a negative loss: {}", loss[2]);
        assert!((loss[3] - 60.0).abs() < 1e-9);
        assert_eq!(points[1].freq_hz, 101e6);
        // What this device cannot measure is not a number, so the read-outs stay empty.
        assert!(points.iter().all(|p| p.swr.is_nan() && p.rs.is_nan() && p.xs.is_nan() && p.z_mag.is_nan() && p.phase_deg.is_nan()));
        assert!(points.iter().all(|p| p.re == 0.0 && p.im == 0.0));
    }

    #[test]
    fn steps_of_10_mhz_or_more_are_swept_finer_and_thinned() {
        // The unit ignores the part of a step above 9.99999 MHz (seen on hardware:
        // 43.65 MHz is swept as 3.65 MHz). 35 to 4400 MHz in 100 steps is 43.65 MHz per
        // step: sweep 5 times finer and keep every 5th point.
        let r = req(35_000_000, 4_400_000_000, 100);
        assert_eq!(r.step_hz(), 43_650_000);
        assert_eq!(thinning(&r), 5);
        assert_eq!(sweep_command(&r), b"\x8fa00350000000873000050100".to_vec());
        assert_eq!(reply_len(&r), 501 * 4);
        let reply: Vec<u8> = (0..501u16).flat_map(|i| [i.to_le_bytes(), i.to_le_bytes()].concat()).collect();
        let levels: Vec<u16> = parse_sweep(&r, &reply).iter().map(|p| p.level).collect();
        assert_eq!(levels.len(), 101);
        assert_eq!(&levels[..3], &[0, 5, 10]);
        assert_eq!(levels[100], 500);

        // Just under the limit nothing changes; just over, the step stays a whole
        // number of device steps.
        let fine = req(100_000_000, 1_099_999_000, 100);
        assert_eq!((fine.step_hz(), thinning(&fine)), (9_999_990, 1));
        let over = req(100_000_000, 1_100_004_000, 100);
        assert_eq!(thinning(&over), 2);
        assert_eq!(over.step_hz() % 20, 0);
        assert_eq!(over.step_hz(), 10_000_040);
        // The coarsest sweep the window allows still fits the unit's 4-digit point count.
        let coarse = req(35_000_000, 4_400_000_000, 10);
        assert!(coarse.samples as u64 * thinning(&coarse) < 9_999);
        assert_eq!((thinning(&coarse), coarse.step_hz()), (44, 436_499_800));
    }
}
