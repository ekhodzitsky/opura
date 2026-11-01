//! Standard channel/subframe MDCT orchestration before CELT band analysis.
//! Copyright (c) 2007-2008 CSIRO; 2007-2010 Xiph.Org Foundation;
//! 2008 Gregory Maxwell. See THIRD_PARTY_NOTICES.md.
use super::{
    FrameSize,
    input::InputRate,
    mdct::{self, MdctError},
};
use std::fmt;

/// Invalid channel/context layout or underlying transform failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum AnalysisError {
    /// Context must be exactly one/two planar input channels; coded channels
    /// must be one or two and cannot exceed the input channel count.
    InvalidGeometry,
    /// A forward transform failed; no partial result is returned.
    Transform(MdctError),
}
impl fmt::Display for AnalysisError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidGeometry => f.write_str("invalid CELT analysis channel geometry"),
            Self::Transform(e) => write!(f, "CELT frame analysis: {e}"),
        }
    }
}
impl std::error::Error for AnalysisError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Transform(e) => Some(e),
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
