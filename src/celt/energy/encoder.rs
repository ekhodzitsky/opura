//! Encoder energy quantization following pinned quant_bands.c.
use super::{BandLayout, EnergyState};
use crate::{
    celt::{FrameSize, energy_tables::PROBABILITY},
    range::{EncoderError, RangeEncoder},
};
use std::fmt;
const PRED: [i32; 4] = [29440, 26112, 21248, 16384];
const BETA: [i32; 4] = [30147, 22282, 12124, 6554];

/// Invalid energy encoding model, arithmetic or entropy operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum EnergyEncodingError {
    /// Effective band end, available byte count or loss rate is invalid.
    InvalidConfiguration,
    /// Active target energies must be within the signed-16 physical Q24 domain
    /// [-28,16]; inactive targets are ignored.
    InvalidEnergy,
    /// Fine quantizers must be 0..=8 bits per selected band.
    InvalidFineBits,
    /// Final refinement budget exceeds remaining conservative frame bits.
    InvalidBudget,
    /// An energy, residual or diagnostic would overflow signed-32 arithmetic.
    ArithmeticOverflow,
    /// Range encoding failed, preserving both caller states.
    Entropy(EncoderError),
}
impl fmt::Display for EnergyEncodingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfiguration => f.write_str("invalid CELT coarse energy configuration"),
            Self::InvalidEnergy => {
                f.write_str("CELT target log energy exceeds supported physical domain")
            }
            Self::InvalidFineBits => f.write_str("CELT energy quantizer requires 0..8 bits"),
            Self::InvalidBudget => {
                f.write_str("CELT final energy budget exceeds remaining frame bits")
            }
            Self::ArithmeticOverflow => f.write_str("CELT encoder energy arithmetic overflow"),
            Self::Entropy(e) => write!(f, "CELT energy encoder: {e}"),
        }
    }
}
impl std::error::Error for EnergyEncodingError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Entropy(e) => Some(e),
            _ => None,
        }
    }
}
impl From<EncoderError> for EnergyEncodingError {
    fn from(error: EncoderError) -> Self {
        Self::Entropy(error)
    }
}

/// Per-call coarse energy choices, validated again on every encoding operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CoarseEncoding {
    /// Selected channels and half-open band interval.
    pub layout: BandLayout,
    /// Encoded CELT transform duration.
    pub size: FrameSize,
    /// Exclusive distortion-analysis end, within the selected band interval.
    pub effective_end: usize,
    /// Bytes available to this layer, no more than the writer's capacity.
    pub available_bytes: usize,
    /// Force intra prediction when the flag can fit.
    pub force_intra: bool,
    /// Compare both inter/intra candidates using native badness and bit cost.
    pub two_pass: bool,
    /// Expected packet-loss percentage, 0..=100, used by intra selection.
    pub loss_rate: u8,
    /// Apply low-frequency-effects decay and positive-delta restrictions.
    pub lfe: bool,
}

