//! Bounded classic Opus multistream mappings and borrowed packet framing.
//!
//! The first `streams - 1` subpackets use RFC6716 Appendix B framing; the
//! final subpacket is externally delimited. Mapping indexes describe decoded
//! channels, with coupled streams first and255 representing a silent output.
//! Framing validation alone does not establish compressed audio decodability.
//! Mapping/stream ordering follows original Opus1.6.1 multistream routines.
//! Copyright (c) 2011 Xiph.Org Foundation; written by Jean-Marc Valin.
//! See THIRD_PARTY_NOTICES.md.
use crate::{
    Packet, PacketError,
    decoder::{DecoderError, OpusDecoder},
    silk::{decoder::PcmChannels, resampler::OutputRate},
};
use std::fmt;

mod encoder;
pub use encoder::{MultistreamEncodeOptions, MultistreamEncodedPacket, MultistreamEncoder};

/// Largest accepted multistream transport packet, including padding (16MiB).
/// This bounds framing work independently of caller-controlled padding lengths.
pub const MAX_PACKET_BYTES: usize = 16 * 1024 * 1024;

/// Invalid layout or multistream transport framing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum MultistreamError {
    /// Channels/streams must be1..255, coupled<=streams and streams+coupled<=255.
    InvalidLayout,
    /// A map entry is neither a coded channel index nor255 (silence).
    InvalidChannelMap,
    /// Encoding requires an input occurrence of every coded channel.
    IncompleteEncoderMap,
    /// A packet exceeds [`MAX_PACKET_BYTES`].
    PacketTooLarge,
    /// A constituent stream has invalid framing; indexes start at zero.
    Framing {
        /// Index of the constituent stream.
        stream: usize,
        /// Underlying packet framing failure.
        error: PacketError,
    },
    /// Constituent streams have different total durations.
    DurationMismatch,
    /// Requested loss/FEC duration is not120..5760 in120-sample steps.
    InvalidDuration,
    /// A constituent stream could not decode; the complete owner is unchanged.
    Decode {
        /// Zero-based constituent stream index.
        stream: usize,
        /// Underlying audio decoding failure.
        error: DecoderError,
    },
    /// Decoded PCM geometry differs from validated stream geometry.
    InvalidOutput,
    /// PCM length does not match mapped input geometry and duration.
    InvalidInput,
    /// Whole-packet capacity is outside resource or minimum framing bounds.
    InvalidBudget,
    /// Common controls or nominal total bitrate are invalid.
    InvalidControl,
    /// A constituent stream could not encode; the complete owner is unchanged.
    Encode {
        /// Zero-based constituent stream index.
        stream: usize,
        /// Underlying encoding or output framing failure.
        error: crate::encoder::EncoderError,
    },
}
impl fmt::Display for MultistreamError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidLayout => f.write_str("invalid Opus multistream layout"),
            Self::InvalidChannelMap => f.write_str("invalid Opus channel mapping index"),
            Self::IncompleteEncoderMap => f.write_str("Opus encoder map omits a coded channel"),
            Self::PacketTooLarge => f.write_str("Opus multistream packet exceeds16MiB"),
            Self::Framing { stream, error } => write!(f, "Opus stream {stream}: {error}"),
            Self::DurationMismatch => f.write_str("Opus stream durations differ"),
            Self::InvalidDuration => {
                f.write_str("Opus loss/FEC duration must be120..5760 in120-sample steps")
            }
            Self::Decode { stream, error } => write!(f, "Opus stream {stream}: {error}"),
            Self::InvalidOutput => f.write_str("inconsistent multistream PCM geometry"),
            Self::InvalidInput => f.write_str("invalid multistream input PCM geometry"),
            Self::InvalidBudget => f.write_str("invalid multistream whole-packet capacity"),
            Self::InvalidControl => f.write_str("invalid multistream encoding controls"),
            Self::Encode { stream, error } => write!(f, "Opus stream {stream}: {error}"),
        }
    }
}
impl std::error::Error for MultistreamError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Framing { error, .. } => Some(error),
            Self::Decode { error, .. } => Some(error),
            Self::Encode { error, .. } => Some(error),
            _ => None,
        }
    }
}

