//! Saved sweeps (`.mvna`): the raw readings and the calibration used, as text.
//! Everything else is recomputed when the file is opened.

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::FileError;
use crate::calc::RawPoint;
use crate::device::nwt::DB_PER_COUNT_DEFAULT;
use crate::device::{DeviceKind, Mode, SweepRequest};

/// What the window looked like when the sweep was saved.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ViewState {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub m1: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub m2: Option<usize>,
    /// SWR, |Z|, RL, Phase, Rs, |Xs|
    pub show: [bool; 6],
    pub scale_60db: bool,
    pub rl_offset_db: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SweepFile {
    pub request: SweepRequest,
    /// Points 0..=samples.
    pub raw: Vec<RawPoint>,
    /// The calibration offset used for each point.
    pub cal: Vec<i16>,
    pub view: ViewState,
    /// dB per count of the NWT sweeper's detector; not used for the miniVNA.
    pub db_per_count: f64,
}

const FORMAT: u32 = 1;

fn is_minivna(kind: &DeviceKind) -> bool {
    *kind == DeviceKind::MiniVna
}

#[derive(Serialize, Deserialize)]
struct Header {
    format: u32,
    /// Missing in files written before there was a second device: the miniVNA.
    #[serde(default, skip_serializing_if = "is_minivna")]
    device: DeviceKind,
    /// NWT sweeper only: dB per count used when the sweep was taken.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    db_per_count: Option<f64>,
    mode: Mode,
    start_hz: u64,
    stop_hz: u64,
    samples: u16,
    extender: u8,
    dds_factor: f64,
    step_bug: bool,
}

#[derive(Serialize)]
struct Tables<'a> {
    sweep: Header,
    view: &'a ViewState,
}

#[derive(Deserialize)]
struct Document {
    /// One `[phase, level, calibration]` triple per point.
    points: Vec<Vec<i64>>,
    sweep: Header,
    view: ViewState,
}

pub fn save(path: &Path, file: &SweepFile) -> Result<(), FileError> {
    let r = &file.request;
    let tables = Tables {
        sweep: Header {
            format: FORMAT,
            device: r.kind,
            db_per_count: (r.kind == DeviceKind::Nwt).then_some(file.db_per_count),
            mode: r.mode,
            start_hz: r.start_hz,
            stop_hz: r.stop_hz,
            samples: r.samples,
            extender: r.extender,
            dds_factor: r.dds_factor,
            step_bug: r.step_bug,
        },
        view: &file.view,
    };
    let mut text = String::from("# minivna sweep. Each point: [phase reading, level reading, calibration offset]\npoints = [\n");
    for (raw, cal) in file.raw.iter().zip(&file.cal) {
        text.push_str(&format!("  [{}, {}, {}],\n", raw.phase, raw.level, cal));
    }
    text.push_str("]\n\n");
    text.push_str(&toml::to_string(&tables).map_err(|e| FileError::Format(e.to_string()))?);
    std::fs::write(path, text)?;
    Ok(())
}

