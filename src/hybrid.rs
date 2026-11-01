//! Normal Hybrid packets: shared SILK low-band and CELT high-band entropy.
//!
//! Output is canonical signed16 PCM. Cross-mode transitions, CELT redundancy
//! tails, loss/DTX and FEC are handled separately and are not yet supported here.
//! Copyright (c) 2010 Xiph.Org Foundation, Skype Limited; 2024 Arm Limited.
//! Written by Jean-Marc Valin and Koen Vos. See THIRD_PARTY_NOTICES.md.
use crate::{
    Mode, Packet, PacketError,
    celt::{
        FrameSize,
        decoder::{CeltDecoder, DecoderError as CeltError, FrameConfig},
        output::Decimation,
    },
    range::RangeError,
    silk::{
        self,
        decoder::{DecoderError as SilkError, PcmChannels, SilkDecoder},
        pitch::SilkRate,
        resampler::OutputRate,
    },
};
use std::fmt;

/// A Hybrid operation fails atomically, including both layers and entropy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum DecoderError {
    /// Invalid externally delimited packet.
    Framing(PacketError),
    /// This decoder accepts Hybrid packets only.
    UnsupportedMode,
    /// Empty/one-byte payload needs loss/DTX orchestration.
    MissingPayload,
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
            Self::MissingPayload => f.write_str("Hybrid loss/DTX orchestration is not implemented"),
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

/// Fixed-storage normal Hybrid decoder; every complete packet is transactional.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HybridDecoder {
    silk: SilkDecoder,
    celt: CeltDecoder,
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
    }
    /// Last successfully decoded frame's public final-range diagnostic.
    pub fn final_range(&self) -> u32 {
        self.celt.final_range()
    }
    /// Decode normal Hybrid audio. Transition tails and loss/DTX return errors.
    pub fn decode(&mut self, data: &[u8]) -> Result<DecodedPcm, DecoderError> {
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
        let mut output = DecodedPcm {
            values: [0; 11520],
            length: 0,
            rate: self.output_rate(),
            channels: self.output_channels(),
            range: 0,
        };
        for (index, payload) in packet.frames().iter().enumerate() {
            if payload.len() <= 1 {
                return Err(DecoderError::MissingPayload);
            }
            let (header, mut entropy) =
                silk::decode_header(&packet, index).map_err(DecoderError::Header)?;
            let low = next
                .silk
                .decode_layer(&header, &mut entropy, SilkRate::Khz16, false)
                .map_err(DecoderError::Silk)?;
            // Hybrid reserves a presence bit only with room for the complete
            // transition syntax. Keep the same precondition as opus_decode_frame.
            if entropy.tell() + 37 <= payload.len() as u32 * 8
                && entropy.decode_bit_logp(12).map_err(DecoderError::Entropy)?
            {
                return Err(DecoderError::UnsupportedTransition);
            }
            let high = next
                .celt
                .decode_layer(&mut entropy, config, 17)
                .map_err(DecoderError::Celt)?;
            let end = output.length + low.samples().len();
            if low.samples().len() != high.samples().len() || end > output.values.len() {
                return Err(DecoderError::InvalidOutput);
            }
            // Canonical FIXED_POINT without RES24 applies SIG2RES (round and
            // saturate CELT to signed16) before ADD_RES saturates the layer sum.
            for (target, (&low, &high)) in output.values[output.length..end]
                .iter_mut()
                .zip(low.samples().iter().zip(high.samples()))
            {
                *target = (i32::from(low) + i32::from(high)).clamp(-32768, 32767) as i16;
            }
            output.length = end;
        }
        output.range = next.final_range();
        *self = next;
        Ok(output)
    }
}
