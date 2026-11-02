//! Hybrid packets, loss/DTX and FEC through shared SILK/CELT synthesis.
//!
//! Output is canonical signed16 PCM. Cross-mode transitions and redundant CELT
//! transition tails use [`crate::decoder::OpusDecoder`] instead of this subset API.
//! Copyright (c) 2010 Xiph.Org Foundation, Skype Limited; 2024 Arm Limited.
//! Written by Jean-Marc Valin and Koen Vos. See THIRD_PARTY_NOTICES.md.
use crate::{
    Bandwidth, Mode, Packet, PacketError,
    celt::{
        FrameSize,
        decoder::{CeltDecoder, DecoderError as CeltError, FrameConfig},
        output::Decimation,
    },
    range::RangeError,
    silk::{
        self,
        decoder::{DecoderError as SilkError, PcmChannels, SilkDecoder},
        gains::FrameDuration,
        pitch::SilkRate,
        resampler::OutputRate,
    },
};
use std::fmt;

pub mod encoder;

/// A Hybrid operation fails atomically, including both layers and entropy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum DecoderError {
    /// Invalid externally delimited packet.
    Framing(PacketError),
    /// This decoder accepts Hybrid packets only.
    UnsupportedMode,
    /// Legacy missing-payload diagnostic; empty/one-byte frames now invoke PLC.
    MissingPayload,
    /// Loss duration must be120..5760 samples on a48kHz clock, in120-sample steps.
    InvalidLossDuration,
    /// A redundant CELT transition tail requires transition reconstruction.
    UnsupportedTransition,
    /// SILK header parsing failed.
    Header(silk::SilkError),
    /// Low-band reconstruction failed.
    Silk(SilkError),
    /// High-band reconstruction failed.
    Celt(CeltError),
    /// Shared entropy decoding failed.
    Entropy(RangeError),
    /// Layer PCM lengths or packet duration are inconsistent.
    InvalidOutput,
}
impl fmt::Display for DecoderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Framing(e) => write!(f, "Hybrid framing: {e}"),
            Self::UnsupportedMode => f.write_str("Hybrid packets required"),
            Self::MissingPayload => f.write_str("missing Hybrid payload"),
            Self::InvalidLossDuration => {
                f.write_str("Hybrid loss duration must be120..5760 samples in120-sample steps")
            }
            Self::UnsupportedTransition => {
                f.write_str("Hybrid CELT transition tail is not implemented")
            }
            Self::Header(e) => write!(f, "Hybrid SILK header: {e}"),
            Self::Silk(e) => write!(f, "Hybrid SILK: {e}"),
            Self::Celt(e) => write!(f, "Hybrid CELT: {e}"),
            Self::Entropy(e) => write!(f, "Hybrid entropy: {e}"),
            Self::InvalidOutput => f.write_str("inconsistent Hybrid output geometry"),
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

/// At most120ms of interleaved signed16 PCM, with final-range diagnostics.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DecodedPcm {
    values: [i16; 11520],
    length: usize,
    rate: OutputRate,
    channels: PcmChannels,
    range: u32,
}
impl DecodedPcm {
    /// Interleaved output samples.
    pub fn samples(&self) -> &[i16] {
        &self.values[..self.length]
    }
    /// Selected output sampling rate in Hz.
    pub fn sample_rate(&self) -> u32 {
        self.rate.hz()
    }
    /// Selected channel count, independent of the encoded channel count.
    pub fn channels(&self) -> usize {
        self.channels.count()
    }
    /// Samples per output channel.
    pub fn samples_per_channel(&self) -> usize {
        self.length / self.channels.count()
    }
    /// Last frame's final range.
    pub fn final_range(&self) -> u32 {
        self.range
    }
}

