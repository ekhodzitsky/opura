//! CELT packet encoding, decoding and transform-layer primitives.
//!
//! [`decoder`] provides stateful PCM output and loss concealment.
//! Use [`encoder::CeltEncoder`] for CELT-only encoding or
//! [`crate::encoder::OpusEncoder`] for automatic mode selection.
//! [`crate::decoder::OpusDecoder`] handles transitions between coding modes.
pub mod encoder;
pub mod input;
pub mod lpc;
pub mod math;
pub mod partition;
mod partition_tables;
pub mod pitch;
pub mod pulses;
pub mod pvq;

pub mod allocation;
mod allocation_tables;
pub mod analysis;
mod band_tables;
pub mod bands;
pub mod control;
mod control_tables;
pub mod decoder;
pub mod energy;
mod energy_tables;
pub mod fft;
mod fft_tables;
pub mod mdct;
mod mdct_tables;
pub mod output;
mod output_tables;
pub mod postfilter;
pub mod prefilter;
pub mod synthesis;

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

// Canonical Opus2.5ms transition window, shared with the decoder orchestrator.
pub(crate) fn transition_window() -> &'static [i16; 120] {
    &mdct_tables::WINDOW
}
