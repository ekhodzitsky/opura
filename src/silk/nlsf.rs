//! SILK normalized line spectral frequencies (RFC 6716 section 4.2.7.5).
//!
//! Consumes NLSF symbols after the gains; produces stabilized Q15 frequencies,
//! convertible to LPC coefficients via [`FrameNlsf::to_lpc`]. No PCM is produced.
//! All storage is fixed and decoding is transactional.
//! Adapted from Opus v1.6.1; see THIRD_PARTY_NOTICES.md.
use super::gains::{FrameDuration, SignalType};
use crate::range::{RangeDecoder, RangeError};
use std::fmt;

#[path = "nlsf_tables.rs"]
mod tables;
use tables::*;

/// Number of spectral parameters in a SILK channel.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NlsfOrder {
    /// Narrowband or mediumband (8 or 12 kHz SILK).
    Order10,
    /// Wideband, including the SILK layer in Hybrid (16 kHz SILK).
    Order16,
}
impl NlsfOrder {
    fn len(self) -> usize {
        match self {
            Self::Order10 => 10,
            Self::Order16 => 16,
        }
    }
    fn codebook(self) -> Codebook {
        match self {
            Self::Order10 => Codebook {
                cb1: &NB_MB_CB1,
                weights: &NB_MB_WEIGHTS,
                icdf1: &NB_MB_ICDF1,
                select: &NB_MB_SELECT,
                icdf2: &NB_MB_ICDF2,
                pred: &NB_MB_PRED,
                delta: &NB_MB_DELTA,
                step: 11796,
            },
            Self::Order16 => Codebook {
                cb1: &WB_CB1,
                weights: &WB_WEIGHTS,
                icdf1: &WB_ICDF1,
                select: &WB_SELECT,
                icdf2: &WB_ICDF2,
                pred: &WB_PRED,
                delta: &WB_DELTA,
                step: 9830,
            },
        }
    }
}

/// Failure while reading spectral side information.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum NlsfError {
    /// Consumed entropy exceeds the compressed frame's bit budget.
    BitBudgetExceeded,
    /// An entropy operation failed.
    Entropy(RangeError),
}
impl fmt::Display for NlsfError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BitBudgetExceeded => f.write_str("SILK NLSF exceeds frame bit budget"),
            Self::Entropy(error) => write!(f, "SILK NLSF entropy error: {error}"),
        }
    }
}
impl std::error::Error for NlsfError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Entropy(error) => Some(error),
            _ => None,
        }
    }
}
impl From<RangeError> for NlsfError {
    fn from(error: RangeError) -> Self {
        Self::Entropy(error)
    }
}

/// Spectral parameters of one internal 10 or 20 ms SILK frame.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FrameNlsf {
    order: NlsfOrder,
    indices: [i8; 17],
    current: [i16; 16],
    first_half: [i16; 16],
    coded_factor: u8,
    factor: u8,
}
impl FrameNlsf {
    /// First-stage codebook index followed by signed residual indices.
    pub fn indices(&self) -> &[i8] {
        &self.indices[..=self.order.len()]
    }
    /// Stabilized current spectrum, normalized to 0..32768 in Q15.
    pub fn current_q15(&self) -> &[i16] {
        &self.current[..self.order.len()]
    }
    /// Spectrum for the first half of a 20 ms frame; current spectrum for 10 ms.
    pub fn first_half_q15(&self) -> &[i16] {
        &self.first_half[..self.order.len()]
    }
    /// Transmitted factor (0..=4); implicitly 4 for 10 ms frames.
    pub fn coded_interpolation_q2(&self) -> u8 {
        self.coded_factor
    }
    /// Effective factor after reset handling: 4 disables interpolation.
    pub fn interpolation_q2(&self) -> u8 {
        self.factor
    }
}

/// Spectral history for one channel, empty after default/reset.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct NlsfState {
    order: Option<NlsfOrder>,
    previous: [i16; 16],
}
impl NlsfState {
    /// Last current spectrum, if a frame has been decoded since reset.
    pub fn previous_q15(&self) -> Option<&[i16]> {
        self.order.map(|order| &self.previous[..order.len()])
    }
    /// Forget interpolation history. Call on sample-rate changes, including
    /// 8↔12 kHz where the order stays 10, and when resetting an uncoded channel.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Decode NLSF indices, reconstruct and interpolate after gain symbols.
    ///
    /// A new order disables interpolation for this frame automatically. Both
    /// history and entropy are unchanged on error. For rollback of a whole
    /// frame, decode into cloned states until all remaining fields succeed.
    /// Packet-loss concealment is not implemented here; LPC conversion is separate.
    pub fn decode(
        &mut self,
        decoder: &mut RangeDecoder<'_>,
        order: NlsfOrder,
        duration: FrameDuration,
        signal: SignalType,
    ) -> Result<FrameNlsf, NlsfError> {
        let mut next = decoder.clone();
        let cb = order.codebook();
        let count = order.len();
        let context = if signal == SignalType::Voiced { 32 } else { 0 };
        let first = next.decode_icdf(&cb.icdf1[context..context + 32], 8)?;
        let mut indices = [0_i8; 17];
        indices[0] = first as i8;
        let (entropy, prediction) = cb.unpack(first, count);
        for i in 0..count {
            let offset = entropy[i];
            let mut index = next.decode_icdf(&cb.icdf2[offset..offset + 9], 8)? as i8;
            if index == 0 {
                index -= next.decode_icdf(&[100, 40, 16, 7, 3, 1, 0], 8)? as i8;
            } else if index == 8 {
                index += next.decode_icdf(&[100, 40, 16, 7, 3, 1, 0], 8)? as i8;
            }
            indices[i + 1] = index - 4;
        }
        let coded_factor = if duration == FrameDuration::Ms20 {
            next.decode_icdf(&[243, 221, 192, 181, 0], 8)? as u8
        } else {
            4
        };
        if !next.is_within_budget() {
            return Err(NlsfError::BitBudgetExceeded);
        }
        let mut current = [0_i16; 16];
        let mut residual = 0_i32;
        for i in (0..count).rev() {
            let predicted = (i32::from(residual as i16) * i32::from(prediction[i])) >> 8;
            let level = i32::from(indices[i + 1]) * 1024;
            residual = predicted + (((level - level.signum() * 102) * cb.step) >> 16);
            let position = first * count + i;
            let frequency = ((i32::from(residual as i16) * 16384)
                / i32::from(cb.weights[position]))
                + i32::from(cb.cb1[position]) * 128;
            current[i] = frequency.clamp(0, 32767) as i16;
        }
        stabilize(&mut current[..count], cb.delta);
        let factor = if self.order == Some(order) {
            coded_factor
        } else {
            4
        };
        let mut first_half = current;
        if factor < 4 {
            for i in 0..count {
                first_half[i] = (i32::from(self.previous[i])
                    + ((i32::from(factor) * (i32::from(current[i]) - i32::from(self.previous[i])))
                        >> 2)) as i16;
            }
        }
        self.order = Some(order);
        self.previous = current;
        *decoder = next;
        Ok(FrameNlsf {
            order,
            indices,
            current,
            first_half,
            coded_factor,
            factor,
        })
    }
}

