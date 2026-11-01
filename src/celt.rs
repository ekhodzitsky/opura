//! CELT transform-layer primitives under development.
//!
//! These primitives do not yet decode CELT or Hybrid audio packets.
pub mod math;
pub mod pulses;
pub mod pvq;

pub mod energy;
mod energy_tables;

/// Classic CELT transform duration at the 48 kHz internal clock.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrameSize {
    /// 120 samples, 2.5 ms.
    Ms2_5,
    /// 240 samples, 5 ms.
    Ms5,
    /// 480 samples, 10 ms.
    Ms10,
    /// 960 samples, 20 ms.
    Ms20,
}
impl FrameSize {
    /// Samples per channel at the internal 48 kHz rate.
    pub fn samples(self) -> usize {
        120 << self.lm()
    }
    pub(super) fn lm(self) -> usize {
        match self {
            Self::Ms2_5 => 0,
            Self::Ms5 => 1,
            Self::Ms10 => 2,
            Self::Ms20 => 3,
        }
    }
}