/// Fixed-storage encoder energy histories, current residuals and intra diagnostic.
///
/// Coarse, fine and final operations commit this complete state and the range
/// writer together, or change neither. Residuals outside each new coarse layout
/// are zeroed; reconstructed histories outside the layout persist. Stages must
/// be interleaved with the matching allocation/shape bitstream operations.
/// This primitive alone does not encode a complete audio frame.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct EnergyEncoder {
    energies: EnergyState,
    residuals: [[i32; 21]; 2],
    delayed_intra: i32,
}
impl EnergyEncoder {
    /// Clear all histories, residuals and delayed-intra state.
    pub fn reset(&mut self) {
        *self = Self::default();
    }
    pub(crate) fn silence(&mut self, layout: BandLayout) {
        for channel in 0..layout.channels() {
            self.energies.values[channel][layout.bands()].fill(-(28 << 24));
        }
    }
    // Native CC2/C1 packet completion primes the dormant coded channel.
    // Residuals stay zero there, as set by coarse quantization.
    pub(crate) fn mirror_mono(&mut self) {
        self.energies.values[1] = self.energies.values[0];
    }
    // Packet completion, not primitive quantizer semantics: native CELT zeros
    // inactive reconstructed energies even in the dormant API channel.
    pub(crate) fn clear_inactive_bands(&mut self, layout: BandLayout, input_channels: usize) {
        for values in self.energies.values.iter_mut().take(input_channels) {
            values[..layout.bands().start].fill(0);
            values[layout.bands().end..].fill(0);
        }
    }
    /// Reconstructed energy histories, including inactive bands/channels.
    pub fn energies(&self) -> &EnergyState {
        &self.energies
    }
    /// Current Q24 residuals for either persistent channel, or `None` past two.
    pub fn residuals(&self, channel: usize) -> Option<&[i32; 21]> {
        self.residuals.get(channel)
    }
    /// Native delayed distortion used by subsequent intra decisions.
    pub fn delayed_intra(&self) -> i32 {
        self.delayed_intra
    }
    /// Quantize coarse log energies, returning the selected intra flag.
    ///
    /// The physical writer capacity is the bit budget. Active targets are Q24
    /// [-28,16], covering standard signed-16 PCM analysis; inactive targets are
    /// ignored. Input flags/options are overridden when the intra flag cannot
    /// fit. The native one-bit/no-bit scarcity behavior and LFE policy apply.
    /// Budget reservation and subsequent flag reading remain caller contracts.
    pub fn coarse(
        &mut self,
        entropy: &mut RangeEncoder,
        target: &[[i32; 21]; 2],
        config: CoarseEncoding,
    ) -> Result<bool, EnergyEncodingError> {
        check_writer(entropy)?;
        let layout = config.layout;
        if config.effective_end < layout.start
            || config.effective_end > layout.end
            || config.available_bytes > entropy.capacity()
            || config.loss_rate > 100
        {
            return Err(EnergyEncodingError::InvalidConfiguration);
        }
        for channel in target.iter().take(layout.channels) {
            if channel[layout.start..layout.end]
                .iter()
                .any(|&x| !(-(28 << 24)..=16 << 24).contains(&x))
            {
                return Err(EnergyEncodingError::InvalidEnergy);
            }
        }
        let budget = (entropy.capacity() * 8) as u32;
        let count = layout.end - layout.start;
        let mut intra = config.force_intra
            || (!config.two_pass
                && self.delayed_intra > 2 * (layout.channels * count) as i32
                && config.available_bytes > count * layout.channels);
        let mut two_pass = config.two_pass;
        if entropy.tell() + 3 > budget {
            intra = false;
            two_pass = false;
        }
        let bias = checked(
            (i64::from(budget) * i64::from(self.delayed_intra) * i64::from(config.loss_rate))
                / (layout.channels as i64 * 512),
        )?;
        let distortion = self.distortion(target, layout, config.effective_end)?;
        let mut max_decay = 16 << 24;
        if count > 10 {
            max_decay = (128.min(config.available_bytes) as i32) << 21;
        }
        if config.lfe {
            max_decay = 3 << 24;
        }
        let mut chosen = self.clone();
        let mut writer = entropy.clone();
        let mut intra_badness = 0;
        if two_pass || intra {
            intra_badness = chosen.attempt(&mut writer, target, config, true, max_decay)?;
        }
        if !intra {
            let mut inter = self.clone();
            let mut inter_writer = entropy.clone();
            let inter_badness =
                inter.attempt(&mut inter_writer, target, config, false, max_decay)?;
            if two_pass
                && (intra_badness < inter_badness
                    || (intra_badness == inter_badness
                        && i64::from(inter_writer.tell_frac()) + i64::from(bias)
                            > i64::from(writer.tell_frac())))
            {
                intra = true;
            } else {
                chosen = inter;
                writer = inter_writer;
            }
        }
        let coefficient = PRED[config.size.lm()];
        chosen.delayed_intra = if intra {
            distortion
        } else {
            checked(
                i64::from(product_q15(
                    product_q15(coefficient, coefficient),
                    self.delayed_intra,
                )) + i64::from(distortion),
            )?
        };
        *self = chosen;
        *entropy = writer;
        Ok(intra)
    }
    fn distortion(
        &self,
        target: &[[i32; 21]; 2],
        layout: BandLayout,
        end: usize,
    ) -> Result<i32, EnergyEncodingError> {
        let mut sum = 0i32;
        for (channel, targets) in target.iter().enumerate().take(layout.channels) {
            for (band, &value) in targets.iter().enumerate().take(end).skip(layout.start) {
                let difference =
                    checked(i64::from(value) - i64::from(self.energies.values[channel][band]))?;
                let delta = checked(i64::from(difference) + (1 << 16))? >> 17;
                sum = checked(i64::from(sum) + i64::from(delta) * i64::from(delta))?;
            }
        }
        Ok((sum >> 14).min(200))
    }
    fn attempt(
        &mut self,
        entropy: &mut RangeEncoder,
        target: &[[i32; 21]; 2],
        config: CoarseEncoding,
        intra: bool,
        max_decay: i32,
    ) -> Result<i32, EnergyEncodingError> {
        self.residuals = [[0; 21]; 2];
        let layout = config.layout;
        let lm = config.size.lm();
        let budget = (entropy.capacity() * 8) as i32;
        if entropy.tell() + 3 <= budget as u32 {
            entropy.encode_bit_logp(intra, 3)?;
        }
        let (coefficient, beta) = if intra {
            (0, 4915)
        } else {
            (PRED[lm], BETA[lm])
        };
        let mut previous = [0i32; 2];
        let mut badness = 0i32;
        for band in layout.bands() {
            for (channel, prev) in previous.iter_mut().enumerate().take(layout.channels) {
                let x = target[channel][band];
                let old = self.energies.values[channel][band];
                let predicted = product_q15(coefficient, old.max(-(9 << 24)));
                let residual = checked(i64::from(x) - i64::from(predicted) - i64::from(*prev))?;
                let mut qi = checked(i64::from(residual) + (1 << 23))? >> 24;
                let decay_bound = checked(i64::from(old) - i64::from(max_decay))?.max(-(28 << 24));
                if qi < 0 && x < decay_bound {
                    qi = checked(
                        i64::from(qi)
                            + i64::from(checked(i64::from(decay_bound) - i64::from(x))? >> 24),
                    )?
                    .min(0);
                }
                let initial = qi;
                let available = budget - entropy.tell() as i32;
                let bits_left = available - 3 * (layout.channels * (layout.end - band)) as i32;
                if band != layout.start && bits_left < 30 {
                    if bits_left < 24 {
                        qi = qi.min(1);
                    }
                    if bits_left < 16 {
                        qi = qi.max(-1);
                    }
                }
                if config.lfe && band >= 2 {
                    qi = qi.min(0);
                }
                if available >= 15 {
                    let model = PROBABILITY[lm][usize::from(intra)][band];
                    qi = entropy.encode_laplace(
                        qi,
                        u16::from(model[0]) << 7,
                        u16::from(model[1]) << 6,
                    )?;
                } else if available >= 2 {
                    qi = qi.clamp(-1, 1);
                    let symbol = (2 * qi) ^ -i32::from(qi < 0);
                    entropy.encode_icdf(symbol as usize, &[2, 1, 0], 2)?;
                } else if available >= 1 {
                    qi = qi.min(0);
                    entropy.encode_bit_logp(qi != 0, 1)?;
                } else {
                    qi = -1;
                }
                let q = checked(i64::from(qi) * (1 << 24))?;
                self.residuals[channel][band] = checked(i64::from(residual) - i64::from(q))?;
                badness = checked(i64::from(badness) + (i64::from(initial) - i64::from(qi)).abs())?;
                self.energies.values[channel][band] =
                    checked(i64::from(predicted) + i64::from(*prev) + i64::from(q))?
                        .max(-(28 << 24));
                *prev = checked(i64::from(*prev) + i64::from(q) - i64::from(product_q15(beta, q)))?;
            }
        }
        Ok(if config.lfe { 0 } else { badness })
    }
    /// Quantize selected fine raw bits (0..8 per band), with no QEXT prior pass.
    /// A band that cannot fit all its channels is skipped entirely.
    pub fn fine(
        &mut self,
        entropy: &mut RangeEncoder,
        layout: BandLayout,
        bits: &[u8; 21],
    ) -> Result<(), EnergyEncodingError> {
        check_writer(entropy)?;
        check_bits(layout, bits)?;
        let mut next = self.clone();
        let mut writer = entropy.clone();
        for band in layout.bands() {
            let count = u32::from(bits[band]);
            if count == 0 || count * layout.channels as u32 > remaining(&writer) {
                continue;
            }
            for channel in 0..layout.channels {
                let residual = next.residuals[channel][band];
                let q = (checked(i64::from(residual) + (1 << 23))? >> (24 - count))
                    .clamp(0, (1 << count) - 1);
                writer.encode_bits(q as u32, count)?;
                let offset = ((2 * q + 1) << (23 - count)) - (1 << 23);
                next.refine(channel, band, offset)?;
            }
        }
        *self = next;
        *entropy = writer;
        Ok(())
    }
    /// Refine eligible bands by at most one raw bit/channel in priority order.
    /// Returns unused bits; the input budget cannot exceed conservative remaining
    /// frame bits. Eight-bit bands are complete. Every failure rolls back both states.
    pub fn finalise(
        &mut self,
        entropy: &mut RangeEncoder,
        layout: BandLayout,
        bits: &[u8; 21],
        priority: &[bool; 21],
        mut bits_left: u32,
    ) -> Result<u32, EnergyEncodingError> {
        check_writer(entropy)?;
        check_bits(layout, bits)?;
        if bits_left > remaining(entropy) {
            return Err(EnergyEncodingError::InvalidBudget);
        }
        let mut next = self.clone();
        let mut writer = entropy.clone();
        for pass in [false, true] {
            for band in layout.bands() {
                if bits_left < layout.channels as u32 {
                    break;
                }
                if bits[band] >= 8 || priority[band] != pass {
                    continue;
                }
                for channel in 0..layout.channels {
                    let bit = i32::from(next.residuals[channel][band] >= 0);
                    writer.encode_bits(bit as u32, 1)?;
                    next.refine(channel, band, ((bit << 24) - (1 << 23)) >> (bits[band] + 1))?;
                    bits_left -= 1;
                }
            }
        }
        *self = next;
        *entropy = writer;
        Ok(bits_left)
    }
    fn refine(
        &mut self,
        channel: usize,
        band: usize,
        offset: i32,
    ) -> Result<(), EnergyEncodingError> {
        self.energies.values[channel][band] =
            checked(i64::from(self.energies.values[channel][band]) + i64::from(offset))?;
        self.residuals[channel][band] =
            checked(i64::from(self.residuals[channel][band]) - i64::from(offset))?;
        Ok(())
    }
}
fn checked(value: i64) -> Result<i32, EnergyEncodingError> {
    i32::try_from(value).map_err(|_| EnergyEncodingError::ArithmeticOverflow)
}
fn product_q15(a: i32, b: i32) -> i32 {
    ((i64::from(a) * i64::from(b)) >> 15) as i32
}
fn check_writer(writer: &RangeEncoder) -> Result<(), EnergyEncodingError> {
    if writer.is_finished() {
        Err(EncoderError::Finished.into())
    } else {
        Ok(())
    }
}
fn check_bits(layout: BandLayout, bits: &[u8; 21]) -> Result<(), EnergyEncodingError> {
    if layout.bands().any(|i| bits[i] > 8) {
        Err(EnergyEncodingError::InvalidFineBits)
    } else {
        Ok(())
    }
}
fn remaining(writer: &RangeEncoder) -> u32 {
    (writer.capacity() as u32 * 8).saturating_sub(writer.tell())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn late_coarse_fine_and_final_arithmetic_failures_preserve_both_states()
    -> Result<(), Box<dyn std::error::Error>> {
        let layout = BandLayout::new(2, 0, 2)?;
        let mut state = EnergyEncoder::default();
        let mut writer = RangeEncoder::new(64)?;
        writer.encode_uint(173, 256)?;
        state.energies.values[1][1] = i32::MIN;
        let original = state.clone();
        let entropy = writer.clone();
        let config = CoarseEncoding {
            layout,
            size: FrameSize::Ms20,
            effective_end: 0,
            available_bytes: 64,
            force_intra: true,
            two_pass: false,
            loss_rate: 0,
            lfe: false,
        };
        assert_eq!(
            state.coarse(&mut writer, &[[0; 21]; 2], config),
            Err(EnergyEncodingError::ArithmeticOverflow)
        );
        assert_eq!(state, original);
        assert_eq!(writer, entropy);
        state.reset();
        state.energies.values[1][1] = i32::MAX;
        let original = state.clone();
        let mut target = [[0; 21]; 2];
        target[1][1] = -(28 << 24);
        assert_eq!(
            state.coarse(&mut writer, &target, config),
            Err(EnergyEncodingError::ArithmeticOverflow)
        );
        assert_eq!(state, original);
        assert_eq!(writer, entropy);
        state.reset();
        state.residuals[1][1] = i32::MAX;
        let original = state.clone();
        let mut bits = [0; 21];
        bits[0] = 2;
        bits[1] = 2;
        assert_eq!(
            state.fine(&mut writer, layout, &bits),
            Err(EnergyEncodingError::ArithmeticOverflow)
        );
        assert_eq!(state, original);
        assert_eq!(writer, entropy);
        state.reset();
        state.energies.values[1][1] = i32::MAX;
        let original = state.clone();
        assert_eq!(
            state.finalise(&mut writer, layout, &[0; 21], &[false; 21], 4),
            Err(EnergyEncodingError::ArithmeticOverflow)
        );
        assert_eq!(state, original);
        assert_eq!(writer, entropy);
        Ok(())
    }
}
