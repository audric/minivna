//! Everything the program remembers between runs.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::device::DDS_FACTOR_DEFAULT;

/// A colour as red, green, blue.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rgb(pub u8, pub u8, pub u8);

/// One set of colours. There is one set for the screen and one for PNG export.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Colors {
    pub swr: Rgb,
    pub z: Rgb,
    pub rl: Rgb,
    pub phase: Rgb,
    pub rs: Rgb,
    pub xs: Rgb,
    pub background: Rgb,
    pub frame: Rgb,
    pub ohms: Rgb,
    pub m1: Rgb,
    pub m2: Rgb,
    pub swr_min: Rgb,
}

impl Default for Colors {
    /// The original program's defaults (IG_Colors.ini).
    fn default() -> Self {
        Colors {
            swr: Rgb(255, 0, 0),
            z: Rgb(0, 128, 0),
            rl: Rgb(0, 0, 255),
            phase: Rgb(255, 0, 255),
            rs: Rgb(128, 128, 128),
            xs: Rgb(0, 128, 128),
            background: Rgb(255, 255, 255),
            frame: Rgb(0, 0, 0),
            ohms: Rgb(128, 128, 128),
            m1: Rgb(255, 0, 0),
            m2: Rgb(0, 128, 0),
            swr_min: Rgb(0, 255, 0),
        }
    }
}

/// A band preset: a sweep range and default marker positions.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Band {
    pub name: String,
    pub start_mhz: f64,
    pub stop_mhz: f64,
    pub m1_mhz: f64,
    pub m2_mhz: f64,
    /// Above 180 MHz: only usable with a frequency extender.
    #[serde(default)]
    pub needs_extender: bool,
}

/// One line of the table of usual velocity factors.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Velocity {
    pub factor: f64,
    pub note: String,
}

/// The original program's band presets (IG_Bands.ini).
pub fn default_bands() -> Vec<Band> {
    [
        ("160m", 1.7, 2.0, 1.81, 1.85),
        ("80m", 3.4, 3.9, 3.5, 3.8),
        ("60m", 5.0, 5.5, 5.1, 5.2),
        ("40m", 6.9, 7.2, 7.0, 7.1),
        ("30m", 10.0, 10.3, 10.1, 10.15),
        ("20m", 13.9, 14.5, 14.0, 14.35),
        ("17m", 17.95, 18.3, 18.068, 18.168),
        ("15m", 19.8, 21.6, 21.0, 21.45),
        ("12m", 24.7, 25.1, 24.89, 24.99),
        ("10m", 27.9, 29.8, 28.0, 29.7),
        ("6m", 49.9, 52.0, 50.2, 51.2),
        ("4m", 70.0, 71.0, 70.2, 70.8),
        ("2m", 143.9, 146.1, 144.0, 146.0),
        ("70cm", 429.0, 441.0, 430.0, 440.0),
        ("23cm", 1230.0, 1310.0, 1240.0, 1300.0),
        ("13cm", 2400.0, 2500.0, 2410.0, 2490.0),
        ("Personal 1", 8.0, 35.0, 10.0, 30.0),
        ("Personal 2", 88.0, 108.0, 90.0, 98.0),
        ("Personal 3", 88.0, 108.0, 90.0, 98.0),
    ]
    .into_iter()
    .map(|(name, start_mhz, stop_mhz, m1_mhz, m2_mhz): (&str, f64, f64, f64, f64)| Band {
        name: name.to_string(),
        start_mhz,
        stop_mhz,
        m1_mhz,
        m2_mhz,
        needs_extender: stop_mhz > 180.0,
    })
    .collect()
}

