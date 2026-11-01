//! SILK-only mono/stereo packet output, loss concealment and in-band FEC.
//!
//! Follows pinned Opus dec_API channel ordering and transition rules; see
//! THIRD_PARTY_NOTICES.md. CELT/Hybrid and implicit DTX remain unsupported.
use super::{
    FrameHeader, SilkError,
    channel::{ChannelError, ChannelState},
    decode_header,
    gains::{FrameActivity, FrameDuration},
    mono::RedundancyState,
    pitch::{PitchCoding, SilkRate},
    resampler::{OutputRate, ResampleError, ResamplerState},
    stereo::{StereoError, StereoPrediction, StereoState, decode_mid_only},
    synthesis::PcmFrame,
};
use crate::{Bandwidth, Mode, Packet, PacketError, range::RangeDecoder};
use std::fmt;

/// Number of PCM output channels, independent of the encoded channel count.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PcmChannels {
    /// One mono channel; encoded stereo is decoded to its mid channel.
    Mono,
    /// Two interleaved left/right channels; encoded mono is duplicated.
    Stereo,
}
impl PcmChannels {
    /// Channel count (one or two).
    pub fn count(self) -> usize {
        if self == Self::Mono { 1 } else { 2 }
    }
}

/// Complete SILK operation failure; no partial history/output is committed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum DecoderError {
    /// Invalid externally delimited Opus framing.
    Framing(PacketError),
    /// Only SILK-only packets are supported by this decoder.
    UnsupportedMode,
    /// Empty/one-byte payloads need explicit loss/DTX handling.
    MissingPayload,
    /// Concealment needs a preceding packet to establish internal configuration.
    MissingHistory,
    /// Missing interval must be 10..=120 ms in 10 ms steps.
    InvalidLossDuration,
    /// SILK header decoding failed.
    Header(SilkError),
    /// Channel decoding or concealment failed.
    Channel(ChannelError),
    /// Stereo fields or reconstruction failed.
    Stereo(StereoError),
    /// PCM resampling failed.
    Resample(ResampleError),
    /// Normal audio has a CELT transition redundancy tail.
    UnsupportedTransition,
}
impl fmt::Display for DecoderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Framing(e) => write!(f, "SILK framing: {e}"),
            Self::UnsupportedMode => f.write_str("decoder requires SILK-only mode"),
            Self::MissingPayload => {
                f.write_str("missing SILK payload requires explicit concealment")
            }
            Self::MissingHistory => f.write_str("SILK concealment needs prior configuration"),
            Self::InvalidLossDuration => {
                f.write_str("SILK loss duration must be 10..120 ms in 10 ms steps")
            }
            Self::Header(e) => write!(f, "SILK header: {e}"),
            Self::Channel(e) => write!(f, "SILK channel: {e}"),
            Self::Stereo(e) => write!(f, "SILK stereo: {e}"),
            Self::Resample(e) => write!(f, "SILK output: {e}"),
            Self::UnsupportedTransition => {
                f.write_str("CELT transition reconstruction is not implemented")
            }
        }
    }
}
impl std::error::Error for DecoderError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Framing(e) => Some(e),
            Self::Header(e) => Some(e),
            Self::Channel(e) => Some(e),
            Self::Stereo(e) => Some(e),
            Self::Resample(e) => Some(e),
            _ => None,
        }
    }
}
impl From<ChannelError> for DecoderError {
    fn from(e: ChannelError) -> Self {
        Self::Channel(e)
    }
}
impl From<StereoError> for DecoderError {
    fn from(e: StereoError) -> Self {
        Self::Stereo(e)
    }
}
impl From<ResampleError> for DecoderError {
    fn from(e: ResampleError) -> Self {
        Self::Resample(e)
    }
}

