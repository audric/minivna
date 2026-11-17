//! A simulated miniVNA: a series-resonant antenna on the DUT port. Used by the tests and
//! for running the program without hardware.

use std::collections::VecDeque;
use std::time::Duration;

use super::transport::Transport;
use super::DDS_FACTOR_DEFAULT;

const RESONANCE_HZ: f64 = 14.1e6;
const RESISTANCE: f64 = 50.0;
const Q: f64 = 20.0;

pub struct SimTransport {
    /// The next this-many sweep commands get no reply at all.
    pub fail_next_replies: usize,
    /// Cut every reply to this many bytes.
    pub truncate_to: Option<usize>,
    /// Everything written so far.
    pub written: Vec<u8>,
    pub(crate) command: Vec<u8>,
    pub(crate) reply: VecDeque<u8>,
    /// How long a real device would take to answer a command; zero in tests.
    pub pace: Duration,
    /// The frequency last set with the NWT generator command.
    pub nwt_generator_hz: u64,
    pub(crate) ready_at: Option<std::time::Instant>,
}

impl SimTransport {
    pub fn new() -> Self {
        SimTransport {
            fail_next_replies: 0,
            truncate_to: None,
            written: Vec::new(),
            command: Vec::new(),
            reply: VecDeque::new(),
            pace: Duration::ZERO,
            nwt_generator_hz: 0,
            ready_at: None,
        }
    }

    fn run_command(&mut self, numbers: [i64; 4]) {
        let [mode, start, count, step] = numbers;
        if count <= 1 {
            return; // generator and park commands: the real reply is never read
        }
        if self.fail_next_replies > 0 {
            self.fail_next_replies -= 1;
            return;
        }
        let mut bytes = Vec::with_capacity(count as usize * 4);
        for i in 0..count {
            let freq = (start + i * step) as f64 / DDS_FACTOR_DEFAULT;
            let (mut phase, level) = if mode == 0 { antenna_readings(freq) } else { (400, 1000) };
            if i == 0 {
                phase = 87; // the real firmware's first point is wrong
            }
            bytes.extend_from_slice(&phase.to_le_bytes());
            bytes.extend_from_slice(&level.to_le_bytes());
        }
        if let Some(limit) = self.truncate_to {
            bytes.truncate(limit);
        }
        self.reply.extend(bytes);
    }
}

/// The NWT sweeper side of the simulation: a band-pass filter centred on 433 MHz between
/// output and input.
fn filter_reading(freq_hz: f64) -> u16 {
    let off_mhz = (freq_hz - 433e6).abs() / 1e6;
    (600.0 - (off_mhz * 6.0).min(540.0)) as u16
}

impl SimTransport {
    /// Handle complete commands of the NWT protocol (they start with 0x8f) at the front
    /// of what has been written; an incomplete one waits for more bytes, like the real unit.
    fn run_nwt_commands(&mut self) {
        while self.command.first() == Some(&0x8f) {
            let Some(&letter) = self.command.get(1) else { return };
            let digits = match letter {
                b'a' => 23,
                b'f' => 9,
                _ => 0,
            };
            if self.command.len() < 2 + digits {
                return;
            }
            let text: String = self.command[2..2 + digits].iter().map(|b| *b as char).collect();
            self.command.drain(..2 + digits);
            let number = |range: std::ops::Range<usize>| text.get(range).and_then(|t| t.parse::<u64>().ok()).unwrap_or(0);
            let reading = |freq_hz: f64| {
                let level = filter_reading(freq_hz).to_le_bytes();
                [level[0], level[1], level[0], level[1]]
            };
            let mut bytes = Vec::new();
            match letter {
                b'v' => bytes.push(119),
                b'm' => bytes.extend(reading(self.nwt_generator_hz as f64)),
                b'f' => self.nwt_generator_hz = number(0..9) * 10,
                b'a' => {
                    if self.fail_next_replies > 0 {
                        self.fail_next_replies -= 1;
                        continue;
                    }
                    // Like the real unit, the step's digits from 10 MHz up are ignored.
                    let (start, step, count) = (number(0..9) * 10, number(11..17) * 10, number(17..21));
                    for i in 0..count {
                        bytes.extend(reading((start + i * step) as f64));
                    }
                }
                _ => {}
            }
            if let Some(limit) = self.truncate_to {
                bytes.truncate(limit);
            }
            self.reply.extend(bytes);
            self.ready_at = (!self.pace.is_zero()).then(|| std::time::Instant::now() + self.pace);
        }
    }
}

impl Default for SimTransport {
    fn default() -> Self {
        Self::new()
    }
}

/// Readings the device would give for a series RLC load, by inverting the conversion formulas.
fn antenna_readings(freq_hz: f64) -> (u16, u16) {
    let f = freq_hz.max(1.0);
    let x = RESISTANCE * Q * (f / RESONANCE_HZ - RESONANCE_HZ / f);
    // reflection = (Z - 50) / (Z + 50) with Z = R + jX
    let (nr, ni) = (RESISTANCE - 50.0, x);
    let (dr, di) = (RESISTANCE + 50.0, x);
    let den = dr * dr + di * di;
    let (re, im) = ((nr * dr + ni * di) / den, (ni * dr - nr * di) / den);
    let rho = re.hypot(im).max(0.003);
    let phase_deg = im.atan2(re).to_degrees().abs();
    let loss_db = -20.0 * rho.log10();
    let level = ((loss_db + 20.16) * 1023.0 / 60.0).round().clamp(0.0, 1023.0);
    let phase = (phase_deg * 1023.0 / 180.0).round().clamp(0.0, 1023.0);
    (phase as u16, level as u16)
}

impl Transport for SimTransport {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        self.written.extend_from_slice(bytes);
        self.command.extend_from_slice(bytes);
        self.run_nwt_commands();
        while self.command.first() != Some(&0x8f) && self.command.iter().filter(|b| **b == b'\r').count() >= 4 {
            let mut numbers = [0i64; 4];
            for slot in numbers.iter_mut() {
                let end = self.command.iter().position(|b| *b == b'\r').unwrap_or(0);
                let text = String::from_utf8_lossy(&self.command[..end]).into_owned();
                *slot = text.trim().parse().unwrap_or(0);
                self.command.drain(..=end);
            }
            self.run_command(numbers);
            self.ready_at = (!self.pace.is_zero()).then(|| std::time::Instant::now() + self.pace);
        }
        Ok(())
    }

    fn read(&mut self, buf: &mut [u8], timeout: Duration) -> std::io::Result<usize> {
        if let Some(ready_at) = self.ready_at {
            let wait = ready_at.saturating_duration_since(std::time::Instant::now());
            if wait > timeout {
                std::thread::sleep(timeout);
                return Ok(0);
            }
            std::thread::sleep(wait);
            self.ready_at = None;
        }
        let n = buf.len().min(self.reply.len());
        for (slot, byte) in buf.iter_mut().zip(self.reply.drain(..n)) {
            *slot = byte;
        }
        Ok(n)
    }

    fn discard_input(&mut self) -> std::io::Result<()> {
        self.reply.clear();
        Ok(())
    }
}