/// The original program's table of usual velocity factors, read from its code.
pub fn default_velocities() -> Vec<Velocity> {
    [
        (0.66, "RG 58, 122, 174, 213, 214, 217, 218, 219, 8"),
        (0.8, "RG 213 Foam"),
        (0.81, "CATV Hardline"),
        (0.85, "Aircom +"),
        (0.86, "Ecoflex"),
        (0.92, "Open-wire line 600 ohms"),
        (0.83, "AirCell7"),
        (0.66, "H-1, H-3"),
        (0.84, "H-100"),
        (0.83, "H-2000flex, H-1000"),
        (0.55, "RG 55"),
        (0.69, "RG 141"),
        (0.7, "RG 142, RG 178"),
        (0.66, "KX4"),
        (0.81, "H-155"),
    ]
    .into_iter()
    .map(|(factor, note): (f64, &str)| Velocity { factor, note: note.to_string() })
    .collect()
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Serial port name; `None` means search for the device.
    pub port: Option<String>,
    /// Hz to DDS tuning word; adjusted by the DDS calibration.
    pub dds_factor: f64,
    pub step_bug: bool,
    pub extender_menu: bool,
    pub signed_phase: bool,
    pub line_50_ohms: bool,
    pub vertical_lines: bool,
    pub m1_at_start: bool,
    pub markers_from_band: bool,
    pub thick_lines: bool,
    pub velocity: f64,
    pub start_mhz: f64,
    pub stop_mhz: f64,
    pub samples: u16,
    pub screen: Colors,
    pub png: Colors,
    pub bands: Vec<Band>,
    pub velocities: Vec<Velocity>,
    /// Which instrument is on the port.
    #[serde(default)]
    pub device: crate::device::DeviceKind,
    /// The mode the program was last in (see `UiMode::key`).
    #[serde(default)]
    pub last_mode: String,
    /// Window position and size: x, y, width, height.
    pub window: Option<[f32; 4]>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            port: None,
            dds_factor: DDS_FACTOR_DEFAULT,
            step_bug: true,
            extender_menu: false,
            signed_phase: false,
            line_50_ohms: false,
            vertical_lines: true,
            m1_at_start: false,
            markers_from_band: false,
            thick_lines: false,
            velocity: 0.66,
            start_mhz: 1.0,
            stop_mhz: 180.0,
            samples: 500,
            screen: Colors::default(),
            png: Colors::default(),
            bands: default_bands(),
            velocities: default_velocities(),
            device: Default::default(),
            last_mode: String::new(),
            window: None,
        }
    }
}

