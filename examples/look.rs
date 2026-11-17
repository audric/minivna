//! Sweep a range on the real device with the saved calibration and print what it sees.
//! Usage: cargo run --example look /dev/ttyUSB0 <start MHz> <stop MHz> [sweeps]
use minivna::cal::Calibration;
use minivna::device::{Device, Mode, SerialTransport, SweepRequest, DDS_FACTOR_DEFAULT};
use minivna::store;
use minivna::sweep::build_result;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let (start, stop): (f64, f64) = (a[2].parse().unwrap(), a[3].parse().unwrap());
    let sweeps: usize = a.get(4).and_then(|s| s.parse().ok()).unwrap_or(1);
    let cal = Calibration::load(&store::calibration_path()).unwrap_or_default();
    let mut device = Device::new(SerialTransport::open(&a[1]).expect("open port"));
    let request = SweepRequest { kind: minivna::device::DeviceKind::MiniVna, mode: Mode::Reflection, start_hz: (start * 1e6) as u64, stop_hz: (stop * 1e6) as u64, samples: 500, extender: 1, dds_factor: DDS_FACTOR_DEFAULT, step_bug: true };
    for n in 0..sweeps {
        let raw = device.sweep(&request, &mut |_| {}).expect("sweep");
        let offsets = cal.offsets(false, &request.device_freqs_hz());
        let r = build_result(request, raw, offsets, false, minivna::device::nwt::DB_PER_COUNT_DEFAULT);
        let best = r.points.iter().enumerate().max_by(|x, y| x.1.rl_db.total_cmp(&y.1.rl_db)).unwrap();
        let levels: Vec<u16> = r.raw.iter().map(|p| p.level).collect();
        print!("sweep {n}: raw level {}..{}, deepest return loss {:.2} dB (SWR {:.2}) at {:.3} MHz |", levels.iter().min().unwrap(), levels.iter().max().unwrap(), best.1.rl_db, best.1.swr, best.1.freq_hz / 1e6);
        for i in (0..=500).step_by(50) {
            print!(" {:.0}:{:.1}", r.points[i].freq_hz / 1e6, r.points[i].rl_db);
        }
        println!();
    }
    device.park().unwrap();
}
