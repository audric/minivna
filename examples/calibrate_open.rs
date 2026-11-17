//! Reflection calibration from the command line: with NOTHING connected between DUT and
//! DET, measure the calibration table, save it where the program looks for it, and check
//! a sweep against it. Usage: cargo run --example calibrate_open /dev/ttyUSB0

use std::time::Duration;

use minivna::cal::Calibration;
use minivna::device::{Mode, SweepRequest, DDS_FACTOR_DEFAULT};
use minivna::store;
use minivna::sweep::{spawn, Command, Event, PortChoice};

fn main() {
    let port = std::env::args().nth(1).expect("usage: calibrate_open <serial port>");
    let path = store::calibration_path();
    let mut cal = Calibration::load(&path).unwrap_or_default();
    let worker = spawn(PortChoice::Serial(port), cal.clone(), Box::new(|| {}));
    let next = || worker.rx.recv_timeout(Duration::from_secs(20)).expect("no answer from the worker");

    worker.tx.send(Command::Calibrate(Mode::Reflection, true, DDS_FACTOR_DEFAULT)).unwrap();
    let table = loop {
        match next() {
            Event::CalTable(_, table) => break table,
            Event::Error(why) => panic!("calibration failed: {why}"),
            _ => {}
        }
    };
    println!("table: {} entries, {}..{}", table.len(), table.iter().min().unwrap(), table.iter().max().unwrap());
    cal.reflection = Some(table);
    cal.save(&path).expect("save");
    println!("saved to {}", path.display());

    worker.tx.send(Command::SetCalibration(cal)).unwrap();
    let request = SweepRequest {
        mode: Mode::Reflection,
        start_hz: 1_000_000,
        stop_hz: 180_000_000,
        samples: 500,
        extender: 1,
        dds_factor: DDS_FACTOR_DEFAULT,
        step_bug: true,
    };
    worker.tx.send(Command::Single(request)).unwrap();
    let result = loop {
        match next() {
            Event::Sweep(result) => break result,
            Event::Error(why) => panic!("sweep failed: {why}"),
            _ => {}
        }
    };
    let loss = |f: fn(f64, f64) -> f64, start: f64| result.points.iter().map(|p| p.rl_db).fold(start, f);
    let unclamped: Vec<f64> = result.raw.iter().zip(&result.cal).map(|(r, c)| (r.level as f64 + *c as f64) * 60.0 / 1023.0 - 20.16).collect();
    let min_swr = result.points.iter().map(|p| p.swr).fold(f64::INFINITY, f64::min);
    println!(
        "calibrated open port: loss {:.2}..{:.2} dB (before clamping {:.2}..{:.2}), lowest SWR {:.0}",
        loss(f64::min, f64::INFINITY),
        loss(f64::max, 0.0),
        unclamped.iter().copied().fold(f64::INFINITY, f64::min),
        unclamped.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        min_swr
    );
    worker.tx.send(Command::Shutdown).unwrap();
}
