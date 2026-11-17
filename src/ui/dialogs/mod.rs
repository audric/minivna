//! The windows opened from the menus. Each keeps its own small state and reports what
//! the user asked for as actions.

pub mod bands;
pub mod calibrate;
pub mod colors;
pub mod dds;
pub mod generator;
pub mod lc;
pub mod port;
pub mod velocity;

#[derive(Default)]
pub struct Dialogs {
    pub port: port::PortDialog,
    pub calibrate: calibrate::CalDialog,
    pub dds: dds::DdsDialog,
    pub generator: generator::GeneratorDialog,
    pub bands: bands::BandsDialog,
    pub colors: colors::ColorsDialog,
    pub velocity: velocity::VelocityDialog,
    pub lc: lc::LcDialog,
}