/// A validated classic channel map for1..255 output channels.
///
/// Coded channel indexes0..`2*coupled` belong to coupled streams, followed by
/// one channel for each mono stream. Repeated/omitted indexes are valid for
/// decoding;255 produces silence. This owns at most255 mapping bytes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChannelMapping {
    streams: usize,
    coupled: usize,
    mapping: Vec<u8>,
}
impl ChannelMapping {
    /// Validate and own a map without changing or normalizing its entries.
    pub fn new(streams: usize, coupled: usize, mapping: &[u8]) -> Result<Self, MultistreamError> {
        validate_counts(streams, coupled)?;
        if mapping.is_empty() || mapping.len() > 255 {
            return Err(MultistreamError::InvalidLayout);
        }
        let coded_channels = streams + coupled;
        if mapping
            .iter()
            .any(|&n| n != 255 && usize::from(n) >= coded_channels)
        {
            return Err(MultistreamError::InvalidChannelMap);
        }
        Ok(Self {
            streams,
            coupled,
            mapping: mapping.to_vec(),
        })
    }
    /// Number of output/input channels in the map.
    pub fn channels(&self) -> usize {
        self.mapping.len()
    }
    /// Total number of constituent streams.
    pub fn streams(&self) -> usize {
        self.streams
    }
    /// Number of initial streams that use a stereo decoder/encoder.
    pub fn coupled_streams(&self) -> usize {
        self.coupled
    }
    /// Number of coded channels before output mapping.
    pub fn coded_channels(&self) -> usize {
        self.streams + self.coupled
    }
    /// Mapping in interleaved PCM channel order;255 denotes silence.
    pub fn mapping(&self) -> &[u8] {
        &self.mapping
    }
    /// Check the stricter encoder requirement that every coded channel has an
    /// input occurrence. Repeated entries use the first occurrence, matching
    /// native generic multistream input selection; silence inputs are ignored.
    pub fn validate_encoder(&self) -> Result<(), MultistreamError> {
        let mut present = [false; 255];
        for &entry in &self.mapping {
            if entry != 255 {
                present[usize::from(entry)] = true;
            }
        }
        if present[..self.coded_channels()].contains(&false) {
            return Err(MultistreamError::IncompleteEncoderMap);
        }
        Ok(())
    }
}
fn validate_counts(streams: usize, coupled: usize) -> Result<(), MultistreamError> {
    if streams == 0 || streams > 255 || coupled > streams || coupled > 255 - streams {
        Err(MultistreamError::InvalidLayout)
    } else {
        Ok(())
    }
}

/// Borrowed frame views for1..255 duration-matched constituent streams.
///
/// Heap storage holds bounded packet views, never a copy of compressed payload.
/// Trailing padding is consumed per subpacket. Constituent frame counts and TOC
/// configurations may differ when their total durations agree.
#[derive(Debug)]
pub struct MultistreamPacket<'a> {
    streams: Vec<Packet<'a>>,
    duration: u16,
}
impl<'a> MultistreamPacket<'a> {
    /// Parse exactly one externally delimited multistream packet.
    pub fn parse(input: &'a [u8], streams: usize) -> Result<Self, MultistreamError> {
        validate_counts(streams, 0)?;
        if input.len() > MAX_PACKET_BYTES {
            return Err(MultistreamError::PacketTooLarge);
        }
        let mut remaining = input;
        let mut packets = Vec::with_capacity(streams);
        let mut duration = 0;
        for stream in 0..streams {
            let (packet, consumed) = if stream + 1 == streams {
                (
                    Packet::parse(remaining)
                        .map_err(|error| MultistreamError::Framing { stream, error })?,
                    remaining.len(),
                )
            } else {
                Packet::parse_self_delimited(remaining)
                    .map_err(|error| MultistreamError::Framing { stream, error })?
            };
            if stream != 0 && packet.duration_samples() != duration {
                return Err(MultistreamError::DurationMismatch);
            }
            duration = packet.duration_samples();
            remaining = &remaining[consumed..];
            packets.push(packet);
        }
        Ok(Self {
            streams: packets,
            duration,
        })
    }
    /// Constituent packets, in encoded stream order.
    pub fn streams(&self) -> &[Packet<'a>] {
        &self.streams
    }
    /// Common duration in samples on the48kHz clock (at most5760).
    pub fn duration_samples(&self) -> u16 {
        self.duration
    }
}

/// At most120ms of mapped interleaved signed16 PCM, owned on the heap.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MultistreamPcm {
    values: Vec<i16>,
    rate: OutputRate,
    channels: usize,
    range: u32,
}
impl MultistreamPcm {
    /// Signed16 samples in channel-map order, interleaved per output sample.
    pub fn samples(&self) -> &[i16] {
        &self.values
    }
    /// Output sampling rate in Hz.
    pub fn sample_rate(&self) -> u32 {
        self.rate.hz()
    }
    /// Number of mapped PCM channels.
    pub fn channels(&self) -> usize {
        self.channels
    }
    /// Number of PCM samples per mapped channel.
    pub fn samples_per_channel(&self) -> usize {
        self.values.len() / self.channels
    }
    /// XOR of constituent final ranges, including omitted or silent outputs.
    pub fn final_range(&self) -> u32 {
        self.range
    }
}

