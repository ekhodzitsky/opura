//! Mono SILK packet streams to final PCM at a selected Opus output rate.
//!
//! Composes channel decoding/recovery, dec_API mono buffering and the reference
//! resampler. Stereo, CELT/Hybrid transitions and implicit DTX remain unsupported.
use super::{
    mono::{MonoDecoder, MonoError, MonoPcm},
    resampler::{OutputRate, ResampleError, ResamplerState},
};
use std::fmt;

/// Packet or output-stage failure; complete decoder history remains unchanged.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum OutputError {
    /// Packet decoding, explicit loss scheduling or FEC recovery failed.
    Decode(MonoError),
    /// Internal PCM could not be resampled with the validated output geometry.
    Resample(ResampleError),
}
impl fmt::Display for OutputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Decode(e) => write!(f, "SILK mono output: {e}"),
            Self::Resample(e) => write!(f, "SILK mono output: {e}"),
        }
    }
}
impl std::error::Error for OutputError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Decode(e) => Some(e),
            Self::Resample(e) => Some(e),
        }
    }
}

/// Up to 120 ms of signed 16-bit mono PCM, after buffering and resampling.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OutputPcm {
    samples: [i16; 5760],
    length: usize,
    rate: OutputRate,
    final_range: u32,
}
impl OutputPcm {
    /// Final output samples at the requested sampling rate.
    pub fn samples(&self) -> &[i16] {
        &self.samples[..self.length]
    }
    /// Output sampling rate in Hz.
    pub fn sample_rate(&self) -> u32 {
        self.rate.hz()
    }
    /// Entropy diagnostic inherited from the packet/FEC result, or zero for loss.
    pub fn final_range(&self) -> u32 {
        self.final_range
    }
}

/// Allocation-free mono SILK output decoder at a fixed sampling rate.
///
/// Successful results include PLC/CNG/recovery, mono delay buffering and output
/// resampling. Any error retains all packet, predictor, delay and filter history.
/// This is limited to SILK-only mono streams; it is not a complete Opus decoder.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MonoOutputDecoder {
    decoder: MonoDecoder,
    resampler: ResamplerState,
    previous: i16,
}
impl MonoOutputDecoder {
    /// Start an independent stream at the selected standard Opus output rate.
    pub fn new(rate: OutputRate) -> Self {
        Self {
            decoder: MonoDecoder::default(),
            resampler: ResamplerState::new(rate),
            previous: 0,
        }
    }
    /// Fixed output rate selected at construction.
    pub fn output_rate(&self) -> OutputRate {
        self.resampler.output_rate()
    }
    /// Forget all histories while retaining the selected output rate.
    pub fn reset(&mut self) {
        *self = Self::new(self.output_rate());
    }
    /// Decode one complete externally delimited mono SILK-only packet.
    ///
    /// Uses [`MonoDecoder::decode`] restrictions and transaction semantics.
    /// Samples are now aligned with public Opus output for supported streams.
    pub fn decode(&mut self, data: &[u8]) -> Result<OutputPcm, OutputError> {
        self.advance(|decoder| decoder.decode(data))
    }
    /// Conceal a missing 10..=120 ms interval in 10 ms steps.
    ///
    /// Uses the rate and scheduling history required by [`MonoDecoder::conceal`].
    pub fn conceal(&mut self, duration_ms: u16) -> Result<OutputPcm, OutputError> {
        self.advance(|decoder| decoder.conceal(duration_ms))
    }
    /// Recover the preceding first-frame interval from this packet's in-band FEC.
    ///
    /// Follows [`MonoDecoder::decode_fec`]. Decode this packet normally afterwards
    /// to obtain primary audio and continue from the recovered state.
    pub fn decode_fec(&mut self, data: &[u8]) -> Result<OutputPcm, OutputError> {
        self.advance(|decoder| decoder.decode_fec(data))
    }

    fn advance(
        &mut self,
        operation: impl FnOnce(&mut MonoDecoder) -> Result<MonoPcm, MonoError>,
    ) -> Result<OutputPcm, OutputError> {
        let mut next = self.clone();
        let raw = operation(&mut next.decoder).map_err(OutputError::Decode)?;
        // dec_API resamples from temporary[1]: previous final channel sample,
        // followed by all but the current frame's final sample. Across a packet,
        // this is one continuous delay line. Rate switches retain that sample.
        // MonoPcm constructors guarantee nonempty output of at most 1920 samples.
        let length = raw.samples().len();
        let mut delayed = [0i16; 1920];
        delayed[0] = next.previous;
        delayed[1..length].copy_from_slice(&raw.samples()[..length - 1]);
        next.previous = raw.samples()[length - 1];
        let mut result = OutputPcm {
            samples: [0; 5760],
            length: 0,
            rate: next.output_rate(),
            final_range: raw.final_range(),
        };
        result.length = next
            .resampler
            .process(raw.rate(), &delayed[..length], &mut result.samples)
            .map_err(OutputError::Resample)?;
        *self = next;
        Ok(result)
    }
}
