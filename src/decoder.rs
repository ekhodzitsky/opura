//! Unified standard Opus packet, loss and FEC decoding.
//!
//! Native fixed-point packet ordering and transitions follow opus_decoder.c.
//! Copyright (c) 2010 Xiph.Org Foundation, Skype Limited; 2024 Arm Limited.
//! Written by Jean-Marc Valin and Koen Vos. See THIRD_PARTY_NOTICES.md.
use crate::{
    Bandwidth, Mode, Packet, PacketError,
    celt::{
        self, FrameSize,
        decoder::{CeltDecoder, DecoderError as CeltError, FrameConfig},
        output::Decimation,
    },
    range::{RangeDecoder, RangeError},
    silk::{
        self, FrameHeader,
        decoder::{DecoderError as SilkError, PcmChannels, SilkDecoder},
        gains::FrameDuration,
        pitch::SilkRate,
        resampler::OutputRate,
    },
};
use std::fmt;
/// A packet, loss or FEC failure preserves all stream state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum DecoderError {
    /// Packet framing is invalid.
    Framing(PacketError),
    /// Requested FEC/loss duration must be120..5760 samples in120-sample steps.
    InvalidDuration,
    /// Transition redundancy exceeds the remaining primary entropy storage.
    InvalidRedundancy,
    /// Layer output or retained geometry is inconsistent.
    InvalidOutput,
    /// SILK header failed.
    Header(silk::SilkError),
    /// SILK decoding or concealment failed.
    Silk(SilkError),
    /// CELT decoding or concealment failed.
    Celt(CeltError),
    /// Shared entropy or transition fields failed.
    Entropy(RangeError),
}
impl fmt::Display for DecoderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Framing(e) => write!(f, "Opus framing: {e}"),
            Self::InvalidDuration => {
                f.write_str("Opus loss/FEC duration must be120..5760 samples in120-sample steps")
            }
            Self::InvalidRedundancy => {
                f.write_str("Opus transition redundancy exceeds primary storage")
            }
            Self::InvalidOutput => f.write_str("inconsistent Opus output geometry"),
            Self::Header(e) => write!(f, "Opus SILK header: {e}"),
            Self::Silk(e) => write!(f, "Opus SILK: {e}"),
            Self::Celt(e) => write!(f, "Opus CELT: {e}"),
            Self::Entropy(e) => write!(f, "Opus entropy: {e}"),
        }
    }
}
impl std::error::Error for DecoderError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Framing(e) => Some(e),
            Self::Header(e) => Some(e),
            Self::Silk(e) => Some(e),
            Self::Celt(e) => Some(e),
            Self::Entropy(e) => Some(e),
            _ => None,
        }
    }
}
impl From<CeltError> for DecoderError {
    fn from(e: CeltError) -> Self {
        Self::Celt(e)
    }
}
impl From<SilkError> for DecoderError {
    fn from(e: SilkError) -> Self {
        Self::Silk(e)
    }
}
impl From<RangeError> for DecoderError {
    fn from(e: RangeError) -> Self {
        Self::Entropy(e)
    }
}
/// At most120ms of interleaved signed16 PCM.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DecodedPcm {
    values: [i16; 11520],
    length: usize,
    rate: OutputRate,
    channels: PcmChannels,
    range: u32,
}
impl DecodedPcm {
    /// Signed16 output, interleaved when stereo.
    pub fn samples(&self) -> &[i16] {
        &self.values[..self.length]
    }
    /// Selected PCM sampling rate.
    pub fn sample_rate(&self) -> u32 {
        self.rate.hz()
    }
    /// Selected output channel count.
    pub fn channels(&self) -> usize {
        self.channels.count()
    }
    /// Number of samples per channel.
    pub fn samples_per_channel(&self) -> usize {
        self.length / self.channels.count()
    }
    /// Public final range, including transition-tail XOR; zero for data loss.
    pub fn final_range(&self) -> u32 {
        self.range
    }
    fn append(&mut self, samples: &[i16]) -> Result<(), DecoderError> {
        let end = self.length + samples.len();
        if end > self.values.len() {
            return Err(DecoderError::InvalidOutput);
        }
        self.values[self.length..end].copy_from_slice(samples);
        self.length = end;
        Ok(())
    }
}
/// One fixed-storage standard Opus stream with shared SILK/CELT histories.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OpusDecoder {
    silk: SilkDecoder,
    celt: CeltDecoder,
    mode: Option<Mode>,
    previous: Option<Mode>,
    redundancy: bool,
    bandwidth: Bandwidth,
    wire_channels: usize,
    clock: u16,
    range: u32,
    last_duration: u16,
    gain_q8: i16,
}
impl OpusDecoder {
    /// Select any standard output rate and one/two output channels.
    pub fn new(rate: OutputRate, channels: PcmChannels) -> Result<Self, DecoderError> {
        let decimation = match rate {
            OutputRate::Khz8 => Decimation::By6,
            OutputRate::Khz12 => Decimation::By4,
            OutputRate::Khz16 => Decimation::By3,
            OutputRate::Khz24 => Decimation::By2,
            OutputRate::Khz48 => Decimation::By1,
        };
        Ok(Self {
            silk: SilkDecoder::new(rate, channels),
            celt: CeltDecoder::new(decimation, channels.count())?,
            mode: None,
            previous: None,
            redundancy: false,
            bandwidth: Bandwidth::Fullband,
            wire_channels: channels.count(),
            clock: 120,
            range: 0,
            last_duration: 0,
            gain_q8: 0,
        })
    }
    /// Fixed PCM sampling rate.
    pub fn output_rate(&self) -> OutputRate {
        self.silk.output_rate()
    }
    /// Fixed PCM channel count.
    pub fn output_channels(&self) -> PcmChannels {
        self.silk.output_channels()
    }
    /// Signed Q8 dB output gain, initially zero. Reset retains this setting.
    pub fn gain_q8(&self) -> i16 {
        self.gain_q8
    }
    /// Set native signed Q8 dB output gain without changing decoder histories.
    /// The full signed16 range is valid. Gain applies with native fixed-point
    /// rounding/saturation after frame synthesis, including PLC and FEC.
    pub fn set_gain_q8(&mut self, gain: i16) {
        self.gain_q8 = gain;
    }
    /// Read-only CELT synthesis, energy and PLC diagnostics.
    pub fn celt(&self) -> &CeltDecoder {
        &self.celt
    }
    /// Last successful operation's public final range.
    pub fn final_range(&self) -> u32 {
        self.range
    }
    /// Last successful operation's duration on a48kHz clock.
    pub fn last_packet_duration(&self) -> u16 {
        self.last_duration
    }
    /// Last frame's primary mode, or actual PLC mode after loss/DTX.
    pub fn previous_mode(&self) -> Option<Mode> {
        self.previous
    }
    /// Whether the preceding frame ended with a SILK-to-CELT redundant tail.
    pub fn previous_redundancy(&self) -> bool {
        self.redundancy
    }
    /// Native stream reset, retaining output geometry and CELT band limits.
    pub fn reset(&mut self) {
        self.silk.reset();
        self.celt.reset();
        self.mode = None;
        self.previous = None;
        self.redundancy = false;
        self.bandwidth = Bandwidth::Fullband;
        self.wire_channels = self.output_channels().count();
        self.clock = 120;
        self.range = 0;
        self.last_duration = 0;
    }
    fn empty_output(&self) -> DecodedPcm {
        DecodedPcm {
            values: [0; 11520],
            length: 0,
            rate: self.output_rate(),
            channels: self.output_channels(),
            range: 0,
        }
    }
    /// Decode one complete externally delimited Opus packet, including DTX.
    pub fn decode(&mut self, data: &[u8]) -> Result<DecodedPcm, DecoderError> {
        let packet = Packet::parse(data).map_err(DecoderError::Framing)?;
        self.decode_packet(&packet)
    }
    pub(crate) fn decode_packet(
        &mut self,
        packet: &Packet<'_>,
    ) -> Result<DecodedPcm, DecoderError> {
        let mut next = self.clone();
        next.configure_packet(packet);
        let mut output = self.empty_output();
        for index in 0..packet.frames().len() {
            let frame = next.packet_frame(packet, index, false)?;
            output.append(frame.samples())?;
        }
        next.last_duration = packet.duration_samples();
        output.range = next.range;
        *self = next;
        Ok(output)
    }
    /// Conceal120..5760 samples on a48kHz clock, in120-sample steps.
    pub fn conceal_samples(&mut self, duration: u16) -> Result<DecodedPcm, DecoderError> {
        validate_duration(duration)?;
        let mut next = self.clone();
        let output = next.loss(usize::from(duration))?;
        next.last_duration = duration;
        *self = next;
        Ok(output)
    }
    /// Recover a requested missing interval using the first compressed frame's
    /// LBRR. Earlier samples use PLC; unavailable FEC falls back to PLC.
    /// Decode the received packet normally afterwards for its primary audio.
    pub fn decode_fec(&mut self, data: &[u8], duration: u16) -> Result<DecodedPcm, DecoderError> {
        validate_duration(duration)?;
        let packet = Packet::parse(data).map_err(DecoderError::Framing)?;
        self.decode_fec_packet(&packet, duration)
    }
    pub(crate) fn decode_fec_packet(
        &mut self,
        packet: &Packet<'_>,
        duration: u16,
    ) -> Result<DecodedPcm, DecoderError> {
        validate_duration(duration)?;
        let mut next = self.clone();
        if duration < packet.frame_samples()
            || packet.mode() == Mode::Celt
            || next.mode == Some(Mode::Celt)
        {
            let output = next.loss(usize::from(duration))?;
            next.last_duration = duration;
            *self = next;
            return Ok(output);
        }
        let mut output = self.empty_output();
        let prefix = duration - packet.frame_samples();
        if prefix != 0 {
            let loss = next.loss(usize::from(prefix))?;
            output.append(loss.samples())?;
        }
        next.configure_packet(packet);
        let fec = next.packet_frame(packet, 0, true)?;
        output.append(fec.samples())?;
        next.last_duration = duration;
        output.range = next.range;
        *self = next;
        Ok(output)
    }
    fn configure_packet(&mut self, packet: &Packet<'_>) {
        self.mode = Some(packet.mode());
        self.bandwidth = packet.bandwidth();
        self.wire_channels = usize::from(packet.channels());
        self.clock = packet.frame_samples();
    }
    fn packet_frame(
        &mut self,
        packet: &Packet<'_>,
        index: usize,
        fec: bool,
    ) -> Result<DecodedPcm, DecoderError> {
        let payload = packet
            .frames()
            .get(index)
            .ok_or(DecoderError::InvalidOutput)?;
        if payload.len() <= 1 {
            return self.loss(usize::from(self.clock));
        }
        let (header, mut entropy) = if packet.mode() == Mode::Celt {
            (None, RangeDecoder::new(payload))
        } else {
            let (header, mut entropy) =
                silk::decode_header(packet, index).map_err(DecoderError::Header)?;
            if packet.mode() == Mode::Silk {
                entropy.allow_silk_zero_extension();
            }
            (Some(header), entropy)
        };
        self.normal(payload, header.as_ref(), &mut entropy, fec)
    }
    fn loss(&mut self, duration: usize) -> Result<DecodedPcm, DecoderError> {
        let mut output = self.empty_output();
        let mut remaining = duration;
        while remaining > 0 {
            let limit = remaining.min(usize::from(self.clock)).min(960);
            let mode = if self.redundancy {
                Some(Mode::Celt)
            } else {
                self.previous
            };
            if mode.is_none() {
                let count = limit * self.output_rate().hz() as usize / 48000
                    * self.output_channels().count();
                output.append(&[0; 1920][..count])?;
                remaining -= limit;
                continue;
            }
            let size = if limit >= 960 {
                FrameSize::Ms20
            } else if limit >= 480 {
                FrameSize::Ms10
            } else if mode == Some(Mode::Silk) {
                // SILK can synthesize the requested fractional interval by
                // advancing10ms and returning only its required prefix.
                FrameSize::Ms2_5
            } else if limit >= 240 {
                FrameSize::Ms5
            } else {
                FrameSize::Ms2_5
            };
            let n = if mode == Some(Mode::Silk) && limit < 480 {
                limit
            } else {
                size.samples()
            };
            let count =
                n * self.output_rate().hz() as usize / 48000 * self.output_channels().count();
            let mut frame = self.empty_output();
            if mode != Some(Mode::Celt) {
                let duration = if n == 960 {
                    FrameDuration::Ms20
                } else {
                    FrameDuration::Ms10
                };
                let low = self.silk.conceal_layer(duration)?;
                frame.append(&low.samples()[..count])?;
            }
            let start = if mode == Some(Mode::Celt) { 0 } else { 17 };
            self.celt
                .configure_bands(start, self.celt.concealment().end_band())?;
            if mode != Some(Mode::Silk) {
                let high = self.celt.conceal_configured(size)?;
                if mode == Some(Mode::Celt) {
                    frame.append(high.samples())?;
                } else {
                    accumulate(&mut frame.values[..frame.length], high.samples())?;
                }
            }
            self.apply_gain(&mut frame);
            output.append(frame.samples())?;
            self.previous = mode;
            self.redundancy = false;
            remaining -= n;
        }
        self.range = 0;
        output.range = 0;
        Ok(output)
    }
    fn normal(
        &mut self,
        payload: &[u8],
        header: Option<&FrameHeader>,
        entropy: &mut RangeDecoder<'_>,
        fec: bool,
    ) -> Result<DecodedPcm, DecoderError> {
        let mode = self.mode.ok_or(DecoderError::InvalidOutput)?;
        let mut transition = self.previous.is_some()
            && ((mode == Mode::Celt && self.previous != Some(Mode::Celt) && !self.redundancy)
                || (mode != Mode::Celt && self.previous == Some(Mode::Celt)));
        let mut previous_pcm = None;
        if transition && mode == Mode::Celt {
            previous_pcm = Some(self.loss(usize::from(self.clock).min(240))?);
        }
        let mut output = self.empty_output();
        if mode != Mode::Celt {
            if self.previous == Some(Mode::Celt) {
                self.silk.reset();
            }
            let rate = if mode == Mode::Hybrid {
                SilkRate::Khz16
            } else {
                match self.bandwidth {
                    Bandwidth::Narrowband => SilkRate::Khz8,
                    Bandwidth::Mediumband => SilkRate::Khz12,
                    _ => SilkRate::Khz16,
                }
            };
            let low = self.silk.decode_layer(
                header.ok_or(DecoderError::InvalidOutput)?,
                entropy,
                rate,
                fec,
            )?;
            output.append(low.samples())?;
        }
        let mut primary = payload.len();
        let mut redundant = None;
        let mut celt_to_silk = false;
        if !fec
            && mode != Mode::Celt
            && entropy.tell() + 17 + 20 * u32::from(mode == Mode::Hybrid)
                <= payload.len() as u32 * 8
        {
            let present = mode == Mode::Silk || entropy.decode_bit_logp(12)?;
            if present {
                celt_to_silk = entropy.decode_bit_logp(1)?;
                let bytes = if mode == Mode::Hybrid {
                    entropy.decode_uint(256)? as usize + 2
                } else {
                    payload
                        .len()
                        .saturating_sub(entropy.tell().div_ceil(8) as usize)
                };
                primary = payload
                    .len()
                    .checked_sub(bytes)
                    .ok_or(DecoderError::InvalidRedundancy)?;
                if primary * 8 < (entropy.tell() as usize) || bytes < 2 {
                    return Err(DecoderError::InvalidRedundancy);
                }
                entropy.shrink_storage(primary)?;
                redundant = Some(&payload[primary..]);
                transition = false;
            }
        }
        if transition && mode != Mode::Celt {
            previous_pcm = Some(self.loss(usize::from(self.clock).min(240))?);
        }
        let end = match self.bandwidth {
            Bandwidth::Narrowband => 13,
            Bandwidth::Mediumband | Bandwidth::Wideband => 17,
            Bandwidth::SuperWideband => 19,
            Bandwidth::Fullband => 21,
        };
        let band = if self.bandwidth == Bandwidth::Mediumband {
            Bandwidth::Wideband
        } else {
            self.bandwidth
        };
        let mut redundant_pcm = None;
        let mut redundant_range = 0;
        if let Some(tail) = redundant.filter(|_| celt_to_silk) {
            self.celt.configure_bands(0, end)?;
            let pcm = self.celt.decode_layer(
                &mut RangeDecoder::new(tail),
                FrameConfig {
                    size: FrameSize::Ms5,
                    channels: self.wire_channels,
                    bandwidth: band,
                },
                0,
            )?;
            redundant_range = self.celt.rng_state();
            redundant_pcm = Some(pcm);
        }
        let start = if mode == Mode::Celt { 0 } else { 17 };
        self.celt.configure_bands(start, end)?;
        if mode != Mode::Silk {
            if self.previous.is_some() && Some(mode) != self.previous && !self.redundancy {
                self.celt.reset();
            }
            let size = frame_size(usize::from(self.clock).min(960))?;
            let high = if fec || primary <= 1 {
                self.celt.conceal_configured(size)?
            } else {
                self.celt.decode_layer(
                    entropy,
                    FrameConfig {
                        size,
                        channels: self.wire_channels,
                        bandwidth: band,
                    },
                    start,
                )?
            };
            if mode == Mode::Celt {
                output.append(high.samples())?;
            } else {
                accumulate(&mut output.values[..output.length], high.samples())?;
            }
            self.range = self.celt.rng_state();
        } else {
            if self.previous == Some(Mode::Hybrid)
                && !(redundant.is_some() && celt_to_silk && self.redundancy)
            {
                self.celt.configure_bands(0, end)?;
                let high = self.celt.decode_layer(
                    &mut RangeDecoder::new(&[0xff, 0xff]),
                    FrameConfig {
                        size: FrameSize::Ms2_5,
                        channels: self.wire_channels,
                        bandwidth: band,
                    },
                    0,
                )?;
                accumulate(&mut output.values[..high.samples().len()], high.samples())?;
            }
            self.range = entropy.range();
        }
        let f2_5 = self.output_rate().hz() as usize / 400 * self.output_channels().count();
        if let Some(tail) = redundant.filter(|_| !celt_to_silk) {
            self.celt.reset();
            self.celt.configure_bands(0, end)?;
            let pcm = self.celt.decode_layer(
                &mut RangeDecoder::new(tail),
                FrameConfig {
                    size: FrameSize::Ms5,
                    channels: self.wire_channels,
                    bandwidth: band,
                },
                0,
            )?;
            redundant_range = self.celt.rng_state();
            let offset = output
                .length
                .checked_sub(f2_5)
                .ok_or(DecoderError::InvalidOutput)?;
            fade(
                &mut output.values[offset..output.length],
                &pcm.samples()[f2_5..],
                false,
                self.output_rate(),
            )?;
        }
        if redundant.is_some()
            && celt_to_silk
            && (self.previous != Some(Mode::Silk) || self.redundancy)
        {
            let pcm = redundant_pcm.as_ref().ok_or(DecoderError::InvalidOutput)?;
            output.values[..f2_5].copy_from_slice(&pcm.samples()[..f2_5]);
            fade(
                &mut output.values[f2_5..2 * f2_5],
                &pcm.samples()[f2_5..],
                true,
                self.output_rate(),
            )?;
        }
        if transition {
            let old = previous_pcm.as_ref().ok_or(DecoderError::InvalidOutput)?;
            if output.length >= 2 * f2_5 {
                output.values[..f2_5].copy_from_slice(&old.samples()[..f2_5]);
                fade(
                    &mut output.values[f2_5..2 * f2_5],
                    &old.samples()[f2_5..],
                    true,
                    self.output_rate(),
                )?;
            } else {
                fade(
                    &mut output.values[..f2_5],
                    old.samples(),
                    true,
                    self.output_rate(),
                )?;
            }
        }
        self.range = if primary <= 1 {
            0
        } else {
            self.range ^ redundant_range
        };
        self.previous = Some(mode);
        self.redundancy = redundant.is_some() && !celt_to_silk;
        output.range = self.range;
        self.apply_gain(&mut output);
        Ok(output)
    }
    fn apply_gain(&self, output: &mut DecodedPcm) {
        if self.gain_q8 == 0 {
            return;
        }
        // Native QCONST16(6.48814081e-4,25), MULT16_16_P15 and
        // celt_exp2 convert Q8 dB to Q16 amplitude. Apply this per decoded
        // frame, including recursive transition PLC, rather than once to a
        // completed packet; native transition fades can reuse gained PLC.
        let exponent = ((21771 * i32::from(self.gain_q8) + 16384) >> 15) as i16;
        let gain = i64::from(crate::celt::math::exp2(exponent));
        for value in &mut output.values[..output.length] {
            let scaled = (i64::from(*value) * gain + 32768) >> 16;
            *value = scaled.clamp(-32767, 32767) as i16;
        }
    }
}
fn validate_duration(duration: u16) -> Result<(), DecoderError> {
    if !(120..=5760).contains(&duration) || duration % 120 != 0 {
        Err(DecoderError::InvalidDuration)
    } else {
        Ok(())
    }
}
fn frame_size(n: usize) -> Result<FrameSize, DecoderError> {
    match n {
        120 => Ok(FrameSize::Ms2_5),
        240 => Ok(FrameSize::Ms5),
        480 => Ok(FrameSize::Ms10),
        960 => Ok(FrameSize::Ms20),
        _ => Err(DecoderError::InvalidOutput),
    }
}
fn accumulate(output: &mut [i16], input: &[i16]) -> Result<(), DecoderError> {
    if output.len() != input.len() {
        return Err(DecoderError::InvalidOutput);
    }
    for (target, &value) in output.iter_mut().zip(input) {
        *target = (i32::from(*target) + i32::from(value)).clamp(-32768, 32767) as i16;
    }
    Ok(())
}
// `other_is_old` selects old->current versus current->new without aliasing the
// destination. Q15ONE is32767; one final floor shift preserves native bias.
fn fade(
    output: &mut [i16],
    other: &[i16],
    other_is_old: bool,
    rate: OutputRate,
) -> Result<(), DecoderError> {
    let samples = rate.hz() as usize / 400;
    if output.len() != other.len()
        || output.is_empty()
        || output.len() % samples != 0
        || output.len() / samples > 2
    {
        return Err(DecoderError::InvalidOutput);
    }
    let channels = output.len() / samples;
    let step = 48000 / rate.hz() as usize;
    let window = celt::transition_window();
    for i in 0..samples {
        let w = i32::from(window[i * step]);
        let w = (w * w) >> 15;
        for c in 0..channels {
            let k = i * channels + c;
            let (old, new) = if other_is_old {
                (other[k], output[k])
            } else {
                (output[k], other[k])
            };
            output[k] = ((w * i32::from(new) + (32767 - w) * i32::from(old)) >> 15) as i16;
        }
    }
    Ok(())
}
