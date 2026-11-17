//! Marker read-outs and the small calculations of the cable and LC panels.
//! Reference: docs/reverse-engineering/ig-minivna-findings.md, sections 2, 5 and 6.

use super::Point;

/// Index of the lowest SWR below 200, ignoring point 0.
pub fn swr_min_index(points: &[Point]) -> Option<usize> {
    let mut best = 200.0;
    let mut found = None;
    for (i, p) in points.iter().enumerate().skip(1) {
        if p.swr < best {
            best = p.swr;
            found = Some(i);
        }
    }
    found
}

/// Index of the highest loss, ignoring point 0.
pub fn max_rl_index(points: &[Point]) -> Option<usize> {
    extreme_rl(points, |candidate, best| candidate > best)
}

/// Index of the lowest loss, ignoring point 0.
pub fn min_rl_index(points: &[Point]) -> Option<usize> {
    extreme_rl(points, |candidate, best| candidate < best)
}

fn extreme_rl(points: &[Point], better: impl Fn(f64, f64) -> bool) -> Option<usize> {
    let mut found: Option<usize> = None;
    for (i, p) in points.iter().enumerate().skip(1) {
        if found.is_none_or(|best| better(p.rl_db, points[best].rl_db)) {
            found = Some(i);
        }
    }
    found
}

/// Bandwidth in Hz between two marker frequencies, and Q = centre / bandwidth.
pub fn bandwidth_q(f1_hz: f64, f2_hz: f64) -> Option<(f64, f64)> {
    let bw = (f1_hz - f2_hz).abs();
    (bw > 0.0).then(|| (bw, (f1_hz + f2_hz) / 2.0 / bw))
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CableLoss {
    pub min: f64,
    pub max: f64,
    pub avg: f64,
}

/// One-way cable loss from the return loss at the two markers (far end open).
pub fn cable_loss(rl1_db: f64, rl2_db: f64) -> CableLoss {
    let (a, b) = (rl1_db / 2.0, rl2_db / 2.0);
    CableLoss { min: a.min(b), max: a.max(b), avg: (a + b) / 2.0 }
}

const LIGHT_M_PER_S: f64 = 3e8;
/// Fixed correction the original subtracts, in metres.
const LENGTH_CORRECTION_M: f64 = 0.045;

fn half_wave_m(f1_hz: f64, f2_hz: f64) -> Option<f64> {
    let gap = (f1_hz - f2_hz).abs();
    (gap > 0.0).then(|| LIGHT_M_PER_S / gap / 2.0)
}

/// Cable length from the frequency gap of one full phase period.
pub fn cable_length_m(f1_hz: f64, f2_hz: f64, velocity: f64) -> Option<f64> {
    half_wave_m(f1_hz, f2_hz).map(|h| h * velocity - LENGTH_CORRECTION_M)
}

/// Velocity factor of a cable of known length, from the same two frequencies.
pub fn velocity_from_length(f1_hz: f64, f2_hz: f64, length_m: f64) -> Option<f64> {
    half_wave_m(f1_hz, f2_hz).map(|h| (length_m + LENGTH_CORRECTION_M) / h)
}

fn near_90(phase_deg: f64, low: f64, high: f64) -> bool {
    let a = phase_deg.abs();
    a > low && a < high
}

/// Indices of the first and third place where phase passes through 90 degrees.
/// Together they span one full phase period of a cable.
pub fn quarter_wave_crossings(points: &[Point]) -> Option<(usize, usize)> {
    let mut hits = Vec::with_capacity(3);
    let mut i = 0;
    while i < points.len() && hits.len() < 3 {
        if near_90(points[i].phase_deg, 85.0, 95.0) {
            hits.push(i);
            // Leave the band around 90 degrees before looking for the next crossing.
            while i < points.len() && near_90(points[i].phase_deg, 84.0, 96.0) {
                i += 1;
            }
        } else {
            i += 1;
        }
    }
    (hits.len() == 3).then(|| (hits[0], hits[2]))
}

/// Move `idx` forward by up to 10 points to the one whose phase is nearest 90 degrees.
pub fn refine_to_90(points: &[Point], idx: usize) -> usize {
    let distance = |i: usize| (points[i].phase_deg.abs() - 90.0).abs();
    let mut best = idx;
    for j in idx + 1..points.len().min(idx + 11) {
        if distance(j) < distance(best) {
            best = j;
        }
    }
    best
}

/// Resonance partner at `f_mhz`: inductance in µH gives capacitance in pF, and the reverse.
pub fn lc_resonance(f_mhz: f64, value: f64) -> f64 {
    1.0 / (3.948e-05 * f_mhz * f_mhz * value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calc::test_support::{close, pts, pts_with};

    #[test]
    fn swr_min_ignores_point_zero_and_values_of_200_or_more() {
        let p = pts_with(&[1.0, 250.0, 3.0, 1.5, 9.0], |p, v| p.swr = v);
        assert_eq!(swr_min_index(&p), Some(3));
        let p = pts_with(&[1.0, 250.0, 300.0], |p, v| p.swr = v);
        assert_eq!(swr_min_index(&p), None);
    }

    #[test]
    fn rl_extremes_ignore_point_zero() {
        let p = pts_with(&[99.0, 3.0, 20.0, 1.0, 5.0], |p, v| p.rl_db = v);
        assert_eq!(max_rl_index(&p), Some(2));
        assert_eq!(min_rl_index(&p), Some(3));
        assert_eq!(max_rl_index(&p[..1]), None);
    }

    #[test]
    fn bandwidth_and_q() {
        let (bw, q) = bandwidth_q(7.0e6, 7.2e6).unwrap();
        close(bw, 200_000.0);
        close(q, 35.5);
        assert!(bandwidth_q(7e6, 7e6).is_none());
    }

    #[test]
    fn cable_loss_is_half_the_return_loss() {
        let c = cable_loss(6.0, 4.0);
        close(c.min, 2.0);
        close(c.max, 3.0);
        close(c.avg, 2.5);
    }

    #[test]
    fn cable_length() {
        close(cable_length_m(10e6, 20e6, 0.66).unwrap(), 9.855);
        close(velocity_from_length(10e6, 20e6, 10.0).unwrap(), 0.669667);
        assert!(cable_length_m(1e6, 1e6, 0.66).is_none());
        assert!(velocity_from_length(1e6, 1e6, 10.0).is_none());
    }

    #[test]
    fn lc() {
        close(lc_resonance(7.1, 15.0), 33.497693);
    }

    #[test]
    fn crossings_found() {
        let p = pts(&[20., 60., 88., 92., 130., 170., 120., 90., 60., 20., 50., 89., 91., 140.]);
        assert_eq!(quarter_wave_crossings(&p), Some((2, 11)));
        assert_eq!(quarter_wave_crossings(&pts(&[20., 60., 88., 130.])), None);
        assert_eq!(quarter_wave_crossings(&[]), None);
    }

    #[test]
    fn refine_picks_nearest_90_within_ten() {
        assert_eq!(refine_to_90(&pts(&[86., 88., 90.5, 93., 99.]), 0), 2);
        assert_eq!(refine_to_90(&pts(&[90., 88.]), 0), 0);
        assert_eq!(refine_to_90(&pts(&[86., 88.]), 1), 1);
    }
}
