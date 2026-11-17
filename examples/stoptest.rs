//! Stop a long sweep half way on the real device, then check that the next sweep is normal.
use std::time::{Duration, Instant};

use minivna::cal::Calibration;
use minivna::device::{Mode, SweepRequest, DDS_FACTOR_DEFAULT};
use minivna::sweep::{spawn, Command, Event, PortChoice};

fn main() {
    let port = std::env::args().nth(1).expect("usage: stoptest <serial port>");
    let mut w = spawn(PortChoice::Serial(port), Calibration::default(), Box::new(|| {}));
    let request = |samples| SweepRequest { kind: minivna::device::DeviceKind::MiniVna, mode: Mode::Reflection, start_hz: 1_000_000, stop_hz: 180_000_000, samples, extender: 1, dds_factor: DDS_FACTOR_DEFAULT, step_bug: true };
    for round in 0..3 {
        let started = Instant::now();
        w.tx.send(Command::Single(request(1000))).unwrap();
        std::thread::sleep(Duration::from_millis(400));
        w.tx.send(Command::Stop).unwrap();
        let mut outcome = "stopped";
        loop {
            match w.rx.recv_timeout(Duration::from_secs(15)).expect("worker went quiet") {
                Event::Busy(false) => break,
                Event::Sweep(_) => outcome = "COMPLETED (not interrupted)",
                Event::Error(why) => panic!("error after stop: {why}"),
                _ => {}
            }
        }
        println!("round {round}: {outcome} after {:.2?}", started.elapsed());
        w.tx.send(Command::Single(request(500))).unwrap();
        loop {
            match w.rx.recv_timeout(Duration::from_secs(15)).expect("worker went quiet") {
                Event::Sweep(r) => {
                    let levels: Vec<u16> = r.raw.iter().map(|p| p.level).collect();
                    println!("   next sweep: {} points, level {}..{}, phase {}..{}", r.raw.len(), levels.iter().min().unwrap(), levels.iter().max().unwrap(), r.raw.iter().map(|p| p.phase).min().unwrap(), r.raw.iter().map(|p| p.phase).max().unwrap());
                }
                Event::Error(why) => panic!("sweep after stop failed: {why}"),
                Event::Busy(false) => break,
                _ => {}
            }
        }
    }
    println!("shutdown clean: {}", w.shutdown(Duration::from_secs(3)));
}
