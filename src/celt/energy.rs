//! Classic CELT log-energy quantization/reconstruction in Q24 (21 bands, one/two channels).
//!
//! Coarse, fine and final stages are separate because allocation and shape
//! coding occur between them. [`EnergyEncoder`] adds coarse inter/intra RDO and
//! fine/final quantization. These primitives do not encode/decode a full packet. QEXT/RES24 are
//! unsupported. Algorithms/tables follow pinned Opus; see THIRD_PARTY_NOTICES.md.
mod encoder;
pub use encoder::{CoarseEncoding, EnergyEncoder, EnergyEncodingError};

use super::{FrameSize, energy_tables::PROBABILITY};
use crate::range::{RangeDecoder, RangeError};
use std::{fmt, ops::Range};

/// Energy decoding validation, arithmetic or entropy failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum EnergyError {
    /// Channels must be one/two and bands satisfy start<=end<=21.
    InvalidLayout,
    /// Selected fine quantizers require zero through eight bits.
    InvalidFineBits,
    /// Final refinement budget exceeds remaining physical storage bits.
    InvalidBudget,
    /// Repeated refinements would overflow a Q24 energy value.
    ArithmeticOverflow,
    /// Entropy decoding failed.
    Entropy(RangeError),
}
impl fmt::Display for EnergyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidLayout => f.write_str("invalid CELT band/channel layout"),
            Self::InvalidFineBits => f.write_str("CELT fine energy requires 0..8 bits"),
            Self::InvalidBudget => f.write_str("CELT final energy budget exceeds remaining bits"),
            Self::ArithmeticOverflow => f.write_str("CELT Q24 energy refinement overflow"),
            Self::Entropy(e) => write!(f, "CELT energy entropy: {e}"),
        }
    }
}
impl std::error::Error for EnergyError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Entropy(e) => Some(e),
            _ => None,
        }
    }
}
impl From<RangeError> for EnergyError {
    fn from(e: RangeError) -> Self {
        Self::Entropy(e)
    }
}

/// Validated active channels and half-open band interval; other histories remain.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BandLayout {
    channels: usize,
    start: usize,
    end: usize,
}
impl BandLayout {
    /// Select one/two channels and start<=end<=21, allowing an empty interval.
    pub fn new(channels: usize, start: usize, end: usize) -> Result<Self, EnergyError> {
        if !(1..=2).contains(&channels) || start > end || end > 21 {
            return Err(EnergyError::InvalidLayout);
        }
        Ok(Self {
            channels,
            start,
            end,
        })
    }
    /// Active encoded channel count.
    pub fn channels(self) -> usize {
        self.channels
    }
    /// Selected half-open CELT band interval.
    pub fn bands(self) -> Range<usize> {
        self.start..self.end
    }
}

/// Persistent per-channel Q24 energies; default/reset starts all 42 values at zero.
/// Each stage commits both this state and entropy together, or changes neither.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct EnergyState {
    values: [[i32; 21]; 2],
}
impl EnergyState {
    pub(crate) fn values(&self) -> &[[i32; 21]; 2] {
        &self.values
    }
    pub(crate) fn values_mut(&mut self) -> &mut [[i32; 21]; 2] {
        &mut self.values
    }

