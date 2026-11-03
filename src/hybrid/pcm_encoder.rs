//! Connected fixed-band Hybrid PCM encoding, sharing one unfinished writer.
use super::{RateAllocation, RateControls, RateError};
use crate::{
    Bandwidth,
    celt::{
        FrameSize,
        encoder::{CeltEncoder, EncoderError as CeltError, HybridControls, HybridVbr},
        input::InputRate,
    },
    encoder::pcm::PcmState,
    range::{EncoderError as EntropyError, RangeEncoder},
    silk::{
        encoder::{
            channel::{FrameControls, FrameError},
            dtx::Decision,
            mono::{PacketDuration, target_snr},
            state::{AnalysisControls, SpeechState, StereoFrameError},
            stereo::StereoEncodeError,
        },
        gains::{FrameDuration, SignalType},
        pitch::{PitchCoding, SilkRate},
        resampler::{OutputRate, ResampleError},
    },
};
use std::fmt;
#[path = "frame_encoder.rs"]
pub(crate) mod frame;
#[path = "fec.rs"]
mod protection;

pub(crate) struct PreparedFrame {
    pub writer: RangeEncoder,
    pub decision: Decision,
}

/// Hybrid input, analysis, entropy or finalization failure. Errors preserve the
/// complete encoder state and the immutable caller's PCM.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum EncoderError {
    /// Only24/48kHz PCM, one/two channels and an attainable SWB/FB are supported.
    InvalidConfiguration,
    /// PCM length does not match frame duration, input rate and channel count.
    InvalidInput,
    /// Invalid byte cap, nominal VBR rate, complexity or loss percentage.
    InvalidOptions,
    /// Native low/high band rate controls failed.
    Rate(RateError),
    /// Input resampling failed.
    Resampler(ResampleError),
    /// SILK analysis/quantization failed or could not fit its bit budget.
    Silk(FrameError),
    /// Stereo analysis or predictor signaling failed.
    Stereo(StereoEncodeError),
    /// CELT analysis/quantization failed or could not fit the shared budget.
    Celt(CeltError),
    /// Shared entropy writer failed.
    Entropy(EntropyError),
}
impl fmt::Display for EncoderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfiguration => f.write_str("invalid Hybrid rate/channels/bandwidth"),
            Self::InvalidInput => f.write_str("Hybrid PCM does not match frame geometry"),
            Self::InvalidOptions => {
                f.write_str("invalid Hybrid byte budget/nominal bitrate/complexity/loss")
            }
            Self::Rate(e) => write!(f, "Hybrid rate: {e}"),
            Self::Resampler(e) => write!(f, "Hybrid resampler: {e}"),
            Self::Silk(e) => write!(f, "Hybrid SILK: {e}"),
            Self::Stereo(e) => write!(f, "Hybrid stereo: {e}"),
            Self::Celt(e) => write!(f, "Hybrid CELT: {e}"),
            Self::Entropy(e) => write!(f, "Hybrid entropy: {e}"),
        }
    }
}
impl From<StereoFrameError> for EncoderError {
    fn from(error: StereoFrameError) -> Self {
        match error {
            StereoFrameError::Stereo(e) => Self::Stereo(e),
            StereoFrameError::Frame(e) => Self::Silk(e),
        }
    }
}
impl std::error::Error for EncoderError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Rate(e) => Some(e),
            Self::Resampler(e) => Some(e),
            Self::Silk(e) => Some(e),
            Self::Stereo(e) => Some(e),
            Self::Celt(e) => Some(e),
            Self::Entropy(e) => Some(e),
            _ => None,
        }
    }
}

/// Fixed-band Hybrid quality/loss settings. Enable silence suppression separately with
/// [`HybridEncoder::set_dtx`] and FEC with [`HybridEncoder::set_inband_fec`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Options {
    /// Native analysis/quantization complexity,0..=10.
    pub complexity: u8,
    /// Expected loss percentage,0..=100; does not enable in-band FEC.
    pub packet_loss_percent: u8,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            complexity: 10,
            packet_loss_percent: 0,
        }
    }
}

