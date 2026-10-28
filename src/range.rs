//! Frame-local entropy decoding from RFC 6716 section 4.1.
//!
//! Range input is zero-extended as required by Opus. Raw bits are read from
//! the end of the same frame and may overlap range input, but cannot extend
//! beyond it. No operation allocates. Every error leaves the decoder unchanged.
//! The caller must supply the correct probability models and enforce the
//! codec's bit budget using [`RangeDecoder::tell`] / [`RangeDecoder::tell_frac`].
//! This primitive alone cannot determine whether a frame is valid audio.
//!
//! Arithmetic follows the Opus reference entropy decoder. See
//! `THIRD_PARTY_NOTICES.md` for attribution and redistribution terms.

use std::fmt;

/// An invalid model, request, or entropy-coded value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum RangeError {
    /// The cumulative distribution is invalid or empty.
    InvalidDistribution,
    /// A bit precision is outside the method's documented range.
    InvalidPrecision,
    /// Uniform integer decoding requires at least two possible values.
    InvalidTotal,
    /// The requested raw bits are not present in this frame.
    RawBitsExhausted,
    /// A split uniform integer exceeds the specified alphabet.
    CorruptUniformValue,
    /// Further decoding would exceed the bit accounting capacity.
    BitCountOverflow,
}

impl fmt::Display for RangeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidDistribution => "invalid cumulative distribution",
            Self::InvalidPrecision => "invalid bit precision",
            Self::InvalidTotal => "uniform integer total must be at least two",
            Self::RawBitsExhausted => "raw bits extend beyond the frame",
            Self::CorruptUniformValue => "uniform value exceeds its alphabet",
            Self::BitCountOverflow => "range decoder bit counter exhausted",
        })
    }
}
impl std::error::Error for RangeError {}

/// A zero-allocation range decoder borrowing exactly one compressed frame.
///
/// Use a frame returned by [`crate::Packet::frames`], not the whole packet.
/// Empty input is permitted and supplies zero range bytes. Successful calls
/// can exceed the frame's bit budget; codec-specific callers must check it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RangeDecoder<'a> {
    input: &'a [u8],
    position: usize,
    previous: u8,
    range: u32,
    value: u32,
    bits_total: u32,
    end_bytes: usize,
    raw_window: u32,
    raw_available: u32,
}

impl<'a> RangeDecoder<'a> {
    /// Initialize from one frame, including RFC-mandated zero extension.
    pub fn new(input: &'a [u8]) -> Self {
        let mut decoder = Self {
            input,
            position: 0,
            previous: 0,
            range: 128,
            value: 0,
            bits_total: 9,
            end_bytes: 0,
            raw_window: 0,
            raw_available: 0,
        };
        decoder.previous = decoder.read_byte();
        decoder.value = 127 - u32::from(decoder.previous >> 1);
        decoder.normalize();
        decoder
    }

    /// Decode using nondecreasing cumulative boundaries, beginning with zero.
    ///
    /// The final boundary is the total (1..=65535). Adjacent equal boundaries
    /// represent zero-probability symbols. Returns the zero-based symbol index.
    pub fn decode_cdf(&mut self, cdf: &[u16]) -> Result<usize, RangeError> {
        let total = u32::from(*cdf.last().ok_or(RangeError::InvalidDistribution)?);
        if cdf.len() < 2
            || cdf.first() != Some(&0)
            || total == 0
            || cdf.windows(2).any(|pair| pair[0] > pair[1])
        {
            return Err(RangeError::InvalidDistribution);
        }
        self.transaction(|d| {
            let frequency = d.frequency(total);
            for (symbol, pair) in cdf.windows(2).enumerate() {
                if frequency < u32::from(pair[1]) {
                    d.update(u32::from(pair[0]), u32::from(pair[1]), total);
                    return Ok(symbol);
                }
            }
            Err(RangeError::InvalidDistribution)
        })
    }

    /// Decode an inverse CDF with precision 1..=8 and a final zero entry.
    ///
    /// Entries must be non-increasing, with the first strictly below 2^bits.
    /// Equal adjacent entries are allowed (zero-probability symbols).
    pub fn decode_icdf(&mut self, icdf: &[u8], bits: u32) -> Result<usize, RangeError> {
        if !(1..=8).contains(&bits) {
            return Err(RangeError::InvalidPrecision);
        }
        let total = 1u32 << bits;
        if icdf.is_empty()
            || icdf.last() != Some(&0)
            || icdf.first().is_some_and(|&first| u32::from(first) >= total)
            || icdf.windows(2).any(|pair| pair[0] < pair[1])
        {
            return Err(RangeError::InvalidDistribution);
        }
        self.transaction(|d| {
            let unit = d.range >> bits;
            let mut upper = d.range;
            for (symbol, &entry) in icdf.iter().enumerate() {
                let lower = unit * u32::from(entry);
                if d.value >= lower {
                    d.value -= lower;
                    d.range = upper - lower;
                    d.normalize();
                    return Ok(symbol);
                }
                upper = lower;
            }
            Err(RangeError::InvalidDistribution)
        })
    }

