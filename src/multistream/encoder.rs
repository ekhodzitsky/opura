//! Generic and canonical surround multistream encoding with native budgets.
//! Copyright (c) 2011 Xiph.Org Foundation; written by Jean-Marc Valin.
//! See THIRD_PARTY_NOTICES.md. Surround analysis retains the documented Q24 correction.
use super::{ChannelMapping, MAX_PACKET_BYTES, MultistreamError};
use crate::{
    celt::analysis::SurroundState,
    celt::input::InputRate,
    encoder::{
        EncodeOptions, OpusEncoder, PacketSize, SurroundContext,
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
    /// Common controls. Application must match construction. Generic coding
    /// forwards controls to each stream. Canonical surround selects native
    /// bandwidth, forces coupled CELT/stereo (except restricted-SILK's mode
    /// constraint), and applies LFE mode/band policy. Mono overrides must remain
    /// valid for each selected stream.
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
/// Persistent mapped encoder with complete-owner transactions.
///
/// [`Self::with_application`] uses generic mapped coding: first occurrences of
/// input channels are selected, duplicates/silent slots are ignored, and every
/// coded channel needs an input occurrence. [`Self::with_surround_application`]
/// adds canonical Vorbis-role analysis, native surround rates and mask/LFE
/// processing. Both paths preserve complete histories on any error.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MultistreamEncoder {
    layout: ChannelMapping,
    rate: InputRate,
    application: Application,
    streams: Vec<OpusEncoder>,
    sources: Vec<[usize; 2]>,
    surround: Option<SurroundState>,
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
            surround: None,
        })
    }
    /// Construct native surround coding for a canonical1..8channel Vorbis layout.
    ///
    /// PCM is interleaved in Vorbis order, not WAVE order. Mono/stereo retain
    /// generic coding; larger layouts use persistent spectral masks, native
    /// surround stream policies and LFE allocation/coding when present. Q24
    /// analysis uses the documented full-precision correction. Whole-owner
    /// transactions include analysis, common owners and output framing.
    pub fn with_surround_application(
        rate: InputRate,
        channels: usize,
        application: Application,
    ) -> Result<Self, MultistreamError> {
        let (streams, coupled, mapping): (usize, usize, &[u8]) = match channels {
            1 => (1, 0, &[0]),
            2 => (1, 1, &[0, 1]),
            3 => (2, 1, &[0, 2, 1]),
            4 => (2, 2, &[0, 1, 2, 3]),
            5 => (3, 2, &[0, 4, 1, 2, 3]),
            6 => (4, 2, &[0, 4, 1, 2, 3, 5]),
            7 => (4, 3, &[0, 4, 1, 2, 3, 5, 6]),
            8 => (5, 3, &[0, 6, 1, 2, 3, 4, 5, 7]),
            _ => return Err(MultistreamError::InvalidLayout),
        };
        let mut result = Self::with_application(
            rate,
            ChannelMapping::new(streams, coupled, mapping)?,
            application,
        )?;
        if channels > 2 {
            result.surround =
                Some(SurroundState::new(rate, channels).map_err(MultistreamError::Analysis)?);
        }
        if channels >= 6 {
            result
                .streams
                .last_mut()
                .ok_or(MultistreamError::InvalidLayout)?
                .set_surround_context(SurroundContext::new(true, None, 1).map_err(|error| {
                    MultistreamError::Encode {
                        stream: streams - 1,
                        error: error.into(),
                    }
                })?);
        }
        Ok(result)
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
        *self = if self.surround.is_some() {
            Self::with_surround_application(self.rate, self.layout.channels(), self.application)?
        } else {
            Self::with_application(self.rate, self.layout.clone(), self.application)?
        };
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
        let lfe = self.surround.is_some() && self.layout.channels() >= 6;
        let allocation = if lfe {
            allocate_lfe(&self.layout, size, bitrate, true)
        } else {
            allocate(&self.layout, size, bitrate)
        };
        let mut next = self.clone();
        let masks = if self.application != Application::RestrictedSilk {
            next.surround
                .as_mut()
                .map(|state| state.analyze(size.samples(), input))
                .transpose()
                .map_err(MultistreamError::Analysis)?
        } else {
            None
        };
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
            let mut controls = options.controls;
            if next.surround.is_some() {
                let equivalent = i64::from(bitrate)
                    - 60 * ((48000 / size.samples()) as i64 - 50).max(0)
                        * self.layout.channels() as i64;
                controls.bandwidth = Some(if equivalent > 10000 * self.layout.channels() as i64 {
                    crate::Bandwidth::Fullband
                } else if equivalent > 7000 * self.layout.channels() as i64 {
                    crate::Bandwidth::SuperWideband
                } else if equivalent > 5000 * self.layout.channels() as i64 {
                    crate::Bandwidth::Wideband
                } else {
                    crate::Bandwidth::Narrowband
                });
                if width == 2 {
                    controls.mode = Some(crate::Mode::Celt);
                    controls.channels = Some(2);
                }
            }
            let mut stream_masks = [[0; 21]; 2];
            if let Some(masks) = &masks {
                stream_masks[0] = *masks
                    .channel(source[0])
                    .ok_or(MultistreamError::InvalidInput)?;
                if width == 2 {
                    stream_masks[1] = *masks
                        .channel(source[1])
                        .ok_or(MultistreamError::InvalidInput)?;
                }
            }
            stream.set_surround_context(
                SurroundContext::new(
                    lfe && remaining == 0,
                    masks.as_ref().map(|_| &stream_masks[..width]),
                    width,
                )
                .map_err(|error| MultistreamError::Encode {
                    stream: index,
                    error: error.into(),
                })?,
            );
            let packet = stream
                .encode_packet(
                    size,
                    &pcm,
                    cap,
                    EncodeOptions {
                        bitrate: nominal,
                        controls,
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
    allocate_lfe(layout, size, bitrate, false)
}
fn allocate_lfe(layout: &ChannelMapping, size: PacketSize, bitrate: u32, lfe: bool) -> Vec<u32> {
    let channels = layout.coded_channels() as i64 - i64::from(lfe);
    let streams = layout.streams() as i64;
    let channel_offset = 40 * (48000 / size.samples()).max(50) as i64;
    let lfe_offset = if lfe {
        (i64::from(bitrate) / 20).min(3000) + 15 * (48000 / size.samples()).max(50) as i64
    } else {
        0
    };
    let offset = ((i64::from(bitrate) - channel_offset * channels - lfe_offset) / channels / 2)
        .clamp(0, 20000);
    let channel_rate = 256
        * (i64::from(bitrate)
            - lfe_offset
            - offset * (streams - i64::from(lfe))
            - channel_offset * channels)
        / (256 * channels + if lfe { 32 } else { 0 });
    (0..layout.streams())
        .map(|index| {
            if lfe && index + 1 == layout.streams() {
                return (lfe_offset + ((channel_rate * 32) >> 8)).max(500) as u32;
            }
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
    fn original_native_surround_factory_and_lfe_rate_boundaries_match()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut layout = None;
        let mut rows = 0;
        for line in include_str!("../../tests/surround-rate-fixtures.txt")
            .lines()
            .filter(|l| !l.starts_with('#'))
        {
            let fields: Vec<_> = line.split_whitespace().collect();
            match fields[0] {
                "layout" => {
                    let rate = match fields[1] {
                        "8000" => InputRate::Khz8,
                        "12000" => InputRate::Khz12,
                        "16000" => InputRate::Khz16,
                        "24000" => InputRate::Khz24,
                        "48000" => InputRate::Khz48,
                        _ => return Err("invalid rate".into()),
                    };
                    let channels = fields[2].parse()?;
                    let owner = MultistreamEncoder::with_surround_application(
                        rate,
                        channels,
                        Application::Audio,
                    )?;
                    let mapping: Vec<u8> = fields[5]
                        .split(',')
                        .map(str::parse)
                        .collect::<Result<_, _>>()?;
                    assert_eq!(
                        owner.channel_mapping(),
                        &ChannelMapping::new(fields[3].parse()?, fields[4].parse()?, &mapping)?
                    );
                    layout = Some(owner.layout);
                }
                "rates" => {
                    let layout = layout.as_ref().ok_or("missing layout")?;
                    let values: Vec<u32> = fields[1..]
                        .iter()
                        .map(|v| v.parse())
                        .collect::<Result<_, _>>()?;
                    let size = PacketSize::try_from(values[0] as usize)?;
                    let normalized = normalize_bitrate(layout, values[1])?;
                    assert_eq!(normalized, values[2]);
                    let actual = allocate_lfe(layout, size, normalized, layout.channels() >= 6);
                    assert_eq!(actual.as_slice(), &values[4..], "{line}");
                    assert_eq!(actual.iter().sum::<u32>(), values[3]);
                    rows += 1;
                }
                _ => return Err("invalid native rate row".into()),
            }
        }
        assert_eq!(rows, 3240);
        Ok(())
    }
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