/// One complete10/20ms Hybrid Opus packet, without transport/container headers.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EncodedPacket {
    bytes: [u8; 1276],
    length: usize,
    range: u32,
    duration: FrameDuration,
}
impl EncodedPacket {
    /// TOC followed by a single shared SILK/CELT entropy payload.
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.length]
    }
    /// Last shared range-coder state before termination.
    pub fn final_range(&self) -> u32 {
        self.range
    }
    /// Standard Hybrid frame duration.
    pub fn frame_duration(&self) -> FrameDuration {
        self.duration
    }
}

/// Pure Rust fixed-band mono/stereo Hybrid PCM encoder with bounded storage.
///
/// Uses one shared range stream, WB SILK, CELT bands17..19(SWB)/21(FB), native
/// rate allocation,4ms CELT alignment, high-band gain/stereo fades and optional
/// DTX/FEC and unconstrained VBR. Cross-mode switching and general rate/channel
/// selection remain unfinished.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HybridEncoder {
    rate: InputRate,
    bandwidth: Bandwidth,
    channels: usize,
    pcm: PcmState,
    speech: SpeechState<2>,
    high_gain: i16,
    width: i16,
    celt: CeltEncoder,
}
impl HybridEncoder {
    /// Create a fresh stream.24kHz accepts SWB only;48kHz accepts SWB/FB.
    pub fn new(
        rate: InputRate,
        channels: usize,
        bandwidth: Bandwidth,
    ) -> Result<Self, EncoderError> {
        if !(1..=2).contains(&channels)
            || !matches!(rate, InputRate::Khz24 | InputRate::Khz48)
            || !matches!(bandwidth, Bandwidth::SuperWideband | Bandwidth::Fullband)
            || (rate == InputRate::Khz24 && bandwidth == Bandwidth::Fullband)
        {
            return Err(EncoderError::InvalidConfiguration);
        }
        let input = output_rate(rate);
        let mut celt = CeltEncoder::new(rate, channels).map_err(EncoderError::Celt)?;
        celt.set_transient_detection(true);
        celt.set_dynamic_allocation(true);
        celt.set_allocation_analysis(true);
        Ok(Self {
            rate,
            bandwidth,
            channels,
            pcm: if channels == 1 {
                PcmState::mono(input)
            } else {
                PcmState::stereo(input)
            },
            speech: SpeechState::new(input, SilkRate::Khz16),
            high_gain: 32767,
            width: 16384,
            celt,
        })
    }
    /// Fixed PCM input rate.
    pub fn input_rate(&self) -> InputRate {
        self.rate
    }
    /// Fixed encoded and interleaved input channel count.
    pub fn channels(&self) -> usize {
        self.channels
    }
    /// Fixed encoded high-band bandwidth.
    pub fn bandwidth(&self) -> Bandwidth {
        self.bandwidth
    }
    /// Native6.5ms pre-skip, expressed on the48kHz Opus clock.
    pub fn lookahead_48k(&self) -> u32 {
        312
    }
    /// Enable native silence suppression. Disabled by default. A suppressed
    /// packet contains only TOC and reports final range zero, even in CBR mode.
    /// SILK inactivity can retain CELT history; outer digital silence advances
    /// both layers before suppression. Errors preserve all state and clocks.
    pub fn set_dtx(&mut self, enabled: bool) {
        self.speech.dtx.enabled = enabled;
    }
    /// Request native in-band FEC using `Options::packet_loss_percent`.
    /// Disabled by default. Fixed SWB/FB is retained; insufficient rate disables
    /// new protection. Previously queued backups may still be sent after this
    /// setting is disabled. Errors preserve all main and redundant histories.
    pub fn set_inband_fec(&mut self, enabled: bool) {
        self.speech.fec.enabled = enabled;
    }
    /// Whether the last successful packet carries prior-frame LBRR.
    pub fn has_fec(&self) -> bool {
        self.speech.fec.sent[..self.channels].iter().any(|&v| v)
    }
    /// Whether the last successful packet suppressed its payload.
    pub fn in_dtx(&self) -> bool {
        self.speech.dtx.suppressed
    }
    /// Clear both layers, filters, delays and bitrate histories, retaining DTX/FEC.
    pub fn reset(&mut self) {
        self.speech.reset(output_rate(self.rate), SilkRate::Khz16);
        self.pcm = if self.channels == 1 {
            PcmState::mono(output_rate(self.rate))
        } else {
            PcmState::stereo(output_rate(self.rate))
        };
        self.high_gain = 32767;
        self.width = 16384;
        self.celt.reset();
        self.celt.set_transient_detection(true);
        self.celt.set_dynamic_allocation(true);
        self.celt.set_allocation_analysis(true);
    }
    /// Encode one interleaved signed16 frame. Budget includes TOC,3..=1276 bytes.
    /// Output fills the budget unless DTX emits one TOC byte. Tiny budgets may
    /// fail after bounded RDO, including when silence suppression is enabled.
    /// Every error preserves complete stream state; input is never modified.
    pub fn encode(
        &mut self,
        duration: FrameDuration,
        input: &[i16],
        packet_bytes: usize,
        options: Options,
    ) -> Result<EncodedPacket, EncoderError> {
        self.encode_with(duration, input, packet_bytes, options, RangeEncoder::finish)
    }
    fn encode_with(
        &mut self,
        duration: FrameDuration,
        input: &[i16],
        packet_bytes: usize,
        options: Options,
        finish: impl for<'a> FnOnce(&'a mut RangeEncoder) -> Result<&'a [u8], EntropyError>,
    ) -> Result<EncodedPacket, EncoderError> {
        self.encode_with_rate(duration, input, packet_bytes, options, None, finish)
    }
    /// Encode unconstrained Hybrid VBR at a TOC-inclusive nominal bitrate with
    /// a separate hard packet cap. DTX/FEC settings apply. The bitrate must be
    /// at least24 bits per frame and at most1024000bps; physical capacity can
    /// further cap it. The selected high-band rate must exceed500bps, matching
    /// the native CELT bitrate-control domain. Unattainable targets/caps fail
    /// explicitly. Errors preserve all state, including pending backups.
    pub fn encode_vbr(
        &mut self,
        duration: FrameDuration,
        input: &[i16],
        bitrate: u32,
        packet_bytes: usize,
        options: Options,
    ) -> Result<EncodedPacket, EncoderError> {
        self.encode_with_rate(
            duration,
            input,
            packet_bytes,
            options,
            Some(bitrate),
            RangeEncoder::finish,
        )
    }
    fn encode_with_rate(
        &mut self,
        duration: FrameDuration,
        input: &[i16],
        packet_bytes: usize,
        options: Options,
        bitrate: Option<u32>,
        finish: impl for<'a> FnOnce(&'a mut RangeEncoder) -> Result<&'a [u8], EntropyError>,
    ) -> Result<EncodedPacket, EncoderError> {
        let samples = self.rate.hz() as usize
            / if duration == FrameDuration::Ms10 {
                100
            } else {
                50
            };
        if input.len() != samples * self.channels {
            return Err(EncoderError::InvalidInput);
        }
        if !(3..=1276).contains(&packet_bytes)
            || options.complexity > 10
            || options.packet_loss_percent > 100
        {
            return Err(EncoderError::InvalidOptions);
        }
        let fps = if duration == FrameDuration::Ms10 {
            100
        } else {
            50
        };
        if bitrate.is_some_and(|rate| !(24 * fps..=1024000).contains(&rate)) {
            return Err(EncoderError::InvalidOptions);
        }
        let mut next = self.clone();
        let packet_duration = if duration == FrameDuration::Ms10 {
            PacketDuration::Ms10
        } else {
            PacketDuration::Ms20
        };
        let backup = next
            .speech
            .fec
            .has_pending(packet_duration)
            .then(|| next.clone());
        let attempt = next.encode_inner(duration, input, packet_bytes, options, bitrate, true);
        let prepared = match attempt {
            Err(error) if retryable_budget(&error) && backup.is_some() => {
                // Exactly one retry from identical complete pre-packet state.
                // Entropy finalization happens only after an attempt succeeds.
                next = backup.ok_or(error)?;
                next.encode_inner(duration, input, packet_bytes, options, bitrate, false)?
            }
            other => other?,
        };
        let result = next.finish_packet(duration, packet_bytes, prepared, finish)?;
        *self = next;
        Ok(result)
    }
    fn frame_encoder(&mut self) -> super::HybridFrameEncoder<'_> {
        super::HybridFrameEncoder {
            rate: self.rate,
            bandwidth: self.bandwidth,
            channels: self.channels,
            pcm: &mut self.pcm,
            speech: &mut self.speech,
            high_gain: &mut self.high_gain,
            width: &mut self.width,
            celt: &mut self.celt,
        }
    }
    fn encode_inner(
        &mut self,
        duration: FrameDuration,
        input: &[i16],
        packet_bytes: usize,
        options: Options,
        bitrate: Option<u32>,
        include_fec: bool,
    ) -> Result<PreparedFrame, EncoderError> {
        self.frame_encoder()
            .prepare(duration, input, packet_bytes, options, bitrate, include_fec)
    }
    fn finish_packet(
        &mut self,
        duration: FrameDuration,
        packet_bytes: usize,
        prepared: PreparedFrame,
        finish: impl for<'a> FnOnce(&'a mut RangeEncoder) -> Result<&'a [u8], EntropyError>,
    ) -> Result<EncodedPacket, EncoderError> {
        self.frame_encoder()
            .finish(duration, packet_bytes, prepared, finish)
    }
}
fn retryable_budget(error: &EncoderError) -> bool {
    let mut source: Option<&(dyn std::error::Error + 'static)> = Some(error);
    while let Some(error) = source {
        if error.downcast_ref::<EntropyError>() == Some(&EntropyError::CapacityExhausted)
            || error.downcast_ref::<FrameError>() == Some(&FrameError::BitBudgetExceeded)
            || error.downcast_ref::<CeltError>() == Some(&CeltError::BudgetExhausted)
            || error.downcast_ref::<crate::celt::allocation::AllocationError>()
                == Some(&crate::celt::allocation::AllocationError::BitBudgetExceeded)
        {
            return true;
        }
        source = error.source();
    }
    false
}
fn output_rate(rate: InputRate) -> OutputRate {
    if rate == InputRate::Khz24 {
        OutputRate::Khz24
    } else {
        OutputRate::Khz48
    }
}
fn width_q15(width: i16) -> i16 {
    if width == 16384 { 32767 } else { width * 2 }
}
fn fade(pcm: &mut [i16], rate: InputRate, channels: usize, old: i16, new: i16, stereo: bool) {
    if old == 32767 && new == 32767 {
        return;
    }
    let (old, new) = if stereo {
        (32767 - i32::from(old), 32767 - i32::from(new))
    } else {
        (i32::from(old), i32::from(new))
    };
    let overlap = 120 / rate.factor();
    for (i, frame) in pcm.chunks_exact_mut(channels).enumerate() {
        let gain = if i < overlap {
            let w = i32::from(crate::celt::transition_window()[i * rate.factor()]);
            let w = (w * w) >> 15;
            (w * new + (32767 - w) * old) >> 15
        } else {
            new
        };
        if stereo {
            let difference = (((i32::from(frame[0]) - i32::from(frame[1])) >> 1) * gain) >> 15;
            frame[0] = (i32::from(frame[0]) - difference) as i16;
            frame[1] = (i32::from(frame[1]) + difference) as i16;
        } else {
            for sample in frame {
                *sample = ((i32::from(*sample) * gain) >> 15) as i16;
            }
        }
    }
}

#[cfg(test)]
#[path = "pcm_encoder_tests.rs"]
mod tests;