struct Codebook {
    cb1: &'static [u8],
    weights: &'static [i16],
    icdf1: &'static [u8],
    select: &'static [u8],
    icdf2: &'static [u8],
    pred: &'static [u8],
    delta: &'static [i16],
    step: i32,
}
impl Codebook {
    fn unpack(&self, first: usize, count: usize) -> ([usize; 16], [u8; 16]) {
        let mut entropy = [0; 16];
        let mut prediction = [0; 16];
        for i in (0..count).step_by(2) {
            let entry = self.select[first * count / 2 + i / 2] as usize;
            entropy[i] = ((entry >> 1) & 7) * 9;
            prediction[i] = self.pred[i + (entry & 1) * (count - 1)];
            entropy[i + 1] = ((entry >> 5) & 7) * 9;
            prediction[i + 1] = self.pred[i + ((entry >> 4) & 1) * (count - 1) + 1];
        }
        (entropy, prediction)
    }
}

// Enforce the codebook's minimum spacings with the reference's 20-step limit
// and bounded sort fallback. i32 intermediates preserve signed rounding.
fn stabilize(values: &mut [i16], delta: &[i16]) {
    let count = values.len();
    for _ in 0..20 {
        let mut minimum = i32::from(values[0]) - i32::from(delta[0]);
        let mut index = 0;
        for i in 1..count {
            let distance = i32::from(values[i]) - i32::from(values[i - 1]) - i32::from(delta[i]);
            if distance < minimum {
                minimum = distance;
                index = i;
            }
        }
        let distance = 32768 - i32::from(values[count - 1]) - i32::from(delta[count]);
        if distance < minimum {
            minimum = distance;
            index = count;
        }
        if minimum >= 0 {
            return;
        }
        if index == 0 {
            values[0] = delta[0];
        } else if index == count {
            values[count - 1] = (32768 - i32::from(delta[count])) as i16;
        } else {
            let half = i32::from(delta[index]) >> 1;
            let lower = delta[..index].iter().copied().map(i32::from).sum::<i32>() + half;
            let upper = 32768
                - delta[index + 1..]
                    .iter()
                    .copied()
                    .map(i32::from)
                    .sum::<i32>()
                - half;
            let center = ((i32::from(values[index - 1]) + i32::from(values[index]) + 1) >> 1)
                .clamp(lower, upper);
            values[index - 1] = (center - half) as i16;
            values[index] = values[index - 1] + delta[index];
        }
    }
    values.sort_unstable();
    values[0] = values[0].max(delta[0]);
    for i in 1..count {
        values[i] = values[i].max(values[i - 1].saturating_add(delta[i]));
    }
    values[count - 1] = values[count - 1].min((32768 - i32::from(delta[count])) as i16);
    for i in (0..count - 1).rev() {
        values[i] = values[i].min(values[i + 1] - delta[i + 1]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upstream_stabilization_extremes() -> Result<(), Box<dyn std::error::Error>> {
        let mut cases = 0;
        for line in include_str!("../../tests/silk-nlsf-stabilize-fixtures.txt").lines() {
            if line.starts_with('#') {
                continue;
            }
            let numbers: Vec<i32> = line
                .split_whitespace()
                .map(str::parse)
                .collect::<Result<_, _>>()?;
            let count = numbers[0] as usize;
            let order = if count == 10 {
                NlsfOrder::Order10
            } else {
                NlsfOrder::Order16
            };
            let mut values: Vec<i16> = numbers[1..=count].iter().map(|&v| v as i16).collect();
            stabilize(&mut values, order.codebook().delta);
            assert_eq!(
                values.iter().copied().map(i32::from).collect::<Vec<_>>(),
                numbers[count + 1..]
            );
            cases += 1;
        }
        assert_eq!(cases, 266);
        Ok(())
    }
}