/// Persistent classic multistream decoding with complete-owner transactions.
///
/// Each stream retains independent SILK/CELT histories. Any returned error
/// preserves every stream and the last successful duration/range, even when a
/// later stream fails after earlier streams decode. State and mapped PCM use
/// bounded heap storage (at most255 streams/channels,5760 samples/channel).
/// Signed Q8 dB output gain applies equally to every constituent decoder.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MultistreamDecoder {
    layout: ChannelMapping,
    rate: OutputRate,
    streams: Vec<OpusDecoder>,
    duration: u16,
    range: u32,
    gain_q8: i16,
}
impl MultistreamDecoder {
    /// Construct persistent decoders at any standard output sampling rate.
    pub fn new(rate: OutputRate, layout: ChannelMapping) -> Result<Self, MultistreamError> {
        let mut streams = Vec::with_capacity(layout.streams());
        for stream in 0..layout.streams() {
            let channels = if stream < layout.coupled_streams() {
                PcmChannels::Stereo
            } else {
                PcmChannels::Mono
            };
            streams.push(
                OpusDecoder::new(rate, channels)
                    .map_err(|error| MultistreamError::Decode { stream, error })?,
            );
        }
        Ok(Self {
            layout,
            rate,
            streams,
            duration: 0,
            range: 0,
            gain_q8: 0,
        })
    }
    /// Fixed standard PCM output rate.
    pub fn output_rate(&self) -> OutputRate {
        self.rate
    }
    /// Fixed validated channel map.
    pub fn channel_mapping(&self) -> &ChannelMapping {
        &self.layout
    }
    /// Common signed Q8 dB output gain, initially zero and retained by reset.
    pub fn gain_q8(&self) -> i16 {
        self.gain_q8
    }
    /// Set the same native output gain for every stream without resetting audio
    /// histories. Every signed16 value is valid; silent mapped outputs stay zero.
    pub fn set_gain_q8(&mut self, gain: i16) {
        for stream in &mut self.streams {
            stream.set_gain_q8(gain);
        }
        self.gain_q8 = gain;
    }
    /// Last successful operation's XOR of constituent final ranges.
    pub fn final_range(&self) -> u32 {
        self.range
    }
    /// Last successful operation's duration on the48kHz clock, zero initially.
    pub fn last_packet_duration(&self) -> u16 {
        self.duration
    }
    /// Reset every history while retaining output rate and channel map.
    pub fn reset(&mut self) {
        for stream in &mut self.streams {
            stream.reset();
        }
        self.duration = 0;
        self.range = 0;
    }
    /// Decode one complete externally delimited multistream transport packet.
    /// Empty input is invalid; use [`Self::conceal_samples`] for packet loss.
    pub fn decode(&mut self, data: &[u8]) -> Result<MultistreamPcm, MultistreamError> {
        let packet = MultistreamPacket::parse(data, self.streams.len())?;
        self.run(Some(&packet), packet.duration_samples(), false)
    }
    /// Conceal a whole missing packet for120..5760 samples on the48kHz clock,
    /// in120-sample steps. Mapped silent channels remain exactly zero.
    pub fn conceal_samples(&mut self, duration: u16) -> Result<MultistreamPcm, MultistreamError> {
        self.run(None, duration, false)
    }
    /// Recover a missing interval with each stream's LBRR, falling back to PLC
    /// where unavailable. Decode the received packet normally afterwards.
    pub fn decode_fec(
        &mut self,
        data: &[u8],
        duration: u16,
    ) -> Result<MultistreamPcm, MultistreamError> {
        validate_duration(duration)?;
        let packet = MultistreamPacket::parse(data, self.streams.len())?;
        self.run(Some(&packet), duration, true)
    }
    fn run(
        &mut self,
        packet: Option<&MultistreamPacket<'_>>,
        duration: u16,
        fec: bool,
    ) -> Result<MultistreamPcm, MultistreamError> {
        validate_duration(duration)?;
        let samples = usize::from(duration) * self.rate.hz() as usize / 48000;
        let channels = self.layout.channels();
        let mut output = MultistreamPcm {
            values: vec![0; samples * channels],
            rate: self.rate,
            channels,
            range: 0,
        };
        let mut next = self.clone();
        for (index, stream) in next.streams.iter_mut().enumerate() {
            let pcm = match packet {
                Some(packet) if fec => stream.decode_fec_packet(&packet.streams()[index], duration),
                Some(packet) => stream.decode_packet(&packet.streams()[index]),
                None => stream.conceal_samples(duration),
            }
            .map_err(|error| MultistreamError::Decode {
                stream: index,
                error,
            })?;
            let width = if index < self.layout.coupled_streams() {
                2
            } else {
                1
            };
            let first = if width == 2 {
                2 * index
            } else {
                index + self.layout.coupled_streams()
            };
            if pcm.channels() != width
                || pcm.samples_per_channel() != samples
                || pcm.sample_rate() != self.rate.hz()
            {
                return Err(MultistreamError::InvalidOutput);
            }
            for (channel, &mapping) in self.layout.mapping().iter().enumerate() {
                let coded = usize::from(mapping);
                if mapping != 255 && coded >= first && coded < first + width {
                    for (destination, source) in output
                        .values
                        .chunks_exact_mut(channels)
                        .zip(pcm.samples().chunks_exact(width))
                    {
                        destination[channel] = source[coded - first];
                    }
                }
            }
            output.range ^= pcm.final_range();
        }
        next.range = output.range;
        next.duration = duration;
        *self = next;
        Ok(output)
    }
}
fn validate_duration(duration: u16) -> Result<(), MultistreamError> {
    if !(120..=5760).contains(&duration) || duration % 120 != 0 {
        Err(MultistreamError::InvalidDuration)
    } else {
        Ok(())
    }
}
