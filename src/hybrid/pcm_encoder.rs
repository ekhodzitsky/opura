//! Connected fixed-band Hybrid PCM encoding, sharing one unfinished writer.
use super::{RateAllocation, RateControls, RateError};
use crate::{
    Bandwidth,
    celt::{
        FrameSize,
        encoder::{CeltEncoder, EncoderError as CeltError, HybridControls},
        input::InputRate,
    },
    range::{EncoderError as EntropyError, RangeEncoder},
    silk::{
        encoder::{
            analysis::VoiceActivityDetector,
            channel::{ChannelFrameEncoder, FrameControls, FrameError},
            dtx::{Decision, DtxState},
            mono::{PacketDuration, dc_reject, target_snr},
            stereo::{StereoAnalysisState, StereoControls, StereoEncodeError},
        },
        gains::{FrameDuration, SignalType},
        pitch::{PitchCoding, SilkRate},
        resampler::{EncoderResamplerState, OutputRate, ResampleError},
    },
};
use std::fmt;

/// Hybrid input, analysis, entropy or finalization failure. Errors preserve the
/// complete encoder state and the immutable caller's PCM.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum EncoderError {
    /// Only24/48kHz PCM, one/two channels and an attainable SWB/FB are supported.
    InvalidConfiguration,
    /// PCM length does not match frame duration, input rate and channel count.
    InvalidInput,
    /// Packet bytes must be3..=1276, complexity0..=10 and loss0..=100.
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
            Self::InvalidOptions => f.write_str("invalid Hybrid byte budget/complexity/loss"),
            Self::Rate(e) => write!(f, "Hybrid rate: {e}"),
            Self::Resampler(e) => write!(f, "Hybrid resampler: {e}"),
            Self::Silk(e) => write!(f, "Hybrid SILK: {e}"),
            Self::Stereo(e) => write!(f, "Hybrid stereo: {e}"),
            Self::Celt(e) => write!(f, "Hybrid CELT: {e}"),
            Self::Entropy(e) => write!(f, "Hybrid entropy: {e}"),
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