/// Up to 120 ms of final signed-16 PCM, interleaved when stereo output is selected.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DecodedPcm {
    samples: [i16; 11520],
    length: usize,
    rate: OutputRate,
    channels: PcmChannels,
    range: u32,
}
impl DecodedPcm {
    /// Interleaved PCM samples at the selected output rate/channel count.
    pub fn samples(&self) -> &[i16] {
        &self.samples[..self.length]
    }
    /// Output sample rate in Hz.
    pub fn sample_rate(&self) -> u32 {
        self.rate.hz()
    }
    /// PCM output channel count.
    pub fn channels(&self) -> usize {
        self.channels.count()
    }
    /// Number of samples per output channel.
    pub fn samples_per_channel(&self) -> usize {
        self.length / self.channels.count()
    }
    /// Public Opus final-range diagnostic; zero for explicit loss.
    pub fn final_range(&self) -> u32 {
        self.range
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum Kind {
    Normal,
    Fec,
    Loss,
}
struct FrameFlags {
    vad: [bool; 2],
    redundant: [bool; 2],
    previous: [bool; 2],
}
struct FramePlan {
    kind: Kind,
    rate: SilkRate,
    duration: FrameDuration,
    channels: usize,
    index: usize,
    vad: [bool; 2],
    redundant: [bool; 2],
    previous_redundant: [bool; 2],
    prediction: StereoPrediction,
    mid_only: bool,
    collapse: bool,
}

/// Fixed-storage SILK-only decoder for encoded mono/stereo packet streams.
///
/// Output rate/channels are fixed at construction. Packet, loss and FEC calls
/// commit all channel, stereo, resampler and configuration state atomically.
/// This does not decode CELT/Hybrid, CELT transition tails or implicit DTX.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SilkDecoder {
    channel: [ChannelState; 2],
    stereo: StereoState,
    resampler: [ResamplerState; 2],
    output_channels: PcmChannels,
    internal_channels: usize,
    rate: Option<SilkRate>,
    last_duration: FrameDuration,
    previous_mid_only: bool,
}
impl SilkDecoder {
    /// Start an independent stream with selected output geometry.
    pub fn new(rate: OutputRate, channels: PcmChannels) -> Self {
        Self {
            channel: [ChannelState::default(), ChannelState::default()],
            stereo: StereoState::default(),
            resampler: [ResamplerState::new(rate), ResamplerState::new(rate)],
            output_channels: channels,
            internal_channels: 0,
            rate: None,
            last_duration: FrameDuration::Ms20,
            previous_mid_only: false,
        }
    }
    /// Fixed output sampling rate.
    pub fn output_rate(&self) -> OutputRate {
        self.resampler[0].output_rate()
    }
    /// Fixed output channel count.
    pub fn output_channels(&self) -> PcmChannels {
        self.output_channels
    }
    /// Clear all histories, retaining selected output rate and channel count.
    pub fn reset(&mut self) {
        *self = Self::new(self.output_rate(), self.output_channels);
    }
    /// Decode normal audio from one complete externally delimited SILK packet.
    pub fn decode(&mut self, data: &[u8]) -> Result<DecodedPcm, DecoderError> {
        self.packet(data, false)
    }
    /// Recover the preceding first-Opus-frame interval using in-band FEC.
    ///
    /// Missing redundant frames use PLC/CNG. Decode this received packet normally
    /// afterwards for its primary audio. Later compressed Opus frames are ignored.
    pub fn decode_fec(&mut self, data: &[u8]) -> Result<DecodedPcm, DecoderError> {
        self.packet(data, true)
    }
    /// Conceal a missing interval, 10..=120 ms in 10 ms steps.
    ///
    /// Requires prior packet/FEC configuration. Follows 10 ms scheduling after
    /// a 10 ms encoded frame, otherwise 20 ms plus a final 10 ms remainder.
    pub fn conceal(&mut self, duration_ms: u16) -> Result<DecodedPcm, DecoderError> {
        if !(10..=120).contains(&duration_ms) || duration_ms % 10 != 0 {
            return Err(DecoderError::InvalidLossDuration);
        }
        let rate = self.rate.ok_or(DecoderError::MissingHistory)?;
        let mut next = self.clone();
        let mut result = self.empty_output();
        let mut remaining = duration_ms;
        while remaining != 0 {
            let duration = if self.last_duration == FrameDuration::Ms10 || remaining == 10 {
                FrameDuration::Ms10
            } else {
                FrameDuration::Ms20
            };
            let channels = next.internal_channels;
            let collapse = next.enter_frame(channels, rate);
            let plan = FramePlan {
                kind: Kind::Loss,
                rate,
                duration,
                channels,
                index: 0,
                vad: [false; 2],
                redundant: [false; 2],
                previous_redundant: [false; 2],
                prediction: next.stereo.previous_prediction(),
                mid_only: false,
                collapse,
            };
            next.frame(None, plan, &mut result)?;
            remaining -= if duration == FrameDuration::Ms10 {
                10
            } else {
                20
            };
        }
        *self = next;
        Ok(result)
    }

