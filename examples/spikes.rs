//! Look for spikes in repeated full sweeps: points that jump away from both neighbours.
//! Usage: cargo run --example spikes /dev/ttyUSB0 [sweeps] [transmission]
use std::time::Duration;

use minivna::cal::Calibration;
use minivna::device::{DeviceKind, Mode, SweepRequest, DDS_FACTOR_DEFAULT};
use minivna::sweep::{spawn, Command, Event, PortChoice};

fn main() {
    let port = std::env::args().nth(1).expect("port");
    let wanted: usize = std::env::args().nth(2).and_then(|n| n.parse().ok()).unwrap_or(20);
    let request = SweepRequest {
        kind: DeviceKind::MiniVna,
        mode: if std::env::args().any(|a| a == "transmission") { Mode::Transmission } else { Mode::Reflection },
        start_hz: 1_000_000,
        stop_hz: 180_000_000,
        samples: 500,
        extender: 1,
        dds_factor: DDS_FACTOR_DEFAULT,
        step_bug: true,
    };
    let mut worker = spawn(PortChoice::Serial(port), Calibration::default(), Box::new(|| {}));
    worker.tx.send(Command::Continuous(request)).unwrap();
    let (mut sweeps, mut spiky) = (0, 0);
    while sweeps < wanted {
        match worker.rx.recv_timeout(Duration::from_secs(20)).expect("no answer") {
            Event::Sweep(r) => {
                sweeps += 1;
                let mut found = Vec::new();
                for i in 1..r.raw.len() - 1 {
                    let (a, b, c) = (&r.raw[i - 1], &r.raw[i], &r.raw[i + 1]);
                    let jump = |x: u16, y: u16, z: u16| (y as i32 - x as i32).abs().min((y as i32 - z as i32).abs());
                    let (dp, dl) = (jump(a.phase, b.phase, c.phase), jump(a.level, b.level, c.level));
                    if dp > 12 || dl > 6 {
                        found.push(format!("#{i} {:.1}MHz phase {} {} {} level {} {} {}", r.points[i].freq_hz / 1e6, a.phase, b.phase, c.phase, a.level, b.level, c.level));
                    }
                }
                if !found.is_empty() {
                    spiky += 1;
                    println!("sweep {sweeps}: {} spike(s): {}", found.len(), found.iter().take(4).cloned().collect::<Vec<_>>().join(" | "));
                }
            }
            Event::Error(why) => println!("error: {why}"),
            _ => {}
        }
    }
    worker.tx.send(Command::Stop).unwrap();
    println!("{spiky} of {sweeps} sweeps had spikes; shutdown clean: {}", worker.shutdown(Duration::from_secs(5)));
}