/// Fixed-storage Hybrid decoder; packet, loss and FEC calls are transactional.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HybridDecoder {
    silk: SilkDecoder,
    celt: CeltDecoder,
    initialized: bool,
    last_size: FrameSize,
    range: u32,
}
impl HybridDecoder {
    /// Start a stream at any standard Opus PCM rate and one/two output channels.
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
            initialized: false,
            last_size: FrameSize::Ms2_5,
            range: 0,
            celt: CeltDecoder::new(decimation, channels.count()).map_err(DecoderError::Celt)?,
        })
    }
    /// Fixed output sampling rate.
    pub fn output_rate(&self) -> OutputRate {
        self.silk.output_rate()
    }
    /// Fixed output channel count.
    pub fn output_channels(&self) -> PcmChannels {
        self.silk.output_channels()
    }
    /// Inspect the high-band synthesis and energy histories.
    pub fn celt(&self) -> &CeltDecoder {
        &self.celt
    }
    /// Clear histories while retaining output geometry and CELT band limits.
    pub fn reset(&mut self) {
        self.silk.reset();
        self.celt.reset();
        self.initialized = false;
        self.last_size = FrameSize::Ms2_5;
        self.range = 0;
    }
    /// Last successfully decoded frame's public final-range diagnostic.
    pub fn final_range(&self) -> u32 {
        self.range
    }
    /// Decode normal Hybrid audio or implicit empty/one-byte loss/DTX frames.
    /// CELT transition tails still require unified transition reconstruction.
    pub fn decode(&mut self, data: &[u8]) -> Result<DecodedPcm, DecoderError> {
        self.packet(data, false)
    }
    /// Recover the preceding first Opus-frame interval from in-band FEC.
    /// Missing LBRR uses SILK PLC/CNG; high bands always use CELT noise PLC.
    /// Decode the received packet normally afterwards to obtain primary audio.
    pub fn decode_fec(&mut self, data: &[u8]) -> Result<DecodedPcm, DecoderError> {
        self.packet(data, true)
    }
    /// Conceal120..5760 samples on a48kHz clock, in120-sample steps.
    /// Scheduling follows the last packet clock; initial loss produces silence.
    /// Sub-10ms intervals advance a full10ms SILK frame and truncate its output.
    pub fn conceal_samples(&mut self, duration: u16) -> Result<DecodedPcm, DecoderError> {
        if !(120..=5760).contains(&duration) || duration % 120 != 0 {
            return Err(DecoderError::InvalidLossDuration);
        }
        let mut next = self.clone();
        let mut output = self.empty_output();
        let mut remaining = usize::from(duration);
        while remaining > 0 {
            let limit = remaining.min(next.last_size.samples()).min(960);
            let size = if limit >= 960 {
                FrameSize::Ms20
            } else if limit >= 480 {
                FrameSize::Ms10
            } else if limit >= 240 {
                FrameSize::Ms5
            } else {
                FrameSize::Ms2_5
            };
            next.loss_interval(size, &mut output)?;
            remaining -= size.samples();
        }
        *self = next;
        Ok(output)
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
    fn accumulate(output: &mut DecodedPcm, low: &[i16], high: &[i16]) -> Result<(), DecoderError> {
        let end = output.length + high.len();
        if low.len() < high.len() || end > output.values.len() {
            return Err(DecoderError::InvalidOutput);
        }
        // Canonical FIXED_POINT without RES24 applies SIG2RES (round and
        // saturate CELT to signed16) before ADD_RES saturates the layer sum.
        for (target, (&low, &high)) in output.values[output.length..end]
            .iter_mut()
            .zip(low.iter().zip(high))
        {
            *target = (i32::from(low) + i32::from(high)).clamp(-32768, 32767) as i16;
        }
        output.length = end;
        Ok(())
    }
    fn loss_interval(
        &mut self,
        size: FrameSize,
        output: &mut DecodedPcm,
    ) -> Result<(), DecoderError> {
        if !self.initialized {
            let count = size.samples() * self.output_rate().hz() as usize / 48000
                * self.output_channels().count();
            let end = output.length + count;
            if end > output.values.len() {
                return Err(DecoderError::InvalidOutput);
            }
            output.values[output.length..end].fill(0);
            output.length = end;
        } else {
            let duration = if size == FrameSize::Ms20 {
                FrameDuration::Ms20
            } else {
                FrameDuration::Ms10
            };
            let low = self
                .silk
                .conceal_layer(duration)
                .map_err(DecoderError::Silk)?;
            let high = self
                .celt
                .conceal_layer(size, self.celt.concealment().end_band())
                .map_err(DecoderError::Celt)?;
            Self::accumulate(output, low.samples(), high.samples())?;
        }
        self.range = 0;
        output.range = 0;
        Ok(())
    }
    fn packet(&mut self, data: &[u8], fec: bool) -> Result<DecodedPcm, DecoderError> {
        let packet = Packet::parse(data).map_err(DecoderError::Framing)?;
        if packet.mode() != Mode::Hybrid {
            return Err(DecoderError::UnsupportedMode);
        }
        let size = match packet.frame_samples() {
            480 => FrameSize::Ms10,
            960 => FrameSize::Ms20,
            _ => return Err(DecoderError::InvalidOutput),
        };
        let config = FrameConfig {
            size,
            channels: usize::from(packet.channels()),
            bandwidth: packet.bandwidth(),
        };
        let mut next = self.clone();
        let mut output = self.empty_output();
        next.last_size = size;
        let count = if fec { 1 } else { packet.frames().len() };
        for (index, payload) in packet.frames()[..count].iter().enumerate() {
            if payload.len() <= 1 {
                next.loss_interval(size, &mut output)?;
                continue;
            }
            let (header, mut entropy) =
                silk::decode_header(&packet, index).map_err(DecoderError::Header)?;
            let low = next
                .silk
                .decode_layer(&header, &mut entropy, SilkRate::Khz16, fec)
                .map_err(DecoderError::Silk)?;
            let high = if fec {
                let end = if config.bandwidth == Bandwidth::SuperWideband {
                    19
                } else {
                    21
                };
                let high = next
                    .celt
                    .conceal_layer(size, end)
                    .map_err(DecoderError::Celt)?;
                // Native GET_FINAL_RANGE returns persistent CELT RNG for FEC,
                // while explicit/implicit data loss reports public range zero.
                next.range = next.celt.rng_state();
                high
            } else {
                // Reserve the transition presence bit only with room for its
                // complete syntax, as opus_decode_frame does.
                if entropy.tell() + 37 <= payload.len() as u32 * 8
                    && entropy.decode_bit_logp(12).map_err(DecoderError::Entropy)?
                {
                    return Err(DecoderError::UnsupportedTransition);
                }
                let high = next
                    .celt
                    .decode_layer(&mut entropy, config, 17)
                    .map_err(DecoderError::Celt)?;
                next.range = next.celt.final_range();
                high
            };
            Self::accumulate(&mut output, low.samples(), high.samples())?;
            next.initialized = true;
        }
        output.range = next.range;
        *self = next;
        Ok(output)
    }
}