pub fn load(path: &Path) -> Result<SweepFile, FileError> {
    let text = std::fs::read_to_string(path)?;
    let bad = |why: String| FileError::Format(why);
    let doc: Document = toml::from_str(&text).map_err(|e| bad(e.message().to_string()))?;
    if doc.sweep.format != FORMAT {
        return Err(bad(format!("format {} is not supported", doc.sweep.format)));
    }
    let request = SweepRequest {
        kind: doc.sweep.device,
        mode: doc.sweep.mode,
        start_hz: doc.sweep.start_hz,
        stop_hz: doc.sweep.stop_hz,
        samples: doc.sweep.samples,
        extender: doc.sweep.extender,
        dds_factor: doc.sweep.dds_factor,
        step_bug: doc.sweep.step_bug,
    };
    request.validate().map_err(|e| bad(e.to_string()))?;
    if doc.points.len() != request.samples as usize + 1 {
        return Err(bad(format!(
            "{} points for {} samples",
            doc.points.len(),
            request.samples
        )));
    }
    let mut raw = Vec::with_capacity(doc.points.len());
    let mut cal = Vec::with_capacity(doc.points.len());
    for (i, triple) in doc.points.iter().enumerate() {
        let [phase, level, offset] = triple[..] else {
            return Err(bad(format!("point {i} does not have three values")));
        };
        let reading = |v: i64| u16::try_from(v).map_err(|_| bad(format!("point {i} has a reading out of range")));
        raw.push(RawPoint { phase: reading(phase)?, level: reading(level)? });
        cal.push(i16::try_from(offset).map_err(|_| bad(format!("point {i} has a calibration out of range")))?);
    }
    let mut view = doc.view;
    let last = raw.len() - 1;
    view.m1 = view.m1.filter(|m| *m <= last);
    view.m2 = view.m2.filter(|m| *m <= last);
    let db_per_count = doc.sweep.db_per_count.filter(|s| s.is_finite() && *s > 0.0).unwrap_or(DB_PER_COUNT_DEFAULT);
    Ok(SweepFile { request, raw, cal, view, db_per_count })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::DDS_FACTOR_DEFAULT;

    fn sample() -> SweepFile {
        SweepFile {
            request: SweepRequest { kind: crate::device::DeviceKind::MiniVna,
                mode: Mode::Reflection,
                start_hz: 7_000_000,
                stop_hz: 7_200_000,
                samples: 10,
                extender: 1,
                dds_factor: DDS_FACTOR_DEFAULT,
                step_bug: true,
            },
            raw: (0..11).map(|i| RawPoint { phase: 100 + i, level: 400 + 2 * i }).collect(),
            cal: (0..11).map(|i| -20 + i).collect(),
            view: ViewState {
                m1: Some(3),
                m2: None,
                show: [true, false, true, false, false, true],
                scale_60db: true,
                rl_offset_db: -2.5,
            },
            db_per_count: DB_PER_COUNT_DEFAULT,
        }
    }

    fn load_text(text: &str) -> Result<SweepFile, FileError> {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.mvna");
        std::fs::write(&path, text).unwrap();
        load(&path)
    }

    #[test]
    fn sweep_file_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("antenna.mvna");
        save(&path, &sample()).unwrap();
        assert_eq!(load(&path).unwrap(), sample());
    }

    #[test]
    fn sweep_file_rejects_bad_input() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("good.mvna");
        save(&path, &sample()).unwrap();
        let good = std::fs::read_to_string(&path).unwrap();
        assert!(load_text(&good).is_ok());

        assert!(matches!(load_text("this is not a sweep file"), Err(FileError::Format(_))));
        assert!(matches!(load_text(""), Err(FileError::Format(_))));
        assert!(matches!(load_text(&good.replace("format = 1", "format = 2")), Err(FileError::Format(_))));
        assert!(matches!(load_text(&good.replace("samples = 10", "samples = 12")), Err(FileError::Format(_))));
        assert!(matches!(load_text(&good.replace("samples = 10", "samples = 5")), Err(FileError::Format(_))));
        assert!(matches!(load_text(&good.replace("[100, 400, -20]", "[100, 400]")), Err(FileError::Format(_))));
        assert!(matches!(load_text(&good.replace("[100, 400, -20]", "[100, 99999, -20]")), Err(FileError::Format(_))));
        assert!(matches!(load(&dir.path().join("absent.mvna")), Err(FileError::Io(_))));
    }

    #[test]
    fn markers_beyond_the_data_are_dropped() {
        let text = {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("m.mvna");
            save(&path, &sample()).unwrap();
            std::fs::read_to_string(&path).unwrap().replace("m1 = 3", "m1 = 300")
        };
        assert_eq!(load_text(&text).unwrap().view.m1, None);
    }

    #[test]
    fn the_device_is_recorded_and_old_files_mean_the_minivna() {
        use crate::device::DeviceKind;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("filter.mvna");
        let request = SweepRequest {
            kind: DeviceKind::Nwt,
            mode: Mode::Transmission,
            start_hz: 400_000_000,
            stop_hz: 500_000_000,
            samples: 10,
            extender: 1,
            dds_factor: 10.73741824,
            step_bug: false,
        };
        let file = SweepFile { request, raw: vec![RawPoint { phase: 0, level: 300 }; 11], cal: vec![600; 11], view: ViewState { m1: None, m2: None, show: [false, false, true, false, false, false], scale_60db: true, rl_offset_db: 0.0 }, db_per_count: 0.125 };
        save(&path, &file).unwrap();
        let back = load(&path).unwrap();
        assert_eq!(back.request.kind, DeviceKind::Nwt);
        assert_eq!(back.request.start_hz, 400_000_000);
        assert_eq!(back.db_per_count, 0.125);

        // A file written before there was a second device has no "device" entry.
        let text = std::fs::read_to_string(&path).unwrap().replace("device = \"nwt\"\n", "");
        assert!(!text.contains("device"));
        std::fs::write(&path, text).unwrap();
        // ... so it is read as a miniVNA sweep, for which 400 MHz is out of range.
        assert!(load(&path).is_err());
    }
}
