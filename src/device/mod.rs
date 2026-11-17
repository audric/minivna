//! Talking to the miniVNA. Nothing here knows about the interface.
//! Reference: docs/reverse-engineering/ig-minivna-findings.md, sections 4 and 7.

use serde::{Deserialize, Serialize};

pub mod nwt;
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
/// Which instrument is on the port.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DeviceKind {
    /// The original mRS miniVNA: 0.1-180 MHz, reflection and transmission with phase.
    #[default]
    MiniVna,
    /// A 35-4400 MHz sweep generator with a detector ("NWT" protocol): transmission loss only.
    Nwt,
}

impl DeviceKind {
    /// Lowest and highest frequency the instrument itself can make, in Hz.
    pub fn range_hz(self) -> (u64, u64) {
        match self {
            DeviceKind::MiniVna => (F_MIN_HZ as u64, F_MAX_HZ as u64),
            DeviceKind::Nwt => (35_000_000, 4_400_000_000),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            DeviceKind::MiniVna => "miniVNA",
            DeviceKind::Nwt => "NWT sweeper 35-4400 MHz",
        }
    }

    pub fn baud(self) -> u32 {
        match self {
            DeviceKind::MiniVna => 115_200,
            DeviceKind::Nwt => 57_600,
        }
    }
}

pub const F_MIN_HZ: u32 = 100_000;
pub const F_MAX_HZ: u32 = 180_000_000;
/// Hz to DDS tuning word: 2^32 / 400 MHz.
pub const DDS_FACTOR_DEFAULT: f64 = 10.73741824;

use std::time::{Duration, Instant};

use crate::calc::RawPoint;
pub use transport::{find_device, find_minivna, list_ports, SerialTransport, Transport};

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
    #[error("the sweep was stopped")]
    Cancelled,
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
    kind: DeviceKind,
}

impl<T: Transport> Device<T> {
    pub fn new(transport: T) -> Self {
        Self::with_timing(transport, Duration::from_millis(10), Duration::from_secs(5))
    }

    pub fn with_timing(transport: T, part_pause: Duration, reply_timeout: Duration) -> Self {
        Device { transport, part_pause, reply_timeout, kind: DeviceKind::MiniVna }
    }

    pub fn kind(&self) -> DeviceKind {
        self.kind
    }

    /// Say which instrument this is; it decides how `generator` and `park` talk to it.
    pub fn of_kind(mut self, kind: DeviceKind) -> Self {
        self.kind = kind;
        self
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
        self.sweep_with_cancel(req, on_progress, &|| false)
    }

