//! Talking to the miniVNA. Nothing here knows about the interface.
//! Reference: docs/reverse-engineering/ig-minivna-findings.md, sections 4 and 7.

use serde::{Deserialize, Serialize};

pub mod protocol;
pub mod sim;
pub mod transport;

pub use protocol::{generator_parts, parse_points, RequestError, SweepRequest, PARK_PARTS};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Mode {
    Reflection,
    Transmission,
}

/// Frequency limits of the device itself, before any extender.
pub const F_MIN_HZ: u32 = 100_000;
pub const F_MAX_HZ: u32 = 180_000_000;
/// Hz to DDS tuning word: 2^32 / 400 MHz.
pub const DDS_FACTOR_DEFAULT: f64 = 10.73741824;

use std::time::{Duration, Instant};

use crate::calc::RawPoint;
pub use transport::{find_minivna, list_ports, SerialTransport, Transport};

#[derive(thiserror::Error, Debug)]
pub enum DeviceError {
    #[error("cannot open the port ({0})")]
    Open(String),
    #[error("no permission to open {0}")]
    Permission(String),
    #[error("the miniVNA did not answer ({got} of {wanted} bytes received)")]
    Timeout { got: usize, wanted: usize },
    #[error("serial port error: {0}")]
    Io(String),
    #[error("{0}")]
    Request(#[from] RequestError),
}

impl From<std::io::Error> for DeviceError {
    fn from(e: std::io::Error) -> Self {
        DeviceError::Io(e.to_string())
    }
}

/// A miniVNA on some transport.
pub struct Device<T: Transport> {
    transport: T,
    /// Pause after each of the four parts of a command.
    part_pause: Duration,
    /// How long to wait for more data before giving up on a reply.
    reply_timeout: Duration,
}

impl<T: Transport> Device<T> {
    pub fn new(transport: T) -> Self {
        Self::with_timing(transport, Duration::from_millis(10), Duration::from_secs(5))
    }

    pub fn with_timing(transport: T, part_pause: Duration, reply_timeout: Duration) -> Self {
        Device { transport, part_pause, reply_timeout }
    }

    pub fn transport(&mut self) -> &mut T {
        &mut self.transport
    }

    fn send(&mut self, parts: &[&str]) -> Result<(), DeviceError> {
        self.transport.discard_input()?;
        for part in parts {
            self.transport.write(part.as_bytes())?;
            if !self.part_pause.is_zero() {
                std::thread::sleep(self.part_pause);
            }
        }
        Ok(())
    }

    /// Run one sweep. `on_progress` gets 0.0..=1.0 as the reply arrives.
    /// A short reply is retried once.
    pub fn sweep(
        &mut self,
        req: &SweepRequest,
        on_progress: &mut dyn FnMut(f32),
    ) -> Result<Vec<RawPoint>, DeviceError> {
        req.validate()?;
        let parts = req.command_parts();
        let parts: Vec<&str> = parts.iter().map(String::as_str).collect();
        let wanted = req.reply_len();
        let mut got = 0;
        for _attempt in 0..2 {
            self.send(&parts)?;
            let reply = self.read_reply(wanted, on_progress)?;
            if reply.len() == wanted {
                return Ok(req.parse_reply(&reply));
            }
            got = reply.len();
        }
        Err(DeviceError::Timeout { got, wanted })
    }

    fn read_reply(&mut self, wanted: usize, on_progress: &mut dyn FnMut(f32)) -> Result<Vec<u8>, DeviceError> {
        let mut reply = vec![0u8; wanted];
        let mut got = 0;
        let mut last_data = Instant::now();
        while got < wanted {
            let wait = self.reply_timeout.min(Duration::from_millis(100));
            let n = self.transport.read(&mut reply[got..], wait)?;
            if n > 0 {
                got += n;
                last_data = Instant::now();
                on_progress(got as f32 / wanted as f32);
            } else if last_data.elapsed() >= self.reply_timeout {
                break;
            }
        }
        reply.truncate(got);
        Ok(reply)
    }

    /// Fixed-frequency output. The device's 4-byte reply is not read.
    pub fn generator(&mut self, freq_hz: u64, extender: u8, dds_factor: f64) -> Result<(), DeviceError> {
        let parts = generator_parts(freq_hz, extender, dds_factor);
        let parts: Vec<&str> = parts.iter().map(String::as_str).collect();
        self.send(&parts)
    }

