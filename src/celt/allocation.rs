//! Classic CELT dynamic, PVQ and fine-energy bit allocation.
//! Bitstream ordering follows pinned celt_decoder.c/celt_encoder.c/rate.c. Budgets are in
//! eighth-bit units, not integer pulse counts. Copyright (c) 2007-2008 CSIRO;
//! 2007-2010 Xiph.Org Foundation; 2008 Gregory Maxwell. See THIRD_PARTY_NOTICES.md.
use super::{
    FrameSize,
    allocation_tables::{CAPS, LOG_N, LOG2_FRAC, TRIM, VECTORS},
    energy::BandLayout,
    output_tables::BANDS,
};
use crate::range::{EncoderError, RangeDecoder, RangeError};
mod encoder;
pub(crate) use encoder::encode_analyzed_resized;
pub use encoder::{AllocationEncoding, encode_allocation};
use std::fmt;

/// Invalid allocation context/budget or entropy operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum AllocationError {
    /// Allocation needs at least one selected band.
    EmptyBands,
    /// Non-concealment frame payload must be 2..1275 bytes.
    InvalidPayload,
    /// Cursor already exceeds physical frame bits.
    BitBudgetExceeded,
    /// Requested encoder boost/trim/stereo/hysteresis controls are invalid.
    InvalidControls,
    /// Entropy encoding failed without committing the writer.
    Encoding(EncoderError),
    /// Lazy encoder trim analysis failed without committing the writer.
    Analysis(super::analysis::AnalysisError),
    /// Entropy decoding failed.
    Entropy(RangeError),
}
impl fmt::Display for AllocationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyBands => f.write_str("CELT allocation requires nonempty bands"),
            Self::InvalidPayload => {
                f.write_str("CELT allocation payload must contain 2..1275 bytes")
            }
            Self::BitBudgetExceeded => f.write_str("CELT allocation cursor exceeds frame budget"),
            Self::InvalidControls => f.write_str("invalid CELT allocation encoding controls"),
            Self::Analysis(e) => write!(f, "CELT allocation analysis: {e}"),
            Self::Encoding(e) => write!(f, "CELT allocation encoding: {e}"),
            Self::Entropy(e) => write!(f, "CELT allocation entropy: {e}"),
        }
    }
}
impl std::error::Error for AllocationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Entropy(e) => Some(e),
            Self::Encoding(e) => Some(e),
            Self::Analysis(e) => Some(e),
            _ => None,
        }
    }
}
impl From<RangeError> for AllocationError {
    fn from(e: RangeError) -> Self {
        Self::Entropy(e)
    }
}

impl From<EncoderError> for AllocationError {
    fn from(error: EncoderError) -> Self {
        Self::Encoding(error)
    }
}

/// Immutable standard allocation for one encoded CELT frame.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Allocation {
    layout: BandLayout,
    size: FrameSize,
    transient: bool,
    caps: [i32; 21],
    offsets: [i32; 21],
    pvq: [i32; 21],
    fine: [u8; 21],
    priority: [bool; 21],
    trim: u8,
    intensity: usize,
    dual: bool,
    balance: i32,
    coded: usize,
    anti: bool,
}
impl Allocation {
    /// Encoded channel count and selected bands.
    pub fn layout(&self) -> BandLayout {
        self.layout
    }
    /// Complete CELT frame size.
    pub fn size(&self) -> FrameSize {
        self.size
    }
    /// Transient frame context used for the anti-collapse reservation.
    pub fn transient(&self) -> bool {
        self.transient
    }
    /// Standard maximum usable budgets for all 21 bands, in eighth bits.
    pub fn caps(&self) -> &[i32; 21] {
        &self.caps
    }
    /// Dynamic boost offsets in eighth bits, zero outside selected bands.
    pub fn offsets(&self) -> &[i32; 21] {
        &self.offsets
    }
    /// Remaining per-band PVQ budgets in eighth bits, not pulse counts.
    pub fn pvq_bits(&self) -> &[i32; 21] {
        &self.pvq
    }
    /// Raw fine-energy bits per selected band/channel, zero through eight.
    pub fn fine_bits(&self) -> &[u8; 21] {
        &self.fine
    }
    /// Fine-energy final-pass priorities, false then true.
    pub fn fine_priority(&self) -> &[bool; 21] {
        &self.priority
    }
    /// Decoded allocation trim (0..10), or default five.
    pub fn trim(&self) -> u8 {
        self.trim
    }
    /// Intensity-stereo boundary, zero when its field was not reserved.
    pub fn intensity(&self) -> usize {
        self.intensity
    }
    /// Dual-stereo flag, false when unreserved/mono.
    pub fn dual_stereo(&self) -> bool {
        self.dual
    }
    /// Excess budget carried into recursive-band rebalancing, in eighth bits.
    pub fn balance(&self) -> i32 {
        self.balance
    }
    /// Exclusive upper index of the bands retained for PVQ (at least start+1).
    pub fn coded_bands(&self) -> usize {
        self.coded
    }
    /// Whether one raw bit is reserved after shapes for anti-collapse control.
    pub fn anti_collapse_reserved(&self) -> bool {
        self.anti
    }
}

