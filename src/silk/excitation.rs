//! SILK shell pulses and deterministic Q14 excitation, before LTP/LPC synthesis.
//!
//! Fixed buffers, bounded escape chains and transactional entropy updates.
//! Adapted from Opus v1.6.1; see THIRD_PARTY_NOTICES.md.
use super::{
    gains::{FrameDuration, SignalType},
    pitch::SilkRate,
};
use crate::range::{RangeDecoder, RangeError};
use std::fmt;

#[path = "excitation_tables.rs"]
mod tables;
use tables::*;

/// Failure to decode or reconstruct excitation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ExcitationError {
    /// Quantization offset selector must be zero or one.
    InvalidOffset,
    /// The decoded excitation seed must be in 0..=3.
    InvalidSeed,
    /// Excitation symbols exceed the compressed frame's bit budget.
    BitBudgetExceeded,
    /// An underlying entropy operation failed.
    Entropy(RangeError),
}
impl fmt::Display for ExcitationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidOffset => f.write_str("SILK quantization offset must be 0 or 1"),
            Self::InvalidSeed => f.write_str("SILK excitation seed must be in 0..=3"),
            Self::BitBudgetExceeded => f.write_str("SILK excitation exceeds frame bit budget"),
            Self::Entropy(error) => write!(f, "SILK excitation entropy error: {error}"),
        }
    }
}
impl std::error::Error for ExcitationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Entropy(error) => Some(error),
            _ => None,
        }
    }
}
impl From<RangeError> for ExcitationError {
    fn from(error: RangeError) -> Self {
        Self::Entropy(error)
    }
}

/// Signed pulses and Q14 excitation for one internal SILK frame, not PCM.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FrameExcitation {
    pulses: [i16; 320],
    samples: [i32; 320],
    length: usize,
}
impl FrameExcitation {
    /// Signed decoded pulses, excluding any final shell-block padding.
    pub fn pulses(&self) -> &[i16] {
        &self.pulses[..self.length]
    }
    /// Reconstructed excitation with quantization offset and pseudo-random signs.
    /// LTP/LPC filtering and gain scaling have not been applied.
    pub fn samples_q14(&self) -> &[i32] {
        &self.samples[..self.length]
    }
}

/// Decode from a cursor immediately after the pitch/LTP seed field.
///
/// Pass the signal type and offset selector from gains and the seed from pitch.
/// A 10 ms 12 kHz frame consumes a full 128-sample shell representation but
/// returns only 120 samples. Both successful output and entropy commit together;
/// any error leaves the caller's entropy unchanged. Earlier per-frame histories
/// still need an enclosing transaction before complete audio decoding is ready.
pub fn decode_excitation(
    decoder: &mut RangeDecoder<'_>,
    rate: SilkRate,
    duration: FrameDuration,
    signal: SignalType,
    offset: u8,
    seed: u8,
) -> Result<FrameExcitation, ExcitationError> {
    if offset > 1 {
        return Err(ExcitationError::InvalidOffset);
    }
    if seed > 3 {
        return Err(ExcitationError::InvalidSeed);
    }
    let milliseconds = if duration == FrameDuration::Ms10 {
        10
    } else {
        20
    };
    let length = rate.khz() as usize * milliseconds;
    let blocks = length.div_ceil(16);
    let signal_index = match signal {
        SignalType::Inactive => 0,
        SignalType::Unvoiced => 1,
        SignalType::Voiced => 2,
    };
    let mut next = decoder.clone();
    let context = (signal_index >> 1) * 9;
    let level = next.decode_icdf(&RATES[context..context + 9], 8)?;
    let mut sums = [0_usize; 20];
    let mut shifts = [0_u8; 20];
    for block in 0..blocks {
        sums[block] = next.decode_icdf(&SUMS[level * 18..(level + 1) * 18], 8)?;
        while sums[block] == 17 {
            shifts[block] += 1;
            // On the tenth escape the shifted table has only 17 symbols,
            // so another escape is impossible even with zero-extended input.
            let start = 9 * 18 + usize::from(shifts[block] == 10);
            sums[block] = next.decode_icdf(&SUMS[start..], 8)?;
        }
    }
    if !next.is_within_budget() {
        return Err(ExcitationError::BitBudgetExceeded);
    }
    let mut frame = FrameExcitation {
        pulses: [0; 320],
        samples: [0; 320],
        length,
    };
    for (block, &sum) in sums[..blocks].iter().enumerate() {
        shell(
            &mut next,
            &mut frame.pulses[block * 16..(block + 1) * 16],
            sum,
            3,
        )?;
    }
    for (block, &shift) in shifts[..blocks].iter().enumerate() {
        for pulse in &mut frame.pulses[block * 16..(block + 1) * 16] {
            for _ in 0..shift {
                *pulse = *pulse * 2 + next.decode_icdf(&[120, 0], 8)? as i16;
            }
        }
    }
    let sign_context = 7 * (signal_index * 2 + usize::from(offset));
    for (block, &sum) in sums[..blocks].iter().enumerate() {
        let icdf = [SIGNS[sign_context + sum.min(6)], 0];
        for pulse in &mut frame.pulses[block * 16..(block + 1) * 16] {
            if *pulse > 0 && next.decode_icdf(&icdf, 8)? == 0 {
                *pulse = -*pulse;
            }
        }
    }
    if !next.is_within_budget() {
        return Err(ExcitationError::BitBudgetExceeded);
    }
    let quantization_offset = [[100, 240], [32, 100]][signal_index >> 1][usize::from(offset)] * 16;
    let mut random = i32::from(seed);
    for i in 0..length {
        random = random.wrapping_mul(196314165).wrapping_add(907633515);
        let pulse = i32::from(frame.pulses[i]);
        let mut value = pulse * 16384;
        value -= value.signum() * 1280;
        value += quantization_offset;
        if random < 0 {
            value = -value;
        }
        frame.samples[i] = value;
        random = random.wrapping_add(pulse);
    }
    *decoder = next;
    Ok(frame)
}

// Depth-first traversal matches the reference's unrolled tree. Only 16/8/4/2
// sample slices and sums <=16 enter this private helper: at most 15 splits.
fn shell(
    decoder: &mut RangeDecoder<'_>,
    pulses: &mut [i16],
    sum: usize,
    level: usize,
) -> Result<(), RangeError> {
    if sum == 0 {
        return Ok(());
    }
    let table = [&SHELL0, &SHELL1, &SHELL2, &SHELL3][level];
    let offset = usize::from(OFFSETS[sum]);
    let left = decoder.decode_icdf(&table[offset..offset + sum + 1], 8)?;
    if pulses.len() == 2 {
        pulses[0] = left as i16;
        pulses[1] = (sum - left) as i16;
    } else {
        let (first, second) = pulses.split_at_mut(pulses.len() / 2);
        shell(decoder, first, left, level - 1)?;
        shell(decoder, second, sum - left, level - 1)?;
    }
    Ok(())
}
