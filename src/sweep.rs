//! Runs the device on a background thread so the window never waits for it.

use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::time::Duration;

use crate::cal::{nwt_table_from_sweep, table_from_sweep, Calibration};
use crate::calc::{apply_sign, convert, Point, RawPoint};
use crate::device::sim::SimTransport;
use crate::device::nwt::{self, DB_PER_COUNT_DEFAULT};
use crate::device::{Device, DeviceError, DeviceKind, Mode, SerialTransport, SweepRequest, Transport, F_MAX_HZ, F_MIN_HZ};

#[derive(Clone, Debug, PartialEq)]
pub enum PortChoice {
    Simulated,
    Serial(String),
}

pub enum Command {
    Single(SweepRequest),
    Continuous(SweepRequest),
    Stop,
    /// Mode, step-bug option, DDS factor.
    Calibrate(Mode, bool, f64),
    /// Measure the NWT sweeper's 0 dB reference (output joined to input) over its whole range.
    CalibrateNwt,
    Generator { kind: DeviceKind, freq_hz: u64, extender: u8, dds_factor: f64 },
    Park,
    SetPort(PortChoice),
    SetCalibration(Calibration),
    SetSigned(bool),
    Shutdown,
}

/// One finished sweep: what was asked, what came back, and everything computed from it.
#[derive(Clone)]
pub struct SweepResult {
    pub request: SweepRequest,
    pub raw: Vec<RawPoint>,
    /// The calibration offset used for each point.
    pub cal: Vec<i16>,
    pub points: Vec<Point>,
    /// NWT sweeper: dB per count used for `points`. Not used for the miniVNA.
    pub db_per_count: f64,
}

pub enum Event {
    Busy(bool),
    Progress(f32),
    Sweep(SweepResult),
    CalTable(Mode, Vec<i16>),
    NwtReference(Vec<i16>),
    Error(String),
}

