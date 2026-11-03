//! A single PCM, speech and transform owner. Native transition processing is
//! performed on these live histories, never on independently primed mode banks.
use super::{FrontendError, control, pcm::PcmState, stereo_width::StereoWidthState};
use crate::{
    Bandwidth, Mode,
    celt::{
        FrameSize,
        encoder::{CeltEncoder, NativeControls},
        input::InputRate,
    },
    hybrid::encoder::{HybridFrameEncoder, OuterControls, RateAllocation, RateControls, fade},
    range::{EncoderError as EntropyError, RangeEncoder},
    repacketizer::{Repacketizer, RepacketizerError},
    silk::{
        encoder::{
            analysis::VoiceActivityDetector,
            bandwidth::{BandwidthControls, BandwidthError, PacketBandwidth},
            channel::{ChannelFrameEncoder, FrameError},
            dtx::Decision as DtxDecision,
            mono::{MonoOptions, PacketDuration},
            packet::{self, PacketControls, PayloadPolicy},
            state::{AnalysisControls, SpeechState, StereoFrameError},
            stereo::StereoPacketError,
        },
        gains::FrameDuration,
        pitch::SilkRate,
        resampler::{EncoderResamplerState, OutputRate, ResampleError},
    },
};
use std::fmt;

/// Failure of a unified packet transaction. Every error preserves the stream.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum EncoderError {
    /// This application or frame configuration is not implemented yet.
    Unsupported,
    /// Invalid frontend input or controls.
    Frontend(FrontendError),
    /// Speech bandwidth control failed.
    Bandwidth(BandwidthError),
    /// Input conversion failed.
    Resampler(ResampleError),
    /// Speech frame preparation failed.
    Speech(FrameError),
    /// Speech packet coding failed.
    Silk(StereoPacketError),
    /// Shared low/high-band coding failed.
    Hybrid(crate::hybrid::encoder::EncoderError),
    /// Transform coding failed.
    Celt(crate::celt::encoder::EncoderError),
    /// Entropy termination or capacity failed.
    Entropy(EntropyError),
    /// Packet padding failed.
    Framing(RepacketizerError),
}
impl fmt::Display for EncoderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported => f.write_str("unsupported unified encoder application or duration"),
            Self::Frontend(e) => e.fmt(f),
            Self::Bandwidth(e) => e.fmt(f),
            Self::Resampler(e) => e.fmt(f),
            Self::Speech(e) => e.fmt(f),
            Self::Silk(e) => e.fmt(f),
            Self::Hybrid(e) => e.fmt(f),
            Self::Celt(e) => e.fmt(f),
            Self::Entropy(e) => e.fmt(f),
            Self::Framing(e) => e.fmt(f),
        }
    }
}
impl std::error::Error for EncoderError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Unsupported => None,
            Self::Frontend(e) => Some(e),
            Self::Bandwidth(e) => Some(e),
            Self::Resampler(e) => Some(e),
            Self::Speech(e) => Some(e),
            Self::Silk(e) => Some(e),
            Self::Hybrid(e) => Some(e),
            Self::Celt(e) => Some(e),
            Self::Entropy(e) => Some(e),
            Self::Framing(e) => Some(e),
        }
    }
}
macro_rules! error_from {
    ($source:ty, $variant:ident) => {
        impl From<$source> for EncoderError {
            fn from(value: $source) -> Self {
                Self::$variant(value)
            }
        }
    };
}
error_from!(FrontendError, Frontend);
error_from!(BandwidthError, Bandwidth);
error_from!(ResampleError, Resampler);
error_from!(FrameError, Speech);
error_from!(StereoPacketError, Silk);
error_from!(crate::hybrid::encoder::EncoderError, Hybrid);
error_from!(crate::celt::encoder::EncoderError, Celt);
error_from!(EntropyError, Entropy);
error_from!(RepacketizerError, Framing);
impl From<StereoFrameError> for EncoderError {
    fn from(value: StereoFrameError) -> Self {
        crate::hybrid::encoder::EncoderError::from(value).into()
    }
}

