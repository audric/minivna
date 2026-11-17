//! CSV export in the layout the original writes for ZPlots.

use crate::device::Mode;
use crate::sweep::SweepResult;

/// One header line, then one line per point: frequency in whole Hz, loss and phase with
/// two decimals. Always a decimal point and commas, whatever the system's language.
pub fn to_csv(result: &SweepResult) -> String {
    let loss = match result.request.mode {
        Mode::Reflection => "Return Loss(dB)",
        Mode::Transmission => "Transmission Loss(dB)",
    };
    let mut out = format!("\"Frequency(Hz)\",\"{loss}\",\"Phase(deg)\"\r\n");
    for p in &result.points {
        out.push_str(&format!("{:.0},{:.2},{:.2}\r\n", p.freq_hz, p.rl_db, p.phase_deg));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calc::RawPoint;
    use crate::device::{SweepRequest, DDS_FACTOR_DEFAULT};
    use crate::sweep::build_result;

    fn result(mode: Mode) -> SweepResult {
        let request = SweepRequest { kind: crate::device::DeviceKind::MiniVna,
            mode,
            start_hz: 7_000_000,
            stop_hz: 7_358_000,
            samples: 1,
            extender: 1,
            dds_factor: DDS_FACTOR_DEFAULT,
            step_bug: true,
        };
        let raw = vec![RawPoint { phase: 512, level: 514 }, RawPoint { phase: 256, level: 600 }];
        build_result(request, raw, vec![0, -20], false, crate::device::nwt::DB_PER_COUNT_DEFAULT)
    }

    #[test]
    fn csv_reflection() {
        assert_eq!(
            to_csv(&result(Mode::Reflection)),
            "\"Frequency(Hz)\",\"Return Loss(dB)\",\"Phase(deg)\"\r\n7000000,9.99,90.09\r\n7358000,13.86,45.04\r\n"
        );
    }

    #[test]
    fn csv_transmission_header() {
        assert!(to_csv(&result(Mode::Transmission))
            .starts_with("\"Frequency(Hz)\",\"Transmission Loss(dB)\",\"Phase(deg)\"\r\n"));
    }
}
