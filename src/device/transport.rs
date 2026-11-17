//! The byte channel to the device, and the real serial port behind it.

use std::io::{Read, Write};
use std::time::Duration;

use super::{Device, DeviceError, Mode, SweepRequest, DDS_FACTOR_DEFAULT};

/// A byte channel to a miniVNA, real or simulated.
pub trait Transport: Send {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<()>;
    /// Read up to `buf.len()` bytes, returning how many arrived before the timeout (possibly 0).
    fn read(&mut self, buf: &mut [u8], timeout: Duration) -> std::io::Result<usize>;
    /// Throw away anything received but not yet read.
    fn discard_input(&mut self) -> std::io::Result<()>;
}

impl Transport for Box<dyn Transport> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        (**self).write(bytes)
    }
    fn read(&mut self, buf: &mut [u8], timeout: Duration) -> std::io::Result<usize> {
        (**self).read(buf, timeout)
    }
    fn discard_input(&mut self) -> std::io::Result<()> {
        (**self).discard_input()
    }
}

/// The device needs a moment after the port is opened; a command sent at once gets no reply.
const SETTLE_AFTER_OPEN: Duration = Duration::from_millis(300);

pub struct SerialTransport {
    port: Box<dyn serialport::SerialPort>,
}

impl SerialTransport {
    /// 115200 8N1, no flow control, DTR and RTS asserted.
    pub fn open(port: &str) -> Result<Self, DeviceError> {
        let mut handle = serialport::new(port, 115_200)
            .data_bits(serialport::DataBits::Eight)
            .parity(serialport::Parity::None)
            .stop_bits(serialport::StopBits::One)
            .flow_control(serialport::FlowControl::None)
            .timeout(Duration::from_millis(100))
            .open()
            .map_err(|e| match e.kind() {
                serialport::ErrorKind::Io(std::io::ErrorKind::PermissionDenied) => {
                    DeviceError::Permission(port.to_string())
                }
                _ => DeviceError::Open(format!("{port}: {e}")),
            })?;
        let io = |e: serialport::Error| DeviceError::Io(e.to_string());
        handle.write_data_terminal_ready(true).map_err(io)?;
        handle.write_request_to_send(true).map_err(io)?;
        std::thread::sleep(SETTLE_AFTER_OPEN);
        Ok(SerialTransport { port: handle })
    }
}

impl Transport for SerialTransport {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        self.port.write_all(bytes)?;
        self.port.flush()
    }

    fn read(&mut self, buf: &mut [u8], timeout: Duration) -> std::io::Result<usize> {
        self.port.set_timeout(timeout.max(Duration::from_millis(1)))?;
        match self.port.read(buf) {
            Ok(n) => Ok(n),
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut => Ok(0),
            Err(e) => Err(e),
        }
    }

    fn discard_input(&mut self) -> std::io::Result<()> {
        self.port.clear(serialport::ClearBuffer::Input).map_err(std::io::Error::other)
    }
}

/// Names of the serial ports on this system.
pub fn list_ports() -> Vec<String> {
    serialport::available_ports()
        .map(|ports| ports.into_iter().map(|p| p.port_name).collect())
        .unwrap_or_default()
}

/// True if a miniVNA answers a short sweep on this port.
pub fn probe_port(port: &str) -> bool {
    let Ok(transport) = SerialTransport::open(port) else {
        return false;
    };
    let mut device = Device::with_timing(transport, Duration::from_millis(10), Duration::from_secs(1));
    let request = SweepRequest {
        mode: Mode::Reflection,
        start_hz: 1_000_000,
        stop_hz: 2_000_000,
        samples: 10,
        extender: 1,
        dds_factor: DDS_FACTOR_DEFAULT,
        step_bug: true,
    };
    let answered = device.sweep(&request, &mut |_| {}).is_ok();
    let _ = device.park();
    answered
}

/// The first port on which a miniVNA answers.
/// USB vendor number of FTDI, whose serial chip the miniVNA uses.
const FTDI_VID: u16 = 0x0403;

/// Whether a port is worth probing. Probing writes a sweep command, so serial devices that
/// are plainly something else (other USB makers, Bluetooth, built-in ports) are left alone.
pub fn could_be_minivna(kind: &serialport::SerialPortType) -> bool {
    match kind {
        serialport::SerialPortType::UsbPort(usb) => usb.vid == FTDI_VID,
        serialport::SerialPortType::Unknown => true,
        _ => false,
    }
}

pub fn find_minivna() -> Option<String> {
    let ports = serialport::available_ports().unwrap_or_default();
    ports.into_iter().filter(|p| could_be_minivna(&p.port_type)).map(|p| p.port_name).find(|port| probe_port(port))
}

#[cfg(test)]
mod tests {
    use serialport::{SerialPortType, UsbPortInfo};

    use super::*;

    fn usb(vid: u16) -> SerialPortType {
        SerialPortType::UsbPort(UsbPortInfo { vid, pid: 0x6001, serial_number: None, manufacturer: None, product: None })
    }

    #[test]
    fn only_ports_that_can_be_a_minivna_are_probed() {
        assert!(could_be_minivna(&usb(0x0403))); // FTDI, as in the miniVNA
        assert!(!could_be_minivna(&usb(0x2341))); // some other USB serial device
        assert!(!could_be_minivna(&SerialPortType::BluetoothPort));
        assert!(!could_be_minivna(&SerialPortType::PciPort));
        assert!(could_be_minivna(&SerialPortType::Unknown)); // cannot tell: try it
    }
}
