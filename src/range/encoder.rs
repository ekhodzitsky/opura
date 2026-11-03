//! Frame-local dual-ended Opus entropy writing. See `THIRD_PARTY_NOTICES.md`.
use std::fmt;

const TOP: u32 = 1 << 31;
const MAX_BYTES: usize = 1275;

/// Invalid encoder input, lifecycle, or insufficient frame storage.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum EncoderError {
    /// Frame capacity must be in 1..=1275; shrinking cannot grow it.
    InvalidCapacity,
    /// The probability distribution is malformed or selects an empty interval.
    InvalidDistribution,
    /// Precision is outside the documented method domain.
    InvalidPrecision,
    /// The symbol/value is outside its alphabet.
    InvalidValue,
    /// A uniform alphabet requires at least two values.
    InvalidTotal,
    /// Finalization or a write cannot fit the available storage.
    CapacityExhausted,
    /// Encoding, patching or shrinking was requested after finalization.
    Finished,
    /// Initial bits have not yet been encoded and cannot be patched.
    UnavailablePrefix,
    /// The bounded accounting or transaction capacity is exhausted.
    AccountingOverflow,
}
impl fmt::Display for EncoderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidCapacity => "invalid entropy frame capacity",
            Self::InvalidDistribution => "invalid encoder distribution",
            Self::InvalidPrecision => "invalid encoder precision",
            Self::InvalidValue => "encoder value exceeds its alphabet",
            Self::InvalidTotal => "encoder alphabet must contain at least two values",
            Self::CapacityExhausted => "entropy frame storage exhausted",
            Self::Finished => "entropy frame is already finalized",
            Self::UnavailablePrefix => "initial encoded bits are unavailable",
            Self::AccountingOverflow => "encoder accounting capacity exhausted",
        })
    }
}
impl std::error::Error for EncoderError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct State {
    capacity: usize,
    front: usize,
    back: usize,
    range: u32,
    value: u32,
    bits_total: u32,
    pending: i32,
    carry_run: u32,
    raw_window: u32,
    raw_bits: u32,
    finished: bool,
}
impl State {
    fn new(capacity: usize) -> Self {
        Self {
            capacity,
            front: 0,
            back: 0,
            range: TOP,
            value: 0,
            bits_total: 33,
            pending: -1,
            carry_run: 0,
            raw_window: 0,
            raw_bits: 0,
            finished: false,
        }
    }
    fn write(
        &mut self,
        journal: &mut Journal,
        value: u8,
        count: usize,
        end: bool,
    ) -> Result<(), EncoderError> {
        if count > self.capacity - self.front - self.back {
            return Err(EncoderError::CapacityExhausted);
        }
        let start = if end {
            self.back += count;
            self.capacity - self.back
        } else {
            let start = self.front;
            self.front += count;
            start
        };
        journal.push(start, count, value, false)
    }
    fn carry(&mut self, journal: &mut Journal, symbol: u32) -> Result<(), EncoderError> {
        if symbol == 255 {
            self.carry_run = self
                .carry_run
                .checked_add(1)
                .ok_or(EncoderError::AccountingOverflow)?;
        } else {
            let carry = symbol >> 8;
            if self.pending >= 0 {
                self.write(journal, (self.pending as u32 + carry) as u8, 1, false)?;
            }
            self.write(journal, (255 + carry) as u8, self.carry_run as usize, false)?;
            self.carry_run = 0;
            self.pending = (symbol & 255) as i32;
        }
        Ok(())
    }
    fn normalize(&mut self, journal: &mut Journal) -> Result<(), EncoderError> {
        while self.range <= 1 << 23 {
            self.carry(journal, self.value >> 23)?;
            self.value = (self.value << 8) & (TOP - 1);
            self.range <<= 8;
            self.bits_total += 8;
        }
        Ok(())
    }
    fn interval(
        &mut self,
        journal: &mut Journal,
        low: u32,
        high: u32,
        total: u32,
    ) -> Result<(), EncoderError> {
        let unit = self.range / total;
        if low > 0 {
            self.value = self.value.wrapping_add(self.range - unit * (total - low));
            self.range = unit * (high - low);
        } else {
            self.range -= unit * (total - high);
        }
        self.normalize(journal)
    }
    fn raw(&mut self, journal: &mut Journal, value: u32, bits: u32) -> Result<(), EncoderError> {
        if self.raw_bits + bits > 32 {
            loop {
                self.write(journal, self.raw_window as u8, 1, true)?;
                self.raw_window >>= 8;
                self.raw_bits -= 8;
                if self.raw_bits < 8 {
                    break;
                }
            }
        }
        // A zero-bit request also works when the window already holds 32 bits.
        if bits > 0 {
            self.raw_window |= value << self.raw_bits;
        }
        self.raw_bits += bits;
        self.bits_total += bits;
        Ok(())
    }
}

