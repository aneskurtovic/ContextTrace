//! Domain services: logic that belongs to no single entity.
//!
//! A domain service is where a rule lives when it spans several objects and
//! would be arbitrary to attach to any one of them. Calibration is the clearest
//! example: it concerns items, a turn's observed usage and the snapshot
//! invariant together, so it belongs to none of them individually.

pub mod calibration;

pub use calibration::{CalibrationError, TokenCalibrator};
