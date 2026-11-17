//! Files: settings, calibration location, saved sweeps and exports.

use std::path::PathBuf;

pub mod settings;

pub use settings::{Band, Colors, Rgb, Settings, Velocity};

/// The per-user configuration directory, created if it does not exist yet.
pub fn config_dir() -> PathBuf {
    let dir = directories::ProjectDirs::from("", "", "minivna")
        .map(|d| d.config_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));
    let _ = std::fs::create_dir_all(&dir);
    dir
}

pub fn settings_path() -> PathBuf {
    config_dir().join("settings.toml")
}

pub fn calibration_path() -> PathBuf {
    config_dir().join("calibration.toml")
}
