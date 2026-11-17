//! Raw device readings to physical values, as the original IG_miniVNA computes them.
//! Reference: docs/reverse-engineering/ig-minivna-findings.md, sections 1 and 2.

/// One point as the device sends it: two 10-bit readings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RawPoint {
    pub phase: u16,
    pub level: u16,
}

/// One measured point with everything derived from it.
#[derive(Clone, Copy, Debug)]
pub struct Point {
    pub freq_hz: f64,
    /// 0..180 degrees unless the sign estimate has been applied.
    pub phase_deg: f64,
    /// Return loss (or transmission loss) in dB, clamped to >= 0.
    pub rl_db: f64,
    /// Reflection value, real and imaginary part, from the unclamped loss.
    pub re: f64,
    pub im: f64,
    pub swr: f64,
    pub z_mag: f64,
    pub rs: f64,
    pub xs: f64,
    pub l_uh: f64,
    pub c_pf: f64,
}

const PHASE_PER_COUNT: f64 = 180.0 / 1023.0;
const DB_PER_COUNT: f64 = 60.0 / 1023.0;
const DB_OFFSET: f64 = 20.16;
const Z0: f64 = 50.0;

/// Convert a sweep. `raw`, `cal` and the result have the same length; point `i` is at
/// `start_hz + i * step_hz`. `cal` holds the calibration offsets, in counts, that are added
/// to the raw level before scaling.
pub fn convert(raw: &[RawPoint], cal: &[i16], start_hz: f64, step_hz: f64) -> Vec<Point> {
    raw.iter()
        .enumerate()
        .map(|(i, r)| {
            let offset = cal.get(i).copied().unwrap_or(0);
            convert_one(*r, offset, start_hz + i as f64 * step_hz)
        })
        .collect()
}

fn convert_one(raw: RawPoint, cal: i16, freq_hz: f64) -> Point {
    let phase_deg = raw.phase as f64 * PHASE_PER_COUNT;
    let rl = (raw.level as f64 + cal as f64) * DB_PER_COUNT - DB_OFFSET;
    let rho = 10f64.powf(-rl / 20.0);
    let swr = ((1.0 + rho) / (1.0 - rho)).abs();
    let (sin, cos) = phase_deg.to_radians().sin_cos();
    let (re, im) = (rho * cos, rho * sin);
    let d = (1.0 - re).powi(2) + im * im;
    // d is zero only for a perfect open circuit (re = 1, im = 0).
    let (rs, xs) = if d == 0.0 {
        (f64::INFINITY, 0.0)
    } else {
        ((1.0 - re * re - im * im).abs() / d * Z0, (2.0 * im).abs() / d * Z0)
    };
    let z_mag = rs.hypot(xs);
    let w = std::f64::consts::TAU * freq_hz;
    let (l_uh, c_pf) = if xs == 0.0 || w == 0.0 {
        (0.0, f64::INFINITY)
    } else {
        (1e6 * xs / w, 1e12 / (w * xs))
    };
    Point { freq_hz, phase_deg, rl_db: rl.max(0.0), re, im, swr, z_mag, rs, xs, l_uh, c_pf }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) {
        assert!((a - b).abs() <= 1e-4 * b.abs().max(1.0), "{a} is not close to {b}");
    }

    #[test]
    fn resistive_point() {
        let p = convert(&[RawPoint { phase: 0, level: 514 }], &[0], 7e6, 0.0)[0];
        close(p.phase_deg, 0.0);
        close(p.rl_db, 9.986628);
        close(p.swr, 1.927036);
        close(p.rs, 96.351813);
        close(p.xs, 0.0);
        close(p.z_mag, 96.351813);
        close(p.l_uh, 0.0);
    }

    #[test]
    fn reactive_point() {
        let p = convert(&[RawPoint { phase: 512, level: 514 }], &[0], 7e6, 0.0)[0];
        close(p.phase_deg, 90.087977);
        close(p.re, -0.000486);
        close(p.im, 0.316715);
        close(p.rs, 40.847504);
        close(p.xs, 28.758747);
        close(p.z_mag, 49.955822);
        close(p.l_uh, 0.653871);
        close(p.c_pf, 790.591481);
    }

    #[test]
    fn calibration_is_added_to_the_raw_level() {
        let p = convert(&[RawPoint { phase: 256, level: 600 }], &[-20], 14.2e6, 0.0)[0];
        close(p.rl_db, 13.857595);
        close(p.swr, 1.508858);
        close(p.rs, 63.541124);
        close(p.xs, 19.022477);
    }

    #[test]
    fn negative_loss_is_clamped_after_the_maths() {
        let p = convert(&[RawPoint { phase: 100, level: 300 }], &[0], 1e6, 0.0)[0];
        assert_eq!(p.rl_db, 0.0);
        close(p.swr, 6.822577);
        close(p.rs, 165.158657);
        close(p.xs, 166.653487);
    }

    #[test]
    fn frequencies_follow_start_and_step() {
        let raw = [RawPoint { phase: 0, level: 514 }; 3];
        let p = convert(&raw, &[0; 3], 1e6, 358000.0);
        assert_eq!(p[2].freq_hz, 1_716_000.0);
    }

    #[test]
    fn extreme_readings_do_not_produce_nan() {
        for (ph, lv) in [(1023u16, 344u16), (0, 0), (1023, 1023), (0, 344), (65535, 65535)] {
            let p = convert(&[RawPoint { phase: ph, level: lv }], &[0], 1e6, 0.0)[0];
            for v in [p.phase_deg, p.rl_db, p.re, p.im, p.swr, p.z_mag, p.rs, p.xs, p.l_uh, p.c_pf] {
                assert!(!v.is_nan(), "NaN for readings {ph}, {lv}");
            }
        }
    }
}