impl Settings {
    /// Never fails. A missing file gives the defaults; an unreadable one gives the defaults
    /// and a message for the user.
    pub fn load(path: &Path) -> (Settings, Option<String>) {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (Settings::default(), None),
            Err(e) => return (Settings::default(), Some(format!("Settings could not be read ({e}); using defaults."))),
        };
        match toml::from_str::<Settings>(&text) {
            Ok(mut settings) => {
                settings.repair();
                (settings, None)
            }
            Err(e) => {
                // Keep the damaged file: on exit the defaults are written over the original.
                let aside = path.with_extension("toml.damaged");
                let kept = match std::fs::copy(path, &aside) {
                    Ok(_) => format!("the old file was kept as {}", aside.display()),
                    Err(copy) => format!("the old file could not be kept aside ({copy})"),
                };
                (Settings::default(), Some(format!("Settings file is damaged ({}); using defaults; {kept}.", e.message())))
            }
        }
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let text = toml::to_string(self).map_err(std::io::Error::other)?;
        std::fs::write(path, text)
    }

    /// Put anything out of range back to a usable value.
    pub fn repair(&mut self) {
        let defaults = Settings::default();
        self.samples = self.samples.clamp(10, 1000);
        let range_ok = self.start_mhz >= 0.1 && self.stop_mhz <= 180.0 && self.start_mhz < self.stop_mhz;
        if !range_ok {
            self.start_mhz = defaults.start_mhz;
            self.stop_mhz = defaults.stop_mhz;
        }
        if !(self.velocity > 0.0 && self.velocity <= 1.0) {
            self.velocity = defaults.velocity;
        }
        if !(10.0..=11.5).contains(&self.dds_factor) {
            self.dds_factor = defaults.dds_factor;
        }
        if self.bands.is_empty() {
            self.bands = defaults.bands;
        }
        if self.velocities.is_empty() {
            self.velocities = defaults.velocities;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(text: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.toml");
        std::fs::write(&path, text).unwrap();
        (dir, path)
    }

    #[test]
    fn defaults() {
        let s = Settings::default();
        assert_eq!(s.samples, 500);
        assert_eq!((s.start_mhz, s.stop_mhz), (1.0, 180.0));
        assert!(s.step_bug && !s.signed_phase && !s.extender_menu);
        assert_eq!(s.dds_factor, 10.73741824);
        assert_eq!(s.velocity, 0.66);
        assert_eq!(s.bands.len(), 19);
        assert_eq!(s.bands[0].name, "160m");
        assert_eq!(s.bands[0].start_mhz, 1.7);
        assert_eq!(s.bands[0].m2_mhz, 1.85);
        assert_eq!(s.bands[5].name, "20m");
        assert_eq!(s.bands[5].stop_mhz, 14.5);
        assert!(s.bands[13].needs_extender && !s.bands[12].needs_extender);
        assert_eq!(s.bands[18].name, "Personal 3");
        assert_eq!(s.screen.rl, Rgb(0, 0, 255));
        assert_eq!(s.png.swr_min, Rgb(0, 255, 0));
        assert_eq!(s.velocities.len(), 15);
        assert_eq!(s.velocities[0].factor, 0.66);
    }

    #[test]
    fn save_then_load() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.toml");
        let mut s = Settings { port: Some("/dev/ttyUSB0".into()), ..Default::default() };
        s.screen.swr = Rgb(1, 2, 3);
        s.bands[0].stop_mhz = 2.5;
        s.window = Some([10.0, 20.0, 900.0, 600.0]);
        s.save(&path).unwrap();
        let (loaded, message) = Settings::load(&path);
        assert_eq!(loaded, s);
        assert_eq!(message, None);
    }

    #[test]
    fn missing_file_gives_defaults_without_a_message() {
        let dir = tempfile::tempdir().unwrap();
        let (s, message) = Settings::load(&dir.path().join("absent.toml"));
        assert_eq!(s, Settings::default());
        assert_eq!(message, None);
    }

    #[test]
    fn garbage_gives_defaults_with_a_message() {
        let (_dir, path) = file("][ nonsense");
        let (s, message) = Settings::load(&path);
        assert_eq!(s, Settings::default());
        assert!(message.is_some());
    }

    #[test]
    fn partial_file_fills_the_rest_with_defaults() {
        let (_dir, path) = file("samples = 200\n");
        let (s, message) = Settings::load(&path);
        assert_eq!(s.samples, 200);
        assert_eq!(s.bands.len(), 19);
        assert_eq!(s.screen, Colors::default());
        assert_eq!(message, None);
    }

    #[test]
    fn out_of_range_values_are_repaired() {
        let (_dir, path) = file("samples = 5000\nstart_mhz = 500.0\nstop_mhz = 2.0\nvelocity = 0.0\ndds_factor = 0.0\nbands = []\n");
        let (s, _) = Settings::load(&path);
        assert_eq!(s.samples, 1000);
        assert_eq!((s.start_mhz, s.stop_mhz), (1.0, 180.0));
        assert_eq!(s.velocity, 0.66);
        assert_eq!(s.dds_factor, 10.73741824);
        assert_eq!(s.bands.len(), 19);
    }

    #[test]
    fn a_damaged_file_is_kept_aside_before_defaults_replace_it() {
        let (dir, path) = file("bands = this is not toml");
        let (settings, notice) = Settings::load(&path);
        assert_eq!(settings, Settings::default());
        assert!(notice.unwrap().contains("settings.toml.damaged"));
        let kept = std::fs::read_to_string(dir.path().join("settings.toml.damaged")).unwrap();
        assert_eq!(kept, "bands = this is not toml");
    }
}
