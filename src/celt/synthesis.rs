//! Transactional normalized-CELT-spectrum to final signed-16 PCM backend.
//! Combines verified classic stages in pinned celt_synthesis order. Entropy,
//! allocation, recursive bands, loss concealment and Hybrid mixing are separate.
//! Ordering follows celt_decoder.c. Copyright (c) 2007-2008 CSIRO;
//! 2007-2010 Xiph.Org Foundation; 2008 Gregory Maxwell. See THIRD_PARTY_NOTICES.md.
use super::{
    FrameSize,
    mdct::{MdctError, MdctState},
    output::{self, Decimation, DeemphasisState, OutputError, PcmFrame},
    postfilter::{PostfilterError, PostfilterParams, PostfilterState},
};
use std::{fmt, ops::Range};

/// Configuration for one already-decoded normalized spectral frame.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SynthesisControls {
    /// Complete CELT frame duration.
    pub size: FrameSize,
    /// Selected half-open energy-band interval within 0..21.
    pub bands: Range<usize>,
    /// Use 1/2/4/8 interleaved 120-sample short MDCT blocks.
    pub transient: bool,
    /// Zero new spectral input, retaining overlap and filter continuation.
    pub silence: bool,
    /// Newly decoded comb parameters; short-frame delay follows the reference.
    pub postfilter: PostfilterParams,
}
impl SynthesisControls {
    /// Full-band non-transient normal synthesis with disabled new postfilter.
    pub fn new(size: FrameSize) -> Self {
        Self {
            size,
            bands: 0..21,
            transient: false,
            silence: false,
            postfilter: PostfilterParams::default(),
        }
    }
}
/// Synthesis validation or component failure; all backend history remains unchanged.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum SynthesisError {
    /// Encoded/API channel count must be one/two and energy count match spectra.
    InvalidChannels,
    /// Each normalized spectrum must exactly match frame sample count.
    InvalidSpectrum,
    /// Scaling/de-emphasis failed.
    Output(OutputError),
    /// Inverse transform failed.
    Mdct(MdctError),
    /// Postfilter failed.
    Postfilter(PostfilterError),
}
impl fmt::Display for SynthesisError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidChannels => f.write_str("invalid CELT synthesis channel geometry"),
            Self::InvalidSpectrum => f.write_str("invalid CELT normalized spectrum length"),
            Self::Output(e) => write!(f, "CELT synthesis output: {e}"),
            Self::Mdct(e) => write!(f, "CELT synthesis transform: {e}"),
            Self::Postfilter(e) => write!(f, "CELT synthesis postfilter: {e}"),
        }
    }
}
impl std::error::Error for SynthesisError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Output(e) => Some(e),
            Self::Mdct(e) => Some(e),
            Self::Postfilter(e) => Some(e),
            _ => None,
        }
    }
}
impl From<OutputError> for SynthesisError {
    fn from(e: OutputError) -> Self {
        Self::Output(e)
    }
}
impl From<MdctError> for SynthesisError {
    fn from(e: MdctError) -> Self {
        Self::Mdct(e)
    }
}
impl From<PostfilterError> for SynthesisError {
    fn from(e: PostfilterError) -> Self {
        Self::Postfilter(e)
    }
}

