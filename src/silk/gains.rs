//! SILK signal type, quantization offset and subframe gain reconstruction.
//!
//! This is a side-information primitive, not a complete frame decoder. Its
//! entropy cursor must already be at the signal-type symbol, after any stereo
//! prediction fields. Callers must handle LBRR ordering and retain separate
//! gain state for each channel and decoding path. See THIRD_PARTY_NOTICES.md.

use crate::range::{RangeDecoder, RangeError};
use std::fmt;

pub(super) const GAIN_ICDF: [[u8; 8]; 3] = [
    [224, 112, 44, 15, 3, 2, 1, 0],
    [254, 237, 192, 132, 70, 23, 4, 0],
    [255, 252, 226, 155, 61, 11, 2, 0],
];
pub(super) const DELTA_ICDF: [u8; 41] = [
    250, 245, 234, 203, 71, 50, 42, 38, 35, 33, 31, 29, 28, 27, 26, 25, 24, 23, 22, 21, 20, 19, 18,
    17, 16, 15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1, 0,
];

/// Duration of one internal SILK frame, not the entire Opus frame or packet.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrameDuration {
    /// Two 5 ms subframes.
    Ms10,
    /// Four 5 ms subframes.
    Ms20,
}
/// Probability context for this frame's signal type.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrameActivity {
    /// Regular frame with VAD false.
    Inactive,
    /// Regular frame with VAD true.
    Active,
    /// Redundant LBRR frame, always using the active context.
    Redundant,
}
/// How the first gain index is encoded.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GainCoding {
    /// Absolute first index, still limited by available gain history.
    Independent,
    /// First index is a delta; prior history is required.
    Conditional,
}
/// SILK signal classification.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SignalType {
    /// No detected voice activity.
    Inactive,
    /// Active, unvoiced signal.
    Unvoiced,
    /// Active, voiced signal.
    Voiced,
}
/// Failure leaves both entropy and gain state unchanged.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum GainError {
    /// A conditionally coded gain has no preceding decoded gain.
    MissingHistory,
    /// Side information exceeded the compressed frame's bit budget.
    BitBudgetExceeded,
    /// Entropy decoding failed.
    Entropy(RangeError),
}
impl fmt::Display for GainError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingHistory => f.write_str("conditional SILK gain requires prior history"),
            Self::BitBudgetExceeded => f.write_str("SILK gains exceed frame bit budget"),
            Self::Entropy(error) => write!(f, "SILK gain entropy error: {error}"),
        }
    }
}
impl std::error::Error for GainError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Entropy(error) => Some(error),
            _ => None,
        }
    }
}
impl From<RangeError> for GainError {
    fn from(error: RangeError) -> Self {
        Self::Entropy(error)
    }
}

/// Decoded side information and Q16 scale factors for two or four subframes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FrameGains {
    signal: SignalType,
    offset: u8,
    indices: [u8; 4],
    gains: [i32; 4],
    count: usize,
}
impl FrameGains {
    /// Decoded signal type.
    pub fn signal_type(&self) -> SignalType {
        self.signal
    }
    /// Encoded quantization offset selector (0 or 1), not the offset magnitude.
    pub fn offset_type(&self) -> u8 {
        self.offset
    }
    /// Encoded indices: first is absolute or delta, subsequent indices are deltas.
    pub fn indices(&self) -> &[u8] {
        &self.indices[..self.count]
    }
    /// Positive gain scale factors, each in Q16 format.
    pub fn gains_q16(&self) -> &[i32] {
        &self.gains[..self.count]
    }
}

/// Gain history for one channel. Default/reset has no prior gain.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct GainState {
    previous: Option<u8>,
}
impl GainState {
    // dec_API clears the absolute drop clamp after packet loss. Ten is also
    // the reference starting index if a later internal frame is conditional.
    pub(super) fn after_loss(&mut self) {
        self.previous = Some(10);
    }

    /// Last reconstructed logarithmic index (0..=63), if history is available.
    pub fn previous_index(&self) -> Option<u8> {
        self.previous
    }
    /// Forget gain history after a reset or an uncoded side channel.
    /// Packet-loss policy is the responsibility of the future audio decoder.
    pub fn reset(&mut self) {
        self.previous = None;
    }