/// Nominal rate and scalar native selection controls for one packet.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EncodeOptions {
    /// TOC-inclusive target rate, at most1500000bps; physical capacity caps it.
    pub bitrate: u32,
    /// Scalar application, signal, mode, layout, bandwidth, loss and DTX policy.
    /// Currently only AUDIO is accepted by the unified owner.
    pub controls: control::Controls,
    /// Enable CELT's constrained VBR feedback when variable rate is selected.
    pub constrained: bool,
}
impl Default for EncodeOptions {
    fn default() -> Self {
        Self {
            bitrate: 64000,
            controls: control::Controls::default(),
            constrained: false,
        }
    }
}
/// A complete single-frame packet produced by the common owner.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EncodedPacket {
    bytes: Vec<u8>,
    range: u32,
    decision: control::Decision,
    tail_bytes: usize,
}
impl EncodedPacket {
    /// Complete Opus packet, including TOC and any transition tail or padding.
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }
    /// Primary entropy range XOR the redundant transform range.
    pub fn final_range(&self) -> u32 {
        self.range
    }
    /// Actual primary mode, wire channels and bandwidth selected for this call.
    pub fn decision(&self) -> control::Decision {
        self.decision
    }
}

/// Common AUDIO packet encoder with one live input/speech/CELT history.
///
/// This initial API accepts2.5/5/10/20ms frames at standard rates. Short frames
/// select CELT. VOIP/restricted applications and multi-frame packet scheduling
/// are not yet supported. Normal AUDIO lookahead is6.5ms. Whole-stream staging
/// makes input, coding, termination and transition failures atomic.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OpusEncoder {
    rate: InputRate,
    channels: usize,
    pcm: PcmState,
    speech: SpeechState<2>,
    bandwidth: PacketBandwidth<2>,
    celt: CeltEncoder,
    high_gain: i16,
    width: i16,
    stereo_width: StereoWidthState,
    history: control::History,
    internal: SilkRate,
    speech_started: bool,
    speech_channels: usize,
    can_switch: bool,
    bandwidth_prefill: bool,
}
impl OpusEncoder {
    /// Create a mono/stereo AUDIO stream at a standard input rate.
    pub fn new(rate: InputRate, channels: usize) -> Result<Self, EncoderError> {
        let history = control::History::new(channels)?;
        let input = output_rate(rate);
        Ok(Self {
            rate,
            channels,
            pcm: if channels == 1 {
                PcmState::mono(input)
            } else {
                PcmState::stereo(input)
            },
            speech: SpeechState::new(input, SilkRate::Khz16),
            bandwidth: PacketBandwidth::default(),
            celt: CeltEncoder::new(rate, channels)?,
            high_gain: 32767,
            width: 16384,
            stereo_width: StereoWidthState::default(),
            history,
            internal: SilkRate::Khz16,
            speech_started: false,
            speech_channels: 0,
            can_switch: false,
            bandwidth_prefill: false,
        })
    }
    /// Clear all audio and selection histories, retaining immutable API geometry.
    pub fn reset(&mut self) -> Result<(), EncoderError> {
        *self = Self::new(self.rate, self.channels)?;
        Ok(())
    }
    /// Encode interleaved signed16 PCM. The TOC-inclusive cap is3..=1276 bytes.
    /// CBR rounds the nominal target to whole bytes and fills that packet size.
    /// Unsupported applications and every coding failure preserve all state.
    pub fn encode(
        &mut self,
        size: FrameSize,
        input: &[i16],
        packet_bytes: usize,
        options: EncodeOptions,
    ) -> Result<EncodedPacket, EncoderError> {
        if input.len() != size.samples() / self.rate.factor() * self.channels {
            return Err(FrontendError::InvalidGeometry.into());
        }
        if options.controls.application != control::Application::Audio {
            return Err(EncoderError::Unsupported);
        }
        if !(3..=1276).contains(&packet_bytes) || !(1..=1500000).contains(&options.bitrate) {
            return Err(FrontendError::InvalidControl.into());
        }
        let mut next = self.clone();
        let result = match next.encode_inner(size, input, packet_bytes, options, true) {
            Err(error)
                if budget_error(&error) && self.speech.fec.has_pending(packet_duration(size)) =>
            {
                next = self.clone();
                next.encode_inner(size, input, packet_bytes, options, false)?
            }
            other => other?,
        };
        *self = next;
        Ok(result)
    }
    fn frame_encoder(&mut self, decision: control::Decision) -> HybridFrameEncoder<'_> {
        HybridFrameEncoder {
            rate: self.rate,
            channels: decision.channels,
            api_channels: self.channels,
            bandwidth: decision.bandwidth,
            pcm: &mut self.pcm,
            speech: &mut self.speech,
            high_gain: &mut self.high_gain,
            width: &mut self.width,
            celt: &mut self.celt,
        }
    }
    fn encode_inner(
        &mut self,
        size: FrameSize,
        input: &[i16],
        cap: usize,
        options: EncodeOptions,
        include_fec: bool,
    ) -> Result<EncodedPacket, EncoderError> {
        #[cfg(test)]
        self.celt.call_trace.clear();
        let fps = (400 >> size.lm()) as u32;
        let mut bitrate = options.bitrate.min(cap as u32 * 8 * fps);
        let cap = if options.controls.variable {
            cap
        } else {
            let bytes = ((bitrate + 4 * fps) / (8 * fps)).max(1) as usize;
            bitrate = bytes as u32 * 8 * fps;
            bytes.min(cap)
        };
        let silence = input.iter().all(|&v| v == 0);
        let width = if self.channels == 2 {
            self.stereo_width
                .analyze(self.rate, size.samples(), input)?
        } else {
            0
        };
        self.history.allow_bandwidth_switch = self.bandwidth.allowed();
        self.history.wideband_ready =
            self.internal == SilkRate::Khz16 && self.bandwidth.transitions[0].direction() == 0;
        let mut decision = control::select(
            control::Frame {
                rate: self.rate,
                channels: self.channels,
                samples48: size.samples(),
                bitrate,
                packet_bytes: cap,
                digital_silence: silence,
                stereo_width_q15: width,
            },
            options.controls,
            self.history,
        )?;
        let requested_bandwidth = decision.bandwidth;
        let mut filtered = vec![0; input.len()];
        let mut aligned = vec![0; input.len()];
        self.pcm.filter(input, &mut filtered)?;
        self.pcm.aligned(&filtered, &mut aligned)?;
        let mut tail = if decision.redundancy || self.bandwidth_prefill {
            redundancy_bytes(cap, bitrate, fps, decision.channels)
        } else {
            0
        };
        let mut incoming = decision.celt_to_silk || self.bandwidth_prefill;
        if decision.mode == Mode::Celt {
            tail = 0;
        }
        let bits_target = (8 * (cap - tail))
            .min((bitrate / fps) as usize)
            .saturating_sub(8) as u32;
        let duration = frame_duration(size);
        let packet_duration = packet_duration(size);
        let mut writer;
        let mut dtx = DtxDecision::Transmit;
        let mut hybrid = None;
        self.celt.set_native_controls(NativeControls {
            complexity: options.controls.complexity,
            loss_percent: options.controls.loss_percent,
            prediction: self.celt.native_prediction(),
        })?;
        self.speech.dtx.enabled = options.controls.dtx;
        self.speech.fec.enabled = decision.fec;
        if decision.mode != Mode::Celt {
            let desired = silk_rate(decision.bandwidth);
            if decision.prefill_silk {
                self.speech.reset(output_rate(self.rate), desired);
                self.bandwidth = PacketBandwidth::default();
                self.speech_started = false;
                self.speech_channels = 0;
            }
            let maximum_bits = ((cap - 1) * 8).saturating_sub(if tail > 0 {
                tail * 8 + 1 + if decision.mode == Mode::Hybrid { 20 } else { 0 }
            } else {
                0
            }) as u32;
            let internal = self.bandwidth.prepare_outer(
                BandwidthControls {
                    current: self.speech_started.then_some(self.internal),
                    input: output_rate(self.rate),
                    minimum: if decision.mode == Mode::Hybrid {
                        SilkRate::Khz16
                    } else {
                        SilkRate::Khz8
                    },
                    maximum: SilkRate::Khz16,
                    desired,
                    allow_switch: self.bandwidth.allowed(),
                    can_switch: self.can_switch,
                    duration: packet_duration,
                    max_bits: maximum_bits,
                },
                decision.channels,
            )?;
            if !self
                .speech
                .has_rates(output_rate(self.rate), internal, decision.channels)
            {
                self.speech.change_rate(output_rate(self.rate), internal)?;
            }
            self.internal = internal;
            self.speech
                .begin_channels(self.speech_channels.max(1), decision.channels, internal);
            if decision.prefill_silk || self.bandwidth_prefill {
                let rate = if decision.mode == Mode::Hybrid {
                    RateAllocation::with_maximum(
                        RateControls {
                            duration,
                            bandwidth: decision.bandwidth,
                            channels: decision.channels,
                            bitrate: bits_target * fps,
                            payload_bytes: cap - 1,
                            variable_bitrate: options.controls.variable,
                            fec: decision.fec,
                        },
                        maximum_bits,
                    )
                    .map_err(crate::hybrid::encoder::EncoderError::Rate)?
                    .silk_bitrate()
                } else {
                    bits_target * fps
                };
                self.prefill_speech(decision, rate, silence)?;
            }
            self.bandwidth_prefill = false;
            self.speech.dtx.begin(silence);
            if decision.mode == Mode::Hybrid {
                let previous_channels = self.speech_channels.max(1);
                let low = self.frame_encoder(decision).prepare_filtered(
                    duration,
                    &filtered,
                    cap,
                    crate::hybrid::encoder::Options {
                        complexity: options.controls.complexity,
                        packet_loss_percent: options.controls.loss_percent,
                    },
                    OuterControls {
                        previous_channels,
                        to_mono: decision.to_mono,
                        digital_silence: silence,
                        fec: decision.fec,
                        payload_rate: bits_target * fps,
                        maximum_bits,
                        bitrate: options.controls.variable.then_some(bitrate),
                        include_fec,
                    },
                )?;
                self.speech_channels = decision.channels;
                dtx = low.decision;
                hybrid = Some(low.controls);
                let gain =
                    (32767 - (crate::celt::math::exp2((-low.high_band_rate) as i16) >> 1)) as i16;
                // Fades are applied after unmodified input has been retained below.
                writer = low.writer;
                self.bandwidth.finish(self.speech.activity, packet_duration);
                if dtx != DtxDecision::Silk {
                    fade(
                        &mut aligned,
                        self.rate,
                        self.channels,
                        self.high_gain,
                        gain,
                        false,
                    );
                    self.high_gain = gain;
                }
            } else {
                let mut pcm = [[0; 960]; 2];
                self.convert_speech(
                    &filtered,
                    packet_duration,
                    decision.channels,
                    self.speech_channels.max(1),
                    &mut pcm,
                )?;
                let prepared = packet::prepare_stereo(
                    &mut self.speech,
                    &mut self.bandwidth,
                    internal,
                    &pcm,
                    PacketControls {
                        duration: packet_duration,
                        packet_bytes: cap,
                        policy: Some(PayloadPolicy {
                            rate: (bits_target * fps) as i32,
                            fec: decision.fec,
                        }),
                        options: MonoOptions {
                            complexity: options.controls.complexity,
                            variable_bitrate: options.controls.variable,
                            packet_loss_percent: options.controls.loss_percent,
                        },
                        digital_silence: silence,
                        include_fec,
                        channels: decision.channels,
                        to_mono: decision.to_mono,
                    },
                )?;
                writer = prepared.writer;
                dtx = prepared.decision;
                decision.bandwidth = silk_band(internal);
            }
            self.speech_started = true;
            self.can_switch = self.bandwidth.ready();
            if dtx == DtxDecision::Silk {
                self.history.bandwidth = requested_bandwidth;
                self.history.channels = decision.channels;
                self.history.auto_bandwidth = decision.auto_bandwidth;
                self.history.fec = decision.fec;
                self.history.to_mono = decision.to_mono;
                return Ok(EncodedPacket {
                    bytes: vec![toc(decision, size)],
                    range: 0,
                    decision,
                    tail_bytes: 0,
                });
            }
            if self.can_switch {
                tail = redundancy_bytes(cap, bitrate, fps, decision.channels);
                incoming = false;
                self.bandwidth_prefill = tail > 0;
            }
        } else {
            writer = RangeEncoder::new(cap - 1)?;
        }
        // Native early SILK DTX returns before changing CELT wire/band or
        // primary prediction. User complexity/loss controls above still apply.
        self.celt.set_encoded_channels(decision.channels)?;
        self.celt.set_bandwidth(celt_band(decision.bandwidth))?;
        if decision.mode != Mode::Silk {
            self.celt.set_native_controls(NativeControls {
                complexity: options.controls.complexity,
                loss_percent: options.controls.loss_percent,
                prediction: 2,
            })?;
        }
        let n4 = self.rate.hz() as usize / 400 * self.channels;
        let old = self.pcm.history();
        let prefill = old[old.len() - self.rate.hz() as usize / 250 * self.channels - n4
            ..old.len() - self.rate.hz() as usize / 250 * self.channels]
            .to_vec();
        self.pcm.commit(&filtered)?;
        if decision.mode != Mode::Hybrid {
            fade(
                &mut aligned,
                self.rate,
                self.channels,
                self.high_gain,
                32767,
                false,
            );
            self.high_gain = 32767;
        }
        if self.channels == 2 {
            let width = if decision.mode == Mode::Hybrid && decision.channels == 2 {
                if decision.to_mono {
                    0
                } else {
                    self.speech.stereo.smoothed_width()
                }
            } else {
                stereo_target(decision.equivalent_rate)
            };
            fade(
                &mut aligned,
                self.rate,
                2,
                width_q15(self.width),
                width_q15(width),
                true,
            );
            self.width = width;
        }
        if decision.mode != Mode::Celt
            && writer.tell() + 17 + if decision.mode == Mode::Hybrid { 20 } else { 0 }
                <= (cap - 1) as u32 * 8
        {
            if decision.mode == Mode::Hybrid {
                writer.encode_bit_logp(tail > 0, 12)?;
            }
            if tail > 0 {
                writer.encode_bit_logp(incoming, 1)?;
                let reserve = if decision.mode == Mode::Hybrid { 11 } else { 0 };
                let maximum =
                    (cap - 1).saturating_sub((writer.tell() + reserve).div_ceil(8) as usize);
                tail = tail.min(maximum).clamp(2, 257);
                if decision.mode == Mode::Hybrid {
                    writer.encode_uint((tail - 2) as u32, 256)?;
                }
            }
        } else {
            tail = 0;
        }
        if tail == 0 {
            self.bandwidth_prefill = false;
        }
        let mut redundant = Vec::new();
        let mut redundant_range = 0;
        if tail > 0 && incoming {
            let n2 = self.rate.hz() as usize / 200 * self.channels;
            let packet = self
                .celt
                .encode_stream(FrameSize::Ms5, &aligned[..n2], tail, None)?;
            redundant.extend_from_slice(&packet.as_bytes()[1..]);
            redundant_range = packet.final_range();
            self.celt.reset();
        }
        if decision.mode != Mode::Silk
            && self
                .history
                .previous_mode
                .is_some_and(|mode| mode != decision.mode)
        {
            self.celt.reset();
            if let Some(controls) = hybrid {
                self.celt.prefill_high_band(&prefill, controls)?;
            } else {
                self.celt.encode_stream(
                    FrameSize::Ms2_5,
                    &prefill,
                    2,
                    options
                        .controls
                        .variable
                        .then_some((bitrate, options.constrained)),
                )?;
            }
            self.celt.set_native_controls(NativeControls {
                complexity: options.controls.complexity,
                loss_percent: options.controls.loss_percent,
                prediction: 0,
            })?;
        }
        let mut bytes = vec![toc(decision, size)];
        let range = if decision.mode == Mode::Celt {
            let packet = self.celt.encode_stream(
                size,
                &aligned,
                cap - 1,
                options
                    .controls
                    .variable
                    .then_some((bitrate, options.constrained)),
            )?;
            bytes = packet.as_bytes().to_vec();
            packet.final_range()
        } else {
            if let Some(controls) = hybrid {
                writer.shrink(cap - 1 - tail)?;
                self.celt
                    .encode_high_band(size, &aligned, &mut writer, controls)?;
            } else {
                writer.shrink(writer.tell().div_ceil(8) as usize)?;
            }
            let range = writer.range();
            bytes.extend_from_slice(writer.finish()?);
            range
        };
        if tail > 0 && !incoming {
            self.celt.reset();
            self.celt.set_native_controls(NativeControls {
                complexity: options.controls.complexity,
                loss_percent: options.controls.loss_percent,
                prediction: 0,
            })?;
            let n2 = 2 * n4;
            self.celt.encode_stream(
                FrameSize::Ms2_5,
                &aligned[aligned.len() - n2 - n4..aligned.len() - n2],
                2,
                None,
            )?;
            let packet = self.celt.encode_stream(
                FrameSize::Ms5,
                &aligned[aligned.len() - n2..],
                tail,
                None,
            )?;
            redundant.extend_from_slice(&packet.as_bytes()[1..]);
            redundant_range = packet.final_range();
        }
        bytes.extend_from_slice(&redundant);
        self.history = control::History {
            first: false,
            previous_mode: Some(if decision.to_celt {
                Mode::Celt
            } else {
                decision.mode
            }),
            previous_channels: decision.channels,
            channels: decision.channels,
            bandwidth: requested_bandwidth,
            auto_bandwidth: decision.auto_bandwidth,
            allow_bandwidth_switch: self.bandwidth.allowed(),
            wideband_ready: self.internal == SilkRate::Khz16
                && self.bandwidth.transitions[0].direction() == 0,
            fec: decision.fec,
            to_mono: decision.to_mono,
        };
        if decision.mode == Mode::Celt {
            dtx = self
                .speech
                .dtx
                .finish_outer((size.samples() / 24) as u16, silence);
        }
        let range = if dtx == DtxDecision::DigitalSilence {
            bytes.truncate(1);
            self.speech.fec.suppress();
            0
        } else {
            range ^ redundant_range
        };
        if bytes.len() > cap {
            return Err(EntropyError::CapacityExhausted.into());
        }
        if !options.controls.variable && bytes.len() > 1 && bytes.len() < cap {
            let mut padded = vec![0; cap];
            let mut packet = Repacketizer::default();
            packet.add_packet(&bytes)?;
            packet.write_padded(&mut padded, cap)?;
            bytes = padded;
        }
        Ok(EncodedPacket {
            bytes,
            range,
            decision,
            tail_bytes: tail,
        })
    }
    fn convert_speech(
        &mut self,
        input: &[i16],
        duration: PacketDuration,
        channels: usize,
        previous: usize,
        output: &mut [[i16; 960]; 2],
    ) -> Result<(), EncoderError> {
        if self.channels == 2 {
            self.speech.resample_stereo_input(
                input,
                output,
                output_rate(self.rate),
                duration,
                channels,
                previous,
            )?;
        } else {
            self.speech.resampler[0].process(input, &mut output[0])?;
        }
        self.speech_channels = channels;
        Ok(())
    }
    fn prefill_speech(
        &mut self,
        decision: control::Decision,
        rate: u32,
        digital_silence: bool,
    ) -> Result<(), EncoderError> {
        if self.bandwidth_prefill {
            for c in 0..decision.channels {
                self.speech.channels[c] = ChannelFrameEncoder::new(self.internal);
                self.speech.vad[c] = VoiceActivityDetector::default();
                self.speech.resampler[c] =
                    EncoderResamplerState::new(output_rate(self.rate), self.internal);
            }
            self.speech.dtx.reset_speech();
            self.speech.fec.clear_pending();
        }
        let offset = self.rate.hz() as usize * 7 / 2000 * self.channels;
        let n4 = self.rate.hz() as usize / 400 * self.channels;
        let history = self.pcm.history_mut();
        history[..offset].fill(0);
        fade(
            &mut history[offset..offset + n4],
            self.rate,
            self.channels,
            0,
            32767,
            false,
        );
        let input = history.to_vec();
        let mut pcm = [[0; 960]; 2];
        self.convert_speech(
            &input,
            PacketDuration::Ms10,
            decision.channels,
            decision.channels,
            &mut pcm,
        )?;
        let n = match self.internal {
            SilkRate::Khz8 => 80,
            SilkRate::Khz12 => 120,
            SilkRate::Khz16 => 160,
        };
        let controls = AnalysisControls {
            native_activity: true,
            target_rate: (rate as i32 - self.speech.excess * 2)
                .clamp((rate as i32).min(5000), (rate as i32).max(5000)),
            digital_silence,
            to_mono: decision.to_mono,
        };
        let mut frame = if decision.channels == 2 {
            self.speech.analyze_stereo(
                self.internal,
                FrameDuration::Ms10,
                [&pcm[0][..n], &pcm[1][..n]],
                controls,
            )?
        } else {
            self.speech
                .analyze_mono(self.internal, FrameDuration::Ms10, &pcm[0][..n], controls)?
        };
        for c in 0..decision.channels {
            if frame.activities[c].is_some() {
                self.bandwidth.transitions[c].filter(
                    self.internal,
                    FrameDuration::Ms10,
                    &mut frame.pcm[c][..n],
                )?;
                self.speech.channels[c].prefill(FrameDuration::Ms10, &frame.pcm[c][..n])?;
            }
        }
        self.speech.finish_analysis(&frame)?;
        Ok(())
    }
}
fn output_rate(rate: InputRate) -> OutputRate {
    match rate {
        InputRate::Khz8 => OutputRate::Khz8,
        InputRate::Khz12 => OutputRate::Khz12,
        InputRate::Khz16 => OutputRate::Khz16,
        InputRate::Khz24 => OutputRate::Khz24,
        InputRate::Khz48 => OutputRate::Khz48,
    }
}
fn packet_duration(size: FrameSize) -> PacketDuration {
    if size == FrameSize::Ms10 {
        PacketDuration::Ms10
    } else {
        PacketDuration::Ms20
    }
}
fn frame_duration(size: FrameSize) -> FrameDuration {
    if size == FrameSize::Ms10 {
        FrameDuration::Ms10
    } else {
        FrameDuration::Ms20
    }
}
fn silk_rate(band: Bandwidth) -> SilkRate {
    match band {
        Bandwidth::Narrowband => SilkRate::Khz8,
        Bandwidth::Mediumband => SilkRate::Khz12,
        _ => SilkRate::Khz16,
    }
}
fn silk_band(rate: SilkRate) -> Bandwidth {
    match rate {
        SilkRate::Khz8 => Bandwidth::Narrowband,
        SilkRate::Khz12 => Bandwidth::Mediumband,
        SilkRate::Khz16 => Bandwidth::Wideband,
    }
}
fn celt_band(band: Bandwidth) -> Bandwidth {
    if band == Bandwidth::Mediumband {
        Bandwidth::Wideband
    } else {
        band
    }
}
fn width_q15(width: i16) -> i16 {
    if width == 16384 { 32767 } else { width * 2 }
}
fn stereo_target(rate: i32) -> i16 {
    if rate > 32000 {
        16384
    } else if rate < 16000 {
        0
    } else {
        (16384 - 2048 * (32000 - rate) / (rate - 14000)) as i16
    }
}
fn redundancy_bytes(cap: usize, bitrate: u32, fps: u32, channels: usize) -> usize {
    let base = 40 * channels as i32 + 20;
    let rate = (bitrate as i32 + base * (200 - fps as i32)) * 3 / 2;
    let maximum = ((cap as i32 * 8 - 2 * base) * 240 / (240 + 48000 / fps as i32) + base) / 8;
    let bytes = (rate / 1600).min(maximum);
    if bytes > 4 + 8 * channels as i32 {
        bytes.min(257) as usize
    } else {
        0
    }
}
fn toc(decision: control::Decision, size: FrameSize) -> u8 {
    let config = match decision.mode {
        Mode::Silk => {
            (match decision.bandwidth {
                Bandwidth::Narrowband => 0,
                Bandwidth::Mediumband => 4,
                _ => 8,
            }) + u8::from(size != FrameSize::Ms10)
        }
        Mode::Hybrid => {
            (if decision.bandwidth == Bandwidth::SuperWideband {
                12
            } else {
                14
            }) + u8::from(size != FrameSize::Ms10)
        }
        Mode::Celt => {
            (match decision.bandwidth {
                Bandwidth::Narrowband => 16,
                Bandwidth::SuperWideband => 24,
                Bandwidth::Fullband => 28,
                _ => 20,
            }) + size.lm() as u8
        }
    };
    (config << 3) | if decision.channels == 2 { 4 } else { 0 }
}
fn budget_error(error: &EncoderError) -> bool {
    let mut source: Option<&(dyn std::error::Error + 'static)> = Some(error);
    while let Some(error) = source {
        if error.downcast_ref::<EntropyError>() == Some(&EntropyError::CapacityExhausted)
            || error.downcast_ref::<FrameError>() == Some(&FrameError::BitBudgetExceeded)
            || error.downcast_ref::<crate::celt::encoder::EncoderError>()
                == Some(&crate::celt::encoder::EncoderError::BudgetExhausted)
        {
            return true;
        }
        source = error.source();
    }
    false
}

#[cfg(test)]
#[path = "stream_tests.rs"]
mod tests;
