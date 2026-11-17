//! Estimate of the sign of phase and reactance, which the detector does not report.
//! Reference: docs/reverse-engineering/ig-minivna-findings.md, section 1.

use super::Point;

/// For each point, `true` when phase and reactance are taken as positive.
/// Phase falling with frequency means positive, rising means negative.
pub fn estimate_sign(points: &[Point]) -> Vec<bool> {
    let n = points.len();
    let mut s = vec![true; n];
    if n < 3 {
        return s;
    }
    for i in 2..n {
        let (now, before) = (points[i].phase_deg.abs(), points[i - 1].phase_deg.abs());
        s[i] = if now < before {
            true
        } else if now > before {
            false
        } else {
            s[i - 1]
        };
    }
    s[1] = s[2];
    // A single point that disagrees with both neighbours is a glitch.
    for i in 1..n - 1 {
        if s[i - 1] == s[i + 1] && s[i] != s[i - 1] {
            s[i] = s[i - 1];
        }
    }
    s[0] = s[1];
    s[n - 1] = s[n - 2];
    s
}

/// Give phase, reactance and the imaginary part their estimated sign.
pub fn apply_sign(points: &mut [Point]) {
    let signs = estimate_sign(points);
    for (p, positive) in points.iter_mut().zip(signs) {
        if !positive {
            p.phase_deg = -p.phase_deg;
            p.xs = -p.xs;
            p.im = -p.im;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calc::test_support::pts;

    #[test]
    fn sign_follows_slope_and_smooths_single_glitches() {
        let s = estimate_sign(&pts(&[10., 20., 30., 29., 40., 50., 40., 30., 20.]));
        assert_eq!(s, vec![false, false, false, false, false, false, true, true, true]);
    }

    #[test]
    fn sign_handles_short_inputs() {
        assert_eq!(estimate_sign(&pts(&[])).len(), 0);
        assert_eq!(estimate_sign(&pts(&[10.])).len(), 1);
        assert_eq!(estimate_sign(&pts(&[10., 20.])).len(), 2);
    }

    #[test]
    fn apply_sign_negates_phase_reactance_and_imaginary_part() {
        let mut p = pts(&[10., 20., 30., 20., 10.]);
        for q in p.iter_mut() {
            q.xs = 5.0;
            q.im = 0.1;
        }
        apply_sign(&mut p);
        assert_eq!(p[1].phase_deg, -20.0);
        assert_eq!(p[1].xs, -5.0);
        assert_eq!(p[1].im, -0.1);
        assert_eq!(p[4].phase_deg, 10.0);
        assert_eq!(p[4].xs, 5.0);
    }
}
