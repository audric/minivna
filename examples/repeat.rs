//! How much do two identical sweeps differ? Usage: cargo run --example repeat /dev/ttyUSB0
use std::time::{Duration, Instant};

use minivna::device::protocol::parse_points;
use minivna::device::{SerialTransport, Transport};

fn main() {
    let port = std::env::args().nth(1).expect("usage: repeat <serial port>");
    let mut t = SerialTransport::open(&port).expect("open port");
    let mut run = |parts: [&str; 4]| {
        t.discard_input().unwrap();
        for part in parts {
            t.write(part.as_bytes()).unwrap();
            std::thread::sleep(Duration::from_millis(10));
        }
        let mut bytes = vec![0u8; 1002 * 4];
        let (mut got, deadline) = (0, Instant::now() + Duration::from_secs(8));
        while got < bytes.len() && Instant::now() < deadline {
            got += t.read(&mut bytes[got..], Duration::from_millis(500)).unwrap();
        }
        parse_points(&bytes)
    };
    let ours = ["0\r", "1073742\r", "1002\r", "1931662\r"];
    let original = ["0\r", "-857920\r", "1002\r", "1931662\r"];
    let (a, b, c) = (run(ours), run(ours), run(original));
    let report = |name: &str, x: &[minivna::calc::RawPoint], y: &[minivna::calc::RawPoint], shift: usize| {
        let mut phase: Vec<(i32, usize)> = (1..=1000).map(|i| ((x[i + shift].phase as i32 - y[i].phase as i32).abs(), i)).collect();
        let level = (1..=1000).map(|i| (x[i + shift].level as i32 - y[i].level as i32).abs()).max().unwrap();
        phase.sort();
        let over2 = phase.iter().filter(|(d, _)| *d > 2).count();
        println!("{name}: level max {level}; phase max {} at point {}, {} of 1000 points differ by more than 2", phase[999].0, phase[999].1, over2);
    };
    report("ours against ours    ", &a, &b, 0);
    report("original against ours", &c, &a, 1);
}
