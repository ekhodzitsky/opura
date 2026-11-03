//! Generic classic multistream encoding and native rate/budget allocation.
//! Copyright (c) 2011 Xiph.Org Foundation; written by Jean-Marc Valin.
//! See THIRD_PARTY_NOTICES.md. Surround energy masks/LFE are separate work.
use super::{ChannelMapping, MAX_PACKET_BYTES, MultistreamError};
use crate::{
    celt::input::InputRate,
    encoder::{
        EncodeOptions, OpusEncoder, PacketSize,
        control::{Application, Controls},
    },
    repacketizer::Repacketizer,
};

/// Common controls and total nominal bitrate for a mapped packet.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MultistreamEncodeOptions {
    /// Positive total target rate, at most191250000bps. Native normalization
    /// clamps to500..750000bps per mapped input channel before allocation.
    pub bitrate: u32,
    /// Controls forwarded to every constituent stream. Application must match
    /// construction; a channel override must be valid for each selected stream.
    pub controls: Controls,
    /// Use constrained VBR when variable rate is selected.
    pub constrained: bool,
}
impl Default for MultistreamEncodeOptions {
    fn default() -> Self {
        Self {
            bitrate: 64000,
            controls: Controls::default(),
            constrained: false,
        }
    }
}
/// One complete externally delimited multistream transport packet.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MultistreamEncodedPacket {
    bytes: Vec<u8>,
    range: u32,
    rates: Vec<u32>,
    plc: bool,
}
impl MultistreamEncodedPacket {
    /// Transport bytes, with every non-final stream self-delimited.
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }
    /// XOR of all constituent encoder final ranges.
    pub fn final_range(&self) -> u32 {
        self.range
    }
    /// Nominal rates passed to stream APIs after allocation/channel clamps and
    /// final CBR budget adjustment. These are not physical output bitrates.
    pub fn stream_bitrates(&self) -> &[u32] {
        &self.rates
    }
    /// Whether any constituent deliberately emitted native PLC signalling.
    pub fn used_plc(&self) -> bool {
        self.plc
    }
}
/// Persistent generic mapped encoder with complete-owner transactions.
///
/// Uses first occurrences of mapped input channels; duplicates and silent input
/// slots are ignored. All coded channels must have an input occurrence. Rates
/// and whole-packet reservations follow native generic multistream behavior.
/// This does not implement the separate surround energy-mask/LFE analysis.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MultistreamEncoder {
    layout: ChannelMapping,
    rate: InputRate,
    application: Application,
    streams: Vec<OpusEncoder>,
    sources: Vec<[usize; 2]>,
}
impl MultistreamEncoder {
    /// Construct an AUDIO generic mapped encoder at a standard input rate.
    pub fn new(rate: InputRate, layout: ChannelMapping) -> Result<Self, MultistreamError> {
        Self::with_application(rate, layout, Application::Audio)
    }
    /// Construct with an immutable application shared by all streams.
    pub fn with_application(
        rate: InputRate,
        layout: ChannelMapping,
        application: Application,
    ) -> Result<Self, MultistreamError> {
        layout.validate_encoder()?;
        let mut streams = Vec::with_capacity(layout.streams());
        let mut sources = Vec::with_capacity(layout.streams());
        for stream in 0..layout.streams() {
            let width = if stream < layout.coupled_streams() {
                2
            } else {
                1
            };
            let first = if width == 2 {
                2 * stream
            } else {
                stream + layout.coupled_streams()
            };
            let find = |coded| {
                layout
                    .mapping()
                    .iter()
                    .position(|&n| usize::from(n) == coded)
                    .ok_or(MultistreamError::IncompleteEncoderMap)
            };
            let left = find(first)?;
            let right = if width == 2 { find(first + 1)? } else { left };
            sources.push([left, right]);
            streams.push(
                OpusEncoder::with_application(rate, width, application)
                    .map_err(|error| MultistreamError::Encode { stream, error })?,
            );
        }
        Ok(Self {
            layout,
            rate,
            application,
            streams,
            sources,
        })
    }
    /// Immutable standard input sampling rate.
    pub fn input_rate(&self) -> InputRate {
        self.rate
    }
    /// Immutable input/output channel map.
    pub fn channel_mapping(&self) -> &ChannelMapping {
        &self.layout
    }
    /// Shared immutable application.
    pub fn application(&self) -> Application {
        self.application
    }
    /// Common native lookahead on the48kHz clock.
    pub fn lookahead_48k(&self) -> u32 {
        if matches!(
            self.application,
            Application::RestrictedLowDelay | Application::RestrictedCelt
        ) {
            120
        } else {
            312
        }
    }
    /// Reset all histories together, retaining immutable layout/rate/application.
    pub fn reset(&mut self) -> Result<(), MultistreamError> {
        *self = Self::with_application(self.rate, self.layout.clone(), self.application)?;
        Ok(())
    }
    /// Encode exactly one standard2.5..120ms packet from mapped interleaved PCM.
    ///
    /// The whole-packet cap includes all size headers and final CBR padding and
    /// is bounded by16MiB. CBR rounds total bitrate to whole bytes, with native
    /// minimum framing size. Every returned error preserves all histories;
    /// successful output is published only after every stream and framing write.
    pub fn encode_packet(
        &mut self,
        size: PacketSize,
        input: &[i16],
        packet_bytes: usize,
        options: MultistreamEncodeOptions,
    ) -> Result<MultistreamEncodedPacket, MultistreamError> {
        if input.len() != size.samples() / self.rate.factor() * self.layout.channels() {
            return Err(MultistreamError::InvalidInput);
        }
        let count = self.layout.streams();
        let extra = usize::from(size == PacketSize::Ms100);
        let minimum = 2 * count - 1 + extra * count;
        if !(minimum..=MAX_PACKET_BYTES).contains(&packet_bytes) {
            return Err(MultistreamError::InvalidBudget);
        }
        let bitrate = normalize_bitrate(&self.layout, options.bitrate)?;
        let budget = if options.controls.variable {
            packet_bytes
        } else {
            ((((u64::from(bitrate) * size.samples() as u64 / 48000) + 4) / 8) as usize)
                .clamp(minimum, packet_bytes)
        };
        let allocation = allocate(&self.layout, size, bitrate);
        let mut next = self.clone();
        let mut output = MultistreamEncodedPacket {
            bytes: vec![0; budget],
            range: 0,
            rates: Vec::with_capacity(count),
            plc: false,
        };
        let samples = size.samples() / self.rate.factor();
        let mut cursor = 0;
        for (index, stream) in next.streams.iter_mut().enumerate() {
            let remaining = count - index - 1;
            let reserve = (2 * remaining).saturating_sub(1) + extra * remaining;
            let mut current = budget
                .checked_sub(cursor + reserve)
                .ok_or(MultistreamError::InvalidBudget)?
                .min(7662);
            if remaining != 0 {
                current = current
                    .checked_sub(if current > 253 { 2 } else { 1 })
                    .ok_or(MultistreamError::InvalidBudget)?;
            }
            let width = if index < self.layout.coupled_streams() {
                2
            } else {
                1
            };
            let nominal = if remaining == 0 && !options.controls.variable {
                (current as u64 * 8 * 48000 / size.samples() as u64) as u32
            } else {
                allocation[index]
            };
            let nominal = nominal.clamp(500, 750000 * width as u32);
            let source = self.sources[index];
            let mut pcm = Vec::with_capacity(samples * width);
            for frame in input.chunks_exact(self.layout.channels()) {
                pcm.push(frame[source[0]]);
                if width == 2 {
                    pcm.push(frame[source[1]]);
                }
            }
            let cap = current.min(if size.samples() > 960 { 7662 } else { 1276 });
            let packet = stream
                .encode_packet(
                    size,
                    &pcm,
                    cap,
                    EncodeOptions {
                        bitrate: nominal,
                        controls: options.controls,
                        constrained: options.constrained,
                    },
                )
                .map_err(|error| MultistreamError::Encode {
                    stream: index,
                    error,
                })?;
            let mut rp = Repacketizer::new();
            let framing = |error| MultistreamError::Encode {
                stream: index,
                error: crate::encoder::EncoderError::Framing(error),
            };
            rp.add_packet(packet.as_bytes()).map_err(framing)?;
            let written = if remaining != 0 {
                rp.write_self_delimited(&mut output.bytes[cursor..])
            } else if !options.controls.variable {
                rp.write_padded(&mut output.bytes[cursor..], budget - cursor)
            } else {
                rp.write(&mut output.bytes[cursor..])
            }
            .map_err(framing)?;
            cursor += written;
            output.range ^= packet.final_range();
            output.plc |= packet.used_plc();
            output.rates.push(nominal);
        }
        output.bytes.truncate(cursor);
        *self = next;
        Ok(output)
    }
}
fn normalize_bitrate(layout: &ChannelMapping, bitrate: u32) -> Result<u32, MultistreamError> {
    if !(1..=191250000).contains(&bitrate) {
        return Err(MultistreamError::InvalidControl);
    }
    Ok(bitrate.clamp(
        500 * layout.channels() as u32,
        750000 * layout.channels() as u32,
    ))
}
fn allocate(layout: &ChannelMapping, size: PacketSize, bitrate: u32) -> Vec<u32> {
    let channels = layout.coded_channels() as i64;
    let streams = layout.streams() as i64;
    let channel_offset = 40 * (48000 / size.samples()).max(50) as i64;
    let offset = ((i64::from(bitrate) - channel_offset * channels) / channels / 2).clamp(0, 20000);
    let channel_rate =
        (i64::from(bitrate) - offset * streams - channel_offset * channels) / channels;
    (0..layout.streams())
        .map(|index| {
            let width = if index < layout.coupled_streams() {
                2
            } else {
                1
            };
            (width * channel_offset + (offset + width * channel_rate).max(0)).max(500) as u32
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_native_generic_allocation_at_layout_rate_and_duration_boundaries()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut layout = None;
        let mut rows = 0;
        let mut layouts = 0;
        for line in include_str!("../../tests/multistream-rate-fixtures.txt")
            .lines()
            .filter(|l| !l.starts_with('#'))
        {
            let fields: Vec<_> = line.split_whitespace().collect();
            match fields[0] {
                "layout" => {
                    assert_eq!(fields.len(), 5);
                    let rate: u32 = fields[1].parse()?;
                    assert!([8000, 12000, 16000, 24000, 48000].contains(&rate));
                    let mapping: Vec<u8> = fields[4]
                        .split(',')
                        .map(str::parse)
                        .collect::<Result<_, _>>()?;
                    let selected =
                        ChannelMapping::new(fields[2].parse()?, fields[3].parse()?, &mapping)?;
                    selected.validate_encoder()?;
                    layout = Some(selected);
                    layouts += 1;
                }
                "rates" => {
                    let values: Vec<u32> = fields[1..]
                        .iter()
                        .map(|s| s.parse())
                        .collect::<Result<_, _>>()?;
                    let layout = layout.as_ref().ok_or("allocation without layout")?;
                    let size = PacketSize::try_from(values[0] as usize)?;
                    let normalized = normalize_bitrate(layout, values[1])?;
                    assert_eq!(normalized, values[2], "{line}");
                    let actual = allocate(layout, size, normalized);
                    assert_eq!(actual.len(), layout.streams());
                    assert_eq!(actual.as_slice(), &values[4..], "{line}");
                    assert_eq!(actual.iter().sum::<u32>(), values[3], "{line}");
                    rows += 1;
                }
                _ => return Err("invalid native allocation fixture".into()),
            }
        }
        assert_eq!(layouts, 40);
        assert_eq!(rows, 3240);
        let layout = ChannelMapping::new(1, 0, &[0])?;
        assert_eq!(
            normalize_bitrate(&layout, 0),
            Err(MultistreamError::InvalidControl)
        );
        assert_eq!(
            normalize_bitrate(&layout, u32::MAX),
            Err(MultistreamError::InvalidControl)
        );
        Ok(())
    }
}