    /// Decode type/offset and gains from a correctly positioned entropy cursor.
    ///
    /// Commits both states only after all symbols fit the frame bit budget.
    /// Subsequent NLSF and excitation data are not consumed. For frame-level
    /// rollback, callers should decode into a clone and commit after the entire
    /// frame succeeds. Do not call directly after stereo or LBRR headers without
    /// consuming the intervening fields.
    pub fn decode(
        &mut self,
        decoder: &mut RangeDecoder<'_>,
        duration: FrameDuration,
        activity: FrameActivity,
        coding: GainCoding,
    ) -> Result<FrameGains, GainError> {
        if coding == GainCoding::Conditional && self.previous.is_none() {
            return Err(GainError::MissingHistory);
        }
        let mut next = decoder.clone();
        let type_offset = match activity {
            FrameActivity::Inactive => next.decode_icdf(&[230, 0], 8)?,
            _ => next.decode_icdf(&[232, 158, 10, 0], 8)? + 2,
        };
        let signal_index = type_offset >> 1;
        let mut result = FrameGains {
            signal: match signal_index {
                0 => SignalType::Inactive,
                1 => SignalType::Unvoiced,
                _ => SignalType::Voiced,
            },
            offset: (type_offset & 1) as u8,
            indices: [0; 4],
            gains: [0; 4],
            count: match duration {
                FrameDuration::Ms10 => 2,
                FrameDuration::Ms20 => 4,
            },
        };
        result.indices[0] = if coding == GainCoding::Conditional {
            next.decode_icdf(&DELTA_ICDF, 8)? as u8
        } else {
            (next.decode_icdf(&GAIN_ICDF[signal_index], 8)? * 8
                + next.decode_icdf(&[224, 192, 160, 128, 96, 64, 32, 0], 8)?) as u8
        };
        for index in &mut result.indices[1..result.count] {
            *index = next.decode_icdf(&DELTA_ICDF, 8)? as u8;
        }
        if !next.is_within_budget() {
            return Err(GainError::BitBudgetExceeded);
        }
        // Zero history disables the absolute drop clamp and is never used for
        // conditional first gains (checked above).
        let mut previous = self.previous.unwrap_or(0);
        for i in 0..result.count {
            previous = reconstruct_index(
                previous,
                result.indices[i],
                i == 0 && coding == GainCoding::Independent,
            );
            result.gains[i] = linear_gain(previous);
        }
        self.previous = Some(previous);
        *decoder = next;
        Ok(result)
    }
}

// Inputs are bounded: previous 0..63, independent index 0..63, delta 0..40.
fn reconstruct_index(previous: u8, index: u8, independent: bool) -> u8 {
    let previous = i32::from(previous);
    let index = i32::from(index);
    if independent {
        index.max(previous - 16) as u8
    } else {
        (2 * index - 16).max(previous + index - 4).clamp(0, 63) as u8
    }
}

fn linear_gain(index: u8) -> i32 {
    // RFC 6716 4.2.7.4: index in 0..63 gives Q7 log in 2090..3923.
    // These bounds select the high-input branch of silk_log2lin and keep
    // every intermediate and the final positive Q16 gain inside i32.
    let log = ((0x1d1c71 * i32::from(index)) >> 16) + 2090;
    let base = 1 << (log >> 7);
    let fraction = log & 127;
    base + (((-174 * fraction * (128 - fraction)) >> 16) + fraction) * (base >> 7)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_reconstruction_transitions_match_upstream() -> Result<(), Box<dyn std::error::Error>> {
        let mut linear_count = 0;
        let mut transition_count = 0;
        for line in include_str!("../../tests/silk-gain-dequant-fixtures.txt").lines() {
            if line.starts_with('#') {
                continue;
            }
            let columns: Vec<_> = line.split_whitespace().collect();
            let previous: u8 = columns[1].parse()?;
            if columns[0] == "linear" {
                assert_eq!(linear_gain(previous), columns[2].parse::<i32>()?);
                linear_count += 1;
            } else {
                for (independent, count, expected) in
                    [(true, 64, columns[2]), (false, 41, columns[3])]
                {
                    let mut hash = 14695981039346656037u64;
                    for encoded in 0..count {
                        let result = reconstruct_index(previous, encoded, independent);
                        assert!(result <= 63);
                        for number in [u32::from(result), linear_gain(result) as u32] {
                            for byte in number.to_le_bytes() {
                                hash = (hash ^ u64::from(byte)).wrapping_mul(1099511628211);
                            }
                        }
                    }
                    assert_eq!(
                        hash,
                        expected.parse::<u64>()?,
                        "previous={previous}, independent={independent}"
                    );
                }
                transition_count += 1;
            }
        }
        assert_eq!((linear_count, transition_count), (64, 64));
        Ok(())
    }
}
