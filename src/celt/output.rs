//! Classic CELT band scaling and output de-emphasis/decimation.
//! These stages do not supply comb postfiltering or full packet orchestration.
//! Copyright (c) 2007-2010 Xiph.Org Foundation; 2007-2008 CSIRO;
//! 2008-2009 Gregory Maxwell.
//! See THIRD_PARTY_NOTICES.md.
use super::{
    FrameSize,
    math::exp2_frac,
    output_tables::{BANDS, MEANS, PREEMPHASIS},
};
use std::{fmt, ops::Range};

/// Standard CELT output decimation from the 48 kHz internal clock.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Decimation {
    /// 48 kHz output.
    By1,
    /// 24 kHz output.
    By2,
    /// 16 kHz output.
    By3,
    /// 12 kHz output.
    By4,
    /// 8 kHz output.
    By6,
}
impl Decimation {
    /// Integer decimation factor.
    pub fn factor(self) -> usize {
        match self {
            Self::By1 => 1,
            Self::By2 => 2,
            Self::By3 => 3,
            Self::By4 => 4,
            Self::By6 => 6,
        }
    }
    /// Output sampling rate in Hz.
    pub fn sample_rate(self) -> u32 {
        48000 / self.factor() as u32
    }
}
/// Invalid spectral or signal buffer geometry, with no state changes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum OutputError {
    /// Spectrum length must match frame size and bands satisfy start<=end<=21.
    InvalidSpectrum,
    /// Time input must have 120/240/480/960 samples, with equal stereo lengths.
    InvalidSignal,
}
impl fmt::Display for OutputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidSpectrum => "invalid CELT spectrum geometry",
            Self::InvalidSignal => "invalid CELT output signal geometry",
        })
    }
}
impl std::error::Error for OutputError {}

/// De-normalized spectral signal ready for inverse MDCT.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Spectrum {
    values: [i32; 960],
    length: usize,
}
impl Spectrum {
    /// Signed-32 reference synthesis spectrum (before inverse MDCT).
    pub fn values(&self) -> &[i32] {
        &self.values[..self.length]
    }
}
/// Scale a normalized Q24 spectrum using Q24 log-band energies and mean energies.
/// Bins below selected bands and above the output-rate cutoff are zeroed.
/// Silence zeros the whole output. Reference wrap, gain cap and underflow apply.
pub fn denormalize(
    size: FrameSize,
    input: &[i32],
    energies: &[i32; 21],
    bands: Range<usize>,
    rate: Decimation,
    silence: bool,
) -> Result<Spectrum, OutputError> {
    let n = size.samples();
    if input.len() != n || bands.start > bands.end || bands.end > 21 {
        return Err(OutputError::InvalidSpectrum);
    }
    let mut result = Spectrum {
        values: [0; 960],
        length: n,
    };
    if silence {
        return Ok(result);
    }
    let multiplier = n / 120;
    let bound = (multiplier * usize::from(BANDS[bands.end])).min(n / rate.factor());
    for band in bands {
        let log = energies[band].wrapping_add(i32::from(MEANS[band]) << 20);
        let mut shift = 17 - (log >> 24);
        let gain = if shift >= 31 {
            shift = 0;
            0
        } else if shift < 0 {
            shift = 0;
            i32::MAX
        } else {
            let fraction = ((log & ((1 << 24) - 1)) + 8192) >> 14;
            exp2_frac(fraction as i16).wrapping_shl(16)
        };
        let start = multiplier * usize::from(BANDS[band]);
        let end = multiplier * usize::from(BANDS[band + 1]);
        for (target, &value) in result.values[start..end].iter_mut().zip(&input[start..end]) {
            let scaled = ((i64::from(value.wrapping_shl(6)) * i64::from(gain)) >> 31) as i32;
            *target = if shift == 0 {
                scaled
            } else {
                scaled.wrapping_add(1 << (shift - 1)) >> shift
            };
        }
    }
    result.values[bound..n].fill(0);
    Ok(result)
}

/// Final signed-16 samples from de-emphasis, interleaved for two input channels.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PcmFrame {
    samples: [i16; 1920],
    length: usize,
    channels: usize,
    rate: Decimation,
}
impl PcmFrame {
    pub(crate) fn silence(
        size: FrameSize,
        rate: Decimation,
        channels: usize,
    ) -> Result<Self, OutputError> {
        if !(1..=2).contains(&channels) {
            return Err(OutputError::InvalidSignal);
        }
        Ok(Self {
            samples: [0; 1920],
            length: size.samples() / rate.factor() * channels,
            channels,
            rate,
        })
    }

    /// Final signed-16 PCM, interleaved when stereo.
    pub fn samples(&self) -> &[i16] {
        &self.samples[..self.length]
    }
    /// Output channel count.
    pub fn channels(&self) -> usize {
        self.channels
    }
    /// Output samples per channel.
    pub fn samples_per_channel(&self) -> usize {
        self.length / self.channels
    }
    /// Output sampling rate in Hz.
    pub fn sample_rate(&self) -> u32 {
        self.rate.sample_rate()
    }
}
/// Persistent canonical de-emphasis memories for up to two independent channels.
/// Inactive right-channel history survives mono calls. Rate changes retain memory.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DeemphasisState {
    memory: [i32; 2],
}
impl DeemphasisState {
    /// Inspect both signed-32 feedback memories.
    pub fn memory(&self) -> &[i32; 2] {
        &self.memory
    }
    /// Clear both feedback memories for a fresh stream.
    pub fn reset(&mut self) {
        self.memory.fill(0);
    }
    /// Filter the complete 48 kHz signal, then decimate and round/saturate to PCM.
    /// Right input selects stereo and must have exactly the left input length.
    /// Comb postfiltering, when enabled, must precede this operation.
    pub fn process(
        &mut self,
        left: &[i32],
        right: Option<&[i32]>,
        rate: Decimation,
    ) -> Result<PcmFrame, OutputError> {
        let n = left.len();
        if !matches!(n, 120 | 240 | 480 | 960) || right.is_some_and(|r| r.len() != n) {
            return Err(OutputError::InvalidSignal);
        }
        let channels = if right.is_some() { 2 } else { 1 };
        let factor = rate.factor();
        let count = n / factor;
        let mut result = PcmFrame {
            samples: [0; 1920],
            length: count * channels,
            channels,
            rate,
        };
        let mut memory = self.memory;
        for (channel, input) in std::iter::once(left).chain(right).enumerate() {
            for (i, &value) in input.iter().enumerate() {
                let signal = value
                    .wrapping_add(memory[channel])
                    .clamp(-536870911, 536870911);
                memory[channel] = ((i64::from(PREEMPHASIS) * i64::from(signal)) >> 15) as i32;
                if i % factor == 0 {
                    result.samples[(i / factor) * channels + channel] =
                        ((signal + 2048) >> 12).clamp(-32768, 32767) as i16;
                }
            }
        }
        self.memory = memory;
        Ok(result)
    }
}