pub struct Worker {
    pub tx: Sender<Command>,
    pub rx: Receiver<Event>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Worker {
    /// Tell the worker to stop and park the device, and wait until it has done so.
    /// False if it did not finish within `patience` (for example a reply that never comes).
    pub fn shutdown(&mut self, patience: Duration) -> bool {
        let _ = self.tx.send(Command::Shutdown);
        let Some(thread) = self.thread.take() else { return true };
        let deadline = std::time::Instant::now() + patience;
        while !thread.is_finished() {
            if std::time::Instant::now() >= deadline {
                self.thread = Some(thread);
                return false;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        thread.join().is_ok()
    }
}

/// Compute a result from raw readings. Also used when a saved sweep is loaded.
/// For the NWT sweeper `cal` holds the 0 dB reading at each point and `db_per_count` the
/// slope of its detector; the miniVNA ignores `db_per_count`.
pub fn build_result(request: SweepRequest, raw: Vec<RawPoint>, cal: Vec<i16>, signed: bool, db_per_count: f64) -> SweepResult {
    let points = match request.kind {
        DeviceKind::Nwt => {
            let first = request.freqs_hz().first().copied().unwrap_or(request.start_hz as f64);
            nwt::to_points(&raw, &cal, db_per_count, first, request.step_hz() as f64)
        }
        DeviceKind::MiniVna => {
            let mut points = convert(&raw, &cal, request.start_hz as f64, request.step_hz() as f64);
            if signed {
                apply_sign(&mut points);
            }
            points
        }
    };
    SweepResult { request, raw, cal, points, db_per_count }
}

/// Start the worker thread. `wake` is called after every event so the window repaints.
pub fn spawn(port: PortChoice, cal: Calibration, wake: Box<dyn Fn() + Send>) -> Worker {
    let (tx, commands) = mpsc::channel();
    let (events, rx) = mpsc::channel();
    let thread = std::thread::Builder::new()
        .name("minivna-sweep".into())
        .spawn(move || Runner { port, cal, signed: false, device: None, commands, events, wake, waiting: Default::default() }.run())
        .expect("cannot start the sweep thread");
    Worker { tx, rx, thread: Some(thread) }
}

type AnyDevice = Device<Box<dyn Transport>>;

struct Runner {
    port: PortChoice,
    cal: Calibration,
    signed: bool,
    /// Open only while measuring or generating.
    device: Option<AnyDevice>,
    commands: Receiver<Command>,
    events: Sender<Event>,
    wake: Box<dyn Fn() + Send>,
    /// Commands that arrived while a sweep was being read, in order.
    waiting: std::cell::RefCell<std::collections::VecDeque<Command>>,
}

impl Runner {
    fn emit(&self, event: Event) {
        let _ = self.events.send(event);
        (self.wake)();
    }

    fn run(mut self) {
        let mut next = None;
        loop {
            let waiting = self.waiting.borrow_mut().pop_front();
            let command = match next.take().or(waiting) {
                Some(command) => command,
                None => match self.commands.recv() {
                    Ok(command) => command,
                    Err(_) => break, // the window is gone
                },
            };
            match command {
                Command::Shutdown => break,
                Command::Stop => {}
                Command::SetPort(port) => {
                    self.close();
                    self.port = port;
                }
                Command::SetCalibration(cal) => self.cal = cal,
                Command::SetSigned(signed) => self.signed = signed,
                Command::Single(request) => {
                    self.emit(Event::Busy(true));
                    let outcome = self.measure(&request);
                    self.finish(outcome.map(Event::Sweep));
                }
                Command::Continuous(request) => {
                    self.emit(Event::Busy(true));
                    next = self.run_continuous(&request);
                }
                Command::Calibrate(mode, step_bug, dds_factor) => {
                    self.emit(Event::Busy(true));
                    let outcome = self.calibrate(mode, step_bug, dds_factor);
                    self.finish(outcome.map(|table| Event::CalTable(mode, table)));
                }
                Command::CalibrateNwt => {
                    self.emit(Event::Busy(true));
                    let outcome = self.calibrate_nwt();
                    self.finish(outcome.map(Event::NwtReference));
                }
                Command::Generator { kind, freq_hz, extender, dds_factor } => {
                    let outcome = self.open(kind).and_then(|d| d.generator(freq_hz, extender, dds_factor));
                    if let Err(e) = outcome {
                        self.close();
                        self.emit(Event::Error(e.to_string()));
                    }
                }
                Command::Park => self.park_and_close(),
            }
        }
        self.park_and_close();
    }

    /// Sweep again and again until told to stop. Returns a command that arrived meanwhile
    /// and still has to be handled.
    fn run_continuous(&mut self, request: &SweepRequest) -> Option<Command> {
        loop {
            match self.measure(request) {
                Ok(result) => self.emit(Event::Sweep(result)),
                Err(DeviceError::Cancelled) => {}
                Err(e) => {
                    self.close();
                    self.emit(Event::Error(e.to_string()));
                    self.emit(Event::Busy(false));
                    return None;
                }
            }
            let waiting = self.waiting.borrow_mut().pop_front();
            let arrived = match waiting {
                Some(command) => Ok(command),
                None => self.commands.try_recv(),
            };
            let pending = match arrived {
                Ok(Command::Stop) => None,
                // Settings are taken over and the sweep goes on.
                Ok(Command::SetSigned(signed)) => {
                    self.signed = signed;
                    continue;
                }
                Ok(Command::SetCalibration(cal)) => {
                    self.cal = cal;
                    continue;
                }
                Ok(other) => Some(other),
                Err(TryRecvError::Empty) => continue,
                Err(TryRecvError::Disconnected) => Some(Command::Shutdown),
            };
            // A new continuous sweep takes over directly: the device stays busy.
            if matches!(pending, Some(Command::Continuous(_))) {
                return pending;
            }
            self.park_and_close();
            self.emit(Event::Busy(false));
            return pending;
        }
    }

    /// End a one-off action: report it, park the device, and say we are idle.
    fn finish(&mut self, outcome: Result<Event, DeviceError>) {
        match outcome {
            Ok(event) => {
                self.park_and_close();
                self.emit(event);
            }
            // Stopped by the user: not an error. The device is parked like after a sweep.
            Err(DeviceError::Cancelled) => self.park_and_close(),
            Err(e) => {
                self.close();
                self.emit(Event::Error(e.to_string()));
            }
        }
        self.emit(Event::Busy(false));
    }

    /// The device, opened as the given instrument (each has its own line speed).
    fn open(&mut self, kind: DeviceKind) -> Result<&mut AnyDevice, DeviceError> {
        if self.device.as_ref().is_some_and(|d| d.kind() != kind) {
            self.close();
        }
        if self.device.is_none() {
            let device = match &self.port {
                PortChoice::Simulated => {
                    // About as slow as the real device, so that the simulation neither
                    // floods the window nor keeps a processor core busy.
                    let sim = SimTransport { pace: Duration::from_millis(120), ..SimTransport::new() };
                    Device::with_timing(Box::new(sim) as Box<dyn Transport>, Duration::ZERO, Duration::from_secs(1))
                }
                PortChoice::Serial(name) => {
                    Device::new(Box::new(SerialTransport::open_at(name, kind.baud())?) as Box<dyn Transport>)
                }
            };
            self.device = Some(device.of_kind(kind));
        }
        Ok(self.device.as_mut().expect("just opened"))
    }

    fn close(&mut self) {
        self.device = None;
    }

    fn park_and_close(&mut self) {
        if let Some(device) = self.device.as_mut() {
            let _ = device.park();
        }
        self.close();
    }

    fn sweep_raw(&mut self, request: &SweepRequest) -> Result<Vec<RawPoint>, DeviceError> {
        let events = self.events.clone();
        self.open(request.kind)?;
        let wake = &self.wake;
        let device = self.device.as_mut().expect("open");
        // While the reply is being read, look at what arrives: Stop (or the end of the
        // program) interrupts the sweep, anything else waits its turn.
        let (commands, waiting) = (&self.commands, &self.waiting);
        let cancelled = || {
            let mut waiting = waiting.borrow_mut();
            match commands.try_recv() {
                Ok(command) => waiting.push_back(command),
                Err(TryRecvError::Disconnected) => waiting.push_back(Command::Shutdown),
                Err(TryRecvError::Empty) => {}
            }
            waiting.iter().any(|c| matches!(c, Command::Stop | Command::Shutdown))
        };
        device.sweep_with_cancel(
            request,
            &mut |fraction| {
                let _ = events.send(Event::Progress(fraction));
                wake();
            },
            &cancelled,
        )
    }

    fn measure(&mut self, request: &SweepRequest) -> Result<SweepResult, DeviceError> {
        let raw = self.sweep_raw(request)?;
        if request.kind == DeviceKind::Nwt {
            let reference = self.cal.nwt_reference_at(&request.freqs_hz());
            return Ok(build_result(*request, raw, reference, false, self.cal.nwt_slope()));
        }
        let transmission = request.mode == Mode::Transmission;
        let cal = self.cal.offsets(transmission, &request.device_freqs_hz());
        Ok(build_result(*request, raw, cal, self.signed, DB_PER_COUNT_DEFAULT))
    }

    fn calibrate_nwt(&mut self) -> Result<Vec<i16>, DeviceError> {
        let (start_hz, stop_hz) = DeviceKind::Nwt.range_hz();
        let request = SweepRequest {
            kind: DeviceKind::Nwt,
            mode: Mode::Transmission,
            start_hz,
            stop_hz,
            samples: 1000,
            extender: 1,
            dds_factor: 1.0,
            step_bug: false,
        };
        let raw = self.sweep_raw(&request)?;
        nwt_table_from_sweep(&raw).map_err(|e| DeviceError::Io(e.to_string()))
    }

    fn calibrate(&mut self, mode: Mode, step_bug: bool, dds_factor: f64) -> Result<Vec<i16>, DeviceError> {
        let request = SweepRequest { kind: crate::device::DeviceKind::MiniVna,
            mode,
            start_hz: F_MIN_HZ as u64,
            stop_hz: F_MAX_HZ as u64,
            samples: 1000,
            extender: 1,
            dds_factor,
            step_bug,
        };
        let raw = self.sweep_raw(&request)?;
        table_from_sweep(&raw).map_err(|e| DeviceError::Io(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::DDS_FACTOR_DEFAULT;

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

    fn worker() -> Worker {
        spawn(PortChoice::Simulated, Calibration::default(), Box::new(|| {}))
    }

    /// Collect events until `done` says so, failing after two seconds.
    fn collect(w: &Worker, mut done: impl FnMut(&Event) -> bool) -> Vec<Event> {
        let mut seen = Vec::new();
        loop {
            let event = w.rx.recv_timeout(Duration::from_secs(2)).expect("worker went quiet");
            let stop = done(&event);
            seen.push(event);
            if stop {
                return seen;
            }
        }
    }

    fn sweeps(events: &[Event]) -> Vec<&SweepResult> {
        events.iter().filter_map(|e| if let Event::Sweep(r) = e { Some(r) } else { None }).collect()
    }

    #[test]
    fn single_sweep_emits_busy_sweep_idle() {
        let w = worker();
        w.tx.send(Command::Single(req(1_000_000, 180_000_000, 500))).unwrap();
        let events = collect(&w, |e| matches!(e, Event::Busy(false)));
        assert!(matches!(events[0], Event::Busy(true)));
        let s = sweeps(&events);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].points.len(), 501);
        assert_eq!(s[0].raw.len(), 501);
        assert_eq!(s[0].cal.len(), 501);
    }

    #[test]
    fn continuous_runs_until_stopped() {
        let w = worker();
        w.tx.send(Command::Continuous(req(1_000_000, 30_000_000, 100))).unwrap();
        let mut count = 0;
        collect(&w, |e| {
            count += usize::from(matches!(e, Event::Sweep(_)));
            count >= 3
        });
        w.tx.send(Command::Stop).unwrap();
        collect(&w, |e| matches!(e, Event::Busy(false)));
        std::thread::sleep(Duration::from_millis(200));
        assert!(w.rx.try_iter().all(|e| !matches!(e, Event::Sweep(_))));
    }

    #[test]
    fn calibrate_returns_a_1000_entry_table() {
        let w = worker();
        w.tx.send(Command::Calibrate(Mode::Reflection, true, DDS_FACTOR_DEFAULT)).unwrap();
        let events = collect(&w, |e| matches!(e, Event::Busy(false)));
        let table = events.iter().find_map(|e| if let Event::CalTable(Mode::Reflection, t) = e { Some(t) } else { None });
        assert_eq!(table.expect("no table").len(), 1000);
    }

    #[test]
    fn calibration_is_applied() {
        let w = worker();
        let cal = Calibration { reflection: Some(vec![-20; 1000]), transmission: None, ..Default::default() };
        w.tx.send(Command::SetCalibration(cal)).unwrap();
        w.tx.send(Command::Single(req(1_000_000, 30_000_000, 100))).unwrap();
        let events = collect(&w, |e| matches!(e, Event::Busy(false)));
        let r = sweeps(&events)[0];
        assert!(r.cal.iter().all(|c| *c == -20));
        let expected = convert(&r.raw, &r.cal, 1_000_000.0, 290_000.0);
        assert!(r.points.iter().zip(&expected).all(|(a, b)| a.rl_db == b.rl_db && a.freq_hz == b.freq_hz));
    }

    #[test]
    fn error_stops_continuous() {
        let w = worker();
        w.tx.send(Command::SetPort(PortChoice::Serial("/nonexistent/port".into()))).unwrap();
        w.tx.send(Command::Continuous(req(1_000_000, 30_000_000, 100))).unwrap();
        let events = collect(&w, |e| matches!(e, Event::Busy(false)));
        assert!(events.iter().any(|e| matches!(e, Event::Error(_))));
        assert!(sweeps(&events).is_empty());
        std::thread::sleep(Duration::from_millis(100));
        assert!(w.rx.try_iter().next().is_none());
    }

    #[test]
    fn the_worker_recovers_after_an_error() {
        let w = worker();
        w.tx.send(Command::SetPort(PortChoice::Serial("/nonexistent/port".into()))).unwrap();
        w.tx.send(Command::Single(req(1_000_000, 30_000_000, 100))).unwrap();
        collect(&w, |e| matches!(e, Event::Busy(false)));
        w.tx.send(Command::SetPort(PortChoice::Simulated)).unwrap();
        w.tx.send(Command::Single(req(1_000_000, 30_000_000, 100))).unwrap();
        let events = collect(&w, |e| matches!(e, Event::Busy(false)));
        assert_eq!(sweeps(&events).len(), 1);
    }

    #[test]
    fn build_result_applies_sign_only_when_asked() {
        let w = worker();
        let r = req(1_000_000, 30_000_000, 290);
        w.tx.send(Command::Single(r)).unwrap();
        let events = collect(&w, |e| matches!(e, Event::Busy(false)));
        let got = sweeps(&events)[0];
        let unsigned = build_result(r, got.raw.clone(), got.cal.clone(), false, crate::device::nwt::DB_PER_COUNT_DEFAULT);
        assert!(unsigned.points.iter().all(|p| p.phase_deg >= 0.0 && p.xs >= 0.0));
        let signed = build_result(r, got.raw.clone(), got.cal.clone(), true, crate::device::nwt::DB_PER_COUNT_DEFAULT);
        assert!(signed.points.iter().any(|p| p.phase_deg < 0.0));
        assert!(signed.points.iter().any(|p| p.phase_deg > 0.0));
    }

    #[test]
    fn shutdown_waits_for_the_worker_to_finish() {
        let mut w = spawn(PortChoice::Simulated, Calibration::default(), Box::new(|| {}));
        w.tx.send(Command::Continuous(req(1_000_000, 30_000_000, 100))).unwrap();
        assert!(w.shutdown(Duration::from_secs(5)));
        assert!(w.tx.send(Command::Park).is_err(), "the worker should be gone");
    }

    fn next_event(w: &Worker) -> Event {
        w.rx.recv_timeout(Duration::from_secs(3)).expect("worker went quiet")
    }

    #[test]
    fn a_setting_change_does_not_stop_a_continuous_sweep() {
        let w = spawn(PortChoice::Simulated, Calibration::default(), Box::new(|| {}));
        w.tx.send(Command::Continuous(req(1_000_000, 30_000_000, 100))).unwrap();
        while !matches!(next_event(&w), Event::Sweep(_)) {}
        w.tx.send(Command::SetSigned(true)).unwrap();
        w.tx.send(Command::SetCalibration(Calibration::default())).unwrap();
        let mut sweeps = 0;
        while sweeps < 3 {
            match next_event(&w) {
                Event::Sweep(_) => sweeps += 1,
                Event::Busy(false) => panic!("the sweep stopped"),
                _ => {}
            }
        }
        w.tx.send(Command::Shutdown).unwrap();
    }

    #[test]
    fn stop_interrupts_a_single_sweep() {
        let w = spawn(PortChoice::Simulated, Calibration::default(), Box::new(|| {}));
        w.tx.send(Command::Single(req(1_000_000, 30_000_000, 500))).unwrap();
        w.tx.send(Command::Stop).unwrap();
        loop {
            match next_event(&w) {
                Event::Busy(false) => break,
                Event::Sweep(_) => panic!("the sweep was not interrupted"),
                Event::Error(why) => panic!("stopping is not an error: {why}"),
                _ => {}
            }
        }
        // And the worker still works afterwards.
        w.tx.send(Command::Single(req(1_000_000, 30_000_000, 100))).unwrap();
        while !matches!(next_event(&w), Event::Sweep(_)) {}
        w.tx.send(Command::Shutdown).unwrap();
    }

    #[test]
    fn the_simulated_device_takes_its_time() {
        let w = spawn(PortChoice::Simulated, Calibration::default(), Box::new(|| {}));
        w.tx.send(Command::Continuous(req(1_000_000, 30_000_000, 100))).unwrap();
        let started = std::time::Instant::now();
        let mut sweeps = 0;
        while started.elapsed() < Duration::from_millis(600) {
            if let Ok(Event::Sweep(_)) = w.rx.recv_timeout(Duration::from_millis(50)) {
                sweeps += 1;
            }
        }
        assert!((1..=10).contains(&sweeps), "{sweeps} sweeps in 0.6 s");
        w.tx.send(Command::Shutdown).unwrap();
    }

    fn nwt_req(start: u64, stop: u64, samples: u16) -> SweepRequest {
        SweepRequest { kind: DeviceKind::Nwt, mode: Mode::Transmission, step_bug: false, ..req(start, stop, samples) }
    }

    #[test]
    fn the_nwt_sweeper_gives_loss_against_its_reference() {
        let cal = Calibration { nwt_reference: Some(vec![600; 1000]), nwt_db_per_count: Some(0.1), ..Default::default() };
        let w = spawn(PortChoice::Simulated, cal, Box::new(|| {}));
        w.tx.send(Command::Single(nwt_req(400_000_000, 500_000_000, 100))).unwrap();
        let result = loop {
            match next_event(&w) {
                Event::Sweep(r) => break r,
                Event::Error(why) => panic!("{why}"),
                _ => {}
            }
        };
        assert_eq!(result.points.len(), 101);
        // The simulated filter passes 433 MHz without loss and drops 6 counts per MHz away from it.
        assert!(result.points[33].rl_db.abs() < 1e-9, "{}", result.points[33].rl_db);
        assert!((result.points[43].rl_db - 6.0).abs() < 1e-9, "{}", result.points[43].rl_db);
        assert!(result.points[33].swr.is_nan());
        assert_eq!(result.cal, vec![600; 101]);
        w.tx.send(Command::Shutdown).unwrap();
    }

    #[test]
    fn the_nwt_reference_is_measured_over_the_whole_range() {
        let w = spawn(PortChoice::Simulated, Calibration::default(), Box::new(|| {}));
        w.tx.send(Command::CalibrateNwt).unwrap();
        let table = loop {
            match next_event(&w) {
                Event::NwtReference(table) => break table,
                Event::Error(why) => panic!("{why}"),
                _ => {}
            }
        };
        assert_eq!(table.len(), 1000);
        assert_eq!(table.iter().max(), Some(&595)); // the nearest grid point is 0.785 MHz from 433 MHz
        w.tx.send(Command::Shutdown).unwrap();
    }

    #[test]
    fn one_worker_serves_both_devices_in_turn() {
        let w = spawn(PortChoice::Simulated, Calibration::default(), Box::new(|| {}));
        for request in [req(1_000_000, 30_000_000, 100), nwt_req(400_000_000, 500_000_000, 100), req(1_000_000, 30_000_000, 100)] {
            w.tx.send(Command::Single(request)).unwrap();
            loop {
                match next_event(&w) {
                    Event::Sweep(r) => {
                        assert_eq!(r.request.kind, request.kind);
                        break;
                    }
                    Event::Error(why) => panic!("{why}"),
                    _ => {}
                }
            }
        }
        w.tx.send(Command::Shutdown).unwrap();
    }
}
