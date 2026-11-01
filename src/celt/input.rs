//! Standard signed-16 PCM conversion and CELT input pre-emphasis.
//! Copyright (c) 2007-2008 CSIRO; 2007-2010 Xiph.Org Foundation;
//! 2008 Gregory Maxwell. See THIRD_PARTY_NOTICES.md.
use super::{FrameSize, output_tables::PREEMPHASIS};
use std::fmt;

/// Standard PCM input sampling rate, expanded onto CELT's 48 kHz internal clock.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InputRate {
    /// 8000 samples per second, expansion factor 6.
    Khz8,
    /// 12000 samples per second, expansion factor 4.
    Khz12,
    /// 16000 samples per second, expansion factor 3.
    Khz16,
    /// 24000 samples per second, expansion factor 2.
    Khz24,
    /// 48000 samples per second, no expansion.
    Khz48,
}
impl InputRate {
    /// Integer zero-insertion expansion factor at the 48 kHz internal clock.
    pub fn factor(self) -> usize {
        match self {
            Self::Khz8 => 6,
            Self::Khz12 => 4,
            Self::Khz16 => 3,
            Self::Khz24 => 2,
            Self::Khz48 => 1,
        }
    }
    /// Sampling rate in Hz.
    pub fn hz(self) -> u32 {
        48000 / self.factor() as u32
    }
}
/// Invalid input layout or output capacity, preserving signal and filter state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum InputError {
    /// Input must be exactly one mono/stereo interleaved frame; channel must exist.
    InvalidPcm,
    /// Output must hold the internal frame within a buffer of at most 960 samples.
    InvalidOutput,
}
impl fmt::Display for InputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidPcm => "invalid CELT input PCM geometry",
            Self::InvalidOutput => "invalid CELT input signal capacity",
        })
    }
}
impl std::error::Error for InputError {}

/// Persistent pre-emphasis memory for one selected input channel.
///
/// Use a separate instance for each channel. Rate changes retain memory, as in
/// the native primitive; a new independent stream should call [`Self::reset`].
/// This stage does not mix channels, prefilter pitch, retain MDCT overlap, or
/// encode audio. Low-rate analysis also needs spectral scaling/cutoff later.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PreemphasisState {
    memory: i32,
}
impl PreemphasisState {
    /// Clear the per-channel filter memory.
    pub fn reset(&mut self) {
        self.memory = 0;
    }
    /// Pending Q12 filtered input term subtracted from the following sample.
    pub fn memory(&self) -> i32 {
        self.memory
    }
    /// Convert one selected signed-16 channel to pre-emphasized Q12 signal.
    ///
    /// `channels` is 1 or 2, `channel` is zero-based, and input contains exactly
    /// `size.samples()/rate.factor()*channels` interleaved values. Output receives
    /// `size.samples()` values at 48 kHz; extra entries within 960 are preserved.
    /// Low-rate conversion inserts zeros before applying the same standard
    /// coefficient 27853 in Q15. No allocation occurs. Invalid requests preserve
    /// the complete output and memory.
    pub fn process(
        &mut self,
        size: FrameSize,
        rate: InputRate,
        input: &[i16],
        channels: usize,
        channel: usize,
        output: &mut [i32],
    ) -> Result<(), InputError> {
        let n = size.samples();
        let factor = rate.factor();
        if !matches!(channels, 1 | 2) || channel >= channels || input.len() != n / factor * channels
        {
            return Err(InputError::InvalidPcm);
        }
        if output.len() < n || output.len() > 960 {
            return Err(InputError::InvalidOutput);
        }
        let mut memory = self.memory;
        // All fallible checks precede signal writes; arithmetic is bounded by
        // signed-16 input, Q12 conversion and the standard Q15 coefficient.
        for (frame, block) in input
            .chunks_exact(channels)
            .zip(output[..n].chunks_exact_mut(factor))
        {
            let value = i32::from(frame[channel]) << 12;
            block[0] = value - memory;
            memory = ((i64::from(value) * i64::from(PREEMPHASIS)) >> 15) as i32;
            // Apply the filter to inserted zeros without an intermediate buffer
            // or per-sample division. Only the first zero can retain a term.
            for sample in &mut block[1..] {
                *sample = -memory;
                memory = 0;
            }
        }
        self.memory = memory;
        Ok(())
    }
}
