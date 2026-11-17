//! What the File menu does, apart from asking for a file name.

use std::path::{Path, PathBuf};

use super::graph::render_png;
use super::state::AppState;
use super::text::t;
use crate::store::{csv, png, sweep_file};

/// Add the usual extension when the user typed a bare name.
pub fn with_extension(path: &Path, extension: &str) -> PathBuf {
    if path.extension().is_some() {
        path.to_path_buf()
    } else {
        path.with_extension(extension)
    }
}

fn nothing() -> String {
    t("Nothing to save: make a sweep first.").to_string()
}

/// Write the last sweep and its view. Returns the name actually written.
pub fn save_sweep(state: &AppState, path: &Path) -> Result<PathBuf, String> {
    let file = state.to_sweep_file().ok_or_else(nothing)?;
    let path = with_extension(path, "mvna");
    sweep_file::save(&path, &file).map_err(|e| e.to_string())?;
    Ok(path)
}

/// Show a saved sweep. On any failure the state is left as it was.
pub fn open_sweep(state: &mut AppState, path: &Path) -> Result<(), String> {
    let file = sweep_file::load(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if state.load_sweep_file(file) {
        Ok(())
    } else {
        Err(state.status.clone())
    }
}

pub fn save_csv(state: &AppState, path: &Path) -> Result<PathBuf, String> {
    let result = state.result.as_ref().ok_or_else(nothing)?;
    let path = with_extension(path, "csv");
    std::fs::write(&path, csv::to_csv(result)).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(path)
}

/// Write the graph as a picture of the given size, in the PNG colours.
pub fn save_png(state: &AppState, path: &Path, width: u32, height: u32) -> Result<PathBuf, String> {
    if width == 0 || height == 0 {
        return Err(t("The graph has no size.").to_string());
    }
    let path = with_extension(path, "png");
    png::write_png(&path, width, height, &render_png(state, width, height)).map_err(|e| e.to_string())?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::state::tests::{state, swept};

    #[test]
    fn save_then_open_gives_the_same_sweep() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = swept();
        (s.m1, s.m2) = (Some(5), Some(77));
        let path = save_sweep(&s, &dir.path().join("antenna")).unwrap();
        assert_eq!(path.extension().unwrap(), "mvna");
        let mut fresh = state();
        open_sweep(&mut fresh, &path).unwrap();
        assert_eq!((fresh.m1, fresh.m2, fresh.samples), (Some(5), Some(77), 290));
        assert_eq!(fresh.result.as_ref().unwrap().raw, s.result.as_ref().unwrap().raw);
    }

    #[test]
    fn a_bad_file_leaves_everything_as_it_was() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("broken.mvna");
        std::fs::write(&path, "this is not a sweep").unwrap();
        let mut s = swept();
        s.m1 = Some(9);
        assert!(open_sweep(&mut s, &path).is_err());
        assert!(open_sweep(&mut s, &dir.path().join("missing.mvna")).is_err());
        assert_eq!((s.m1, s.start_hz, s.stop_hz), (Some(9), 1_000_000, 30_000_000));
        assert!(s.result.is_some());
    }

    #[test]
    fn nothing_to_save_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let s = state();
        assert!(save_sweep(&s, &dir.path().join("a.mvna")).is_err());
        assert!(save_csv(&s, &dir.path().join("a.csv")).is_err());
        assert!(!dir.path().join("a.mvna").exists() && !dir.path().join("a.csv").exists());
    }

    #[test]
    fn csv_and_png_are_written() {
        let dir = tempfile::tempdir().unwrap();
        let s = swept();
        let csv = save_csv(&s, &dir.path().join("out")).unwrap();
        assert_eq!(csv.extension().unwrap(), "csv");
        assert_eq!(std::fs::read_to_string(&csv).unwrap().lines().count(), 292); // header + 291 points
        let png = save_png(&s, &dir.path().join("out.png"), 320, 200).unwrap();
        let picture = image::open(&png).unwrap();
        assert_eq!((picture.width(), picture.height()), (320, 200));
        assert!(save_png(&s, &dir.path().join("tiny.png"), 0, 0).is_err());
    }

    #[test]
    fn an_existing_extension_is_kept() {
        assert_eq!(with_extension(Path::new("/x/a.txt"), "csv"), Path::new("/x/a.txt"));
        assert_eq!(with_extension(Path::new("/x/a"), "csv"), Path::new("/x/a.csv"));
    }
}
