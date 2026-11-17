#![cfg_attr(windows, windows_subsystem = "windows")]

use minivna::device::find_device;
use minivna::store::{self, Settings};
use minivna::sweep::PortChoice;

fn main() -> eframe::Result<()> {
    let simulate = std::env::args().any(|a| a == "--sim");
    let (mut settings, mut notice) = Settings::load(&store::settings_path());

    let port = if simulate {
        PortChoice::Simulated
    } else if let Some(name) = settings.port.clone() {
        PortChoice::Serial(name)
    } else if let Some(name) = find_device(settings.device) {
        settings.port = Some(name.clone());
        PortChoice::Serial(name)
    } else {
        notice.get_or_insert_with(|| format!("{} {}", settings.device.name(), minivna::ui::text::t("not found: using the simulated device. Choose a port in Configure.")));
        PortChoice::Simulated
    };

    let sweep_at_start = std::env::args().any(|a| a == "--sweep");
    minivna::ui::run(port, settings, notice, sweep_at_start)
}
