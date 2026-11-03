//! A single PCM, speech and transform owner. Native transition processing is
//! performed on these live histories, never on independently primed mode banks.
//! Budget/framing policy adapted from Opus1.6.1 opus_encoder.c.
//! Copyright (c)2010-2011 Xiph.Org Foundation, Skype Limited; see THIRD_PARTY_NOTICES.md.
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

/// Standard complete packet duration on the 48kHz clock.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PacketSize {
    /// 2.5ms transform frame.
    Ms2_5,
    /// 5ms transform frame.
    Ms5,
    /// 10ms packet.
    Ms10,
    /// 20ms packet.
    Ms20,
    /// 40ms packet.
    Ms40,
    /// 60ms packet.
    Ms60,
    /// 80ms packet.
    Ms80,
    /// 100ms packet.
    Ms100,
    /// 120ms packet.
    Ms120,
}
impl PacketSize {
    /// Duration in samples/channel at48kHz.
    pub const fn samples(self) -> usize {
        match self {
            Self::Ms2_5 => 120,
            Self::Ms5 => 240,
            Self::Ms10 => 480,
            Self::Ms20 => 960,
            Self::Ms40 => 1920,
            Self::Ms60 => 2880,
            Self::Ms80 => 3840,
            Self::Ms100 => 4800,
            Self::Ms120 => 5760,
        }
    }
}
impl From<FrameSize> for PacketSize {
    fn from(size: FrameSize) -> Self {
        match size {
            FrameSize::Ms2_5 => Self::Ms2_5,
            FrameSize::Ms5 => Self::Ms5,
            FrameSize::Ms10 => Self::Ms10,
            FrameSize::Ms20 => Self::Ms20,
        }
    }
}
impl TryFrom<usize> for PacketSize {
    type Error = FrontendError;
    fn try_from(samples: usize) -> Result<Self, Self::Error> {
        match samples {
            120 => Ok(Self::Ms2_5),
            240 => Ok(Self::Ms5),
            480 => Ok(Self::Ms10),
            960 => Ok(Self::Ms20),
            1920 => Ok(Self::Ms40),
            2880 => Ok(Self::Ms60),
            3840 => Ok(Self::Ms80),
            4800 => Ok(Self::Ms100),
            5760 => Ok(Self::Ms120),
            _ => Err(FrontendError::InvalidGeometry),
        }
    }
}
#[derive(Clone, Copy)]
struct PlannedFrame {
    samples: usize,
    cap: usize,
    bitrate: u32,
    decision: control::Decision,
    nonfinal: bool,
    reset_silk: bool,
}

/// Failure of a unified packet transaction. Every error preserves the stream.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum EncoderError {
    /// The application request disagrees with immutable stream configuration.
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
    /// Positive target rate up to1500000bps, clamped to at least500bps total
    /// and at most750000bps per API channel before physical/CBR normalization.
    pub bitrate: u32,
    /// Scalar application, signal, mode, layout, bandwidth, loss and DTX policy.
    /// Must match the immutable application selected at construction.
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
/// A complete Opus packet produced by the common owner.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EncodedPacket {
    bytes: Vec<u8>,
    range: u32,
    decision: control::Decision,
    tail_bytes: usize,
    plc: bool,
}
impl EncodedPacket {
    /// Complete Opus packet, including TOC and any transition tail or padding.
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }
    /// Final component primary range XOR its redundant transform range.
    pub fn final_range(&self) -> u32 {
        self.range
    }
    /// At least one component deliberately signals native loss concealment.
    pub fn used_plc(&self) -> bool {
        self.plc
    }
    /// Actual primary mode, wire channels and bandwidth selected for this call.
    pub fn decision(&self) -> control::Decision {
        self.decision
    }
}

