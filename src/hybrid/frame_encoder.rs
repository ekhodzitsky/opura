//! Hybrid packet preparation over borrowed, staged stream histories. The caller
//! owns staging, retries and commit, including discarding partial work on errors.
use super::{
    AnalysisControls, Bandwidth, CeltEncoder, Decision, EncodedPacket, EncoderError, EntropyError,
    FrameControls, FrameDuration, FrameSize, HybridControls, HybridVbr, InputRate, Options,
    PacketDuration, PcmState, PitchCoding, RangeEncoder, RateAllocation, RateControls, SignalType,
    SilkRate, SpeechState, fade, output_rate, protection, target_snr, width_q15,
};
use crate::hybrid::encoder::PreparedFrame;

pub(crate) struct LowBandFrame {
    pub writer: RangeEncoder,
    pub decision: Decision,
    filtered: [i16; 1920],
    length: usize,
    payload_bytes: usize,
    size: FrameSize,
    pub high_band_rate: i32,
    pub controls: HybridControls,
}
impl LowBandFrame {
    pub fn no_redundancy(&mut self) -> Result<(), EncoderError> {
        if self.decision != Decision::Silk
            && self.writer.tell() + 37 <= self.payload_bytes as u32 * 8
        {
            self.writer
                .encode_bit_logp(false, 12)
                .map_err(EncoderError::Entropy)?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy)]
pub(crate) struct OuterControls {
    pub previous_channels: usize,
    pub to_mono: bool,
    pub digital_silence: bool,
    pub fec: bool,
    pub payload_rate: u32,
    pub maximum_bits: u32,
    pub bitrate: Option<u32>,
    pub include_fec: bool,
}

struct Preparation {
    bitrate: Option<u32>,
    include_fec: bool,
    outer: Option<OuterControls>,
}

pub(crate) struct HybridFrameEncoder<'a> {
    pub rate: InputRate,
    pub bandwidth: Bandwidth,
    pub channels: usize,
    pub api_channels: usize,
    pub pcm: &'a mut PcmState,
    pub speech: &'a mut SpeechState<2>,
    pub high_gain: &'a mut i16,
    pub width: &'a mut i16,
    pub celt: &'a mut CeltEncoder,
}
impl HybridFrameEncoder<'_> {
    pub(crate) fn prepare_low_band(
        &mut self,
        duration: FrameDuration,
        input: &[i16],
        packet_bytes: usize,
        options: Options,
        bitrate: Option<u32>,
        include_fec: bool,
    ) -> Result<LowBandFrame, EncoderError> {
        self.prepare_low_band_with(
            duration,
            input,
            packet_bytes,
            options,
            Preparation {
                bitrate,
                include_fec,
                outer: None,
            },
        )
    }
    pub(crate) fn prepare_filtered(
        &mut self,
        duration: FrameDuration,
        input: &[i16],
        packet_bytes: usize,
        options: Options,
        outer: OuterControls,
    ) -> Result<LowBandFrame, EncoderError> {
        self.prepare_low_band_with(
            duration,
            input,
            packet_bytes,
            options,
            Preparation {
                bitrate: outer.bitrate,
                include_fec: outer.include_fec,
                outer: Some(outer),
            },
        )
    }
    fn prepare_low_band_with(
        &mut self,
        duration: FrameDuration,
        input: &[i16],
        packet_bytes: usize,
        options: Options,
        preparation: Preparation,
    ) -> Result<LowBandFrame, EncoderError> {
        let Preparation {
            bitrate,
            include_fec,
            outer,
        } = preparation;
        let short = duration == FrameDuration::Ms10;
        let fps = if short { 100 } else { 50 };
        if !(1..=2).contains(&self.api_channels)
            || !(1..=self.api_channels).contains(&self.channels)
            || (outer.is_none() && self.api_channels != self.channels)
            || outer.is_some_and(|o| !(1..=self.api_channels).contains(&o.previous_channels))
            || !matches!(self.rate, InputRate::Khz24 | InputRate::Khz48)
            || !matches!(
                self.bandwidth,
                Bandwidth::SuperWideband | Bandwidth::Fullband
            )
            || (self.rate == InputRate::Khz24 && self.bandwidth == Bandwidth::Fullband)
            || self.celt.input_rate() != self.rate
            || self.celt.channels() != self.api_channels
            || (outer.is_none() && self.celt.encoded_channels() != self.channels)
            || !self
                .pcm
                .has_geometry(output_rate(self.rate), self.api_channels)
            || !self
                .speech
                .has_rates(output_rate(self.rate), SilkRate::Khz16, self.channels)
        {
            return Err(EncoderError::InvalidConfiguration);
        }
        if input.len() != self.rate.hz() as usize / fps as usize * self.api_channels {
            return Err(EncoderError::InvalidInput);
        }
        if !(3..=1276).contains(&packet_bytes)
            || options.complexity > 10
            || options.packet_loss_percent > 100
            || bitrate.is_some_and(|rate| !(24 * fps as u32..=1024000).contains(&rate))
        {
            return Err(EncoderError::InvalidOptions);
        }
        let size = if short {
            FrameSize::Ms10
        } else {
            FrameSize::Ms20
        };
        let samples = input.len() / self.api_channels;
        let internal = if short { 160 } else { 320 };
        let payload_bytes = packet_bytes - 1;
        let stream_rate = bitrate
            .unwrap_or(packet_bytes as u32 * 8 * fps as u32)
            .min(packet_bytes as u32 * 8 * fps as u32);
        let payload_rate = outer.map_or((stream_rate / fps as u32 - 8) * fps as u32, |o| {
            o.payload_rate
        });
        let digital_silence =
            outer.map_or_else(|| input.iter().all(|&v| v == 0), |o| o.digital_silence);
        self.speech.dtx.begin(digital_silence);
        let packet_duration = if short {
            PacketDuration::Ms10
        } else {
            PacketDuration::Ms20
        };
        let selected = outer.map_or_else(
            || {
                protection::eligible(
                    self.speech.fec.enabled,
                    self.speech.fec.selected(),
                    self.bandwidth,
                    self.channels,
                    duration,
                    protection::FecRate {
                        bitrate: stream_rate,
                        variable: bitrate.is_some(),
                    },
                    options,
                )
            },
            |o| o.fec,
        );
        self.speech.fec.begin_with_policy(
            packet_duration,
            SilkRate::Khz16,
            payload_rate as i32,
            options.packet_loss_percent,
            Some(selected),
        );
        let allocation = RateAllocation::with_maximum(
            RateControls {
                duration,
                bandwidth: self.bandwidth,
                channels: self.channels,
                bitrate: payload_rate,
                payload_bytes,
                variable_bitrate: bitrate.is_some(),
                fec: selected,
            },
            outer.map_or(payload_bytes as u32 * 8, |o| o.maximum_bits),
        )
        .map_err(EncoderError::Rate)?;
        let vbr_rate = if bitrate.is_some() {
            let rate = stream_rate
                .checked_sub(allocation.silk_bitrate())
                .filter(|&rate| rate > 500);
            // The outer native encoder first sets CELT to BITRATE_MAX. A
            // rejected <=500bps high-band setter leaves that physical-budget
            // mode in place, even while outer/SILK processing remains VBR.
            if outer.is_none() && rate.is_none() {
                return Err(EncoderError::InvalidOptions);
            }
            rate
        } else {
            None
        };
        let mut filtered = [0i16; 1920];
        if outer.is_some() {
            filtered[..input.len()].copy_from_slice(input);
        } else {
            self.pcm
                .filter(input, &mut filtered[..input.len()])
                .map_err(|_| EncoderError::InvalidInput)?;
        }
        let mut pcm = [[0i16; 960]; 2];
        if self.api_channels == 2 {
            self.speech
                .resample_stereo_input(
                    &filtered[..input.len()],
                    &mut pcm,
                    output_rate(self.rate),
                    packet_duration,
                    self.channels,
                    outer.map_or(2, |o| o.previous_channels),
                )
                .map_err(EncoderError::Resampler)?;
        } else {
            self.speech.resampler[0]
                .process(&filtered[..samples], &mut pcm[0])
                .map_err(EncoderError::Resampler)?;
        }
        let mut writer = RangeEncoder::new(payload_bytes).map_err(EncoderError::Entropy)?;
        let header_bits = 2 * self.channels as u32;
        writer
            .encode_icdf(0, &[(256 - (256 >> header_bits)) as u8, 0], 8)
            .map_err(EncoderError::Entropy)?;
        let silk = &mut self.speech.channels;
        self.speech
            .fec
            .prefix_for_channels(
                &mut writer,
                include_fec,
                self.channels,
                |c, writer, frame, coding| silk[c].encode_redundant(writer, frame, coding),
            )
            .map_err(EncoderError::Silk)?;
        let target = (allocation.silk_bitrate() as i32
            - self.speech.fec.used_bits * fps
            - self.speech.excess * 2)
            .clamp(
                5000.min(allocation.silk_bitrate() as i32),
                5000.max(allocation.silk_bitrate() as i32),
            );
        let analysis_controls = AnalysisControls {
            native_activity: outer.is_some(),
            target_rate: target,
            digital_silence,
            to_mono: outer.is_some_and(|o| o.to_mono),
        };
        let speech_frame = if self.channels == 2 {
            self.speech
                .analyze_stereo(
                    SilkRate::Khz16,
                    duration,
                    [&pcm[0][..internal], &pcm[1][..internal]],
                    analysis_controls,
                )
                .map_err(EncoderError::from)?
        } else {
            self.speech
                .analyze_mono(
                    SilkRate::Khz16,
                    duration,
                    &pcm[0][..internal],
                    analysis_controls,
                )
                .map_err(EncoderError::Silk)?
        };
        let rates = speech_frame.rates;
        let activities = speech_frame.activities;
        self.speech.fec.stereo[0] = speech_frame.stereo;
        speech_frame
            .encode_prefix(&mut writer)
            .map_err(EncoderError::Stereo)?;
        let mut flags = 0;
        let mut voiced = false;
        let mut offset_q10 = 100;
        for c in 0..self.channels {
            let active = activities[c].is_some_and(|a| a.probability_q8 >= 13);
            flags = (flags << 2) | (u32::from(active) << 1) | u32::from(self.speech.fec.sent[c]);
            let Some(activity) = activities[c] else {
                continue;
            };
            let maximum = allocation.silk_max_bits();
            let cap = if c == 0 && self.channels == 2 && rates[1] > 0 {
                maximum - maximum / 2
            } else {
                maximum
            };
            let redundancy = self.speech.fec.controls(c, 0);
            let hard = if redundancy.is_some()
                || self.speech.fec.sent[..self.channels].iter().any(|&v| v)
            {
                if c == 0 && self.channels == 2 && rates[1] > 0 {
                    maximum.saturating_sub(maximum.saturating_sub(writer.tell()) / 2)
                } else {
                    maximum
                }
            } else {
                cap
            };
            let encoded = self.speech.channels[c]
                .encode_with_redundancy(
                    &mut writer,
                    &speech_frame.pcm[c][..internal],
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
                        redundancy: self.speech.fec.sent[c],
                    },
                    hard,
                    redundancy,
                )
                .map_err(EncoderError::Silk)?;
            self.speech.fec.record(c, 0, encoded.redundancy());
            if c == 0 {
                voiced = encoded.indices().signal == SignalType::Voiced;
                offset_q10 = [[100, 240], [32, 100]][usize::from(voiced)]
                    [usize::from(encoded.indices().offset)];
            }
        }
        self.speech
            .finish_analysis(&speech_frame)
            .map_err(EncoderError::Silk)?;
        writer
            .patch_initial_bits(flags, header_bits)
            .map_err(EncoderError::Entropy)?;
        let decision = self.speech.dtx.finish(
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
        self.speech.excess = (self.speech.excess + silk_bits
            - allocation.silk_bitrate() as i32 / fps)
            .clamp(0, 10000);
        if decision == Decision::Silk {
            self.speech.fec.suppress();
        }
        Ok(LowBandFrame {
            writer,
            decision,
            filtered,
            length: input.len(),
            payload_bytes,
            size,
            high_band_rate: allocation.high_band_bitrate(),
            controls: HybridControls {
                bandwidth: self.bandwidth,
                complexity: options.complexity,
                voiced,
                loss_percent: options.packet_loss_percent,
                vbr: vbr_rate.map(|rate| HybridVbr {
                    rate,
                    offset: offset_q10,
                }),
            },
        })
    }
    pub(crate) fn prepare(
        &mut self,
        duration: FrameDuration,
        input: &[i16],
        packet_bytes: usize,
        options: Options,
        bitrate: Option<u32>,
        include_fec: bool,
    ) -> Result<PreparedFrame, EncoderError> {
        let mut low =
            self.prepare_low_band(duration, input, packet_bytes, options, bitrate, include_fec)?;
        low.no_redundancy()?;
        self.complete_high_band(low)
    }
    pub(crate) fn complete_high_band(
        &mut self,
        low: LowBandFrame,
    ) -> Result<PreparedFrame, EncoderError> {
        let LowBandFrame {
            mut writer,
            decision,
            filtered,
            length,
            size,
            high_band_rate,
            controls,
            ..
        } = low;
        // Native SILK DTX skips input history completion, fades and CELT. Outer
        // digital-silence suppression instead completes both layers normally.
        if decision == Decision::Silk {
            return Ok(PreparedFrame { writer, decision });
        }
        let mut high = [0i16; 1920];
        self.pcm
            .aligned(&filtered[..length], &mut high[..length])
            .map_err(|_| EncoderError::InvalidInput)?;
        self.pcm
            .commit(&filtered[..length])
            .map_err(|_| EncoderError::InvalidInput)?;
        // Preserve native fixed-point narrowing before the exponential and at
        // the Q15 result, including forced-mode extreme-rate behavior.
        let gain = (32767 - (crate::celt::math::exp2((-high_band_rate) as i16) >> 1)) as i16;
        fade(
            &mut high[..length],
            self.rate,
            self.api_channels,
            *self.high_gain,
            gain,
            false,
        );
        *self.high_gain = gain;
        if self.channels == 2 {
            let width = self.speech.stereo.smoothed_width();
            fade(
                &mut high[..length],
                self.rate,
                2,
                width_q15(*self.width),
                width_q15(width),
                true,
            );
            *self.width = width;
        }
        self.celt.set_transient_detection(controls.complexity >= 1);
        self.celt.set_dynamic_allocation(true);
        self.celt.set_allocation_analysis(true);
        self.celt
            .encode_high_band(size, &high[..length], &mut writer, controls)
            .map_err(EncoderError::Celt)?;
        Ok(PreparedFrame { writer, decision })
    }
    pub(crate) fn finish(
        &mut self,
        duration: FrameDuration,
        packet_bytes: usize,
        prepared: PreparedFrame,
        finish: impl for<'a> FnOnce(&'a mut RangeEncoder) -> Result<&'a [u8], EntropyError>,
    ) -> Result<EncodedPacket, EncoderError> {
        let PreparedFrame {
            mut writer,
            decision,
        } = prepared;
        if decision == Decision::Silk {
            return Ok(self.suppressed_packet(duration));
        }
        let range = writer.range();
        let payload = finish(&mut writer).map_err(EncoderError::Entropy)?;
        if decision == Decision::DigitalSilence {
            self.speech.fec.suppress();
            return Ok(self.suppressed_packet(duration));
        }
        let short = duration == FrameDuration::Ms10;
        let config = if self.bandwidth == Bandwidth::SuperWideband {
            12
        } else {
            14
        } + u8::from(!short);
        if payload.len() + 1 > packet_bytes {
            return Err(EncoderError::Entropy(EntropyError::CapacityExhausted));
        }
        let mut packet = EncodedPacket {
            bytes: [0; 1276],
            length: payload.len() + 1,
            range,
            duration,
        };
        packet.bytes[0] = (config << 3) | if self.channels == 2 { 4 } else { 0 };
        packet.bytes[1..packet.length].copy_from_slice(payload);
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
