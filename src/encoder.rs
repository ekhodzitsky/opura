//! Shared Opus packet encoding and native scalar control primitives.
//!
//! [`OpusEncoder`] owns common PCM, speech and CELT histories for automatic
//! single-frame encoding across all native applications, with cross-mode
//! prefills and redundancy. Multi-frame scheduling remains in progress.
//! Fixed PCM encoders remain in [`crate::silk::encoder`],
//! [`crate::celt::encoder`] and [`crate::hybrid::encoder`].
pub mod control;
pub(crate) mod pcm;
pub mod stereo_width;
mod stream;
pub use stream::{EncodeOptions, EncodedPacket, EncoderError, OpusEncoder};

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
