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

mod compact;
pub use compact::CompactOpusDecoder;
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
    /// The caller's PCM buffer cannot hold the packet at the configured output geometry.
    BufferTooSmall {
        /// Number of interleaved i16 samples needed for the whole packet.
        required: usize,
    },
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
            Self::BufferTooSmall { required } => {
                write!(f, "Opus output buffer needs {required} interleaved samples")
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
}
// A frame-local view into the staged public result. All offsets used by
// synthesis/gain/fades stay relative to this frame, even in long packets or
// after a FEC PLC prefix. Errors may touch only the unpublished staged output.
struct PcmBuffer<'a> {
    values: &'a mut [i16],
    length: usize,
}
impl<'a> PcmBuffer<'a> {
    fn new(values: &'a mut [i16]) -> Self {
        Self { values, length: 0 }
    }
    fn samples(&self) -> &[i16] {
        &self.values[..self.length]
    }
    fn append(&mut self, samples: &[i16]) -> Result<(), DecoderError> {
        let end = self
            .length
            .checked_add(samples.len())
            .ok_or(DecoderError::InvalidOutput)?;
        self.values
            .get_mut(self.length..end)
            .ok_or(DecoderError::InvalidOutput)?
            .copy_from_slice(samples);
        self.length = end;
        Ok(())
    }
    fn frame(
        &mut self,
        render: impl FnOnce(&mut PcmBuffer<'_>) -> Result<(), DecoderError>,
    ) -> Result<(), DecoderError> {
        let mut frame = PcmBuffer::new(
            self.values
                .get_mut(self.length..)
                .ok_or(DecoderError::InvalidOutput)?,
        );
        render(&mut frame)?;
        self.length += frame.length;
        Ok(())
    }
}
/// One fixed-storage standard Opus stream with shared SILK/CELT histories.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OpusDecoder {
    state: StreamState,
    celt: CeltDecoder,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct StreamState {
    silk: SilkDecoder,
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

impl StreamState {
    fn new(rate: OutputRate, channels: PcmChannels) -> Self {
        Self {
            silk: SilkDecoder::new(rate, channels),
            mode: None,
            previous: None,
            redundancy: false,
            bandwidth: Bandwidth::Fullband,
            wire_channels: channels.count(),
            clock: 120,
            range: 0,
            last_duration: 0,
            gain_q8: 0,
        }
    }
}

fn decimation(rate: OutputRate) -> Decimation {
    match rate {
        OutputRate::Khz8 => Decimation::By6,
        OutputRate::Khz12 => Decimation::By4,
        OutputRate::Khz16 => Decimation::By3,
        OutputRate::Khz24 => Decimation::By2,
        OutputRate::Khz48 => Decimation::By1,
    }
}

impl OpusDecoder {
    /// Select any standard output rate and one/two output channels.
    pub fn new(rate: OutputRate, channels: PcmChannels) -> Result<Self, DecoderError> {
        Ok(Self {
            state: StreamState::new(rate, channels),
            celt: CeltDecoder::new(decimation(rate), channels.count())?,
        })
    }
    /// Fixed PCM sampling rate.
    pub fn output_rate(&self) -> OutputRate {
        self.state.silk.output_rate()
    }
    /// Fixed PCM channel count.
    pub fn output_channels(&self) -> PcmChannels {
        self.state.silk.output_channels()
    }
    /// Signed Q8 dB output gain, initially zero. Reset retains this setting.
    pub fn gain_q8(&self) -> i16 {
        self.state.gain_q8
    }
    /// Set native signed Q8 dB output gain without changing decoder histories.
    /// The full signed16 range is valid. Gain applies with native fixed-point
    /// rounding/saturation after frame synthesis, including PLC and FEC.
    pub fn set_gain_q8(&mut self, gain: i16) {
        self.state.gain_q8 = gain;
    }
    /// Read-only CELT synthesis, energy and PLC diagnostics.
    pub fn celt(&self) -> &CeltDecoder {
        &self.celt
    }
    /// Last successful operation's public final range.
    pub fn final_range(&self) -> u32 {
        self.state.range
    }
    /// Last successful operation's duration on a48kHz clock.
    pub fn last_packet_duration(&self) -> u16 {
        self.state.last_duration
    }
    /// Last frame's primary mode, or actual PLC mode after loss/DTX.
    pub fn previous_mode(&self) -> Option<Mode> {
        self.state.previous
    }
    /// Whether the preceding frame ended with a SILK-to-CELT redundant tail.
    pub fn previous_redundancy(&self) -> bool {
        self.state.redundancy
    }
    /// Native stream reset, retaining output geometry and CELT band limits.
    pub fn reset(&mut self) {
        self.state.silk.reset();
        self.celt.reset();
        self.state.mode = None;
        self.state.previous = None;
        self.state.redundancy = false;
        self.state.bandwidth = Bandwidth::Fullband;
        self.state.wire_channels = self.output_channels().count();
        self.state.clock = 120;
        self.state.range = 0;
        self.state.last_duration = 0;
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
        let mut output = self.empty_output();
        output.length = self.decode_packet_into(&packet, &mut output.values)?;
        output.range = self.state.range;
        Ok(output)
    }
    /// Decode one complete packet into a reusable interleaved PCM buffer.
    ///
    /// Returns the number of i16 samples written across all output channels.
    /// Samples beyond that prefix are untouched. Use [`Self::final_range`] to
    /// inspect the range state after a successful decode.
    ///
    /// A framing error or [`DecoderError::BufferTooSmall`] leaves the buffer
    /// untouched. On a later decode error, discard the packet's output prefix.
    /// All errors preserve the decoder state, so the caller can retry.
    pub fn decode_into(&mut self, data: &[u8], output: &mut [i16]) -> Result<usize, DecoderError> {
        let packet = Packet::parse(data).map_err(DecoderError::Framing)?;
        let frames =
            usize::from(packet.duration_samples()) * self.output_rate().hz() as usize / 48000;
        let required = frames * self.output_channels().count();
        let out = output
            .get_mut(..required)
            .ok_or(DecoderError::BufferTooSmall { required })?;
        self.decode_packet_into(&packet, out)
    }

    fn decode_packet_into(
        &mut self,
        packet: &Packet<'_>,
        output: &mut [i16],
    ) -> Result<usize, DecoderError> {
        let mut next = self.clone();
        let length = next.decode_packet_into_staged(packet, output)?;
        *self = next;
        Ok(length)
    }

    // The caller owns the transaction and discards this decoder on failure.
    pub(crate) fn decode_packet_staged(
        &mut self,
        packet: &Packet<'_>,
    ) -> Result<DecodedPcm, DecoderError> {
        let mut output = self.empty_output();
        output.length = self.decode_packet_into_staged(packet, &mut output.values)?;
        output.range = self.state.range;
        Ok(output)
    }

    fn decode_packet_into_staged(
        &mut self,
        packet: &Packet<'_>,
        output: &mut [i16],
    ) -> Result<usize, DecoderError> {
        self.configure_packet(packet);
        let mut pcm = PcmBuffer::new(output);
        for index in 0..packet.frames().len() {
            pcm.frame(|frame| self.packet_frame(packet, index, false, frame))?;
        }
        self.state.last_duration = packet.duration_samples();
        Ok(pcm.length)
    }
    /// Conceal120..5760 samples on a48kHz clock, in120-sample steps.
    pub fn conceal_samples(&mut self, duration: u16) -> Result<DecodedPcm, DecoderError> {
        validate_duration(duration)?;
        let mut next = self.clone();
        let mut output = self.empty_output();
        let mut pcm = PcmBuffer::new(&mut output.values);
        next.loss(usize::from(duration), &mut pcm)?;
        output.length = pcm.length;
        output.range = next.state.range;
        next.state.last_duration = duration;
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
        let mut output = self.empty_output();
        let mut pcm = PcmBuffer::new(&mut output.values);
        if duration < packet.frame_samples()
            || packet.mode() == Mode::Celt
            || next.state.mode == Some(Mode::Celt)
        {
            next.loss(usize::from(duration), &mut pcm)?;
        } else {
            let prefix = duration - packet.frame_samples();
            if prefix != 0 {
                pcm.frame(|frame| next.loss(usize::from(prefix), frame))?;
            }
            next.configure_packet(packet);
            pcm.frame(|frame| next.packet_frame(packet, 0, true, frame))?;
        }
        output.length = pcm.length;
        next.state.last_duration = duration;
        output.range = next.state.range;
        *self = next;
        Ok(output)
    }
    fn configure_packet(&mut self, packet: &Packet<'_>) {
        self.state.mode = Some(packet.mode());
        self.state.bandwidth = packet.bandwidth();
        self.state.wire_channels = usize::from(packet.channels());
        self.state.clock = packet.frame_samples();
    }
    fn packet_frame(
        &mut self,
        packet: &Packet<'_>,
        index: usize,
        fec: bool,
        output: &mut PcmBuffer<'_>,
    ) -> Result<(), DecoderError> {
        let payload = packet
            .frames()
            .get(index)
            .ok_or(DecoderError::InvalidOutput)?;
        if payload.len() <= 1 {
            return self.loss(usize::from(self.state.clock), output);
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
        self.normal(payload, header.as_ref(), &mut entropy, fec, output)
    }
    fn loss(&mut self, duration: usize, output: &mut PcmBuffer<'_>) -> Result<(), DecoderError> {
        let mut remaining = duration;
        while remaining > 0 {
            let limit = remaining.min(usize::from(self.state.clock)).min(960);
            let mode = if self.state.redundancy {
                Some(Mode::Celt)
            } else {
                self.state.previous
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
            let mut frame = PcmBuffer::new(
                output
                    .values
                    .get_mut(output.length..)
                    .ok_or(DecoderError::InvalidOutput)?,
            );
            if mode != Some(Mode::Celt) {
                let duration = if n == 960 {
                    FrameDuration::Ms20
                } else {
                    FrameDuration::Ms10
                };
                let low = self.state.silk.conceal_layer(duration)?;
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
            self.apply_gain(&mut frame.values[..frame.length]);
            output.length += frame.length;
            self.state.previous = mode;
            self.state.redundancy = false;
            remaining -= n;
        }
        self.state.range = 0;
        Ok(())
    }
    fn normal(
        &mut self,
        payload: &[u8],
        header: Option<&FrameHeader>,
        entropy: &mut RangeDecoder<'_>,
        fec: bool,
        output: &mut PcmBuffer<'_>,
    ) -> Result<(), DecoderError> {
        let mode = self.state.mode.ok_or(DecoderError::InvalidOutput)?;
        let mut transition = self.state.previous.is_some()
            && ((mode == Mode::Celt
                && self.state.previous != Some(Mode::Celt)
                && !self.state.redundancy)
                || (mode != Mode::Celt && self.state.previous == Some(Mode::Celt)));
        let mut previous_samples = [0; 480];
        let mut previous_pcm = PcmBuffer::new(&mut previous_samples);
        if transition && mode == Mode::Celt {
            self.loss(usize::from(self.state.clock).min(240), &mut previous_pcm)?;
        }
        if mode != Mode::Celt {
            if self.state.previous == Some(Mode::Celt) {
                self.state.silk.reset();
            }
            let rate = if mode == Mode::Hybrid {
                SilkRate::Khz16
            } else {
                match self.state.bandwidth {
                    Bandwidth::Narrowband => SilkRate::Khz8,
                    Bandwidth::Mediumband => SilkRate::Khz12,
                    _ => SilkRate::Khz16,
                }
            };
            let destination = output
                .values
                .get_mut(output.length..)
                .ok_or(DecoderError::InvalidOutput)?;
            output.length += self.state.silk.decode_layer_into(
                header.ok_or(DecoderError::InvalidOutput)?,
                entropy,
                rate,
                fec,
                destination,
            )?;
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
            self.loss(usize::from(self.state.clock).min(240), &mut previous_pcm)?;
        }
        let end = match self.state.bandwidth {
            Bandwidth::Narrowband => 13,
            Bandwidth::Mediumband | Bandwidth::Wideband => 17,
            Bandwidth::SuperWideband => 19,
            Bandwidth::Fullband => 21,
        };
        let band = if self.state.bandwidth == Bandwidth::Mediumband {
            Bandwidth::Wideband
        } else {
            self.state.bandwidth
        };
        let mut redundant_pcm = None;
        let mut redundant_range = 0;
        if let Some(tail) = redundant.filter(|_| celt_to_silk) {
            self.celt.configure_bands(0, end)?;
            let pcm = self.celt.decode_layer(
                &mut RangeDecoder::new(tail),
                FrameConfig {
                    size: FrameSize::Ms5,
                    channels: self.state.wire_channels,
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
            if self.state.previous.is_some()
                && Some(mode) != self.state.previous
                && !self.state.redundancy
            {
                self.celt.reset();
            }
            let size = frame_size(usize::from(self.state.clock).min(960))?;
            let high = if fec || primary <= 1 {
                self.celt.conceal_configured(size)?
            } else {
                self.celt.decode_layer(
                    entropy,
                    FrameConfig {
                        size,
                        channels: self.state.wire_channels,
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
            self.state.range = self.celt.rng_state();
        } else {
            if self.state.previous == Some(Mode::Hybrid)
                && !(redundant.is_some() && celt_to_silk && self.state.redundancy)
            {
                self.celt.configure_bands(0, end)?;
                let high = self.celt.decode_layer(
                    &mut RangeDecoder::new(&[0xff, 0xff]),
                    FrameConfig {
                        size: FrameSize::Ms2_5,
                        channels: self.state.wire_channels,
                        bandwidth: band,
                    },
                    0,
                )?;
                accumulate(&mut output.values[..high.samples().len()], high.samples())?;
            }
            self.state.range = entropy.range();
        }
        let f2_5 = self.output_rate().hz() as usize / 400 * self.output_channels().count();
        if let Some(tail) = redundant.filter(|_| !celt_to_silk) {
            self.celt.reset();
            self.celt.configure_bands(0, end)?;
            let pcm = self.celt.decode_layer(
                &mut RangeDecoder::new(tail),
                FrameConfig {
                    size: FrameSize::Ms5,
                    channels: self.state.wire_channels,
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
            && (self.state.previous != Some(Mode::Silk) || self.state.redundancy)
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
            if previous_pcm.length == 0 {
                return Err(DecoderError::InvalidOutput);
            }
            let old = &previous_pcm;
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
        self.state.range = if primary <= 1 {
            0
        } else {
            self.state.range ^ redundant_range
        };
        self.state.previous = Some(mode);
        self.state.redundancy = redundant.is_some() && !celt_to_silk;
        self.apply_gain(&mut output.values[..output.length]);
        Ok(())
    }
    fn apply_gain(&self, output: &mut [i16]) {
        if self.state.gain_q8 == 0 {
            return;
        }
        // Native QCONST16(6.48814081e-4,25), MULT16_16_P15 and
        // celt_exp2 convert Q8 dB to Q16 amplitude. Apply this per decoded
        // frame, including recursive transition PLC, rather than once to a
        // completed packet; native transition fades can reuse gained PLC.
        let exponent = ((21771 * i32::from(self.state.gain_q8) + 16384) >> 15) as i16;
        let gain = i64::from(crate::celt::math::exp2(exponent));
        for value in output {
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
