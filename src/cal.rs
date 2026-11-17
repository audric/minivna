//! Level calibration: two tables of 1000 offsets, one for reflection and one for transmission.
//! Phase is never calibrated. Reference: docs/reverse-engineering/ig-minivna-findings.md, section 3.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::calc::RawPoint;

pub const CAL_POINTS: usize = 1000;
/// The level reading that the original treats as 0 dB.
pub const CAL_ZERO: i16 = 344;
pub const CAL_START_HZ: f64 = 100_000.0;
/// round((180 MHz - 100 kHz) / 1000)
pub const CAL_STEP_HZ: f64 = 179_900.0;

#[derive(thiserror::Error, Debug)]
pub enum CalError {
    #[error("calibration file: {0}")]
    Io(#[from] std::io::Error),
    #[error("calibration data is not valid: {0}")]
    Format(String),
}

/// `table[k]` belongs to the frequency `CAL_START_HZ + (k + 1) * CAL_STEP_HZ`.
#[derive(Clone, Default, Serialize, Deserialize, PartialEq, Debug)]
pub struct Calibration {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reflection: Option<Vec<i16>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transmission: Option<Vec<i16>>,
}

/// Build a table from the 1001 points (0..=1000) of a calibration sweep. Point 0 is not used.
pub fn table_from_sweep(raw: &[RawPoint]) -> Result<Vec<i16>, CalError> {
    if raw.len() != CAL_POINTS + 1 {
        return Err(CalError::Format(format!(
            "a calibration sweep has {} points, got {}",
            CAL_POINTS + 1,
            raw.len()
        )));
    }
    Ok(raw[1..]
        .iter()
        .map(|p| (CAL_ZERO as i32 - p.level as i32).clamp(i16::MIN as i32, i16::MAX as i32) as i16)
        .collect())
}

impl Calibration {
    /// One offset per frequency, taken from the nearest calibration point. `device_freqs_hz`
    /// are the frequencies the device produced (displayed frequency divided by the extender).
    /// All zeros when the table for that mode is absent.
    pub fn offsets(&self, transmission: bool, device_freqs_hz: &[f64]) -> Vec<i16> {
        let table = if transmission { &self.transmission } else { &self.reflection };
        let Some(table) = table.as_ref().filter(|t| t.len() == CAL_POINTS) else {
            return vec![0; device_freqs_hz.len()];
        };
        device_freqs_hz
            .iter()
            .map(|f| {
                let nearest = ((f - CAL_START_HZ) / CAL_STEP_HZ).round() - 1.0;
                table[nearest.clamp(0.0, (CAL_POINTS - 1) as f64) as usize]
            })
            .collect()
    }

    /// A missing file is not an error: it means "not calibrated yet".
    pub fn load(path: &Path) -> Result<Calibration, CalError> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Calibration::default()),
            Err(e) => return Err(e.into()),
        };
        let cal: Calibration = toml::from_str(&text).map_err(|e| CalError::Format(e.to_string()))?;
        for (name, table) in [("reflection", &cal.reflection), ("transmission", &cal.transmission)] {
            if let Some(t) = table {
                if t.len() != CAL_POINTS {
                    return Err(CalError::Format(format!(
                        "the {name} table has {} entries, expected {CAL_POINTS}",
                        t.len()
                    )));
                }
            }
        }
        Ok(cal)
    }

    pub fn save(&self, path: &Path) -> Result<(), CalError> {
        let text = toml::to_string(self).map_err(|e| CalError::Format(e.to_string()))?;
        std::fs::write(path, text)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_is_344_minus_level() {
        let mut raw = vec![RawPoint { phase: 0, level: 999 }];
        raw.extend((0..1000).map(|k| RawPoint { phase: 0, level: 364 + (k % 3) as u16 }));
        let t = table_from_sweep(&raw).unwrap();
        assert_eq!(t.len(), 1000);
        assert_eq!(&t[..3], &[-20, -21, -22]);
    }

    #[test]
    fn wrong_length_is_refused() {
        assert!(table_from_sweep(&[RawPoint { phase: 0, level: 0 }; 500]).is_err());
    }

    #[test]
    fn lookup_is_nearest_calibration_frequency() {
        let cal = Calibration { reflection: Some((0..1000).collect()), transmission: None };
        assert_eq!(
            cal.offsets(false, &[279_900.0, 459_700.0, 300_000.0, 0.0, 500e6]),
            vec![0, 1, 0, 0, 999]
        );
        assert_eq!(cal.offsets(true, &[1e6, 2e6]), vec![0, 0]);
    }

    #[test]
    fn save_then_load() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("calibration.toml");
        let cal = Calibration {
            reflection: Some((0..1000).map(|k| (k % 7) as i16 - 3).collect()),
            transmission: Some(vec![-600; 1000]),
        };
        cal.save(&path).unwrap();
        assert_eq!(Calibration::load(&path).unwrap(), cal);
    }

    #[test]
    fn missing_file_is_default_and_garbage_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(Calibration::load(&dir.path().join("absent.toml")).unwrap(), Calibration::default());

        let garbage = dir.path().join("garbage.toml");
        std::fs::write(&garbage, "not toml {{{").unwrap();
        assert!(matches!(Calibration::load(&garbage), Err(CalError::Format(_))));

        let short = dir.path().join("short.toml");
        std::fs::write(&short, "reflection = [1, 2, 3, 4, 5, 6, 7]\n").unwrap();
        assert!(matches!(Calibration::load(&short), Err(CalError::Format(_))));
    }
}