    /// Decode a bit whose probability of being one is 1 / 2^logp.
    /// Supported logp values are 1..=15.
    pub fn decode_bit_logp(&mut self, logp: u32) -> Result<bool, RangeError> {
        if !(1..=15).contains(&logp) {
            return Err(RangeError::InvalidPrecision);
        }
        self.transaction(|d| {
            let split = d.range >> logp;
            let one = d.value < split;
            if one {
                d.range = split;
            } else {
                d.value -= split;
                d.range -= split;
            }
            d.normalize();
            Ok(one)
        })
    }

    /// Read 0..=25 raw bits, least significant bit first, from the frame's end.
    /// Unlike range input, raw input is not zero-extended on exhaustion.
    pub fn decode_bits(&mut self, bits: u32) -> Result<u32, RangeError> {
        if bits > 25 {
            return Err(RangeError::InvalidPrecision);
        }
        self.transaction(|d| d.raw_bits(bits))
    }

    /// Decode a uniform integer in 0..total, where total is 2..=u32::MAX.
    /// Large alphabets combine a range-coded prefix with raw suffix bits.
    /// An out-of-alphabet result is reported, not silently saturated.
    pub fn decode_uint(&mut self, total: u32) -> Result<u32, RangeError> {
        if total < 2 {
            return Err(RangeError::InvalidTotal);
        }
        self.transaction(|d| {
            let bits = (32 - (total - 1).leading_zeros()).saturating_sub(8);
            let high_total = ((total - 1) >> bits) + 1;
            let high = d.frequency(high_total);
            d.update(high, high + 1, high_total);
            let value = (high << bits) | d.raw_bits(bits)?;
            if value >= total {
                return Err(RangeError::CorruptUniformValue);
            }
            Ok(value)
        })
    }

    /// Current normalized range, for Opus final-range verification.
    pub fn range(&self) -> u32 {
        self.range
    }

    /// Conservative whole-bit usage, including the termination bit (initially 1).
    pub fn tell(&self) -> u32 {
        self.bits_total - (32 - self.range.leading_zeros())
    }

    /// Conservative usage in eighths of a bit (initially 8), matching libopus.
    pub fn tell_frac(&self) -> u32 {
        // Integer transition thresholds used by the reference's exact fast path.
        const CORRECTION: [u32; 8] = [35733, 38967, 42495, 46340, 50535, 55109, 60097, 65535];
        let log = 32 - self.range.leading_zeros();
        let normalized = self.range >> (log - 16);
        let bucket = (normalized >> 12) - 8;
        let fraction = bucket + u32::from(normalized > CORRECTION[bucket as usize]);
        self.bits_total * 8 - (log * 8 + fraction)
    }

    // Compare rounded-up byte usage to avoid multiplying an arbitrary slice length.
    pub(crate) fn is_within_budget(&self) -> bool {
        self.tell().div_ceil(8) as usize <= self.input.len()
    }

    fn transaction<T>(
        &mut self,
        operation: impl FnOnce(&mut Self) -> Result<T, RangeError>,
    ) -> Result<T, RangeError> {
        // An operation adds at most 16 normalization bits and 25 raw bits.
        // Reserve headroom for arithmetic and eighth-bit reporting.
        if self.bits_total > u32::MAX / 8 - 64 {
            return Err(RangeError::BitCountOverflow);
        }
        let mut next = self.clone();
        let result = operation(&mut next)?;
        *self = next;
        Ok(result)
    }

    fn frequency(&self, total: u32) -> u32 {
        total - (self.value / (self.range / total) + 1).min(total)
    }

    fn update(&mut self, low: u32, high: u32, total: u32) {
        let unit = self.range / total;
        let removed = unit * (total - high);
        self.value -= removed;
        self.range = if low == 0 {
            self.range - removed
        } else {
            unit * (high - low)
        };
        self.normalize();
    }

    fn read_byte(&mut self) -> u8 {
        match self.input.get(self.position) {
            Some(&byte) => {
                self.position += 1;
                byte
            }
            None => 0, // Required zero extension; never reads a neighboring frame.
        }
    }

    fn normalize(&mut self) {
        while self.range <= 1 << 23 {
            self.range <<= 8;
            self.bits_total += 8;
            let next = self.read_byte();
            let symbol = ((self.previous & 1) << 7) | (next >> 1);
            self.previous = next;
            self.value = ((self.value << 8) | u32::from(255 - symbol)) & 0x7fff_ffff;
        }
    }

    fn raw_bits(&mut self, bits: u32) -> Result<u32, RangeError> {
        while self.raw_available < bits {
            let index = self
                .input
                .len()
                .checked_sub(self.end_bytes + 1)
                .ok_or(RangeError::RawBitsExhausted)?;
            self.raw_window |= u32::from(self.input[index]) << self.raw_available;
            self.raw_available += 8;
            self.end_bytes += 1;
        }
        let value = self.raw_window & ((1 << bits) - 1);
        self.raw_window >>= bits;
        self.raw_available -= bits;
        self.bits_total += bits;
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bit_accounting_exhaustion_is_transactional() {
        let mut decoder = RangeDecoder::new(&[]);
        decoder.bits_total = u32::MAX / 8 - 63;
        let before = decoder.clone();
        assert_eq!(decoder.decode_uint(256), Err(RangeError::BitCountOverflow));
        assert_eq!(decoder, before);
        assert_eq!(decoder.tell(), decoder.tell_frac().div_ceil(8));
    }
}
