//! Stateful normal CELT-only packet decoding to final signed-16 PCM.
//! Frame ordering/history follow pinned celt_decoder.c; loss/Hybrid are separate.
//! Copyright (c) 2007-2008 CSIRO; 2007-2010 Xiph.Org Foundation;
//! 2008 Gregory Maxwell. See THIRD_PARTY_NOTICES.md.
use super::{
    FrameSize,
    allocation::{self, AllocationError},
    control::{self, ControlError},
    energy::{BandLayout, EnergyError, EnergyState},
    output::{Decimation, PcmFrame},
    partition::{self, AntiCollapseError, PartitionError},
    synthesis::{CeltSynthesis, SynthesisControls, SynthesisError},
};
use crate::{
    Bandwidth, Mode, Packet, PacketError,
    range::{RangeDecoder, RangeError},
};
use std::fmt;

/// CELT packet/frame failure; all decoder state is preserved on any error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum DecoderError {
    /// Invalid externally delimited packet framing.
    Framing(PacketError),
    /// Only CELT-only normal packets are currently supported.
    UnsupportedMode,
    /// Encoded channels must be one/two and bandwidth a CELT mode.
    InvalidConfiguration,
    /// Empty/one-byte frames require loss/DTX support, not normal decoding.
    MissingPayload,
    /// Entropy tell exceeds the physical frame budget.
    BitBudgetExceeded,
    /// Control decoding failed.
    Control(ControlError),
    /// Energy reconstruction failed.
    Energy(EnergyError),
    /// Allocation failed.
    Allocation(AllocationError),
    /// Spectral reconstruction failed.
    Partition(PartitionError),
    /// Transient restoration failed.
    AntiCollapse(AntiCollapseError),
    /// PCM synthesis failed.
    Synthesis(SynthesisError),
    /// Entropy failed.
    Entropy(RangeError),
}
impl fmt::Display for DecoderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Framing(e) => write!(f, "CELT framing: {e}"),
            Self::UnsupportedMode => f.write_str("CELT-only normal packets required"),
            Self::InvalidConfiguration => f.write_str("invalid CELT frame configuration"),
            Self::MissingPayload => f.write_str("CELT normal frames require 2..1275 bytes"),
            Self::BitBudgetExceeded => f.write_str("CELT entropy exceeds physical frame budget"),
            Self::Control(e) => write!(f, "CELT control: {e}"),
            Self::Energy(e) => write!(f, "CELT energy: {e}"),
            Self::Allocation(e) => write!(f, "CELT allocation: {e}"),
            Self::Partition(e) => write!(f, "CELT spectrum: {e}"),
            Self::AntiCollapse(e) => write!(f, "CELT anti-collapse: {e}"),
            Self::Synthesis(e) => write!(f, "CELT synthesis: {e}"),
            Self::Entropy(e) => write!(f, "CELT entropy: {e}"),
        }
    }
}
impl std::error::Error for DecoderError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Framing(e) => Some(e),
            Self::Control(e) => Some(e),
            Self::Energy(e) => Some(e),
            Self::Allocation(e) => Some(e),
            Self::Partition(e) => Some(e),
            Self::AntiCollapse(e) => Some(e),
            Self::Synthesis(e) => Some(e),
            Self::Entropy(e) => Some(e),
            _ => None,
        }
    }
}
impl From<PacketError> for DecoderError {
    fn from(e: PacketError) -> Self {
        Self::Framing(e)
    }
}
impl From<ControlError> for DecoderError {
    fn from(e: ControlError) -> Self {
        Self::Control(e)
    }
}
impl From<EnergyError> for DecoderError {
    fn from(e: EnergyError) -> Self {
        Self::Energy(e)
    }
}
impl From<AllocationError> for DecoderError {
    fn from(e: AllocationError) -> Self {
        Self::Allocation(e)
    }
}
impl From<PartitionError> for DecoderError {
    fn from(e: PartitionError) -> Self {
        Self::Partition(e)
    }
}
impl From<AntiCollapseError> for DecoderError {
    fn from(e: AntiCollapseError) -> Self {
        Self::AntiCollapse(e)
    }
}
impl From<SynthesisError> for DecoderError {
    fn from(e: SynthesisError) -> Self {
        Self::Synthesis(e)
    }
}
impl From<RangeError> for DecoderError {
    fn from(e: RangeError) -> Self {
        Self::Entropy(e)
    }
}

