//! Hardware check for the 35-4400 MHz NWT sweeper: recognise it, sweep it through the
//! same worker the window uses, stop a sweep half way, and set the generator.
//! Usage: cargo run --example nwtcheck /dev/ttyUSB0

use std::time::{Duration, Instant};

use minivna::cal::Calibration;
use minivna::device::transport::probe_nwt;
use minivna::device::{DeviceKind, Mode, SweepRequest};
use minivna::sweep::{spawn, Command, Event, PortChoice};

fn request(start_hz: u64, stop_hz: u64, samples: u16) -> SweepRequest {
    SweepRequest { kind: DeviceKind::Nwt, mode: Mode::Transmission, start_hz, stop_hz, samples, extender: 1, dds_factor: 1.0, step_bug: false }
}

fn main() {
    let port = std::env::args().nth(1).expect("port name");
    println!("answers the version query: {}", probe_nwt(&port));
    let mut worker = spawn(PortChoice::Serial(port), Calibration::default(), Box::new(|| {}));
    for (start, stop, samples) in [(35_000_000, 4_400_000_000, 1000), (35_000_000, 4_400_000_000, 100), (35_000_000, 4_400_000_000, 20), (430_000_000, 440_000_000, 500), (100_000_000, 100_010_000, 100)] {
        let begun = Instant::now();
        worker.tx.send(Command::Single(request(start, stop, samples))).unwrap();
        loop {
            match worker.rx.recv_timeout(Duration::from_secs(20)).expect("no answer") {
                Event::Sweep(r) => {
                    let levels: Vec<u16> = r.raw.iter().map(|p| p.level).collect();
                    let loss: Vec<f64> = r.points.iter().map(|p| p.rl_db).collect();
                    // With nothing connected this unit reads highest between about 125 and
                    // 193 MHz; the highest reading must be there whatever the step.
                    let peak = levels.iter().enumerate().max_by_key(|(_, l)| **l).unwrap().0;
                    println!("   highest reading at {:.1} MHz (step {:.3} MHz)", r.points[peak].freq_hz / 1e6, r.request.step_hz() as f64 / 1e6);
                    println!(
                        "{:.3}-{:.3} MHz, {} points in {:.2}s: readings {}..{}, loss {:.1}..{:.1} dB (uncalibrated)",
                        start as f64 / 1e6,
                        stop as f64 / 1e6,
                        r.points.len(),
                        begun.elapsed().as_secs_f64(),
                        levels.iter().min().unwrap(),
                        levels.iter().max().unwrap(),
                        loss.iter().cloned().fold(f64::INFINITY, f64::min),
                        loss.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
                    );
                }
                Event::Error(why) => println!("error: {why}"),
                Event::Busy(false) => break,
                _ => {}
            }
        }
    }
    // Stop half way through a full sweep, then sweep again.
    worker.tx.send(Command::Single(request(35_000_000, 4_400_000_000, 1000))).unwrap();
    std::thread::sleep(Duration::from_millis(400));
    let begun = Instant::now();
    worker.tx.send(Command::Stop).unwrap();
    worker.tx.send(Command::Single(request(100_000_000, 200_000_000, 100))).unwrap();
    let mut sweeps = 0;
    while sweeps < 1 {
        match worker.rx.recv_timeout(Duration::from_secs(20)).expect("no answer") {
            Event::Sweep(r) => {
                sweeps += 1;
                println!("after a stop ({:.2}s): {} points, start {} Hz", begun.elapsed().as_secs_f64(), r.points.len(), r.request.start_hz);
            }
            Event::Error(why) => println!("error: {why}"),
            _ => {}
        }
    }
    worker.tx.send(Command::Generator { kind: DeviceKind::Nwt, freq_hz: 145_000_000, extender: 1, dds_factor: 1.0 }).unwrap();
    std::thread::sleep(Duration::from_millis(500));
    println!("shutdown clean: {}", worker.shutdown(Duration::from_secs(5)));
    println!("still answers: {}", probe_nwt(&std::env::args().nth(1).unwrap()));
}