#[derive(Clone, Copy, Default)]
struct Write {
    start: usize,
    count: usize,
    value: u8,
    or: bool,
}
#[derive(Default)]
struct Journal {
    writes: [Write; 16],
    length: usize,
}
impl Journal {
    fn push(
        &mut self,
        start: usize,
        count: usize,
        value: u8,
        or: bool,
    ) -> Result<(), EncoderError> {
        if count == 0 {
            return Ok(());
        }
        let slot = self
            .writes
            .get_mut(self.length)
            .ok_or(EncoderError::AccountingOverflow)?;
        *slot = Write {
            start,
            count,
            value,
            or,
        };
        self.length += 1;
        Ok(())
    }
    fn apply(self, buffer: &mut [u8; MAX_BYTES]) {
        for write in &self.writes[..self.length] {
            if write.or {
                buffer[write.start] |= write.value;
            } else {
                buffer[write.start..write.start + write.count].fill(write.value);
            }
        }
    }
}

/// An allocation-free encoder owning storage for one compressed Opus frame.
///
/// Capacity is 1..=1275 bytes. Model/symbol/write errors preserve all bytes and
/// state; no sticky error is retained. Successful symbol writes do not guarantee
/// that termination will fit: callers must reserve a codec-specific bit budget
/// and check [`Self::finish`]. The full capacity, including zero padding, forms
/// the finalized frame; [`Self::shrink`] can reduce it before finalization.
/// This primitive does not encode audio or construct an Opus packet.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RangeEncoder {
    buffer: [u8; MAX_BYTES],
    state: State,
}
impl RangeEncoder {
    /// Create a frame writer with the given capacity in bytes (1..=1275).
    pub fn new(capacity: usize) -> Result<Self, EncoderError> {
        if !(1..=MAX_BYTES).contains(&capacity) {
            return Err(EncoderError::InvalidCapacity);
        }
        Ok(Self {
            buffer: [0; MAX_BYTES],
            state: State::new(capacity),
        })
    }
    /// Reset bytes and entropy state, retaining the current (possibly shrunk) capacity.
    pub fn reset(&mut self) {
        self.buffer.fill(0);
        self.state = State::new(self.state.capacity);
    }
    /// Current frame capacity in bytes.
    pub fn capacity(&self) -> usize {
        self.state.capacity
    }
    /// Whether the frame has been finalized.
    pub fn is_finished(&self) -> bool {
        self.state.finished
    }
    /// Finalized bytes, or `None` before successful finalization.
    pub fn bytes(&self) -> Option<&[u8]> {
        self.state
            .finished
            .then_some(&self.buffer[..self.state.capacity])
    }
    /// Normalized coding range, unchanged by finalization.
    pub fn range(&self) -> u32 {
        self.state.range
    }
    /// Conservative bit usage including termination overhead (initially 1).
    /// This is a budget estimate; an exact power-of-two endpoint may need one bit less.
    pub fn tell(&self) -> u32 {
        self.state.bits_total - (32 - self.state.range.leading_zeros())
    }
    /// Conservative bit usage in eighths, bit-exact with the Opus entropy coder.
    pub fn tell_frac(&self) -> u32 {
        const CORRECTION: [u32; 8] = [35733, 38967, 42495, 46340, 50535, 55109, 60097, 65535];
        let log = 32 - self.state.range.leading_zeros();
        let normalized = self.state.range >> (log - 16);
        let bucket = (normalized >> 12) - 8;
        let fraction = bucket + u32::from(normalized > CORRECTION[bucket as usize]);
        self.state.bits_total * 8 - (log * 8 + fraction)
    }
    // Native CELT silence reserves the rest of the frame without coding any
    // additional symbols. Keep interval/output state intact for termination.
    pub(crate) fn consume_remaining_budget(&mut self) -> Result<(), EncoderError> {
        if self.state.finished {
            return Err(EncoderError::Finished);
        }
        let remaining = (self.state.capacity as u32 * 8)
            .checked_sub(self.tell())
            .ok_or(EncoderError::CapacityExhausted)?;
        self.state.bits_total += remaining;
        Ok(())
    }
    fn transaction<T>(
        &mut self,
        operation: impl FnOnce(&mut State, &mut Journal) -> Result<T, EncoderError>,
    ) -> Result<T, EncoderError> {
        if self.state.finished {
            return Err(EncoderError::Finished);
        }
        if self.state.bits_total > u32::MAX / 8 - 64 {
            return Err(EncoderError::AccountingOverflow);
        }
        let mut next = self.state;
        // At most three normalization steps (six carry spans), four raw-byte
        // writes, gap clearing and one partial-byte OR fit in sixteen spans.
        // Carry runs are one fill regardless of their length. No frame copy.
        let mut journal = Journal::default();
        let result = operation(&mut next, &mut journal)?;
        journal.apply(&mut self.buffer);
        self.state = next;
        Ok(result)
    }
    /// Encode a symbol using nondecreasing CDF boundaries starting at zero.
    /// The total is 1..=65535; the selected interval must have positive width.
    pub fn encode_cdf(&mut self, symbol: usize, cdf: &[u16]) -> Result<(), EncoderError> {
        let total = u32::from(*cdf.last().ok_or(EncoderError::InvalidDistribution)?);
        if cdf.len() < 2
            || cdf.first() != Some(&0)
            || total == 0
            || cdf.windows(2).any(|p| p[0] > p[1])
        {
            return Err(EncoderError::InvalidDistribution);
        }
        let pair = cdf
            .get(symbol..symbol.checked_add(2).ok_or(EncoderError::InvalidValue)?)
            .ok_or(EncoderError::InvalidValue)?;
        if pair[0] == pair[1] {
            return Err(EncoderError::InvalidDistribution);
        }
        self.transaction(|s, j| s.interval(j, u32::from(pair[0]), u32::from(pair[1]), total))
    }
    /// Encode an inverse CDF symbol with precision 1..=8 and final zero entry.
    /// Entries are non-increasing and the first must be less than 2^bits.
    pub fn encode_icdf(
        &mut self,
        symbol: usize,
        icdf: &[u8],
        bits: u32,
    ) -> Result<(), EncoderError> {
        if !(1..=8).contains(&bits) {
            return Err(EncoderError::InvalidPrecision);
        }
        let total = 1 << bits;
        if icdf.is_empty()
            || icdf.last() != Some(&0)
            || u32::from(icdf[0]) >= total
            || icdf.windows(2).any(|p| p[0] < p[1])
        {
            return Err(EncoderError::InvalidDistribution);
        }
        let lower = u32::from(*icdf.get(symbol).ok_or(EncoderError::InvalidValue)?);
        let upper = if symbol == 0 {
            total
        } else {
            u32::from(icdf[symbol - 1])
        };
        if lower == upper {
            return Err(EncoderError::InvalidDistribution);
        }
        self.transaction(|s, j| s.interval(j, total - upper, total - lower, total))
    }
    /// Encode a bit with probability of one equal to 1 / 2^logp, logp 1..=15.
    pub fn encode_bit_logp(&mut self, value: bool, logp: u32) -> Result<(), EncoderError> {
        if !(1..=15).contains(&logp) {
            return Err(EncoderError::InvalidPrecision);
        }
        self.transaction(|s, j| {
            let split = s.range >> logp;
            let rest = s.range - split;
            if value {
                s.value = s.value.wrapping_add(rest);
                s.range = split;
            } else {
                s.range = rest;
            }
            s.normalize(j)
        })
    }
    /// Append 0..=25 raw bits, least significant first, at the frame end.
    /// Value must fit the requested width, including zero for a zero-bit write.
    pub fn encode_bits(&mut self, value: u32, bits: u32) -> Result<(), EncoderError> {
        if bits > 25 {
            return Err(EncoderError::InvalidPrecision);
        }
        if value >= 1 << bits {
            return Err(EncoderError::InvalidValue);
        }
        self.transaction(|s, j| s.raw(j, value, bits))
    }
    /// Encode a uniform value in 0..total, total 2..=u32::MAX.
    pub fn encode_uint(&mut self, value: u32, total: u32) -> Result<(), EncoderError> {
        if total < 2 {
            return Err(EncoderError::InvalidTotal);
        }
        if value >= total {
            return Err(EncoderError::InvalidValue);
        }
        self.transaction(|s, j| {
            let bits = (32 - (total - 1).leading_zeros()).saturating_sub(8);
            let high_total = ((total - 1) >> bits) + 1;
            let high = value >> bits;
            s.interval(j, high, high + 1, high_total)?;
            s.raw(j, value & ((1 << bits) - 1), bits)
        })
    }
    /// Encode a classic CELT Laplace delta and return the actual coded value.
    /// Zero frequency is 1..=32736 and decay 0..=11456 in Q14. Tail values are
    /// clipped to the representable distribution; `i32::MIN` is rejected.
    pub fn encode_laplace(
        &mut self,
        value: i32,
        zero_frequency: u16,
        decay: u16,
    ) -> Result<i32, EncoderError> {
        if !(1..=32736).contains(&zero_frequency) || decay > 11456 {
            return Err(EncoderError::InvalidDistribution);
        }
        let magnitude = value.checked_abs().ok_or(EncoderError::InvalidValue)?;
        let mut actual = value;
        let mut low = 0i64;
        let mut width = i64::from(zero_frequency);
        if magnitude != 0 {
            let sign = -i64::from(value < 0);
            low = width;
            width = ((32768 - 32 - width) * (16384 - i64::from(decay))) >> 15;
            let mut i = 1i64;
            while width > 0 && i < i64::from(magnitude) {
                low += 2 * width + 2;
                width = (2 * width * i64::from(decay)) >> 15;
                i += 1;
            }
            if width == 0 {
                let limit = (32768 - low - sign) >> 1;
                let delta = (i64::from(magnitude) - i).min(limit - 1);
                low += 2 * delta + 1 + sign;
                width = (32768 - low).min(1);
                actual = ((i + delta + sign) ^ sign) as i32;
            } else {
                width += 1;
                low += width & !sign;
            }
        }
        self.transaction(|s, j| {
            s.interval(j, low as u32, (low + width) as u32, 32768)?;
            Ok(actual)
        })
    }
    /// Replace up to eight initial power-of-two-coded bits. The caller must
    /// ensure these bits were reserved with a compatible uniform prefix model.
    /// The entropy state cannot prove that semantic precondition.
    pub fn patch_initial_bits(&mut self, value: u32, bits: u32) -> Result<(), EncoderError> {
        if bits > 8 {
            return Err(EncoderError::InvalidPrecision);
        }
        if value >= 1 << bits {
            return Err(EncoderError::InvalidValue);
        }
        if self.state.finished {
            return Err(EncoderError::Finished);
        }
        let shift = 8 - bits;
        let mask = ((1 << bits) - 1) << shift;
        if self.state.front > 0 {
            self.buffer[0] = ((u32::from(self.buffer[0]) & !mask) | (value << shift)) as u8;
        } else if self.state.pending >= 0 {
            self.state.pending = ((self.state.pending as u32 & !mask) | (value << shift)) as i32;
        } else if self.state.range <= TOP >> bits {
            self.state.value = (self.state.value & !(mask << 23)) | (value << (23 + shift));
        } else {
            return Err(EncoderError::UnavailablePrefix);
        }
        Ok(())
    }
    /// Reduce capacity, relocating written raw suffix bytes. Pending termination
    /// can still fail later even if shrinking succeeds. Capacity remains positive.
    pub fn shrink(&mut self, capacity: usize) -> Result<(), EncoderError> {
        if self.state.finished {
            return Err(EncoderError::Finished);
        }
        if capacity == 0 || capacity > self.state.capacity {
            return Err(EncoderError::InvalidCapacity);
        }
        if self.state.front + self.state.back > capacity {
            return Err(EncoderError::CapacityExhausted);
        }
        let old = self.state.capacity;
        self.buffer
            .copy_within(old - self.state.back..old, capacity - self.state.back);
        self.state.capacity = capacity;
        Ok(())
    }
    /// Terminate the range interval, flush raw bits and clear unused bytes.
    /// Returns exactly `capacity()` bytes. Repeated finalization is idempotent;
    /// errors leave the writer available for recovery or reset.
    pub fn finish(&mut self) -> Result<&[u8], EncoderError> {
        if !self.state.finished {
            self.transaction(|s, j| {
                let mut bits = 32 - (32 - s.range.leading_zeros()) as i32;
                let mut mask = (TOP - 1) >> bits;
                let mut end = s.value.wrapping_add(mask) & !mask;
                if end | mask >= s.value.wrapping_add(s.range) {
                    bits += 1;
                    mask >>= 1;
                    end = s.value.wrapping_add(mask) & !mask;
                }
                while bits > 0 {
                    s.carry(j, end >> 23)?;
                    end = (end << 8) & (TOP - 1);
                    bits -= 8;
                }
                if s.pending >= 0 || s.carry_run > 0 {
                    s.carry(j, 0)?;
                }
                let mut window = s.raw_window;
                let mut used = s.raw_bits;
                while used >= 8 {
                    s.write(j, window as u8, 1, true)?;
                    window >>= 8;
                    used -= 8;
                }
                j.push(s.front, s.capacity - s.front - s.back, 0, false)?;
                if used > 0 {
                    if s.back >= s.capacity
                        || (s.front + s.back >= s.capacity && (-bits as u32) < used)
                    {
                        return Err(EncoderError::CapacityExhausted);
                    }
                    j.push(s.capacity - s.back - 1, 1, window as u8, true)?;
                }
                s.finished = true;
                Ok(())
            })?;
        }
        Ok(&self.buffer[..self.state.capacity])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn original_output_bytes_and_complete_entropy_state_match()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut encoder = RangeEncoder::new(1)?;
        let mut cases = 0;
        let mut rows = 0;
        let mut maximum_carry = 0;
        for (line_number, line) in include_str!("../../tests/range-encoder-fixtures.txt")
            .lines()
            .enumerate()
        {
            if line.starts_with('#') {
                continue;
            }
            let columns: Vec<_> = line.split_whitespace().collect();
            if columns[0] == "case" {
                encoder = RangeEncoder::new(columns[1].parse()?)?;
                cases += 1;
                continue;
            }
            let operation: Vec<_> = columns[0].split(':').collect();
            let number = |index: usize| operation[index].parse::<u32>();
            let expected: Vec<i64> = columns[1..12]
                .iter()
                .map(|s| s.parse())
                .collect::<Result<_, _>>()?;
            let actual = match operation[0] {
                "init" => 0,
                "u" => {
                    encoder.encode_uint(number(1)?, number(2)?)?;
                    number(1)? as i64
                }
                "r" => {
                    encoder.encode_bits(number(1)?, number(2)?)?;
                    number(1)? as i64
                }
                "b" => {
                    encoder.encode_bit_logp(number(1)? != 0, number(2)?)?;
                    number(1)? as i64
                }
                "c" => {
                    let cdf: Vec<u16> = operation[2]
                        .split(',')
                        .map(|s| s.parse())
                        .collect::<Result<_, _>>()?;
                    encoder.encode_cdf(number(1)? as usize, &cdf)?;
                    number(1)? as i64
                }
                "i" => {
                    let icdf: Vec<u8> = operation[3]
                        .split(',')
                        .map(|s| s.parse())
                        .collect::<Result<_, _>>()?;
                    encoder.encode_icdf(number(1)? as usize, &icdf, number(2)?)?;
                    number(1)? as i64
                }
                "l" => encoder.encode_laplace(
                    operation[1].parse()?,
                    number(2)? as u16,
                    number(3)? as u16,
                )? as i64,
                "p" => {
                    encoder.patch_initial_bits(number(1)?, number(2)?)?;
                    number(1)? as i64
                }
                "s" => {
                    encoder.shrink(number(1)? as usize)?;
                    0
                }
                "f!" => {
                    let before = encoder.clone();
                    assert_eq!(encoder.finish(), Err(EncoderError::CapacityExhausted));
                    assert_eq!(encoder, before);
                    0
                }
                "f" => {
                    let bytes = encoder.finish()?;
                    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
                    assert_eq!(hex, columns[12], "bytes at line{}", line_number + 1);
                    0
                }
                _ => panic!("invalid fixture"),
            };
            let s = encoder.state;
            let actual = [
                actual,
                i64::from(s.range),
                i64::from(encoder.tell()),
                i64::from(encoder.tell_frac()),
                s.front as i64,
                s.back as i64,
                i64::from(s.pending),
                i64::from(s.carry_run),
                i64::from(s.value),
                i64::from(s.raw_window),
                i64::from(s.raw_bits),
            ];
            assert_eq!(
                actual.as_slice(),
                expected,
                "state at line{} {}",
                line_number + 1,
                columns[0]
            );
            maximum_carry = maximum_carry.max(s.carry_run);
            rows += 1;
        }
        assert_eq!(cases, 607);
        assert_eq!(rows, 10969);
        assert!(maximum_carry >= 1199);
        Ok(())
    }

    #[test]
    fn accounting_and_journal_exhaustion_are_atomic() -> Result<(), Box<dyn std::error::Error>> {
        let mut encoder = RangeEncoder::new(1)?;
        encoder.state.bits_total = u32::MAX / 8 - 63;
        let before = encoder.clone();
        assert_eq!(
            encoder.encode_uint(0, 2),
            Err(EncoderError::AccountingOverflow)
        );
        assert_eq!(encoder.finish(), Err(EncoderError::AccountingOverflow));
        assert_eq!(encoder, before);
        let mut journal = Journal::default();
        for i in 0..16 {
            journal.push(i, 1, 1, false)?;
        }
        assert_eq!(
            journal.push(16, 1, 1, false),
            Err(EncoderError::AccountingOverflow)
        );
        let mut state = State::new(1);
        state.carry_run = u32::MAX;
        assert_eq!(
            state.carry(&mut Journal::default(), 255),
            Err(EncoderError::AccountingOverflow)
        );
        Ok(())
    }
}
