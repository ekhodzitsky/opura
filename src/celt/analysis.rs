//! Standard channel/subframe MDCT orchestration before CELT band analysis.
//! Copyright (c) 2007-2008 CSIRO; 2007-2010 Xiph.Org Foundation;
//! 2008 Gregory Maxwell. See THIRD_PARTY_NOTICES.md.
use super::{
    FrameSize,
    allocation_tables::LOG_N,
    input::InputRate,
    math::{self, MathError},
    mdct::{self, MdctError},
    output_tables::{BANDS, MEANS},
};
use std::fmt;

mod dynalloc;
mod dynalloc_tables;
pub use dynalloc::{DynallocAnalysis, DynallocInput, DynallocOptions, analyze_dynalloc};
mod tf;
pub use tf::{TfAnalysis, TfAnalysisOptions, analyze_tf};
mod transient;
mod transient_tables;
pub use transient::{TransientAnalysis, analyze_transients};

/// Invalid channel/context layout or underlying transform failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum AnalysisError {
    /// Signal/spectrum must contain exactly one/two planar channels of the
    /// documented length; coded channel count cannot exceed input channels.
    InvalidGeometry,
    /// A forward transform failed; no partial result is returned.
    Transform(MdctError),
    /// Band ends require 1<=effective_end<=end<=21.
    InvalidBands,
    /// TF estimate is outside Q14 unit range or LM0 is marked transient.
    InvalidTfParameters,
    /// Dynamic-allocation budget, precision, tone or surround controls are invalid.
    InvalidDynallocParameters,
    /// Signal magnitude, headroom or accumulation exceeds the stage's
    /// documented checked arithmetic domain.
    ArithmeticOverflow,
    /// Refined band scalar arithmetic rejected its input.
    Math(MathError),
}
impl fmt::Display for AnalysisError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidGeometry => f.write_str("invalid CELT analysis channel geometry"),
            Self::Transform(e) => write!(f, "CELT frame analysis: {e}"),
            Self::InvalidBands => f.write_str("invalid CELT analysis band ends"),
            Self::InvalidDynallocParameters => {
                f.write_str("invalid CELT dynamic analysis controls")
            }
            Self::InvalidTfParameters => f.write_str("invalid CELT TF analysis controls"),
            Self::ArithmeticOverflow => {
                f.write_str("CELT analysis exceeds checked arithmetic domain")
            }
            Self::Math(e) => write!(f, "CELT band analysis: {e}"),
        }
    }
}
impl std::error::Error for AnalysisError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Transform(e) => Some(e),
            Self::Math(e) => Some(e),
            _ => None,
        }
    }
}
impl From<MdctError> for AnalysisError {
    fn from(error: MdctError) -> Self {
        Self::Transform(error)
    }
}

/// Fixed-storage planar signed-32 spectra ready for CELT band analysis.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpectralFrame {
    values: [i32; 1920],
    size: FrameSize,
    channels: usize,
}
impl SpectralFrame {
    /// Transform duration on the internal 48 kHz clock.
    pub fn size(&self) -> FrameSize {
        self.size
    }
    /// Number of coded spectral channels.
    pub fn channels(&self) -> usize {
        self.channels
    }
    /// Planar coded bins, `size().samples()` consecutive values per channel.
    pub fn values(&self) -> &[i32] {
        &self.values[..self.channels * self.size.samples()]
    }
    /// One coded channel, or `None` if the index is outside the result.
    pub fn channel(&self, index: usize) -> Option<&[i32]> {
        if index >= self.channels {
            None
        } else {
            let n = self.size.samples();
            Some(&self.values[index * n..(index + 1) * n])
        }
    }
}