    /// Park the output near 1 Hz, as the original does after measuring.
    pub fn park(&mut self) -> Result<(), DeviceError> {
        self.send(&PARK_PARTS)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::sim::SimTransport;
    use super::*;
    use crate::calc::convert;

    fn req(start: u64, stop: u64, samples: u16) -> SweepRequest {
        SweepRequest {
            mode: Mode::Reflection,
            start_hz: start,
            stop_hz: stop,
            samples,
            extender: 1,
            dds_factor: DDS_FACTOR_DEFAULT,
            step_bug: true,
        }
    }

    fn device(sim: SimTransport) -> Device<SimTransport> {
        Device::with_timing(sim, Duration::ZERO, Duration::from_millis(50))
    }

    #[test]
    fn sweep_sends_the_command_and_returns_samples_plus_one_points() {
        let mut d = device(SimTransport::new());
        let points = d.sweep(&req(1_000_000, 180_000_000, 500), &mut |_| {}).unwrap();
        assert_eq!(d.transport().written, b"0\r6893423\r502\r3843996\r");
        assert_eq!(points.len(), 501);
    }

    #[test]
    fn sim_looks_like_an_antenna() {
        let mut d = device(SimTransport::new());
        let r = req(1_000_000, 30_000_000, 290);
        let raw = d.sweep(&r, &mut |_| {}).unwrap();
        let points = convert(&raw, &vec![0; raw.len()], r.start_hz as f64, r.step_hz() as f64);
        let best = points.iter().min_by(|a, b| a.swr.total_cmp(&b.swr)).unwrap();
        assert!(best.swr < 1.5, "best SWR {}", best.swr);
        assert!((13e6..15e6).contains(&best.freq_hz), "dip at {}", best.freq_hz);
        assert!(points[1].swr > 5.0);
    }

    #[test]
    fn short_reply_retries_then_errors() {
        let r = req(1_000_000, 180_000_000, 500);

        let mut d = device(SimTransport { fail_next_replies: 1, ..SimTransport::new() });
        assert_eq!(d.sweep(&r, &mut |_| {}).unwrap().len(), 501);
        assert_eq!(d.transport().written, b"0\r6893423\r502\r3843996\r0\r6893423\r502\r3843996\r");

        let mut d = device(SimTransport { fail_next_replies: 2, ..SimTransport::new() });
        assert!(matches!(d.sweep(&r, &mut |_| {}), Err(DeviceError::Timeout { got: 0, wanted: 2008 })));

        let mut d = device(SimTransport { truncate_to: Some(100), ..SimTransport::new() });
        assert!(matches!(d.sweep(&r, &mut |_| {}), Err(DeviceError::Timeout { got: 100, wanted: 2008 })));
    }

    #[test]
    fn invalid_request_is_refused_before_writing() {
        let mut d = device(SimTransport::new());
        let result = d.sweep(&req(9_000_000, 7_000_000, 500), &mut |_| {});
        assert!(matches!(result, Err(DeviceError::Request(RequestError::Range))));
        assert!(d.transport().written.is_empty());
    }

    #[test]
    fn generator_and_park_bytes() {
        let mut d = device(SimTransport::new());
        d.generator(10_000_000, 1, DDS_FACTOR_DEFAULT).unwrap();
        d.park().unwrap();
        assert_eq!(d.transport().written, b"0\r107374182\r1\r0\r0\r10\r1\r0\r");
    }

    #[test]
    fn progress_reaches_one() {
        let mut d = device(SimTransport::new());
        let mut seen = Vec::new();
        d.sweep(&req(1_000_000, 180_000_000, 500), &mut |p| seen.push(p)).unwrap();
        assert_eq!(seen.last().copied(), Some(1.0));
        assert!(seen.windows(2).all(|w| w[0] <= w[1]));
    }

    #[test]
    fn a_sweep_after_a_generator_command_is_not_confused_by_leftovers() {
        let mut d = device(SimTransport::new());
        d.generator(10_000_000, 1, DDS_FACTOR_DEFAULT).unwrap();
        assert_eq!(d.sweep(&req(1_000_000, 180_000_000, 500), &mut |_| {}).unwrap().len(), 501);
    }
}
