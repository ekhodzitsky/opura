//! Shared Opus encoder frontend primitives.
//!
//! These analyze input and select controls only. The unified adaptive PCM
//! encoder, shared layer histories and transition audio are still in progress.
//! Fixed PCM encoders remain in [`crate::silk::encoder`],
//! [`crate::celt::encoder`] and [`crate::hybrid::encoder`].
pub mod control;
pub mod stereo_width;

use crate::celt::math::MathError;
use std::fmt;

/// Invalid frontend input; failed operations preserve their state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum FrontendError {
    /// Frame geometry, channel layout or PCM length is invalid.
    InvalidGeometry,
    /// A control or history lies outside its documented domain.
    InvalidControl,
    /// The normal audio policy cannot handle this budget; the outer packet
    /// encoder must emit native insufficient-budget PLC signalling instead.
    InsufficientBudget,
    /// Constituent fixed-point arithmetic failed its domain checks.
    Math(MathError),
}
impl fmt::Display for FrontendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidGeometry => f.write_str("invalid Opus frontend geometry"),
            Self::InvalidControl => f.write_str("invalid Opus frontend control or history"),
            Self::InsufficientBudget => {
                f.write_str("Opus frontend requires insufficient-budget PLC")
            }
            Self::Math(error) => error.fmt(f),
        }
    }
}
impl std::error::Error for FrontendError {}
impl From<MathError> for FrontendError {
    fn from(error: MathError) -> Self {
        Self::Math(error)
    }
}

fn valid_duration(samples48: usize) -> bool {
    matches!(
        samples48,
        120 | 240 | 480 | 960 | 1920 | 2880 | 3840 | 4800 | 5760
    )
}
fn q15(a: i32, b: i32) -> i32 {
    ((i64::from(a) * i64::from(b)) >> 15) as i32
}