/// Encoded CELT frame geometry, independent of fixed output channels/rate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FrameConfig {
    /// Internal 48kHz frame duration.
    pub size: FrameSize,
    /// Encoded channel count, one/two.
    pub channels: usize,
    /// NB/WB/SWB/FB bandwidth; mediumband is not a CELT-only packet mode.
    pub bandwidth: Bandwidth,
}
/// Up to 120ms of interleaved final PCM and the last frame's final-range value.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DecodedPcm {
    values: [i16; 11520],
    length: usize,
    rate: Decimation,
    channels: usize,
    range: u32,
}
impl DecodedPcm {
    /// Interleaved signed-16 output.
    pub fn samples(&self) -> &[i16] {
        &self.values[..self.length]
    }
    /// Output sample rate in Hz.
    pub fn sample_rate(&self) -> u32 {
        self.rate.sample_rate()
    }
    /// Output channel count, one/two.
    pub fn channels(&self) -> usize {
        self.channels
    }
    /// Samples per output channel.
    pub fn samples_per_channel(&self) -> usize {
        self.length / self.channels
    }
    /// Last compressed frame's public Opus final-range diagnostic.
    pub fn final_range(&self) -> u32 {
        self.range
    }
}
/// Fixed-output pure Rust normal CELT decoder, with allocation-free transactions.
/// Loss/DTX, Hybrid and cross-mode transition handling are not implemented here.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CeltDecoder {
    backend: CeltSynthesis,
    energy: EnergyState,
    previous: [[i32; 21]; 2],
    previous2: [[i32; 21]; 2],
    background: [[i32; 21]; 2],
    rng: u32,
    rate: Decimation,
}
impl CeltDecoder {
    /// Initialize an independent decoder; output rate/channels stay fixed.
    pub fn new(rate: Decimation, channels: usize) -> Result<Self, DecoderError> {
        Ok(Self {
            backend: CeltSynthesis::new(rate, channels)?,
            energy: EnergyState::default(),
            previous: [[-(28 << 24); 21]; 2],
            previous2: [[-(28 << 24); 21]; 2],
            background: [[0; 21]; 2],
            rng: 0,
            rate,
        })
    }
    /// Fixed PCM output channel count.
    pub fn channels(&self) -> usize {
        self.backend.channels()
    }
    /// Fixed PCM output rate in Hz.
    pub fn sample_rate(&self) -> u32 {
        self.rate.sample_rate()
    }
    /// Clear every history and diagnostic, retaining output geometry.
    pub fn reset(&mut self) {
        self.backend.reset();
        self.energy.reset();
        self.previous = [[-(28 << 24); 21]; 2];
        self.previous2 = self.previous;
        self.background = [[0; 21]; 2];
        self.rng = 0;
    }
    /// Last successfully decoded frame's normalized entropy range.
    pub fn final_range(&self) -> u32 {
        self.rng
    }
    /// Coarse/fine energy history, including both encoded channels.
    pub fn energy(&self) -> &EnergyState {
        &self.energy
    }
    /// Latest normal/transient log-energy history for a channel.
    pub fn previous_energy(&self, channel: usize) -> Option<&[i32; 21]> {
        self.previous.get(channel)
    }
    /// Second prior normal-frame energy history for a channel.
    pub fn previous_energy2(&self, channel: usize) -> Option<&[i32; 21]> {
        self.previous2.get(channel)
    }
    /// Current background energy estimate for a channel.
    pub fn background_energy(&self, channel: usize) -> Option<&[i32; 21]> {
        self.background.get(channel)
    }
    /// Inspect final PCM synthesis histories.
    pub fn synthesis(&self) -> &CeltSynthesis {
        &self.backend
    }
    /// Decode a complete CELT-only packet, atomically across all compressed frames.
    pub fn decode(&mut self, data: &[u8]) -> Result<DecodedPcm, DecoderError> {
        let packet = Packet::parse(data)?;
        if packet.mode() != Mode::Celt {
            return Err(DecoderError::UnsupportedMode);
        }
        let size = match packet.frame_samples() {
            120 => FrameSize::Ms2_5,
            240 => FrameSize::Ms5,
            480 => FrameSize::Ms10,
            960 => FrameSize::Ms20,
            _ => return Err(DecoderError::InvalidConfiguration),
        };
        let config = FrameConfig {
            size,
            channels: usize::from(packet.channels()),
            bandwidth: packet.bandwidth(),
        };
        let mut next = self.clone();
        let mut result = DecodedPcm {
            values: [0; 11520],
            length: 0,
            rate: self.rate,
            channels: self.channels(),
            range: 0,
        };
        for payload in packet.frames() {
            let frame = next.frame(payload, config)?;
            let end = result.length + frame.samples().len();
            // Packet framing caps total duration at120ms; supported output is <=48kHz.
            if end > result.values.len() {
                return Err(DecoderError::InvalidConfiguration);
            }
            result.values[result.length..end].copy_from_slice(frame.samples());
            result.length = end;
        }
        result.range = next.rng;
        *self = next;
        Ok(result)
    }
    /// Decode one CELT-only frame payload with explicit encoded configuration.
    pub fn decode_frame(
        &mut self,
        payload: &[u8],
        config: FrameConfig,
    ) -> Result<PcmFrame, DecoderError> {
        let mut next = self.clone();
        let result = next.frame(payload, config)?;
        *self = next;
        Ok(result)
    }
    fn frame(&mut self, payload: &[u8], config: FrameConfig) -> Result<PcmFrame, DecoderError> {
        if !(2..=1275).contains(&payload.len()) {
            return Err(DecoderError::MissingPayload);
        }
        let end = match config.bandwidth {
            Bandwidth::Narrowband => 13,
            Bandwidth::Wideband => 17,
            Bandwidth::SuperWideband => 19,
            Bandwidth::Fullband => 21,
            _ => return Err(DecoderError::InvalidConfiguration),
        };
        if !(1..=2).contains(&config.channels) {
            return Err(DecoderError::InvalidConfiguration);
        }
        let layout = BandLayout::new(config.channels, 0, end)?;
        let size = config.size;
        if config.channels == 1 {
            let values = self.energy.values_mut();
            for i in 0..21 {
                values[0][i] = values[0][i].max(values[1][i]);
            }
        }
        let mut entropy = RangeDecoder::new(payload);
        let prefix = control::decode_prefix(&mut entropy, size, 0)?;
        self.energy
            .coarse(&mut entropy, layout, size, prefix.intra())?;
        let tf = control::decode_tf(&mut entropy, size, 0..end, prefix.transient())?;
        let spread = control::decode_spread(&mut entropy)?;
        let allocation =
            allocation::decode_allocation(&mut entropy, size, layout, prefix.transient())?;
        self.energy
            .fine(&mut entropy, layout, allocation.fine_bits())?;
        let mut spectrum = partition::decode_spectrum(
            &mut entropy,
            &allocation,
            &tf,
            spread,
            self.rng,
            self.channels() == 1,
        )?;
        let anti = allocation.anti_collapse_reserved() && entropy.decode_bits(1)? != 0;
        let bits = (payload.len() as u32 * 8).saturating_sub(entropy.tell());
        self.energy.finalise(
            &mut entropy,
            layout,
            allocation.fine_bits(),
            allocation.fine_priority(),
            bits,
        )?;
        if anti {
            spectrum.anti_collapse(
                &allocation,
                self.energy.values(),
                &self.previous,
                &self.previous2,
            )?;
        }
        if entropy.tell() > payload.len() as u32 * 8 {
            return Err(DecoderError::BitBudgetExceeded);
        }
        if prefix.silence() {
            for channel in self.energy.values_mut().iter_mut().take(config.channels) {
                channel.fill(-(28 << 24));
            }
        }
        let energies = *self.energy.values();
        let controls = SynthesisControls {
            size,
            bands: 0..end,
            transient: prefix.transient(),
            silence: prefix.silence(),
            postfilter: prefix.postfilter(),
        };
        let left = spectrum
            .channel(0)
            .ok_or(DecoderError::InvalidConfiguration)?;
        let pcm = if let Some(right) = spectrum.channel(1) {
            self.backend
                .synthesize(&[left, right], &energies, &controls)?
        } else {
            self.backend
                .synthesize(&[left], &energies[..1], &controls)?
        };
        let values = self.energy.values_mut();
        if config.channels == 1 {
            values[1] = values[0];
        }
        if !prefix.transient() {
            self.previous2 = self.previous;
            self.previous = *values;
        } else {
            for (c, channel) in values.iter().enumerate() {
                for (i, &value) in channel.iter().enumerate() {
                    self.previous[c][i] = self.previous[c][i].min(value);
                }
            }
        }
        let increase = (1i32 << size.lm()) * 16777;
        for (c, channel) in values.iter_mut().enumerate() {
            for (i, value) in channel.iter_mut().enumerate() {
                self.background[c][i] = (self.background[c][i] + increase).min(*value);
                if i >= end {
                    *value = 0;
                    self.previous[c][i] = -(28 << 24);
                    self.previous2[c][i] = -(28 << 24);
                }
            }
        }
        self.rng = entropy.range();
        Ok(pcm)
    }
}