/// Analyze complete channel context using native long/short MDCT orchestration.
///
/// Input is planar with exactly `input_channels*(size.samples()+120)` signed-32
/// pre-emphasized/prefiltered samples, including caller-supplied overlap. One or
/// two input channels may produce the same count or stereo may produce mono.
/// `short_blocks` selects `2^LM` interleaved 120-sample transforms; otherwise one
/// long transform is used. Input is never modified and no allocation occurs.
///
/// Stereo-to-mono halves each channel separately before adding. Low-rate spectra
/// are multiplied by `rate.factor()` below the native cutoff and zeroed above.
/// Signed wrapping, including extreme synthetic signals, matches the reference.
/// This stage does not choose transients, retain overlap, pitch-prefilter, analyze
/// bands, quantize or construct encoded packets.
pub fn analyze_frame(
    size: FrameSize,
    input: &[i32],
    input_channels: usize,
    coded_channels: usize,
    rate: InputRate,
    short_blocks: bool,
) -> Result<SpectralFrame, AnalysisError> {
    let n = size.samples();
    if !matches!(input_channels, 1 | 2)
        || !(1..=input_channels).contains(&coded_channels)
        || input.len() != input_channels * (n + 120)
    {
        return Err(AnalysisError::InvalidGeometry);
    }
    let blocks = if short_blocks { n / 120 } else { 1 };
    let block_size = if short_blocks { FrameSize::Ms2_5 } else { size };
    let count = block_size.samples();
    let mut result = SpectralFrame {
        values: [0; 1920],
        size,
        channels: coded_channels,
    };
    for channel in 0..input_channels {
        for block in 0..blocks {
            let start = channel * (n + 120) + block * count;
            mdct::analyze(
                block_size,
                &input[start..start + count + 120],
                &mut result.values[channel * n + block..(channel + 1) * n],
                blocks,
            )?;
        }
    }
    if input_channels == 2 && coded_channels == 1 {
        for i in 0..n {
            result.values[i] = (result.values[i] >> 1).wrapping_add(result.values[n + i] >> 1);
        }
    }
    let factor = rate.factor();
    for channel in 0..coded_channels {
        let bins = &mut result.values[channel * n..(channel + 1) * n];
        if factor != 1 {
            let bound = n / factor;
            for value in &mut bins[..bound] {
                *value = value.wrapping_mul(factor as i32);
            }
            bins[bound..].fill(0);
        }
    }
    result.values[coded_channels * n..].fill(0);
    Ok(result)
}

impl From<MathError> for AnalysisError {
    fn from(error: MathError) -> Self {
        Self::Math(error)
    }
}

/// Fixed-storage native band amplitudes, normalized vectors and Q24 log energies.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BandAnalysis {
    normalized: [i32; 1920],
    amplitudes: [[i32; 21]; 2],
    logarithms: [[i32; 21]; 2],
    size: FrameSize,
    channels: usize,
    effective_end: usize,
    end: usize,
}
impl BandAnalysis {
    /// Analyzed transform duration.
    pub fn size(&self) -> FrameSize {
        self.size
    }
    /// Active channel count.
    pub fn channels(&self) -> usize {
        self.channels
    }
    /// Exclusive band end with measured amplitudes and normalized vectors.
    pub fn effective_end(&self) -> usize {
        self.effective_end
    }
    /// Exclusive log-energy end, including the fixed -14 tail.
    pub fn end(&self) -> usize {
        self.end
    }
    /// One full normalized Q24 channel; bins beyond effective bands are zero.
    pub fn normalized(&self, channel: usize) -> Option<&[i32]> {
        if channel >= self.channels {
            None
        } else {
            let n = self.size.samples();
            Some(&self.normalized[channel * n..(channel + 1) * n])
        }
    }
    /// Native amplitudes for one channel, zero beyond the effective band end.
    pub fn amplitudes(&self, channel: usize) -> Option<&[i32; 21]> {
        if channel >= self.channels {
            None
        } else {
            self.amplitudes.get(channel)
        }
    }
    /// Mean-adjusted Q24 log energies; effective_end..end is -14 and later is zero.
    pub fn log_energies(&self, channel: usize) -> Option<&[i32; 21]> {
        if channel >= self.channels {
            None
        } else {
            self.logarithms.get(channel)
        }
    }
}