/// Common Opus packet encoder with one live input/speech/CELT history.
///
/// Accepts2.5..120ms packets at standard rates. Short frames select CELT.
/// All five applications are supported; restricted SILK requires at least10ms.
/// Whole-stream staging makes input, coding, termination, repacketization and
/// transition failures atomic. Long packets select policy once for all components.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OpusEncoder {
    rate: InputRate,
    channels: usize,
    application: control::Application,
    hp_smooth: i32,
    pcm: PcmState,
    speech: SpeechState<2>,
    bandwidth: PacketBandwidth<2>,
    celt: CeltEncoder,
    high_gain: i16,
    width: i16,
    stereo_width: StereoWidthState,
    history: control::History,
    selected: Option<control::Decision>,
    internal: SilkRate,
    speech_started: bool,
    speech_channels: usize,
    can_switch: bool,
    bandwidth_prefill: bool,
}
impl OpusEncoder {
    /// Create a mono/stereo AUDIO stream at a standard input rate.
    pub fn new(rate: InputRate, channels: usize) -> Result<Self, EncoderError> {
        Self::with_application(rate, channels, control::Application::Audio)
    }
    /// Create a stream with an immutable native application.
    pub fn with_application(
        rate: InputRate,
        channels: usize,
        application: control::Application,
    ) -> Result<Self, EncoderError> {
        let history = control::History::new(channels)?;
        let input = output_rate(rate);
        Ok(Self {
            rate,
            channels,
            application,
            hp_smooth: crate::silk::encoder::highpass::initial_smoothing(),
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
            selected: None,
            internal: SilkRate::Khz16,
            speech_started: false,
            speech_channels: 0,
            can_switch: false,
            bandwidth_prefill: false,
        })
    }
    /// Clear all audio and selection histories, retaining immutable API geometry.
    pub fn reset(&mut self) -> Result<(), EncoderError> {
        *self = Self::with_application(self.rate, self.channels, self.application)?;
        Ok(())
    }
    /// Immutable application; per-frame controls must agree with this value.
    pub fn application(&self) -> control::Application {
        self.application
    }
    /// Native lookahead on the48kHz clock:120 for restricted CELT/low-delay,
    ///312 for AUDIO, VOIP and restricted SILK.
    pub fn lookahead_48k(&self) -> u32 {
        if matches!(
            self.application,
            control::Application::RestrictedLowDelay | control::Application::RestrictedCelt
        ) {
            120
        } else {
            312
        }
    }
    /// Encode interleaved signed16 PCM. The TOC-inclusive cap is1..=1276 bytes.
    /// CBR rounds the nominal target to whole bytes and fills that packet size.
    /// Unsupported applications and every coding failure preserve all state.
    pub fn encode(
        &mut self,
        size: FrameSize,
        input: &[i16],
        packet_bytes: usize,
        options: EncodeOptions,
    ) -> Result<EncodedPacket, EncoderError> {
        self.encode_packet(size.into(), input, packet_bytes, options)
    }
    /// Encode a complete2.5..120ms packet with one mode/layout selection.
    /// Caps are1..1276 for short packets and1..7662 for longer packets.
    /// A one-byte100ms packet cannot be represented. Insufficient useful audio
    /// budgets emit native PLC; every error preserves all histories.
    pub fn encode_packet(
        &mut self,
        size: PacketSize,
        input: &[i16],
        packet_bytes: usize,
        options: EncodeOptions,
    ) -> Result<EncodedPacket, EncoderError> {
        if input.len() != size.samples() / self.rate.factor() * self.channels {
            return Err(FrontendError::InvalidGeometry.into());
        }
        if options.controls.application != self.application {
            return Err(EncoderError::Unsupported);
        }
        let maximum = if size.samples() > 960 { 7662 } else { 1276 };
        if !(1..=maximum).contains(&packet_bytes) || !(1..=1500000).contains(&options.bitrate) {
            return Err(FrontendError::InvalidControl.into());
        }
        let mut next = self.clone();
        let result = next.encode_inner(size, input, packet_bytes, options)?;
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
        size: PacketSize,
        input: &[i16],
        cap: usize,
        options: EncodeOptions,
    ) -> Result<EncodedPacket, EncoderError> {
        #[cfg(test)]
        self.celt.call_trace.clear();
        let samples = size.samples();
        // The public native bitrate control clamps before physical admission.
        let original_cap = cap;
        if samples == 4800 && original_cap == 1 {
            return Err(FrontendError::InsufficientBudget.into());
        }
        let mut bitrate = options
            .bitrate
            .clamp(500, 750000 * self.channels as u32)
            .min((cap * 8 * 48000 / samples) as u32);
        let cap = if options.controls.variable {
            cap
        } else {
            let bytes = ((bits_for(bitrate, samples) + 4) / 8).max(1) as usize;
            bitrate = (bytes * 8 * 48000 / samples) as u32;
            bytes.min(cap)
        };
        let silence = input.iter().all(|&v| v == 0);
        let width = if self.channels == 2 && options.controls.channels != Some(1) {
            self.stereo_width
                .analyze(self.rate, size.samples(), input)?
        } else {
            0
        };
        let history = control::History {
            allow_bandwidth_switch: self.bandwidth.allowed(),
            wideband_ready: self.internal == SilkRate::Khz16
                && self.bandwidth.transitions[0].direction() == 0,
            ..self.history
        };
        let mut decision = match control::select(
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
            history,
        ) {
            Ok(decision) => decision,
            Err(FrontendError::InsufficientBudget) => {
                return self.admission_plc(samples, original_cap, cap, options.controls.variable);
            }
            Err(error) => return Err(error.into()),
        };
        self.history = history;
        self.selected = Some(decision);
        let unit = if decision.mode == Mode::Silk {
            match samples {
                3840 => 1920,
                5760 => 2880,
                4800 => 960,
                _ => samples,
            }
        } else {
            samples.min(960)
        };
        let count = samples / unit;
        if count == 1 {
            return self.encode_component(
                input,
                options,
                PlannedFrame {
                    samples: unit,
                    cap,
                    bitrate,
                    decision,
                    nonfinal: false,
                    reset_silk: decision.prefill_silk,
                },
            );
        }
        let header = if count == 2 { 3 } else { 2 + (count - 1) * 2 };
        let maximum_sum = count + cap.saturating_sub(header);
        let component_cap = (bits_for(bitrate, unit) as usize / 8).min(maximum_sum / count);
        if component_cap < 3 {
            return Err(FrontendError::InsufficientBudget.into());
        }
        let to_mono = decision.to_mono;
        decision.to_mono = false;
        let mut packets = Vec::with_capacity(count);
        let mut total = 0;
        let mut tail_bytes = 0;
        let mut final_range = 0;
        for (index, pcm) in input
            .chunks_exact(unit / self.rate.factor() * self.channels)
            .enumerate()
        {
            let last = index + 1 == count;
            let component = control::Decision {
                to_celt: decision.to_celt && last,
                redundancy: decision.redundancy
                    && (if decision.to_celt { last } else { index == 0 }),
                ..decision
            };
            let packet = self.encode_component(
                pcm,
                options,
                PlannedFrame {
                    samples: unit,
                    cap: component_cap.min(maximum_sum - total),
                    bitrate,
                    decision: component,
                    nonfinal: !last,
                    reset_silk: decision.prefill_silk && index == 0,
                },
            )?;
            total += packet.bytes.len();
            tail_bytes += packet.tail_bytes;
            final_range = packet.range;
            packets.push(packet);
        }
        self.history.to_mono = to_mono;
        let mut repacketizer = Repacketizer::new();
        for packet in &packets {
            repacketizer.add_packet(&packet.bytes)?;
        }
        let mut bytes = vec![0; cap];
        let length = if !options.controls.variable && packets.iter().any(|p| p.bytes.len() != 1) {
            repacketizer.write_padded(&mut bytes, cap)?
        } else {
            repacketizer.write(&mut bytes)?
        };
        bytes.truncate(length);
        Ok(EncodedPacket {
            bytes,
            range: final_range,
            decision: control::Decision {
                to_mono,
                bandwidth: packets[0].decision.bandwidth,
                ..decision
            },
            tail_bytes,
            plc: packets.iter().any(|p| p.plc),
        })
    }
    fn admission_plc(
        &self,
        samples: usize,
        original_cap: usize,
        cap: usize,
        variable: bool,
    ) -> Result<EncodedPacket, EncoderError> {
        let mut decision = self.selected.unwrap_or(control::Decision {
            mode: Mode::Hybrid,
            channels: self.channels,
            bandwidth: Bandwidth::Fullband,
            auto_bandwidth: None,
            fec: false,
            to_mono: false,
            redundancy: false,
            celt_to_silk: false,
            to_celt: false,
            prefill_silk: false,
            equivalent_rate: 0,
            voice_q7: 0,
            silk_dtx: false,
        });
        let mut fps = 48000 / samples;
        let mut code = 0;
        let mut count = 0;
        if fps > 100 {
            decision.mode = Mode::Celt;
        }
        if fps == 25 && decision.mode != Mode::Silk {
            fps = 50;
            code = 1;
        }
        if fps <= 16 {
            if original_cap == 1 || (decision.mode == Mode::Silk && fps != 10) {
                decision.mode = Mode::Silk;
                code = usize::from(fps <= 12);
                fps = if fps == 12 { 25 } else { 16 };
            } else {
                count = 50 / fps;
                fps = 50;
                code = 3;
            }
        }
        decision.bandwidth = match (decision.mode, decision.bandwidth) {
            (Mode::Silk, Bandwidth::SuperWideband | Bandwidth::Fullband) => Bandwidth::Wideband,
            (Mode::Celt, Bandwidth::Mediumband) => Bandwidth::Narrowband,
            (Mode::Hybrid, Bandwidth::Narrowband | Bandwidth::Mediumband | Bandwidth::Wideband) => {
                Bandwidth::SuperWideband
            }
            (_, band) => band,
        };
        let mut period = 0;
        while fps < 400 {
            fps *= 2;
            period += 1;
        }
        let band: u8 = match decision.bandwidth {
            Bandwidth::Narrowband => 0,
            Bandwidth::Mediumband => 1,
            Bandwidth::Wideband => 2,
            Bandwidth::SuperWideband => 3,
            Bandwidth::Fullband => 4,
        };
        let byte = match decision.mode {
            Mode::Silk => (band << 5) | ((period - 2) << 3),
            Mode::Celt => 0x80 | (band.saturating_sub(1) << 5) | (period << 3),
            Mode::Hybrid => 0x60 | ((band - 3) << 4) | ((period - 2) << 3),
        };
        let mut bytes = vec![byte | if decision.channels == 2 { 4 } else { 0 } | code as u8];
        if code == 3 {
            bytes.push(count as u8);
        }
        if !variable && bytes.len() < cap {
            let mut padded = vec![0; cap];
            let mut rp = Repacketizer::new();
            rp.add_packet(&bytes)?;
            rp.write_padded(&mut padded, cap)?;
            bytes = padded;
        }
        if bytes.len() > original_cap {
            return Err(FrontendError::InsufficientBudget.into());
        }
        decision.redundancy = false;
        decision.celt_to_silk = false;
        decision.to_celt = false;
        decision.prefill_silk = false;
        decision.to_mono = false;
        Ok(EncodedPacket {
            bytes,
            range: 0,
            decision,
            tail_bytes: 0,
            plc: true,
        })
    }
    fn encode_component(
        &mut self,
        input: &[i16],
        options: EncodeOptions,
        plan: PlannedFrame,
    ) -> Result<EncodedPacket, EncoderError> {
        // Only a pending prefix can require the bounded prefix-free retry.
        let before = self
            .speech
            .fec
            .has_pending(packet_duration(plan.samples))
            .then(|| self.clone());
        match self.encode_frame(input, options, plan, true) {
            Err(error) if budget_error(&error) && before.is_some() => {
                if let Some(before) = before {
                    *self = before;
                }
                self.encode_frame(input, options, plan, false)
            }
            result => result,
        }
    }
    fn encode_frame(
        &mut self,
        input: &[i16],
        options: EncodeOptions,
        plan: PlannedFrame,
        include_fec: bool,
    ) -> Result<EncodedPacket, EncoderError> {
        let PlannedFrame {
            samples,
            cap,
            bitrate,
            mut decision,
            nonfinal,
            reset_silk,
        } = plan;
        let fps = (48000 / samples) as u32;
        let original_cap = cap;
        let cap = cap.min(1276);
        let silence = input.iter().all(|&v| v == 0);
        let requested_bandwidth = decision.bandwidth;
        let mut filtered = vec![0; input.len()];
        let mut aligned = vec![0; input.len()];
        let hp_first = if decision.mode == Mode::Celt || reset_silk {
            crate::silk::encoder::highpass::initial_smoothing()
        } else {
            self.speech.channels[0].cutoff.smooth
        };
        let cutoff = crate::silk::encoder::highpass::smooth_cutoff(&mut self.hp_smooth, hp_first);
        if self.application == control::Application::Voip {
            self.pcm.filter_voip(input, &mut filtered, cutoff)?;
        } else {
            self.pcm.filter(input, &mut filtered)?;
        }
        self.pcm.aligned_with_delay(
            &filtered,
            &mut aligned,
            matches!(
                self.application,
                control::Application::Audio | control::Application::Voip
            ),
        )?;
        let mut tail = if decision.redundancy || self.bandwidth_prefill {
            redundancy_bytes(cap, bitrate, fps, decision.channels)
        } else {
            0
        };
        let mut incoming = decision.celt_to_silk || self.bandwidth_prefill;
        if decision.mode == Mode::Celt {
            tail = 0;
        }
        if self.application == control::Application::RestrictedSilk {
            tail = 0;
        }
        let bits_target = (8 * (cap - tail))
            .min(bits_for(bitrate, samples) as usize)
            .saturating_sub(8) as u32;
        let duration = frame_duration(samples);
        let packet_duration = packet_duration(samples);
        let mut writer;
        let mut dtx = DtxDecision::Transmit;
        let mut hybrid = None;
        let mut completed_overflow = false;
        self.celt.set_native_controls(NativeControls {
            complexity: options.controls.complexity,
            loss_percent: options.controls.loss_percent,
            prediction: self.celt.native_prediction(),
        })?;
        self.speech.dtx.enabled = options.controls.dtx;
        self.speech.fec.enabled = decision.fec;
        if decision.mode != Mode::Celt {
            let maximum_rate = if decision.mode == Mode::Silk {
                let mut effective = rate_for(cap as u32 * 8, samples);
                if fps > 50 {
                    effective = effective * 2 / 3;
                }
                if effective < 7000 {
                    SilkRate::Khz8
                } else if effective < 8000 {
                    SilkRate::Khz12
                } else {
                    SilkRate::Khz16
                }
            } else {
                SilkRate::Khz16
            };
            let requested = silk_rate(decision.bandwidth);
            let desired = match (maximum_rate, requested) {
                (SilkRate::Khz8, _) => SilkRate::Khz8,
                (SilkRate::Khz12, SilkRate::Khz16) => SilkRate::Khz12,
                (_, rate) => rate,
            };
            if reset_silk {
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
                    maximum: maximum_rate,
                    desired,
                    allow_switch: self.bandwidth.allowed(),
                    can_switch: self.can_switch,
                    duration: packet_duration,
                    max_bits: maximum_bits,
                },
                decision.channels,
                self.speech_channels,
            )?;
            if !self
                .speech
                .has_rates(output_rate(self.rate), internal, decision.channels)
            {
                self.speech.change_rate_channels(
                    output_rate(self.rate),
                    internal,
                    decision.channels,
                )?;
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
                            bitrate: rate_for(bits_target, samples),
                            payload_bytes: cap - 1,
                            variable_bitrate: options.controls.variable,
                            fec: decision.fec,
                        },
                        maximum_bits,
                    )
                    .map_err(crate::hybrid::encoder::EncoderError::Rate)?
                    .silk_bitrate()
                } else {
                    rate_for(bits_target, samples)
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
                        payload_rate: rate_for(bits_target, samples),
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
                            rate: (rate_for(bits_target, samples)) as i32,
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
                completed_overflow = prepared.overflow;
                writer = prepared.writer;
                dtx = prepared.decision;
                decision.bandwidth = silk_band(internal);
            }
            self.speech_started = true;
            self.can_switch = self.bandwidth.ready() && !nonfinal;
            if nonfinal {
                self.bandwidth.defer_switch();
            }
            if dtx == DtxDecision::Silk {
                self.history.bandwidth = requested_bandwidth;
                self.history.channels = decision.channels;
                self.history.auto_bandwidth = decision.auto_bandwidth;
                self.history.fec = decision.fec;
                self.history.to_mono = decision.to_mono;
                return Ok(EncodedPacket {
                    bytes: vec![toc(decision, samples)],
                    range: 0,
                    decision,
                    tail_bytes: 0,
                    plc: false,
                });
            }
            if self.can_switch {
                tail = if self.application == control::Application::RestrictedSilk {
                    0
                } else {
                    redundancy_bytes(cap, bitrate, fps, decision.channels)
                };
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
        if !matches!(
            self.application,
            control::Application::RestrictedCelt | control::Application::RestrictedSilk
        ) {
            self.pcm.commit(&filtered)?;
        }
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
        let mut bytes = vec![toc(decision, samples)];
        let range = if decision.mode == Mode::Celt {
            let packet = self.celt.encode_stream(
                celt_size(samples)?,
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
                    .encode_high_band(celt_size(samples)?, &aligned, &mut writer, controls)?;
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
            dtx = self.speech.dtx.finish_outer((samples / 24) as u16, silence);
        }
        let range = if dtx == DtxDecision::DigitalSilence {
            bytes.truncate(1);
            self.speech.fec.suppress();
            0
        } else {
            range ^ redundant_range
        };
        let range = if completed_overflow && dtx == DtxDecision::Transmit {
            bytes.clear();
            bytes.extend_from_slice(&[toc(decision, samples), 0]);
            bytes.extend_from_slice(&redundant);
            0
        } else {
            range
        };
        if bytes.len() > cap {
            return Err(EntropyError::CapacityExhausted.into());
        }
        if !options.controls.variable && bytes.len() > 1 && bytes.len() < original_cap {
            let mut padded = vec![0; original_cap];
            let mut packet = Repacketizer::default();
            packet.add_packet(&bytes)?;
            packet.write_padded(&mut padded, original_cap)?;
            bytes = padded;
        }
        Ok(EncodedPacket {
            bytes,
            range,
            decision,
            tail_bytes: tail,
            plc: completed_overflow && dtx == DtxDecision::Transmit,
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
            // Match the actual SILK resampler call boundaries, including its
            // overwritten delay slots, for40/60ms speech components.
            let frames = duration.milliseconds().div_ceil(20) as usize;
            let mut offset = 0;
            for pcm in input.chunks_exact(input.len() / frames) {
                offset += self.speech.resampler[0].process(pcm, &mut output[0][offset..])?;
            }
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
        // Each native prefill call initializes the channel encoders, including
        // repeated prefills inside a long incoming packet. Outer stereo and
        // bit-reservoir history are retained; LP state survives prefill2.
        for c in 0..decision.channels {
            self.speech.channels[c] = ChannelFrameEncoder::new(self.internal);
            self.speech.vad[c] = VoiceActivityDetector::default();
            self.speech.resampler[c] =
                EncoderResamplerState::new(output_rate(self.rate), self.internal);
        }
        self.speech.dtx.reset_speech();
        self.speech.fec.clear_pending();
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
fn packet_duration(samples: usize) -> PacketDuration {
    match samples {
        480 => PacketDuration::Ms10,
        1920 => PacketDuration::Ms40,
        2880 => PacketDuration::Ms60,
        _ => PacketDuration::Ms20,
    }
}
fn celt_size(samples: usize) -> Result<FrameSize, EncoderError> {
    match samples {
        120 => Ok(FrameSize::Ms2_5),
        240 => Ok(FrameSize::Ms5),
        480 => Ok(FrameSize::Ms10),
        960 => Ok(FrameSize::Ms20),
        _ => Err(EncoderError::Unsupported),
    }
}
fn bits_for(rate: u32, samples: usize) -> u32 {
    (u64::from(rate) * samples as u64 / 48000) as u32
}
fn rate_for(bits: u32, samples: usize) -> u32 {
    (u64::from(bits) * 48000 / samples as u64) as u32
}
fn frame_duration(samples: usize) -> FrameDuration {
    if samples == 480 {
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
fn toc(decision: control::Decision, samples: usize) -> u8 {
    let config = match decision.mode {
        Mode::Silk => {
            (match decision.bandwidth {
                Bandwidth::Narrowband => 0,
                Bandwidth::Mediumband => 4,
                _ => 8,
            }) + match samples {
                480 => 0,
                1920 => 2,
                2880 => 3,
                _ => 1,
            }
        }
        Mode::Hybrid => {
            (if decision.bandwidth == Bandwidth::SuperWideband {
                12
            } else {
                14
            }) + match samples {
                480 => 0,
                1920 => 2,
                2880 => 3,
                _ => 1,
            }
        }
        Mode::Celt => {
            (match decision.bandwidth {
                Bandwidth::Narrowband => 16,
                Bandwidth::SuperWideband => 24,
                Bandwidth::Fullband => 28,
                _ => 20,
            }) + match samples {
                120 => 0,
                240 => 1,
                480 => 2,
                _ => 3,
            }
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
            || error.downcast_ref::<crate::celt::allocation::AllocationError>()
                == Some(&crate::celt::allocation::AllocationError::BitBudgetExceeded)
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