    /// Inspect all 21 Q24 band energies for a channel, including inactive bands.
    pub fn channel(&self, index: usize) -> Option<&[i32; 21]> {
        self.values.get(index)
    }
    /// Clear all energies before starting a new independent stream.
    pub fn reset(&mut self) {
        *self = Self::default();
    }
    /// Decode coarse deltas with temporal and inter-band prediction.
    ///
    /// The caller has already decoded the intra flag and selected the encoded
    /// frame size. Physical storage drives Laplace/three-symbol/one-bit/no-bit
    /// fallbacks. Unselected channels and bands retain their prior values.
    pub fn coarse(
        &mut self,
        entropy: &mut RangeDecoder<'_>,
        layout: BandLayout,
        size: FrameSize,
        intra: bool,
    ) -> Result<(), EnergyError> {
        let mut next = self.clone();
        let mut decoder = entropy.clone();
        let mut previous = [0i64; 2];
        let lm = size.lm();
        let (coefficient, beta) = if intra {
            (0, 4915)
        } else {
            (
                [29440, 26112, 21248, 16384][lm],
                [30147, 22282, 12124, 6554][lm],
            )
        };
        let budget = decoder.storage_bits();
        for band in layout.bands() {
            for (channel, prev) in previous.iter_mut().enumerate().take(layout.channels) {
                let available = budget.saturating_sub(decoder.tell());
                let delta = if available >= 15 {
                    let model = PROBABILITY[lm][usize::from(intra)][band];
                    decoder.decode_laplace(u16::from(model[0]) << 7, u16::from(model[1]) << 6)?
                } else if available >= 2 {
                    let symbol = decoder.decode_icdf(&[2, 1, 0], 2)? as i32;
                    (symbol >> 1) ^ -(symbol & 1)
                } else if available >= 1 {
                    -i32::from(decoder.decode_bit_logp(1)?)
                } else {
                    -1
                };
                let q = delta.wrapping_shl(24);
                let old = next.values[channel][band].max(-(9 << 24));
                let predicted = (i64::from(coefficient) * i64::from(old)) >> 15;
                // Upstream truncates the i64 accumulator to signed-32 tmp
                // before clamping; this matters for extreme payloads.
                let accumulated = (predicted + *prev + i64::from(q)) as i32;
                next.values[channel][band] = accumulated.clamp(-(28 << 24), 28 << 24);
                *prev += i64::from(q) - ((i64::from(beta) * i64::from(q)) >> 15);
            }
        }
        *self = next;
        *entropy = decoder;
        Ok(())
    }
    /// Decode raw fine bits (0..8 per selected band), with no QEXT prior stage.
    /// Insufficient remaining packet bits skip that entire band's refinement.
    pub fn fine(
        &mut self,
        entropy: &mut RangeDecoder<'_>,
        layout: BandLayout,
        bits: &[u8; 21],
    ) -> Result<(), EnergyError> {
        validate_bits(layout, bits)?;
        let mut next = self.clone();
        let mut decoder = entropy.clone();
        for band in layout.bands() {
            let count = u32::from(bits[band]);
            if count == 0
                || count * layout.channels as u32
                    > decoder.storage_bits().saturating_sub(decoder.tell())
            {
                continue;
            }
            for channel in 0..layout.channels {
                let value = decoder.decode_bits(count)? as i32;
                let offset = ((2 * value + 1) << (23 - count)) - (1 << 23);
                next.add(channel, band, offset)?;
            }
        }
        *self = next;
        *entropy = decoder;
        Ok(())
    }
    /// Use final raw bits in priority order (false then true), at most one/channel
    /// per eligible band. Eight-bit bands are already complete. Returns unused
    /// caller budget; the budget may not exceed remaining physical packet bits.
    pub fn finalise(
        &mut self,
        entropy: &mut RangeDecoder<'_>,
        layout: BandLayout,
        bits: &[u8; 21],
        priority: &[bool; 21],
        mut bits_left: u32,
    ) -> Result<u32, EnergyError> {
        validate_bits(layout, bits)?;
        if bits_left > entropy.storage_bits().saturating_sub(entropy.tell()) {
            return Err(EnergyError::InvalidBudget);
        }
        let mut next = self.clone();
        let mut decoder = entropy.clone();
        for pass in [false, true] {
            for band in layout.bands() {
                if bits_left < layout.channels as u32 {
                    break;
                }
                if bits[band] >= 8 || priority[band] != pass {
                    continue;
                }
                for channel in 0..layout.channels {
                    let bit = decoder.decode_bits(1)? as i32;
                    let offset = ((bit << 24) - (1 << 23)) >> (bits[band] + 1);
                    next.add(channel, band, offset)?;
                    bits_left -= 1;
                }
            }
        }
        *self = next;
        *entropy = decoder;
        Ok(bits_left)
    }
    fn add(&mut self, channel: usize, band: usize, offset: i32) -> Result<(), EnergyError> {
        self.values[channel][band] = self.values[channel][band]
            .checked_add(offset)
            .ok_or(EnergyError::ArithmeticOverflow)?;
        Ok(())
    }
}
fn validate_bits(layout: BandLayout, bits: &[u8; 21]) -> Result<(), EnergyError> {
    if layout.bands().any(|i| bits[i] > 8) {
        return Err(EnergyError::InvalidFineBits);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn late_refinement_overflow_rolls_back_both_channels_and_entropy()
    -> Result<(), Box<dyn std::error::Error>> {
        let layout = BandLayout::new(2, 0, 2)?;
        for final_stage in [false, true] {
            let mut state = EnergyState::default();
            state.values[1][0] = i32::MAX;
            let before = state.clone();
            let mut entropy = RangeDecoder::new(&[0xff; 4]);
            let cursor = entropy.clone();
            let result = if final_stage {
                state
                    .finalise(&mut entropy, layout, &[0; 21], &[false; 21], 2)
                    .map(|_| ())
            } else {
                state.fine(&mut entropy, layout, &[1; 21])
            };
            assert_eq!(result, Err(EnergyError::ArithmeticOverflow));
            assert_eq!(state, before);
            assert_eq!(entropy, cursor);
        }
        Ok(())
    }
}