/// Decode allocation after spreading, before raw fine-energy/shape decoding.
/// Only physical frame bits are budgeted. Empty bands, invalid payload or an
/// already over-budget cursor reject; every entropy error rolls back the cursor.
pub fn decode_allocation(
    entropy: &mut RangeDecoder<'_>,
    size: FrameSize,
    layout: BandLayout,
    transient: bool,
) -> Result<Allocation, AllocationError> {
    validate_context(entropy.storage_bits(), entropy.tell(), layout)?;
    let c = layout.channels() as i32;
    let lm = size.lm();
    let bands = layout.bands();
    let result = new_allocation(size, layout, transient);
    let mut next = entropy.clone();
    let result = {
        let mut engine = RateEngine {
            entropy: DecodeEntropy(&mut next),
            c,
            lm: lm as i32,
            start: bands.start,
            end: bands.end,
            result,
        };
        let total = engine.dynamic()?;
        engine.compute(total)?;
        engine.result
    };
    *entropy = next;
    Ok(result)
}
fn validate_context(
    storage_bits: u32,
    tell: u32,
    layout: BandLayout,
) -> Result<(), AllocationError> {
    if layout.bands().is_empty() {
        return Err(AllocationError::EmptyBands);
    }
    if !(16..=1275 * 8).contains(&storage_bits) {
        return Err(AllocationError::InvalidPayload);
    }
    if tell > storage_bits {
        return Err(AllocationError::BitBudgetExceeded);
    }
    Ok(())
}
fn new_allocation(size: FrameSize, layout: BandLayout, transient: bool) -> Allocation {
    let c = layout.channels() as i32;
    let lm = size.lm();
    Allocation {
        layout,
        size,
        transient,
        caps: std::array::from_fn(|i| {
            ((i32::from(CAPS[21 * (2 * lm + layout.channels() - 1) + i]) + 64)
                * c
                * (width(i) << lm))
                >> 2
        }),
        offsets: [0; 21],
        pvq: [0; 21],
        fine: [0; 21],
        priority: [false; 21],
        trim: 5,
        intensity: 0,
        dual: false,
        balance: 0,
        coded: 0,
        anti: false,
    }
}
fn width(i: usize) -> i32 {
    i32::from(BANDS[i + 1] - BANDS[i])
}

