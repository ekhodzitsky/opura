//! Mono PCM-to-Opus packets with fixed SILK bandwidth and explicit byte budgets.
use super::{
    analysis::VoiceActivityDetector,
    channel::{ChannelFrameEncoder, FrameControls, FrameError},
};
use crate::{
    range::{EncoderError, RangeEncoder},
    silk::{
        gains::FrameDuration,
        pitch::{PitchCoding, SilkRate},
        resampler::{EncoderResamplerState, OutputRate, ResampleError},
    },
};
use std::fmt;
#[path = "rate_tables.rs"]
mod tables;
/// Standard SILK Opus packet duration; 40/60 ms contain two/three 20 ms frames.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PacketDuration {
    /// One 10 ms internal frame.
    Ms10,
    /// One 20 ms internal frame.
    Ms20,
    /// Two 20 ms internal frames.
    Ms40,
    /// Three 20 ms internal frames.
    Ms60,
}
impl PacketDuration {
    /// Packet duration in milliseconds.
    pub fn milliseconds(self) -> u32 {
        match self {
            Self::Ms10 => 10,
            Self::Ms20 => 20,
            Self::Ms40 => 40,
            Self::Ms60 => 60,
        }
    }
    pub(super) fn frames(self) -> usize {
        match self {
            Self::Ms10 | Self::Ms20 => 1,
            Self::Ms40 => 2,
            Self::Ms60 => 3,
        }
    }
    pub(super) fn frame_duration(self) -> FrameDuration {
        if self == Self::Ms10 {
            FrameDuration::Ms10
        } else {
            FrameDuration::Ms20
        }
    }
}
/// Fixed-bandwidth mono encoder settings. Packet byte budgets are supplied per call.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MonoOptions {
    /// Native complexity, 0..=10.
    pub complexity: u8,
    /// Use variable output length, bounded by the supplied packet bytes.
    /// False fills the exact packet budget using transport-level Opus padding.
    pub variable_bitrate: bool,
    /// Loss percentage, 0..=100, for LTP state scaling (does not enable FEC).
    pub packet_loss_percent: u8,
}
impl Default for MonoOptions {
    fn default() -> Self {
        Self {
            complexity: 10,
            variable_bitrate: false,
            packet_loss_percent: 0,
        }
    }
}
/// Invalid input/configuration, resampling or packet budget failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum MonoError {
    /// PCM length must match rate and packet duration.
    InvalidInputLength,
    /// Byte budget must be 3..=1276, complexity 0..=10 and loss 0..=100.
    InvalidOptions,
    /// Encoder input conversion failed.
    Resampler(ResampleError),
    /// Channel PCM encoding failed.
    Frame(FrameError),
    /// Packet header/termination failed.
    Entropy(EncoderError),
}
impl fmt::Display for MonoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInputLength => {
                f.write_str("SILK mono input length does not match packet duration")
            }
            Self::InvalidOptions => f.write_str("invalid SILK mono options or packet budget"),
            Self::Resampler(e) => write!(f, "SILK input: {e}"),
            Self::Frame(e) => write!(f, "SILK packet: {e}"),
            Self::Entropy(e) => write!(f, "SILK packet entropy: {e}"),
        }
    }
}
impl std::error::Error for MonoError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Resampler(e) => Some(e),
            Self::Frame(e) => Some(e),
            Self::Entropy(e) => Some(e),
            _ => None,
        }
    }
}
impl From<ResampleError> for MonoError {
    fn from(e: ResampleError) -> Self {
        Self::Resampler(e)
    }
}
impl From<FrameError> for MonoError {
    fn from(e: FrameError) -> Self {
        Self::Frame(e)
    }
}
impl From<EncoderError> for MonoError {
    fn from(e: EncoderError) -> Self {
        Self::Entropy(e)
    }
}
/// Complete one-frame Opus packet (possibly containing multiple internal SILK frames).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MonoPacket {
    bytes: [u8; 1276],
    length: usize,
    range: u32,
}
impl MonoPacket {
    /// Opus TOC, compressed frame and optional packet-level padding.
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.length]
    }
    /// Final entropy range for cross-decoder validation.
    pub fn final_range(&self) -> u32 {
        self.range
    }
}
/// Fixed-bandwidth mono PCM encoder for all five external rates. Uses native
/// DC rejection, encoder resampling, VAD, SNR tables and packet bit reservoir.
/// Does not yet perform adaptive bandwidth switching, stereo, DTX or FEC.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MonoEncoder {
    input: OutputRate,
    internal: SilkRate,
    resampler: EncoderResamplerState,
    dc: i32,
    previous: i16,
    vad: VoiceActivityDetector,
    channel: ChannelFrameEncoder,
    excess: i32,
}
impl MonoEncoder {
    /// Start an independent mono stream at a fixed input rate/internal bandwidth.
    pub fn new(input: OutputRate, internal: SilkRate) -> Self {
        Self {
            input,
            internal,
            resampler: EncoderResamplerState::new(input, internal),
            dc: 0,
            previous: 0,
            vad: VoiceActivityDetector::default(),
            channel: ChannelFrameEncoder::new(internal),
            excess: 0,
        }
    }
    /// Nominal pre-skip on the 48 kHz Opus clock: 6.5 ms, including the native
    /// SILK input/resampling compensation and fixed channel lookahead.
    pub fn lookahead_48k(&self) -> u32 {
        312
    }
    /// Reset all input, VAD, predictor and bitrate history, retaining rates.
    pub fn reset(&mut self) {
        *self = Self::new(self.input, self.internal);
    }
    /// Encode exactly one duration of mono PCM. packet_bytes includes TOC and
    /// packet padding, excludes transport/container overhead, and is 3..=1276.
    /// VBR is a hard byte cap; CBR produces exactly packet_bytes. Any error
    /// preserves all input/analysis/quantizer and rate-control history.
    pub fn encode(
        &mut self,
        duration: PacketDuration,
        input: &[i16],
        packet_bytes: usize,
        options: MonoOptions,
    ) -> Result<MonoPacket, MonoError> {
        let ms = duration.milliseconds() as usize;
        let khz = self.input.hz() as usize / 1000;
        if input.len() != ms * khz {
            return Err(MonoError::InvalidInputLength);
        }
        if !(3..=1276).contains(&packet_bytes)
            || options.complexity > 10
            || options.packet_loss_percent > 100
        {
            return Err(MonoError::InvalidOptions);
        }
        let mut staged = self.clone();
        let mut filtered = [0i16; 2880];
        dc_reject(
            &mut staged.dc,
            self.input,
            input,
            &mut filtered[..input.len()],
        );
        let mut pcm = [0i16; 960];
        let converted = staged
            .resampler
            .process(&filtered[..input.len()], &mut pcm)?;
        for sample in &mut pcm[..converted] {
            std::mem::swap(sample, &mut staged.previous);
        }
        let frames = duration.frames();
        let frame_samples = converted / frames;
        let maximum = (packet_bytes as u32 - 1) * 8;
        let bitrate = maximum as i32 * 1000 / ms as i32;
        let nominal_bits = bitrate * ms as i32 / 1000;
        let per_frame = nominal_bits / frames as i32;
        let mut writer = RangeEncoder::new(1275)?;
        let header_bits = frames as u32 + 1;
        writer.encode_icdf(0, &[(256 - (256 >> header_bits)) as u8, 0], 8)?;
        let mut flags = 0;
        for (index, frame) in pcm[..converted].chunks_exact(frame_samples).enumerate() {
            let mut target = per_frame
                * if duration == PacketDuration::Ms10 {
                    100
                } else {
                    50
                }
                - staged.excess * 2;
            if index > 0 {
                target -= (writer.tell() as i32 - per_frame * index as i32) * 2;
            }
            target = target.clamp(bitrate.min(5000), bitrate.max(5000));
            let snr = target_snr(self.internal, duration.frame_duration(), target);
            let activity = staged
                .vad
                .analyze(self.internal, duration.frame_duration(), frame)
                .map_err(FrameError::from)?;
            let active = activity.probability_q8 >= 13;
            let limit = match (frames, index) {
                (2, 0) => maximum * 3 / 5,
                (3, 0) => maximum * 2 / 5,
                (3, 1) => maximum * 3 / 4,
                _ => maximum,
            };
            staged.channel.encode(
                &mut writer,
                frame,
                FrameControls {
                    duration: duration.frame_duration(),
                    complexity: options.complexity,
                    snr_db_q7: snr,
                    activity,
                    active,
                    constant_bitrate: !options.variable_bitrate,
                    fill_budget: !options.variable_bitrate && index + 1 == frames,
                    coding: if index == 0 {
                        PitchCoding::Independent
                    } else {
                        PitchCoding::Conditional
                    },
                    max_bits: limit,
                    packet_loss_percent: options.packet_loss_percent,
                    frames_per_packet: frames as u8,
                    redundancy: false,
                },
            )?;
            flags = (flags << 1) | u32::from(active);
        }
        writer.patch_initial_bits(flags << 1, header_bits)?;
        let payload = writer.tell().div_ceil(8) as usize;
        if payload >= packet_bytes {
            return Err(FrameError::BitBudgetExceeded.into());
        }
        writer.shrink(payload)?;
        let range = writer.range();
        let payload = writer.finish()?;
        let base = match self.internal {
            SilkRate::Khz8 => 0,
            SilkRate::Khz12 => 4,
            SilkRate::Khz16 => 8,
        };
        let config = base
            + match duration {
                PacketDuration::Ms10 => 0,
                PacketDuration::Ms20 => 1,
                PacketDuration::Ms40 => 2,
                PacketDuration::Ms60 => 3,
            };
        let result = packet(
            config << 3,
            payload,
            if options.variable_bitrate {
                payload.len() + 1
            } else {
                packet_bytes
            },
            range,
        );
        staged.excess = (staged.excess + payload.len() as i32 * 8 - nominal_bits).clamp(0, 10000);
        *self = staged;
        Ok(result)
    }
}
pub(super) fn target_snr(rate: SilkRate, duration: FrameDuration, mut bitrate: i32) -> u16 {
    if duration == FrameDuration::Ms10 {
        bitrate -= 2000 + rate.khz() / 16;
    }
    let table: &[u8] = match rate {
        SilkRate::Khz8 => &tables::NB,
        SilkRate::Khz12 => &tables::MB,
        SilkRate::Khz16 => &tables::WB,
    };
    let index = ((bitrate + 200) / 400 - 10).min(table.len() as i32 - 1);
    if index <= 0 {
        0
    } else {
        u16::from(table[index as usize]) * 21
    }
}
pub(super) fn packet(toc: u8, payload: &[u8], length: usize, range: u32) -> MonoPacket {
    let mut result = MonoPacket {
        bytes: [0; 1276],
        length,
        range,
    };
    let extra = length - payload.len() - 1;
    if extra == 0 {
        result.bytes[0] = toc;
        result.bytes[1..1 + payload.len()].copy_from_slice(payload);
        return result;
    }
    result.bytes[0] = toc | 3;
    result.bytes[1] = 1;
    let mut at = 2;
    if extra > 1 {
        result.bytes[1] |= 0x40;
        let padding = extra - 1;
        let full = (padding - 1) / 255;
        for _ in 0..full {
            result.bytes[at] = 255;
            at += 1;
        }
        result.bytes[at] = (padding - 255 * full - 1) as u8;
        at += 1;
    }
    result.bytes[at..at + payload.len()].copy_from_slice(payload);
    result
}

pub(super) fn dc_reject(state: &mut i32, rate: OutputRate, input: &[i16], output: &mut [i16]) {
    let shift = 31 - (rate.hz() / 12).leading_zeros();
    for (out, &sample) in output.iter_mut().zip(input) {
        let y = (i32::from(sample) << 14) - *state;
        *state += crate::silk::synthesis::round_shift(y, shift);
        *out = crate::silk::synthesis::round_shift(y, 14).clamp(-32767, 32767) as i16;
    }
}
#[cfg(test)]
#[path = "mono_tests.rs"]
mod tests;