    fn empty_output(&self) -> DecodedPcm {
        DecodedPcm {
            samples: [0; 11520],
            length: 0,
            rate: self.output_rate(),
            channels: self.output_channels,
            range: 0,
        }
    }
    fn enter_frame(&mut self, channels: usize, rate: SilkRate) -> bool {
        let collapse = channels == 1 && self.internal_channels == 2 && self.rate == Some(rate);
        if channels > self.internal_channels {
            self.channel[1].reset();
        }
        if self.output_channels == PcmChannels::Stereo
            && channels == 2
            && self.internal_channels == 1
        {
            self.stereo.enter_stereo();
            self.resampler[1] = self.resampler[0].clone();
        }
        self.internal_channels = channels;
        self.rate = Some(rate);
        collapse
    }
    fn packet(&mut self, data: &[u8], fec: bool) -> Result<DecodedPcm, DecoderError> {
        let packet = Packet::parse(data).map_err(DecoderError::Framing)?;
        if packet.mode() != Mode::Silk {
            return Err(DecoderError::UnsupportedMode);
        }
        let rate = match packet.bandwidth() {
            Bandwidth::Narrowband => SilkRate::Khz8,
            Bandwidth::Mediumband => SilkRate::Khz12,
            _ => SilkRate::Khz16,
        };
        let channels = usize::from(packet.channels());
        let frame_count = if fec { 1 } else { packet.frames().len() };
        if packet.frames()[..frame_count]
            .iter()
            .any(|frame| frame.len() <= 1)
        {
            return Err(DecoderError::MissingPayload);
        }
        let mut next = self.clone();
        let mut output = self.empty_output();
        for (frame_index, payload) in packet.frames()[..frame_count].iter().enumerate() {
            let (header, mut entropy) =
                decode_header(&packet, frame_index).map_err(DecoderError::Header)?;
            entropy.allow_silk_zero_extension();
            let duration = if header.frame_duration_ms() == 10 {
                FrameDuration::Ms10
            } else {
                FrameDuration::Ms20
            };
            if !fec {
                skip_lbrr(&mut entropy, rate, duration, &header)?;
            }
            for index in 0..header.frame_count() {
                let collapse = next.enter_frame(channels, rate);
                let FrameFlags {
                    vad,
                    redundant,
                    previous: previous_redundant,
                } = frame_flags(&header, index)?;
                let mut prediction = next.stereo.previous_prediction();
                let mut mid_only = false;
                if channels == 2 && (!fec || redundant[0]) {
                    prediction = StereoPrediction::decode(&mut entropy)?;
                    if if fec { !redundant[1] } else { !vad[1] } {
                        mid_only = decode_mid_only(&mut entropy)?;
                    }
                }
                let plan = FramePlan {
                    kind: if fec { Kind::Fec } else { Kind::Normal },
                    rate,
                    duration,
                    channels,
                    index,
                    vad,
                    redundant,
                    previous_redundant,
                    prediction,
                    mid_only,
                    collapse,
                };
                next.frame(Some(&mut entropy), plan, &mut output)?;
            }
            if !fec && entropy.tell() + 17 <= payload.len() as u32 * 8 {
                return Err(DecoderError::UnsupportedTransition);
            }
            output.range = entropy.range();
        }
        next.last_duration = if packet.frame_samples() == 480 {
            FrameDuration::Ms10
        } else {
            FrameDuration::Ms20
        };
        *self = next;
        Ok(output)
    }
    fn frame(
        &mut self,
        mut entropy: Option<&mut RangeDecoder<'_>>,
        plan: FramePlan,
        output: &mut DecodedPcm,
    ) -> Result<(), DecoderError> {
        let has_side = if plan.kind == Kind::Normal {
            !plan.mid_only
        } else {
            !self.previous_mid_only || (plan.kind == Kind::Fec && plan.redundant[1])
        };
        if plan.channels == 2 && !plan.mid_only && self.previous_mid_only {
            self.channel[1].restart_side();
        }
        let length = plan.rate.khz() as usize
            * if plan.duration == FrameDuration::Ms10 {
                10
            } else {
                20
            };
        let mut pcm = [
            PcmFrame {
                samples: [0; 320],
                length,
                rate: plan.rate,
            },
            PcmFrame {
                samples: [0; 320],
                length,
                rate: plan.rate,
            },
        ];
        for (channel, samples) in pcm.iter_mut().enumerate().take(plan.channels) {
            if channel == 1 && !has_side {
                if plan.kind == Kind::Loss {
                    self.channel[channel].clear_loss_gain_clamp();
                }
                continue;
            }
            if plan.kind == Kind::Normal || (plan.kind == Kind::Fec && plan.redundant[channel]) {
                let coding = if plan.index == 0 {
                    PitchCoding::Independent
                } else if plan.kind == Kind::Fec {
                    if plan.previous_redundant[channel] {
                        PitchCoding::Conditional
                    } else {
                        PitchCoding::Independent
                    }
                } else if channel == 1 && self.previous_mid_only {
                    PitchCoding::IndependentWithoutScaling
                } else {
                    PitchCoding::Conditional
                };
                *samples = self.channel[channel].decode(
                    entropy.as_deref_mut().ok_or(DecoderError::MissingPayload)?,
                    plan.rate,
                    plan.duration,
                    if plan.kind == Kind::Fec {
                        FrameActivity::Redundant
                    } else if plan.vad[channel] {
                        FrameActivity::Active
                    } else {
                        FrameActivity::Inactive
                    },
                    coding,
                )?;
            } else if plan.kind == Kind::Loss {
                *samples = self.channel[channel].conceal(plan.rate, plan.duration)?;
            } else {
                *samples = self.channel[channel].conceal_fec_gap(plan.rate, plan.duration)?;
            }
        }
        let mut left = [0i16; 960];
        let mut right = [0i16; 960];
        let count = if self.output_channels == PcmChannels::Stereo && plan.channels == 2 {
            let stereo = self.stereo.reconstruct(
                plan.rate,
                pcm[0].samples(),
                pcm[1].samples(),
                plan.prediction,
            )?;
            let count = self.resampler[0].process(plan.rate, stereo.left(), &mut left)?;
            self.resampler[1].process(plan.rate, stereo.right(), &mut right)?;
            count
        } else {
            let buffered = self.stereo.buffer_mono(pcm[0].samples());
            let count = self.resampler[0].process(plan.rate, &buffered[..length], &mut left)?;
            if self.output_channels == PcmChannels::Stereo {
                if plan.collapse {
                    self.resampler[1].process(plan.rate, &buffered[..length], &mut right)?;
                } else {
                    right[..count].copy_from_slice(&left[..count]);
                }
            }
            count
        };
        let channels = self.output_channels.count();
        // Framing/loss validation bounds the total to 120 ms: at most 11520
        // interleaved samples at 48 kHz stereo. Each frame is at most 20 ms.
        for i in 0..count {
            output.samples[output.length + channels * i] = left[i];
            if channels == 2 {
                output.samples[output.length + channels * i + 1] = right[i];
            }
        }
        output.length += count * channels;
        if plan.kind != Kind::Loss {
            self.previous_mid_only = plan.mid_only;
        }
        Ok(())
    }
}

fn frame_flags(header: &FrameHeader, index: usize) -> Result<FrameFlags, DecoderError> {
    let mut vad = [false; 2];
    let mut redundant = [false; 2];
    let mut previous = [false; 2];
    for channel in 0..header.channel_count() {
        vad[channel] = header
            .voice_activity(channel)
            .ok_or(DecoderError::MissingPayload)?[index];
        let flags = header
            .redundancy(channel)
            .ok_or(DecoderError::MissingPayload)?;
        redundant[channel] = flags[index];
        previous[channel] = index > 0 && flags[index - 1];
    }
    Ok(FrameFlags {
        vad,
        redundant,
        previous,
    })
}
fn skip_lbrr(
    entropy: &mut RangeDecoder<'_>,
    rate: SilkRate,
    duration: FrameDuration,
    header: &FrameHeader,
) -> Result<(), DecoderError> {
    let mut state: [RedundancyState; 2] = std::array::from_fn(|_| RedundancyState::default());
    for index in 0..header.frame_count() {
        let FrameFlags {
            redundant: flags,
            previous,
            ..
        } = frame_flags(header, index)?;
        for channel in 0..header.channel_count() {
            if flags[channel] {
                if header.channel_count() == 2 && channel == 0 {
                    StereoPrediction::decode(entropy)?;
                    if !flags[1] {
                        decode_mid_only(entropy)?;
                    }
                }
                state[channel].consume(entropy, rate, duration, previous[channel])?;
            }
        }
    }
    Ok(())
}