struct RateEngine<E> {
    entropy: E,
    c: i32,
    lm: i32,
    start: usize,
    end: usize,
    result: Allocation,
}
struct SkipContext {
    coded: usize,
    start: usize,
    lm: i32,
    band_bits: i32,
    band_width: i32,
}
trait RateEntropy {
    fn storage_bits(&self) -> u32;
    fn tell_frac(&self) -> u32;
    fn boost(&mut self, band: usize, successful: i32, logp: u32) -> Result<bool, AllocationError>;
    fn trim(&mut self) -> Result<u8, AllocationError>;
    fn keep_band(&mut self, context: SkipContext) -> Result<bool, AllocationError>;
    fn intensity(&mut self, start: usize, coded: usize) -> Result<usize, AllocationError>;
    fn dual(&mut self) -> Result<bool, AllocationError>;
    fn prepare_allocation(&mut self, _boost_q3: u32) -> Result<(), AllocationError> {
        Ok(())
    }
}
struct DecodeEntropy<'r, 'data>(&'r mut RangeDecoder<'data>);
impl RateEntropy for DecodeEntropy<'_, '_> {
    fn storage_bits(&self) -> u32 {
        self.0.storage_bits()
    }
    fn tell_frac(&self) -> u32 {
        self.0.tell_frac()
    }
    fn boost(
        &mut self,
        _band: usize,
        _successful: i32,
        logp: u32,
    ) -> Result<bool, AllocationError> {
        Ok(self.0.decode_bit_logp(logp)?)
    }
    fn trim(&mut self) -> Result<u8, AllocationError> {
        Ok(self.0.decode_icdf(&TRIM, 7)? as u8)
    }
    fn keep_band(&mut self, _context: SkipContext) -> Result<bool, AllocationError> {
        Ok(self.0.decode_bit_logp(1)?)
    }
    fn intensity(&mut self, start: usize, coded: usize) -> Result<usize, AllocationError> {
        Ok(start + self.0.decode_uint((coded + 1 - start) as u32)? as usize)
    }
    fn dual(&mut self) -> Result<bool, AllocationError> {
        Ok(self.0.decode_bit_logp(1)?)
    }
}
struct Interpolation {
    bits1: [i32; 21],
    bits2: [i32; 21],
    threshold: [i32; 21],
    total: i32,
    skip_start: usize,
    skip_rsv: i32,
    intensity_rsv: i32,
    dual_rsv: i32,
}
impl<E: RateEntropy> RateEngine<E> {
    fn dynamic(&mut self) -> Result<i32, AllocationError> {
        let mut total = self.entropy.storage_bits() as i32 * 8;
        let mut tell = self.entropy.tell_frac() as i32;
        let mut logp = 6;
        for band in self.start..self.end {
            let width = (self.c * width(band)) << self.lm;
            let quanta = (width << 3).min(48.max(width));
            let mut loop_logp = logp;
            let mut boost = 0;
            // Each true flag spends at least a bit and reserves >=one bit of
            // boost; frame/cap bounds make the complete search finite.
            while tell + (loop_logp << 3) < total && boost < self.result.caps[band] {
                let flag = self.entropy.boost(band, boost / quanta, loop_logp as u32)?;
                tell = self.entropy.tell_frac() as i32;
                if !flag {
                    break;
                }
                boost += quanta;
                total -= quanta;
                loop_logp = 1;
            }
            self.result.offsets[band] = boost;
            if boost > 0 {
                logp = (logp - 1).max(2);
            }
        }
        if tell + 48 <= total {
            self.result.trim = self.entropy.trim()?;
        }
        let boost = self.result.offsets[self.start..self.end]
            .iter()
            .sum::<i32>();
        self.entropy.prepare_allocation(boost as u32)?;
        let mut bits = self.entropy.storage_bits() as i32 * 8 - self.entropy.tell_frac() as i32 - 1;
        self.result.anti = self.result.transient && self.lm >= 2 && bits >= ((self.lm + 2) << 3);
        if self.result.anti {
            bits -= 8;
        }
        Ok(bits)
    }
    fn compute(&mut self, total: i32) -> Result<(), AllocationError> {
        let mut total = total.max(0);
        let skip_rsv = if total >= 8 { 8 } else { 0 };
        total -= skip_rsv;
        let mut intensity_rsv = 0;
        let mut dual_rsv = 0;
        if self.c == 2 {
            intensity_rsv = i32::from(LOG2_FRAC[self.end - self.start]);
            if intensity_rsv > total {
                intensity_rsv = 0;
            } else {
                total -= intensity_rsv;
                dual_rsv = if total >= 8 { 8 } else { 0 };
                total -= dual_rsv;
            }
        }
        let mut threshold = [0i32; 21];
        let mut trim_offset = [0i32; 21];
        for band in self.start..self.end {
            let n = width(band);
            threshold[band] = (self.c << 3).max(((3 * n) << self.lm << 3) >> 4);
            trim_offset[band] = (self.c
                * n
                * (i32::from(self.result.trim) - 5 - self.lm)
                * (self.end - band - 1) as i32
                * (1 << (self.lm + 3)))
                >> 6;
            if n << self.lm == 1 {
                trim_offset[band] -= self.c << 3;
            }
        }
        let mut lo = 1i32;
        let mut hi = 10i32;
        loop {
            let mut done = false;
            let mut sum = 0;
            let mid = (lo + hi) >> 1;
            for band in (self.start..self.end).rev() {
                let mut bits =
                    (self.c * width(band) * i32::from(VECTORS[mid as usize][band])) << self.lm >> 2;
                if bits > 0 {
                    bits = (bits + trim_offset[band]).max(0);
                }
                bits += self.result.offsets[band];
                if bits >= threshold[band] || done {
                    done = true;
                    sum += bits.min(self.result.caps[band]);
                } else if bits >= self.c << 3 {
                    sum += self.c << 3;
                }
            }
            if sum > total {
                hi = mid - 1;
            } else {
                lo = mid + 1;
            }
            if lo > hi {
                break;
            }
        }
        hi = lo;
        lo -= 1;
        let mut bits1 = [0i32; 21];
        let mut bits2 = [0i32; 21];
        let mut skip_start = self.start;
        for band in self.start..self.end {
            let n = width(band);
            let mut b1 = (self.c * n * i32::from(VECTORS[lo as usize][band])) << self.lm >> 2;
            let mut b2 = if hi >= 11 {
                self.result.caps[band]
            } else {
                (self.c * n * i32::from(VECTORS[hi as usize][band])) << self.lm >> 2
            };
            if b1 > 0 {
                b1 = (b1 + trim_offset[band]).max(0);
            }
            if b2 > 0 {
                b2 = (b2 + trim_offset[band]).max(0);
            }
            if lo > 0 {
                b1 += self.result.offsets[band];
            }
            b2 += self.result.offsets[band];
            if self.result.offsets[band] > 0 {
                skip_start = band;
            }
            bits1[band] = b1;
            bits2[band] = (b2 - b1).max(0);
        }
        self.interpolate(Interpolation {
            bits1,
            bits2,
            threshold,
            total,
            skip_start,
            skip_rsv,
            intensity_rsv,
            dual_rsv,
        })
    }
    fn interpolate(&mut self, model: Interpolation) -> Result<(), AllocationError> {
        let Interpolation {
            bits1,
            bits2,
            threshold,
            mut total,
            skip_start,
            skip_rsv,
            mut intensity_rsv,
            mut dual_rsv,
        } = model;
        let floor = self.c << 3;
        let stereo = i32::from(self.c > 1);
        let log_m = self.lm << 3;
        let mut lo = 0;
        let mut hi = 64;
        for _ in 0..6 {
            let mid = (lo + hi) >> 1;
            let mut sum = 0;
            let mut done = false;
            for band in (self.start..self.end).rev() {
                let bits = bits1[band] + ((mid * bits2[band]) >> 6);
                if bits >= threshold[band] || done {
                    done = true;
                    sum += bits.min(self.result.caps[band]);
                } else if bits >= floor {
                    sum += floor;
                }
            }
            if sum > total {
                hi = mid;
            } else {
                lo = mid;
            }
        }
        let mut sum = 0;
        let mut done = false;
        for band in (self.start..self.end).rev() {
            let mut bits = bits1[band] + ((lo * bits2[band]) >> 6);
            if bits < threshold[band] && !done {
                bits = if bits >= floor { floor } else { 0 };
            } else {
                done = true;
            }
            bits = bits.min(self.result.caps[band]);
            self.result.pvq[band] = bits;
            sum += bits;
        }
        let mut coded = self.end;
        loop {
            let band = coded - 1;
            if band <= skip_start {
                total += skip_rsv;
                break;
            }
            let mut left = total - sum;
            let coefficients = i32::from(BANDS[coded] - BANDS[self.start]);
            let per = left / coefficients;
            left -= coefficients * per;
            let remainder = (left - i32::from(BANDS[band] - BANDS[self.start])).max(0);
            let band_width = i32::from(BANDS[coded] - BANDS[band]);
            let mut band_bits = self.result.pvq[band] + per * band_width + remainder;
            if band_bits >= threshold[band].max(floor + 8) {
                if self.entropy.keep_band(SkipContext {
                    coded,
                    start: self.start,
                    lm: self.lm,
                    band_bits,
                    band_width,
                })? {
                    break;
                }
                sum += 8;
                band_bits -= 8;
            }
            sum -= self.result.pvq[band] + intensity_rsv;
            if intensity_rsv > 0 {
                intensity_rsv = i32::from(LOG2_FRAC[band - self.start]);
            }
            sum += intensity_rsv;
            if band_bits >= floor {
                sum += floor;
                self.result.pvq[band] = floor;
            } else {
                self.result.pvq[band] = 0;
            }
            coded -= 1;
        }
        self.result.coded = coded;
        self.result.intensity = if intensity_rsv > 0 {
            self.entropy.intensity(self.start, coded)?
        } else {
            0
        };
        if self.result.intensity <= self.start {
            total += dual_rsv;
            dual_rsv = 0;
        }
        self.result.dual = if dual_rsv > 0 {
            self.entropy.dual()?
        } else {
            false
        };
        let mut left = total - sum;
        let coefficients = i32::from(BANDS[coded] - BANDS[self.start]);
        let per = left / coefficients;
        left -= coefficients * per;
        for band in self.start..coded {
            self.result.pvq[band] += per * width(band);
        }
        for band in self.start..coded {
            let extra = left.min(width(band));
            self.result.pvq[band] += extra;
            left -= extra;
        }
        let mut balance = 0i32;
        let mut fine = [0i32; 21];
        for (band, energy_bits) in fine.iter_mut().enumerate().take(coded).skip(self.start) {
            let n = width(band) << self.lm;
            let bit = self.result.pvq[band] + balance;
            let mut excess;
            if n > 1 {
                excess = (bit - self.result.caps[band]).max(0);
                self.result.pvq[band] = bit - excess;
                let denominator = self.c * n
                    + i32::from(
                        self.c == 2 && n > 2 && !self.result.dual && band < self.result.intensity,
                    );
                let nclog = denominator * (i32::from(LOG_N[band]) + log_m);
                let mut offset = (nclog >> 1) - denominator * 21;
                if n == 2 {
                    offset += (denominator << 3) >> 2;
                }
                if self.result.pvq[band] + offset < (denominator * 2) << 3 {
                    offset += nclog >> 2;
                } else if self.result.pvq[band] + offset < (denominator * 3) << 3 {
                    offset += nclog >> 3;
                }
                *energy_bits = ((self.result.pvq[band] + offset + (denominator << 2)).max(0)
                    / denominator)
                    >> 3;
                if self.c * *energy_bits > self.result.pvq[band] >> 3 {
                    *energy_bits = self.result.pvq[band] >> stereo >> 3;
                }
                *energy_bits = (*energy_bits).min(8);
                self.result.priority[band] =
                    *energy_bits * (denominator << 3) >= self.result.pvq[band] + offset;
                self.result.pvq[band] -= (self.c * *energy_bits) << 3;
            } else {
                excess = (bit - (self.c << 3)).max(0);
                self.result.pvq[band] = bit - excess;
                self.result.priority[band] = true;
            }
            if excess > 0 {
                let extra = (excess >> (stereo + 3)).min(8 - *energy_bits);
                *energy_bits += extra;
                let extra_bits = (extra * self.c) << 3;
                self.result.priority[band] = extra_bits >= excess - balance;
                excess -= extra_bits;
            }
            balance = excess;
        }
        self.result.balance = balance;
        for (band, energy_bits) in fine.iter_mut().enumerate().take(self.end).skip(coded) {
            *energy_bits = self.result.pvq[band] >> stereo >> 3;
            self.result.pvq[band] = 0;
            self.result.priority[band] = *energy_bits < 1;
        }
        self.result.fine = fine.map(|v| v as u8);
        Ok(())
    }
}
