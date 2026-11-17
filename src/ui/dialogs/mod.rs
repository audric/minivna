//! The windows opened from the menus. Each keeps its own small state and reports what
//! the user asked for as actions.

pub mod calibrate;
pub mod dds;
pub mod generator;
pub mod port;

#[derive(Default)]
pub struct Dialogs {
    pub port: port::PortDialog,
    pub calibrate: calibrate::CalDialog,
    pub dds: dds::DdsDialog,
    pub generator: generator::GeneratorDialog,
}
