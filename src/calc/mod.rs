//! Pure measurement maths. Nothing here knows about the device or the interface.
pub mod convert;
pub mod sign;
pub mod tools;
pub use convert::{convert, Point, RawPoint};
pub use sign::{apply_sign, estimate_sign};

#[cfg(test)]
pub(crate) mod test_support {
    use super::Point;

    pub fn close(a: f64, b: f64) {
        assert!((a - b).abs() <= 1e-4 * b.abs().max(1.0), "{a} is not close to {b}");
    }

    /// Points 1 MHz apart from 1 MHz, with the given phases and SWR 2.
    pub fn pts(phases: &[f64]) -> Vec<Point> {
        pts_with(phases, |p, v| p.phase_deg = v)
    }

    pub fn pts_with(values: &[f64], set: impl Fn(&mut Point, f64)) -> Vec<Point> {
        values
            .iter()
            .enumerate()
            .map(|(i, v)| {
                let mut p = Point {
                    freq_hz: 1e6 * (i + 1) as f64,
                    phase_deg: 0.0,
                    rl_db: 10.0,
                    re: 0.0,
                    im: 0.0,
                    swr: 2.0,
                    z_mag: 50.0,
                    rs: 50.0,
                    xs: 0.0,
                    l_uh: 0.0,
                    c_pf: 0.0,
                };
                set(&mut p, *v);
                p
            })
            .collect()
    }
}