/// Fixed-output-rate/channel CELT synthesis backend, with no allocation.
/// Input spectra are decoded left/right channels, not mid/side pulse vectors.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CeltSynthesis {
    rate: Decimation,
    channels: usize,
    mdct: [MdctState; 2],
    postfilter: [PostfilterState; 2],
    deemphasis: DeemphasisState,
}
impl CeltSynthesis {
    /// Start an independent backend with one/two PCM output channels.
    pub fn new(rate: Decimation, channels: usize) -> Result<Self, SynthesisError> {
        if !(1..=2).contains(&channels) {
            return Err(SynthesisError::InvalidChannels);
        }
        Ok(Self {
            rate,
            channels,
            mdct: [MdctState::default(), MdctState::default()],
            postfilter: [PostfilterState::default(), PostfilterState::default()],
            deemphasis: DeemphasisState::default(),
        })
    }
    /// Fixed PCM output channel count.
    pub fn channels(&self) -> usize {
        self.channels
    }
    /// Fixed PCM output rate in Hz.
    pub fn sample_rate(&self) -> u32 {
        self.rate.sample_rate()
    }
    /// Clear all channel histories, retaining fixed output geometry.
    pub fn reset(&mut self) {
        for state in &mut self.mdct {
            state.reset();
        }
        for state in &mut self.postfilter {
            state.reset();
        }
        self.deemphasis.reset();
    }
    /// Pending inverse-transform overlap for an active API output channel.
    pub fn overlap(&self, channel: usize) -> Option<&[i32; 60]> {
        if channel >= self.channels {
            None
        } else {
            self.mdct.get(channel).map(MdctState::overlap)
        }
    }
    /// Processed postfilter history/parameters for an active API output channel.
    pub fn postfilter(&self, channel: usize) -> Option<&PostfilterState> {
        if channel >= self.channels {
            None
        } else {
            self.postfilter.get(channel)
        }
    }
    /// Both canonical de-emphasis feedback memories.
    pub fn deemphasis_memory(&self) -> &[i32; 2] {
        self.deemphasis.memory()
    }
    /// Synthesize one/two normalized Q24 spectra with 21 Q24 energies each.
    ///
    /// Mono to stereo retains separate output-channel overlap/filter histories.
    /// Stereo to mono halves each de-normalized spectrum before addition. Long
    /// or transient MDCT output is clipped after all blocks, then postfiltered,
    /// de-emphasized and decimated. Errors preserve every backend history.
    pub fn synthesize(
        &mut self,
        spectra: &[&[i32]],
        energies: &[[i32; 21]],
        controls: &SynthesisControls,
    ) -> Result<PcmFrame, SynthesisError> {
        let encoded = spectra.len();
        let n = controls.size.samples();
        if !(1..=2).contains(&encoded) || energies.len() != encoded {
            return Err(SynthesisError::InvalidChannels);
        }
        if spectra.iter().any(|s| s.len() != n) {
            return Err(SynthesisError::InvalidSpectrum);
        }
        let mut frequencies = [[0i32; 960]; 2];
        for channel in 0..encoded {
            let spectrum = output::denormalize(
                controls.size,
                spectra[channel],
                &energies[channel],
                controls.bands.clone(),
                self.rate,
                controls.silence,
            )?;
            frequencies[channel][..n].copy_from_slice(spectrum.values());
        }
        if encoded == 1 && self.channels == 2 {
            frequencies[1] = frequencies[0];
        } else if encoded == 2 && self.channels == 1 {
            for i in 0..n {
                frequencies[0][i] = (frequencies[0][i] >> 1).wrapping_add(frequencies[1][i] >> 1);
            }
        }
        let mut next = self.clone();
        let mut time = [[0i32; 960]; 2];
        let blocks = if controls.transient { n / 120 } else { 1 };
        let block_size = if controls.transient {
            FrameSize::Ms2_5
        } else {
            controls.size
        };
        let block_length = block_size.samples();
        for channel in 0..self.channels {
            for block in 0..blocks {
                let frame = next.mdct[channel].synthesize(
                    block_size,
                    &frequencies[channel][block..n],
                    blocks,
                )?;
                time[channel][block * block_length..(block + 1) * block_length]
                    .copy_from_slice(frame.samples());
            }
            for value in &mut time[channel][..n] {
                *value = (*value).clamp(-536870911, 536870911);
            }
        }
        let left = next.postfilter[0].process(&time[0][..n], controls.postfilter)?;
        let right = if self.channels == 2 {
            Some(next.postfilter[1].process(&time[1][..n], controls.postfilter)?)
        } else {
            None
        };
        let pcm = next.deemphasis.process(
            left.samples(),
            right.as_ref().map(|r| r.samples()),
            self.rate,
        )?;
        *self = next;
        Ok(pcm)
    }
}