    /// Like `sweep`, but gives up with `Cancelled` as soon as `cancelled` says so
    /// (asked about ten times a second while waiting for the reply).
    pub fn sweep_with_cancel(
        &mut self,
        req: &SweepRequest,
        on_progress: &mut dyn FnMut(f32),
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Vec<RawPoint>, DeviceError> {
        req.validate()?;
        if req.kind == DeviceKind::Nwt {
            return self.sweep_nwt(req, on_progress, cancelled);
        }
        let parts = req.command_parts();
        let parts: Vec<&str> = parts.iter().map(String::as_str).collect();
        let wanted = req.reply_len();
        let mut got = 0;
        for _attempt in 0..2 {
            self.send(&parts)?;
            let reply = self.read_reply(wanted, on_progress, cancelled)?;
            if reply.len() == wanted {
                return Ok(req.parse_reply(&reply));
            }
            got = reply.len();
        }
        Err(DeviceError::Timeout { got, wanted })
    }

    /// After a cancelled sweep: the miniVNA cannot be interrupted and goes on sending the
    /// rest of its reply. Read that away (up to `remaining` bytes, or until it has been
    /// quiet for a while), so that it is not taken for the answer to the next command.
    fn drain(&mut self, mut remaining: usize) {
        let quiet_limit = self.reply_timeout.min(Duration::from_millis(400));
        let mut scratch = [0u8; 512];
        let mut last_data = Instant::now();
        while remaining > 0 && last_data.elapsed() < quiet_limit {
            let want = remaining.min(scratch.len());
            match self.transport.read(&mut scratch[..want], Duration::from_millis(50)) {
                Ok(0) => {}
                Ok(n) => {
                    remaining -= n;
                    last_data = Instant::now();
                }
                Err(_) => break,
            }
        }
    }

    fn read_reply(
        &mut self,
        wanted: usize,
        on_progress: &mut dyn FnMut(f32),
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Vec<u8>, DeviceError> {
        let mut reply = vec![0u8; wanted];
        let mut got = 0;
        let mut last_data = Instant::now();
        while got < wanted {
            if cancelled() {
                self.drain(wanted - got);
                return Err(DeviceError::Cancelled);
            }
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
    fn sweep_nwt(
        &mut self,
        req: &SweepRequest,
        on_progress: &mut dyn FnMut(f32),
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Vec<RawPoint>, DeviceError> {
        let (command, wanted) = (nwt::sweep_command(req), nwt::reply_len(req));
        let mut got = 0;
        for _attempt in 0..2 {
            self.transport.discard_input()?;
            self.transport.write(&command)?;
            let reply = self.read_reply(wanted, on_progress, cancelled)?;
            if reply.len() == wanted {
                return Ok(nwt::parse_sweep(req, &reply));
            }
            got = reply.len();
        }
        Err(DeviceError::Timeout { got, wanted })
    }

    pub fn generator(&mut self, freq_hz: u64, extender: u8, dds_factor: f64) -> Result<(), DeviceError> {
        if self.kind == DeviceKind::Nwt {
            self.transport.discard_input()?;
            return Ok(self.transport.write(&nwt::generator_command(freq_hz))?);
        }
        let parts = generator_parts(freq_hz, extender, dds_factor);
        let parts: Vec<&str> = parts.iter().map(String::as_str).collect();
        self.send(&parts)
    }

    /// Park the output near 1 Hz, as the original does after measuring.
    /// Leave the output idle. The NWT sweeper has no such command and is left as it is.
    pub fn park(&mut self) -> Result<(), DeviceError> {
        if self.kind == DeviceKind::Nwt {
            return Ok(());
        }
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
        SweepRequest { kind: crate::device::DeviceKind::MiniVna,
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

    #[test]
    fn a_sweep_can_be_cancelled() {
        let mut device = Device::with_timing(SimTransport::new(), Duration::ZERO, Duration::from_millis(50));
        let outcome = device.sweep_with_cancel(&req(1_000_000, 30_000_000, 100), &mut |_| {}, &|| true);
        assert!(matches!(outcome, Err(DeviceError::Cancelled)));
        // The device goes on sending the sweep it was asked for: all of that must have been
        // read away, or it would be taken for the answer to the next command.
        assert!(device.transport().reply.is_empty(), "{} bytes left unread", device.transport().reply.len());
        // The device is usable again afterwards.
        assert_eq!(device.sweep(&req(1_000_000, 30_000_000, 100), &mut |_| {}).unwrap().len(), 101);
    }

    #[test]
    fn the_nwt_sweeper_is_swept_through_the_same_device() {
        let mut device = Device::with_timing(SimTransport::new(), Duration::ZERO, Duration::from_millis(50)).of_kind(DeviceKind::Nwt);
        let request = SweepRequest { kind: DeviceKind::Nwt, mode: Mode::Transmission, step_bug: false, ..req(400_000_000, 500_000_000, 100) };
        let raw = device.sweep(&request, &mut |_| {}).unwrap();
        assert_eq!(raw.len(), 101);
        let peak = raw.iter().enumerate().max_by_key(|(_, p)| p.level).unwrap().0;
        assert_eq!(peak, 33); // the simulated filter sits at 433 MHz
        assert_eq!(device.transport().written, b"\x8fa04000000000100000010100".to_vec());

        device.generator(145_000_000, 1, DDS_FACTOR_DEFAULT).unwrap();
        assert_eq!(device.transport().nwt_generator_hz, 145_000_000);
        let before = device.transport().written.len();
        device.park().unwrap(); // nothing to send
        assert_eq!(device.transport().written.len(), before);
    }

    #[test]
    fn a_coarse_nwt_sweep_lands_on_the_frequencies_asked_for() {
        let mut device = Device::with_timing(SimTransport::new(), Duration::ZERO, Duration::from_millis(50)).of_kind(DeviceKind::Nwt);
        // 35 to 4400 MHz in 100 steps of 43.65 MHz; the simulated filter is at 433 MHz.
        let request = SweepRequest { kind: DeviceKind::Nwt, mode: Mode::Transmission, step_bug: false, ..req(35_000_000, 4_400_000_000, 100) };
        let raw = device.sweep(&request, &mut |_| {}).unwrap();
        assert_eq!(raw.len(), 101);
        let peak = raw.iter().enumerate().max_by_key(|(_, p)| p.level).unwrap().0;
        assert_eq!(request.freqs_hz()[peak], 427_850_000.0); // the point nearest 433 MHz
    }

    #[test]
    fn a_silent_nwt_sweeper_is_a_timeout() {
        let sim = SimTransport { fail_next_replies: 2, ..SimTransport::new() };
        let mut device = Device::with_timing(sim, Duration::ZERO, Duration::from_millis(30)).of_kind(DeviceKind::Nwt);
        let request = SweepRequest { kind: DeviceKind::Nwt, mode: Mode::Transmission, ..req(400_000_000, 500_000_000, 100) };
        assert!(matches!(device.sweep(&request, &mut |_| {}), Err(DeviceError::Timeout { got: 0, wanted: 404 })));
    }
}
