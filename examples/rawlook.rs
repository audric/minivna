//! Print raw phase and level at a few points of a 1-180 MHz sweep. Usage: rawlook <port>
use minivna::device::{Device, Mode, SerialTransport, SweepRequest, DDS_FACTOR_DEFAULT};
fn main() {
    let port = std::env::args().nth(1).unwrap();
    let mut device = Device::new(SerialTransport::open(&port).expect("open port"));
    let request = SweepRequest { mode: Mode::Reflection, start_hz: 1_000_000, stop_hz: 180_000_000, samples: 500, extender: 1, dds_factor: DDS_FACTOR_DEFAULT, step_bug: true };
    let raw = device.sweep(&request, &mut |_| {}).expect("sweep");
    device.park().unwrap();
    let (lmin, lmax) = (raw.iter().map(|p| p.level).min().unwrap(), raw.iter().map(|p| p.level).max().unwrap());
    let (pmin, pmax) = (raw.iter().map(|p| p.phase).min().unwrap(), raw.iter().map(|p| p.phase).max().unwrap());
    println!("level {lmin}..{lmax}, phase {pmin}..{pmax}");
    for i in (0..=500).step_by(50) {
        println!("{:6.1} MHz  phase {:4}  level {:4}", 1.0 + 179.0 * i as f64 / 500.0, raw[i].phase, raw[i].level);
    }
}