/// Measure and normalize standard CELT bands of a planar signed-32 spectrum.
///
/// Input contains exactly `channels*size.samples()` values, channels is 1/2, and
/// band ends satisfy 1<=effective_end<=end<=21. Active bands use native headroom,
/// refined energy/reciprocal estimates, low-energy protection and mean-adjusted
/// log conversion. Inactive normalized bins/amplitudes and logs beyond end are
/// zero; effective_end..end logs are -14 in Q24. No allocation or input mutation.
///
/// Only active coefficients are inspected. Unrepresentable absolute values,
/// headroom additions or accumulated signed-32 squared energies return
/// [`AnalysisError::ArithmeticOverflow`] rather than using overflowed energies.
/// This domain includes normal PCM-derived spectra, but not arbitrary extreme
/// vectors. No partial result is returned on errors.
pub fn analyze_bands(
    size: FrameSize,
    spectrum: &[i32],
    channels: usize,
    effective_end: usize,
    end: usize,
) -> Result<BandAnalysis, AnalysisError> {
    let n = size.samples();
    if !matches!(channels, 1 | 2) || spectrum.len() != channels * n {
        return Err(AnalysisError::InvalidGeometry);
    }
    if !(1..=21).contains(&end) || !(1..=end).contains(&effective_end) {
        return Err(AnalysisError::InvalidBands);
    }
    let lm = size.lm();
    let mut result = BandAnalysis {
        normalized: [0; 1920],
        amplitudes: [[0; 21]; 2],
        logarithms: [[0; 21]; 2],
        size,
        channels,
        effective_end,
        end,
    };
    for channel in 0..channels {
        for band in 0..effective_end {
            let start = usize::from(BANDS[band]) << lm;
            let stop = usize::from(BANDS[band + 1]) << lm;
            let values = &spectrum[channel * n + start..channel * n + stop];
            let mut maximum = 0i32;
            for &value in values {
                maximum = maximum.max(
                    value
                        .checked_abs()
                        .ok_or(AnalysisError::ArithmeticOverflow)?,
                );
            }
            let amplitude = if maximum == 0 {
                1
            } else {
                let padded = maximum
                    .checked_add(maximum >> 14)
                    .and_then(|v| v.checked_add(1))
                    .ok_or(AnalysisError::ArithmeticOverflow)?;
                let width_bits = ((i32::from(LOG_N[band]) + 7) >> 3) + lm as i32 + 1;
                let shift =
                    (30 - (31 - padded.leading_zeros() as i32) - (width_bits >> 1)).max(0) as u32;
                let mut sum = 0i32;
                for &value in values {
                    let scaled = value.wrapping_shl(shift);
                    sum = sum
                        .checked_add(multiply_q31(scaled, scaled))
                        .ok_or(AnalysisError::ArithmeticOverflow)?;
                }
                maximum.max(rounded_shift(math::sqrt_extended(sum >> 1)?, shift))
            };
            result.amplitudes[channel][band] = amplitude;
            let energy = if amplitude < 10 {
                amplitude + 1
            } else {
                amplitude
            };
            let shift = (30 - (31 - energy.leading_zeros() as i32)) as u32;
            let gain = math::reciprocal_normalized_q31(energy.wrapping_shl(shift))?;
            for (i, &value) in values.iter().enumerate() {
                result.normalized[channel * n + start + i] =
                    rounded_shift(multiply_q31(gain, value.wrapping_shl(shift)), 6);
            }
            result.logarithms[channel][band] = (i32::from(math::log2(amplitude)?) << 14)
                - (i32::from(MEANS[band]) << 20)
                + (2 << 24);
        }
        result.logarithms[channel][effective_end..end].fill(-(14 << 24));
    }
    Ok(result)
}
fn multiply_q31(a: i32, b: i32) -> i32 {
    ((i64::from(a) * i64::from(b)) >> 31) as i32
}
fn rounded_shift(value: i32, shift: u32) -> i32 {
    if shift == 0 {
        value
    } else {
        value.wrapping_add(1 << (shift - 1)) >> shift
    }
}