/// Fixed-band CBR Hybrid settings. Enable silence suppression separately with
/// [`HybridEncoder::set_dtx`]; loss percentage controls LTP/intra policy, not FEC.
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
/// DTX. FEC, cross-mode switching and general rate/channel selection remain unfinished.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HybridEncoder {
    rate: InputRate,
    bandwidth: Bandwidth,
    channels: usize,
    resamplers: [EncoderResamplerState; 2],
    dc: [i32; 2],
    previous: i16,
    vad: [VoiceActivityDetector; 2],
    silk: [ChannelFrameEncoder; 2],
    stereo: StereoAnalysisState,
    previous_mid_only: bool,
    activity: u8,
    excess: i32,
    delay: [i16; 384],
    high_gain: i16,
    width: i16,
    celt: CeltEncoder,
    dtx: DtxState,
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
            resamplers: std::array::from_fn(|_| EncoderResamplerState::new(input, SilkRate::Khz16)),
            dc: [0; 2],
            previous: 0,
            vad: std::array::from_fn(|_| VoiceActivityDetector::default()),
            silk: std::array::from_fn(|_| ChannelFrameEncoder::new(SilkRate::Khz16)),
            stereo: StereoAnalysisState::default(),
            previous_mid_only: false,
            activity: 0,
            excess: 0,
            delay: [0; 384],
            high_gain: 32767,
            width: 16384,
            celt,
            dtx: DtxState::default(),
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
        self.dtx.enabled = enabled;
    }
    /// Whether the last successful packet suppressed its payload.
    pub fn in_dtx(&self) -> bool {
        self.dtx.suppressed
    }
    /// Clear both layers, filters, delays and bitrate histories, retaining DTX.
    pub fn reset(&mut self) {
        let dtx = self.dtx.enabled;
        self.dtx = DtxState::default();
        self.dtx.enabled = dtx;
        self.resamplers = std::array::from_fn(|_| {
            EncoderResamplerState::new(output_rate(self.rate), SilkRate::Khz16)
        });
        self.dc = [0; 2];
        self.previous = 0;
        self.vad = std::array::from_fn(|_| VoiceActivityDetector::default());
        self.silk = std::array::from_fn(|_| ChannelFrameEncoder::new(SilkRate::Khz16));
        self.stereo.reset();
        self.previous_mid_only = false;
        self.activity = 0;
        self.excess = 0;
        self.delay = [0; 384];
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
        let mut next = self.clone();
        let result = next.encode_inner(duration, input, packet_bytes, options, finish)?;
        *self = next;
        Ok(result)
    }
    fn encode_inner(
        &mut self,
        duration: FrameDuration,
        input: &[i16],
        packet_bytes: usize,
        options: Options,
        finish: impl for<'a> FnOnce(&'a mut RangeEncoder) -> Result<&'a [u8], EntropyError>,
    ) -> Result<EncodedPacket, EncoderError> {
        let short = duration == FrameDuration::Ms10;
        let fps = if short { 100 } else { 50 };
        let size = if short {
            FrameSize::Ms10
        } else {
            FrameSize::Ms20
        };
        let samples = input.len() / self.channels;
        let internal = if short { 160 } else { 320 };
        let payload_bytes = packet_bytes - 1;
        let digital_silence = input.iter().all(|&v| v == 0);
        self.dtx.begin(digital_silence);
        let allocation = RateAllocation::new(RateControls {
            duration,
            bandwidth: self.bandwidth,
            channels: self.channels,
            bitrate: payload_bytes as u32 * 8 * fps,
            payload_bytes,
            variable_bitrate: false,
            fec: false,
        })
        .map_err(EncoderError::Rate)?;
        let mut filtered = [0i16; 1920];
        let mut pcm = [[0i16; 320]; 2];
        for (c, converted) in pcm.iter_mut().enumerate().take(self.channels) {
            let mut planar = [0i16; 960];
            let mut rejected = [0i16; 960];
            for i in 0..samples {
                planar[i] = input[i * self.channels + c];
            }
            dc_reject(
                &mut self.dc[c],
                output_rate(self.rate),
                &planar[..samples],
                &mut rejected[..samples],
            );
            for i in 0..samples {
                filtered[i * self.channels + c] = rejected[i];
            }
            self.resamplers[c]
                .process(&rejected[..samples], converted)
                .map_err(EncoderError::Resampler)?;
        }
        let mut writer = RangeEncoder::new(payload_bytes).map_err(EncoderError::Entropy)?;
        let header_bits = 2 * self.channels as u32;
        writer
            .encode_icdf(0, &[(256 - (256 >> header_bits)) as u8, 0], 8)
            .map_err(EncoderError::Entropy)?;
        let target = (allocation.silk_bitrate() as i32 - self.excess * 2).clamp(
            5000.min(allocation.silk_bitrate() as i32),
            5000.max(allocation.silk_bitrate() as i32),
        );
        let mut rates = [target, 0];
        let stereo = if self.channels == 2 {
            Some(
                self.stereo
                    .analyze(
                        SilkRate::Khz16,
                        duration,
                        &pcm[0][..internal],
                        &pcm[1][..internal],
                        StereoControls {
                            total_rate_bps: target,
                            previous_activity_q8: u16::from(self.activity),
                            to_mono: false,
                        },
                    )
                    .map_err(EncoderError::Stereo)?,
            )
        } else {
            None
        };
        if let Some(s) = &stereo {
            pcm[0][..internal].copy_from_slice(s.mid());
            pcm[1][..internal].copy_from_slice(s.side());
            rates = *s.rates_bps();
            if !s.mid_only() && self.previous_mid_only {
                self.silk[1].restart_prediction();
            }
        } else {
            for value in &mut pcm[0][..internal] {
                std::mem::swap(value, &mut self.previous);
            }
        }
        let mid_only = stereo.as_ref().is_some_and(|s| s.mid_only());
        let mut activities = [None; 2];
        for c in 0..self.channels {
            if c == 1 && mid_only {
                continue;
            }
            activities[c] = Some(
                self.vad[c]
                    .analyze(SilkRate::Khz16, duration, &pcm[c][..internal])
                    .map_err(|e| EncoderError::Silk(FrameError::from(e)))?,
            );
        }
        for (c, activity) in activities.iter_mut().enumerate().take(self.channels) {
            if let Some(activity) = activity {
                if self.dtx.enabled && digital_silence {
                    activity.probability_q8 = activity.probability_q8.min(12);
                }
                self.dtx
                    .frame(c, activity.probability_q8 >= 13 && !digital_silence);
            }
        }
        if let Some(s) = &stereo {
            s.encode_prefix(
                &mut writer,
                activities[1].is_some_and(|a| a.probability_q8 >= 13),
            )
            .map_err(EncoderError::Stereo)?;
        }
        let mut flags = 0;
        let mut voiced = false;
        for c in 0..self.channels {
            let active = activities[c].is_some_and(|a| a.probability_q8 >= 13);
            flags = (flags << 2) | (u32::from(active) << 1);
            let Some(activity) = activities[c] else {
                continue;
            };
            let maximum = allocation.silk_max_bits();
            let cap = if c == 0 && self.channels == 2 && rates[1] > 0 {
                maximum - maximum / 2
            } else {
                maximum
            };
            let encoded = self.silk[c]
                .encode(
                    &mut writer,
                    &pcm[c][..internal],
                    FrameControls {
                        duration,
                        complexity: options.complexity,
                        snr_db_q7: target_snr(SilkRate::Khz16, duration, rates[c]),
                        activity,
                        active,
                        constant_bitrate: false,
                        fill_budget: false,
                        coding: PitchCoding::Independent,
                        max_bits: cap,
                        packet_loss_percent: options.packet_loss_percent,
                        frames_per_packet: 1,
                        redundancy: false,
                    },
                )
                .map_err(EncoderError::Silk)?;
            if c == 0 {
                voiced = encoded.indices().signal == SignalType::Voiced;
                self.activity = activity.probability_q8;
            }
        }
        self.previous_mid_only = mid_only;
        writer
            .patch_initial_bits(flags, header_bits)
            .map_err(EncoderError::Entropy)?;
        let decision = self.dtx.finish(
            if short {
                PacketDuration::Ms10
            } else {
                PacketDuration::Ms20
            },
            self.channels,
            digital_silence,
        );
        let silk_bits = if decision == Decision::Silk {
            0
        } else {
            writer.tell().div_ceil(8) as i32 * 8
        };
        self.excess = (self.excess + silk_bits - allocation.silk_bitrate() as i32 / fps as i32)
            .clamp(0, 10000);
        // Native SILK DTX returns before the outer delay copy, HB fades and
        // CELT call. Digital-silence DTX instead runs both layers to completion.
        if decision == Decision::Silk {
            return Ok(self.suppressed_packet(duration));
        }
        if writer.tell() + 37 <= payload_bytes as u32 * 8 {
            writer
                .encode_bit_logp(false, 12)
                .map_err(EncoderError::Entropy)?;
        }
        let delay = self.rate.hz() as usize / 250 * self.channels;
        let mut high = [0i16; 1920];
        high[..delay].copy_from_slice(&self.delay[..delay]);
        high[delay..input.len()].copy_from_slice(&filtered[..input.len() - delay]);
        self.delay[..delay].copy_from_slice(&filtered[input.len() - delay..input.len()]);
        // Preserve native fixed-point narrowing before the exponential and at
        // the Q15 result, including forced-mode extreme-rate behavior.
        let gain = (32767
            - (crate::celt::math::exp2((-allocation.high_band_bitrate()) as i16) >> 1))
            as i16;
        fade(
            &mut high[..input.len()],
            self.rate,
            self.channels,
            self.high_gain,
            gain,
            false,
        );
        self.high_gain = gain;
        if self.channels == 2 {
            let width = self.stereo.smoothed_width();
            fade(
                &mut high[..input.len()],
                self.rate,
                2,
                width_q15(self.width),
                width_q15(width),
                true,
            );
            self.width = width;
        }
        self.celt.set_transient_detection(options.complexity >= 1);
        self.celt.set_dynamic_allocation(true);
        self.celt.set_allocation_analysis(true);
        self.celt
            .encode_high_band(
                size,
                &high[..input.len()],
                &mut writer,
                HybridControls {
                    bandwidth: self.bandwidth,
                    complexity: options.complexity,
                    voiced,
                    loss_percent: options.packet_loss_percent,
                },
            )
            .map_err(EncoderError::Celt)?;
        let range = writer.range();
        let payload = finish(&mut writer).map_err(EncoderError::Entropy)?;
        if decision == Decision::DigitalSilence {
            return Ok(self.suppressed_packet(duration));
        }
        let config = if self.bandwidth == Bandwidth::SuperWideband {
            12
        } else {
            14
        } + u8::from(!short);
        let mut packet = EncodedPacket {
            bytes: [0; 1276],
            length: packet_bytes,
            range,
            duration,
        };
        packet.bytes[0] = (config << 3) | if self.channels == 2 { 4 } else { 0 };
        packet.bytes[1..packet_bytes].copy_from_slice(payload);
        Ok(packet)
    }
    fn suppressed_packet(&self, duration: FrameDuration) -> EncodedPacket {
        let config = if self.bandwidth == Bandwidth::SuperWideband {
            12
        } else {
            14
        } + u8::from(duration == FrameDuration::Ms20);
        let mut packet = EncodedPacket {
            bytes: [0; 1276],
            length: 1,
            range: 0,
            duration,
        };
        packet.bytes[0] = (config << 3) | if self.channels == 2 { 4 } else { 0 };
        packet
    }
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
