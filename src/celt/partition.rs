//! Classic recursive mono spectral partition encoding/decoding and band orchestration.
//! Copyright (c) 2007-2008 CSIRO; 2007-2009 Xiph.Org Foundation;
//! 2008-2009 Gregory Maxwell. See THIRD_PARTY_NOTICES.md.
use super::{
    allocation_tables::LOG_N,
    bands::{self, BandError},
    output_tables::BANDS,
    partition_tables::{CACHE_BITS, CACHE_INDEX},
    pulses::{PulseCodebook, PulseError},
    pvq::{self, PvqError, Spread},
};
use crate::range::{EncoderError, RangeDecoder, RangeEncoder, RangeError};
use std::fmt;

mod encoder;
pub use encoder::encode_partition;

mod band;
mod spectrum;
pub use band::{Band, BandControls, StereoControls, decode_band};
pub use spectrum::{AntiCollapseError, Spectrum, decode_spectrum};

/// Invalid geometry/budget/vector or spectral entropy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum PartitionError {
    /// Standard band/LM/block geometry must yield 2..176 coefficients.
    InvalidGeometry,
    /// Eighth-bit budgets must be 0..81600 and payload 2..1275 bytes.
    InvalidBudget,
    /// Q31 gain must be nonnegative.
    InvalidGain,
    /// Fold input must match the dimension and have bounded Q14 energy.
    InvalidFold,
    /// Input must match dimension and have normalized Q24 squared norm at most 2.
    InvalidInput,
    /// Range/raw encoding failed without committing the writer or shared state.
    Encoding(EncoderError),
    /// Band arithmetic failed.
    Band(BandError),
    /// Pulse geometry failed.
    Pulses(PulseError),
    /// Pulse reconstruction failed.
    Pvq(PvqError),
    /// Entropy failed.
    Range(RangeError),
}
impl fmt::Display for PartitionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidGeometry => f.write_str("invalid CELT partition geometry"),
            Self::InvalidBudget => f.write_str("invalid CELT partition bit budget"),
            Self::InvalidGain => f.write_str("negative CELT partition gain"),
            Self::InvalidFold => f.write_str("invalid CELT partition fold vector"),
            Self::InvalidInput => f.write_str("invalid normalized CELT partition input"),
            Self::Encoding(e) => write!(f, "CELT partition encoding: {e}"),
            Self::Band(e) => write!(f, "CELT partition band: {e}"),
            Self::Pulses(e) => write!(f, "CELT partition pulses: {e}"),
            Self::Pvq(e) => write!(f, "CELT partition PVQ: {e}"),
            Self::Range(e) => write!(f, "CELT partition entropy: {e}"),
        }
    }
}
impl std::error::Error for PartitionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Band(e) => Some(e),
            Self::Pulses(e) => Some(e),
            Self::Pvq(e) => Some(e),
            Self::Range(e) => Some(e),
            Self::Encoding(e) => Some(e),
            _ => None,
        }
    }
}
impl From<BandError> for PartitionError {
    fn from(e: BandError) -> Self {
        Self::Band(e)
    }
}
impl From<PulseError> for PartitionError {
    fn from(e: PulseError) -> Self {
        Self::Pulses(e)
    }
}
impl From<PvqError> for PartitionError {
    fn from(e: PvqError) -> Self {
        Self::Pvq(e)
    }
}
impl From<RangeError> for PartitionError {
    fn from(e: RangeError) -> Self {
        Self::Range(e)
    }
}
impl From<EncoderError> for PartitionError {
    fn from(error: EncoderError) -> Self {
        Self::Encoding(error)
    }
}
impl From<super::math::MathError> for PartitionError {
    fn from(error: super::math::MathError) -> Self {
        Self::Band(BandError::Math(error))
    }
}
/// Shared shape-coding budget (eighth bits) and deterministic folding seed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PartitionState {
    remaining: i32,
    seed: u32,
}
impl PartitionState {
    /// Initialize a bounded frame-local shape budget, independently of entropy tell.
    pub fn new(remaining: i32, seed: u32) -> Result<Self, PartitionError> {
        if !(0..=81600).contains(&remaining) {
            return Err(PartitionError::InvalidBudget);
        }
        Ok(Self { remaining, seed })
    }
    /// Unspent eighth bits, possibly negative after a malformed theta overrun.
    pub fn remaining_bits(&self) -> i32 {
        self.remaining
    }
    /// Current noise/folding LCG seed.
    pub fn seed(&self) -> u32 {
        self.seed
    }
}
/// One standard mono partition request. LM -1 is used by recursive band splits.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PartitionControls {
    /// Standard band index 0..21 (exclusive upper bound).
    pub band: usize,
    /// Dimension multiplier exponent -1..3; base width comes from standard bands.
    pub lm: i8,
    /// Equal contiguous blocks 1/2/4/8/16, dividing the dimension.
    pub blocks: usize,
    /// Requested eighth-bit budget, 0..81600.
    pub bits: i32,
    /// Nonnegative Q31 gain.
    pub gain: i32,
    /// Pulse spreading policy.
    pub spread: Spread,
    /// Per-block fold/noise fill mask; bits above blocks are ignored.
    pub fill: u16,
}
/// Fixed-storage reconstructed Q24 coordinates and pre-TF collapse mask.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Partition {
    values: [i32; 176],
    length: usize,
    collapse: u16,
}
impl Partition {
    /// Q24 coefficients in partition time-block order.
    pub fn values(&self) -> &[i32] {
        &self.values[..self.length]
    }
    /// Nonempty block flags, up to sixteen blocks before TF reorganization.
    pub fn collapse_mask(&self) -> u16 {
        self.collapse
    }
}
/// Decode a mono partition. Both entropy and shared state commit only on success.
/// Fold input, if supplied, must match the dimension. Its rounded Q14 norm,
/// including a conservative per-coordinate dither margin, must fit signed i32.
/// This is a shape primitive, not a compressed-frame decoder.
pub fn decode_partition(
    entropy: &mut RangeDecoder<'_>,
    state: &mut PartitionState,
    controls: PartitionControls,
    fold: Option<&[i32]>,
) -> Result<Partition, PartitionError> {
    let c = controls;
    let n = validate(entropy, state, c, fold, 2)?;
    let mut next = entropy.clone();
    let mut shared = state.clone();
    let mut result = Partition {
        values: [0; 176],
        length: n,
        collapse: 0,
    };
    let mut decoder = Decoder {
        entropy: &mut next,
        state: &mut shared,
        band: c.band,
        spread: c.spread,
        intensity: 21,
        disable_inv: false,
        tf_change: 0,
    };
    result.collapse = decoder.partition(
        &mut result.values[..n],
        fold,
        Node {
            bits: c.bits,
            blocks: c.blocks,
            lm: c.lm,
            gain: c.gain,
            fill: u32::from(c.fill),
        },
    )? as u16;
    *entropy = next;
    *state = shared;
    Ok(result)
}
fn validate(
    entropy: &RangeDecoder<'_>,
    state: &PartitionState,
    c: PartitionControls,
    fold: Option<&[i32]>,
    minimum: usize,
) -> Result<usize, PartitionError> {
    validate_context(
        entropy.storage_bits(),
        entropy.tell(),
        state,
        c,
        fold,
        minimum,
    )
}
fn validate_context(
    storage_bits: u32,
    tell: u32,
    state: &PartitionState,
    c: PartitionControls,
    fold: Option<&[i32]>,
    minimum: usize,
) -> Result<usize, PartitionError> {
    if c.band >= 21 || !(-1..=3).contains(&c.lm) {
        return Err(PartitionError::InvalidGeometry);
    }
    let width = usize::from(BANDS[c.band + 1] - BANDS[c.band]);
    let n = if c.lm < 0 { width / 2 } else { width << c.lm };
    if !(minimum..=176).contains(&n) || !matches!(c.blocks, 1 | 2 | 4 | 8 | 16) || n % c.blocks != 0
    {
        return Err(PartitionError::InvalidGeometry);
    }
    if !(0..=81600).contains(&state.remaining)
        || !(0..=81600).contains(&c.bits)
        || !(16..=10200).contains(&storage_bits)
        || tell > storage_bits
    {
        return Err(PartitionError::InvalidBudget);
    }
    if c.gain < 0 {
        return Err(PartitionError::InvalidGain);
    }
    if let Some(fold) = fold {
        if fold.len() != n {
            return Err(PartitionError::InvalidFold);
        }
        let mut energy = 1i64;
        for &v in fold {
            let bound = ((i64::from(v).abs() + 512) >> 10) + 4;
            if bound > 32767 {
                return Err(PartitionError::InvalidFold);
            }
            energy += bound * bound;
            if energy > i64::from(i32::MAX) {
                return Err(PartitionError::InvalidFold);
            }
        }
    }
    Ok(n)
}
struct Decoder<'a, 'b> {
    entropy: &'a mut RangeDecoder<'b>,
    state: &'a mut PartitionState,
    band: usize,
    spread: Spread,
    intensity: usize,
    disable_inv: bool,
    tf_change: i8,
}
impl Decoder<'_, '_> {
    fn partition(
        &mut self,
        x: &mut [i32],
        fold: Option<&[i32]>,
        node: Node,
    ) -> Result<u32, PartitionError> {
        let Node {
            mut bits,
            mut blocks,
            mut lm,
            gain,
            mut fill,
        } = node;
        let cache = cache(self.band, lm);
        let original_blocks = blocks;
        let n = x.len();
        if lm != -1 && bits > i32::from(cache[usize::from(cache[0])]) + 12 && n > 2 {
            let half = n / 2;
            lm -= 1;
            if blocks == 1 {
                fill = (fill & 1) | (fill << 1);
            }
            blocks = (blocks + 1) >> 1;
            let split = self.theta(
                &mut bits,
                &mut fill,
                Theta {
                    n: half,
                    blocks,
                    original_blocks,
                    lm,
                    stereo: false,
                },
            )?;
            let mut delta = split.delta;
            if original_blocks > 1 && (split.angle & 0x3fff) != 0 {
                if split.angle > 8192 {
                    delta -= delta >> (4 - lm);
                } else {
                    delta = 0.min(delta + ((half as i32 * 8) >> (5 - lm)));
                }
            }
            let mut mid_bits = 0.max(bits.min((bits - delta) / 2));
            let mut side_bits = bits - mid_bits;
            self.state.remaining -= split.cost;
            let before = self.state.remaining;
            let (left, right) = x.split_at_mut(half);
            let (fold_left, fold_right) = if let Some(fold) = fold {
                (Some(&fold[..half]), Some(&fold[half..]))
            } else {
                (None, None)
            };
            let mgain = mul31(gain, i32::from(split.gains[0]) << 16);
            let sgain = mul31(gain, i32::from(split.gains[1]) << 16);
            let cm;
            if mid_bits >= side_bits {
                cm = self.partition(
                    left,
                    fold_left,
                    Node {
                        bits: mid_bits,
                        blocks,
                        lm,
                        gain: mgain,
                        fill,
                    },
                )?;
                let rebalance = mid_bits - (before - self.state.remaining);
                if rebalance > 24 && split.angle != 0 {
                    side_bits += rebalance - 24;
                }
                Ok(cm
                    | (self.partition(
                        right,
                        fold_right,
                        Node {
                            bits: side_bits,
                            blocks,
                            lm,
                            gain: sgain,
                            fill: fill >> blocks,
                        },
                    )? << (original_blocks >> 1)))
            } else {
                cm = self.partition(
                    right,
                    fold_right,
                    Node {
                        bits: side_bits,
                        blocks,
                        lm,
                        gain: sgain,
                        fill: fill >> blocks,
                    },
                )? << (original_blocks >> 1);
                let rebalance = side_bits - (before - self.state.remaining);
                if rebalance > 24 && split.angle != 16384 {
                    mid_bits += rebalance - 24;
                }
                Ok(cm
                    | self.partition(
                        left,
                        fold_left,
                        Node {
                            bits: mid_bits,
                            blocks,
                            lm,
                            gain: mgain,
                            fill,
                        },
                    )?)
            }
        } else {
            let mut q = bits_to_pulses(cache, bits);
            let mut cost = pulse_cost(cache, q);
            self.state.remaining -= cost;
            while self.state.remaining < 0 && q > 0 {
                self.state.remaining += cost;
                q -= 1;
                cost = pulse_cost(cache, q);
                self.state.remaining -= cost;
            }
            if q > 0 {
                let pulses = if q < 8 {
                    q
                } else {
                    (8 + (q & 7)) << ((q >> 3) - 1)
                };
                let band = pvq::decode_partition_band(
                    self.entropy,
                    PulseCodebook::new(n, pulses)?,
                    self.spread,
                    blocks,
                    gain,
                )?;
                x.copy_from_slice(band.values());
                Ok(u32::from(band.wide_collapse_mask()))
            } else {
                fill_partition(x, fold, self.state, blocks, gain, fill)
            }
        }
    }
    fn theta(&mut self, bits: &mut i32, fill: &mut u32, t: Theta) -> Result<Split, PartitionError> {
        let Theta {
            n,
            blocks,
            original_blocks,
            lm,
            stereo,
        } = t;
        let pulse_cap = i32::from(LOG_N[self.band]) + i32::from(lm) * 8;
        let offset = (pulse_cap >> 1) - if stereo && n == 2 { 16 } else { 4 };
        let qn = if stereo && self.band >= self.intensity {
            1
        } else {
            compute_qn(n, *bits, offset, pulse_cap, stereo)
        };
        let mut inv = false;
        let before = self.entropy.tell_frac();
        let angle = if qn == 1 {
            if stereo && *bits > 16 && self.state.remaining > 16 {
                inv = self.entropy.decode_bit_logp(2)?;
            }
            if self.disable_inv {
                inv = false;
            }
            0
        } else {
            let index = if stereo && n > 2 {
                let mut cdf = [0u16; 258];
                let q = qn as usize;
                for i in 0..=q {
                    cdf[i + 1] = cdf[i] + if i <= q / 2 { 3 } else { 1 };
                }
                self.entropy.decode_cdf(&cdf[..q + 2])? as u32
            } else if original_blocks > 1 || stereo {
                self.entropy.decode_uint(qn + 1)?
            } else {
                let mut cdf = [0u16; 258];
                let q = qn as usize;
                for i in 0..=q {
                    cdf[i + 1] = cdf[i] + (if i <= q / 2 { i + 1 } else { q + 1 - i }) as u16;
                }
                self.entropy.decode_cdf(&cdf[..q + 2])? as u32
            };
            (index * 16384 / qn) as u16
        };
        let cost = (self.entropy.tell_frac() - before) as i32;
        *bits -= cost;
        let gains = bands::split_gains(angle)?;
        let delta = if angle == 0 {
            *fill &= (1 << blocks) - 1;
            -16384
        } else if angle == 16384 {
            *fill &= ((1 << blocks) - 1) << blocks;
            16384
        } else {
            (16384
                + (((n - 1) << 7) as i16 as i32)
                    * bands::log2_tan(gains[1], gains[0])? as i16 as i32)
                >> 15
        };
        Ok(Split {
            inv,
            angle,
            gains,
            delta,
            cost,
        })
    }
}
fn fill_partition(
    x: &mut [i32],
    fold: Option<&[i32]>,
    state: &mut PartitionState,
    blocks: usize,
    gain: i32,
    mut fill: u32,
) -> Result<u32, PartitionError> {
    let mask = (1u32 << blocks) - 1;
    fill &= mask;
    if fill == 0 {
        x.fill(0);
        return Ok(0);
    }
    for (j, value) in x.iter_mut().enumerate() {
        state.seed = state.seed.wrapping_mul(1664525).wrapping_add(1013904223);
        *value = if let Some(fold) = fold {
            fold[j]
                + if state.seed & 0x8000 != 0 {
                    4096
                } else {
                    -4096
                }
        } else {
            ((state.seed as i32) >> 20) << 10
        };
    }
    bands::renormalize(x, gain)?;
    Ok(if fold.is_some() { fill } else { mask })
}
// LM decreases at each split and stops at -1: at most four splits deep
// (31 nodes). Standard dimensions and physical budgets bound all arithmetic.
struct Node {
    bits: i32,
    blocks: usize,
    lm: i8,
    gain: i32,
    fill: u32,
}
struct Theta {
    n: usize,
    blocks: usize,
    original_blocks: usize,
    lm: i8,
    stereo: bool,
}
struct Split {
    inv: bool,
    angle: u16,
    gains: [i16; 2],
    delta: i32,
    cost: i32,
}
fn cache(band: usize, lm: i8) -> &'static [u8] {
    // Validated standard dimension>=2 excludes the unavailable LM=-1/N=0 cells.
    let index = CACHE_INDEX[((lm + 1) as usize) * 21 + band] as usize;
    &CACHE_BITS[index..]
}
fn pulse_cost(cache: &[u8], q: usize) -> i32 {
    if q == 0 { 0 } else { i32::from(cache[q]) + 1 }
}
fn bits_to_pulses(cache: &[u8], mut bits: i32) -> usize {
    let mut lo = 0;
    let mut hi = usize::from(cache[0]);
    bits -= 1;
    for _ in 0..6 {
        let mid = (lo + hi + 1) >> 1;
        if i32::from(cache[mid]) >= bits {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    if bits - if lo == 0 { -1 } else { i32::from(cache[lo]) } <= i32::from(cache[hi]) - bits {
        lo
    } else {
        hi
    }
}
fn compute_qn(n: usize, bits: i32, offset: i32, pulse_cap: i32, stereo: bool) -> u32 {
    const EXP: [i32; 8] = [16384, 17866, 19483, 21247, 23170, 25267, 27554, 30048];
    let n2 = (2 * n - 1 - usize::from(stereo && n == 2)) as i32;
    let qb = ((bits + n2 * offset) / n2)
        .min(bits - pulse_cap - 32)
        .min(64);
    if qb < 4 {
        1
    } else {
        (((EXP[(qb & 7) as usize] >> (14 - (qb >> 3))) + 1) >> 1 << 1) as u32
    }
}
fn mul31(a: i32, b: i32) -> i32 {
    ((i64::from(a) * i64::from(b)) >> 31) as i32
}
