//! Run one reflection sweep on a real miniVNA and print a few points.
//! Usage: cargo run --example probe /dev/ttyUSB0

use minivna::calc::convert;
use minivna::device::{Device, Mode, SerialTransport, SweepRequest, DDS_FACTOR_DEFAULT};

fn main() {
    let port = std::env::args().nth(1).expect("usage: probe <serial port>");
    let transport = SerialTransport::open(&port).expect("open port");
    let mut device = Device::new(transport);
    let request = SweepRequest { kind: minivna::device::DeviceKind::MiniVna,
        mode: Mode::Reflection,
        start_hz: 1_000_000,
        stop_hz: 180_000_000,
        samples: 500,
        extender: 1,
        dds_factor: DDS_FACTOR_DEFAULT,
        step_bug: true,
    };
    let started = std::time::Instant::now();
    let raw = device.sweep(&request, &mut |_| {}).expect("sweep");
    let took = started.elapsed();
    device.park().expect("park");
    let points = convert(&raw, &vec![0; raw.len()], request.start_hz as f64, request.step_hz() as f64);
    println!("{} points in {:.2?}", raw.len(), took);
    for i in [0, 1, 250, 500] {
        println!(
            "point {i:3}: {:8.3} MHz  phase {:4}  level {:4}  ->  {:6.2} deg  {:5.2} dB",
            points[i].freq_hz / 1e6, raw[i].phase, raw[i].level, points[i].phase_deg, points[i].rl_db
        );
    }
}
