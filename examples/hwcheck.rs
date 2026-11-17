//! Checks on a real miniVNA that need nothing connected to it:
//! calibrated open port, a minute of continuous sweeps, the original's negative-start
//! calibration command against ours, and the step-bug option on and off.
//! Usage: cargo run --example hwcheck /dev/ttyUSB0 [seconds of continuous sweeping]

use std::time::{Duration, Instant};

use minivna::cal::Calibration;
use minivna::calc::RawPoint;
use minivna::device::protocol::parse_points;
use minivna::device::{Device, Mode, SerialTransport, SweepRequest, Transport, DDS_FACTOR_DEFAULT};
use minivna::store;
use minivna::sweep::{spawn, Command, Event, PortChoice};

fn request(step_bug: bool) -> SweepRequest {
    SweepRequest { mode: Mode::Reflection, start_hz: 1_000_000, stop_hz: 180_000_000, samples: 500, extender: 1, dds_factor: DDS_FACTOR_DEFAULT, step_bug }
}

/// Send four command parts as the driver does and read `points` points.
fn raw_command(t: &mut SerialTransport, parts: [&str; 4], points: usize) -> Vec<RawPoint> {
    t.discard_input().unwrap();
    for part in parts {
        t.write(part.as_bytes()).unwrap();
        std::thread::sleep(Duration::from_millis(10));
    }
    let mut bytes = vec![0u8; points * 4];
    let (mut got, deadline) = (0, Instant::now() + Duration::from_secs(8));
    while got < bytes.len() && Instant::now() < deadline {
        got += t.read(&mut bytes[got..], Duration::from_millis(500)).unwrap();
    }
    assert_eq!(got, bytes.len(), "short reply: {got} of {} bytes", bytes.len());
    parse_points(&bytes)
}

fn main() {
    let port = std::env::args().nth(1).expect("usage: hwcheck <serial port> [seconds]");
    let seconds: u64 = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(60);

    // 1. Calibrated open port, then continuous sweeping.
    let cal = Calibration::load(&store::calibration_path()).unwrap_or_default();
    assert!(cal.reflection.is_some(), "no reflection calibration saved: run calibrate_open first");
    let worker = spawn(PortChoice::Serial(port.clone()), cal, Box::new(|| {}));
    worker.tx.send(Command::Continuous(request(true))).unwrap();
    let (started, mut sweeps, mut errors) = (Instant::now(), 0u32, Vec::new());
    let (mut low, mut high) = (f64::INFINITY, f64::NEG_INFINITY);
    while started.elapsed() < Duration::from_secs(seconds) {
        match worker.rx.recv_timeout(Duration::from_secs(10)) {
            Ok(Event::Sweep(result)) => {
                sweeps += 1;
                for (raw, cal) in result.raw.iter().zip(&result.cal) {
                    let db = (raw.level as f64 + *cal as f64) * 60.0 / 1023.0 - 20.16;
                    (low, high) = (low.min(db), high.max(db));
                }
            }
            Ok(Event::Error(why)) => errors.push(why),
            Ok(_) => {}
            Err(_) => errors.push("no event for 10 s".to_string()),
        }
    }
    worker.tx.send(Command::Stop).unwrap();
    worker.tx.send(Command::Shutdown).unwrap();
    std::thread::sleep(Duration::from_millis(1500));
    println!("1. continuous: {sweeps} sweeps in {seconds} s, {} errors {errors:?}; open-port loss {low:.2}..{high:.2} dB (unclamped)", errors.len());

    // 2. The original's calibration command (negative start) against ours.
    let mut t = SerialTransport::open(&port).expect("open port");
    let original = raw_command(&mut t, ["0\r", "-857920\r", "1002\r", "1931662\r"], 1002);
    let ours = raw_command(&mut t, ["0\r", "1073742\r", "1002\r", "1931662\r"], 1002);
    let (mut worst_level, mut worst_phase) = (0i32, 0i32);
    for i in 1..=1000 {
        worst_level = worst_level.max((original[i + 1].level as i32 - ours[i].level as i32).abs());
        worst_phase = worst_phase.max((original[i + 1].phase as i32 - ours[i].phase as i32).abs());
    }
    println!("2. negative start: largest difference level {worst_level} counts, phase {worst_phase} counts (original 2..1001 against ours 1..1000)");
    println!("   original first points {:?}", &original[..4]);
    println!("   ours first points     {:?}", &ours[..4]);

    // 3. Step-bug option off and on.
    let mut device = Device::new(t);
    for step_bug in [false, true] {
        let raw = device.sweep(&request(step_bug), &mut |_| {}).expect("sweep");
        println!("3. step bug option {}: {} points, first four {:?}", if step_bug { "on " } else { "off" }, raw.len(), &raw[..4]);
    }
    device.park().unwrap();
}
