//! Talking to the miniVNA. Nothing here knows about the interface.
//! Reference: docs/reverse-engineering/ig-minivna-findings.md, sections 4 and 7.

use serde::{Deserialize, Serialize};

pub mod protocol;

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
