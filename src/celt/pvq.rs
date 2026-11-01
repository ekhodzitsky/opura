//! Classic CELT normalized algebraic pulse bands, before band/stereo orchestration.
//! Q24 normalization, spreading and collapse masks follow pinned Opus vq.c.
//! Copyright (c) 2007-2008 CSIRO; 2007-2009 Xiph.Org Foundation.
//! See THIRD_PARTY_NOTICES.md. QEXT refinement is not supported.
use super::{
    math,
    pulses::{PulseCodebook, PulseError},
};
use crate::range::RangeDecoder;
use std::fmt;

/// Classic spectral spreading policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Spread {
    /// Preserve pulse coordinate sparsity.
    None,
    /// Light spreading (factor 15).
    Light,
    /// Normal spreading (factor 10).
    Normal,
    /// Aggressive spreading (factor 5).
    Aggressive,
}
/// Invalid PVQ band geometry, gain, math or pulse entropy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum PvqError {
    /// PVQ reconstruction requires at least one pulse.
    NoPulses,
    /// Block count must be 1/2/4/8 and divide the codebook dimension.
    InvalidBlocks,
    /// Q31 gain must be nonnegative (0..i32::MAX).
    InvalidGain,
    /// Pulse codebook decoding failed.
    Pulses(PulseError),
    /// Scalar normalization failed.
    Math(math::MathError),
}
impl fmt::Display for PvqError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoPulses => f.write_str("CELT PVQ band requires a positive pulse count"),
            Self::InvalidBlocks => {
                f.write_str("CELT PVQ requires 1/2/4/8 blocks dividing dimension")
            }
            Self::InvalidGain => f.write_str("CELT PVQ Q31 gain must be nonnegative"),
            Self::Pulses(e) => write!(f, "CELT PVQ pulses: {e}"),
            Self::Math(e) => write!(f, "CELT PVQ normalization: {e}"),
        }
    }
}
impl std::error::Error for PvqError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Pulses(e) => Some(e),
            Self::Math(e) => Some(e),
            _ => None,
        }
    }
}
impl From<PulseError> for PvqError {
    fn from(e: PulseError) -> Self {
        Self::Pulses(e)
    }
}
impl From<math::MathError> for PvqError {
    fn from(e: math::MathError) -> Self {
        Self::Math(e)
    }
}

/// Fixed-storage normalized Q24 band and its nonempty short-block mask.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NormalizedBand {
    values: [i32; 176],
    length: usize,
    collapse: u16,
}
impl NormalizedBand {
    /// Normalized Q24 spectral coordinates after inverse spreading.
    pub fn values(&self) -> &[i32] {
        &self.values[..self.length]
    }
    /// Bit i is set when raw pulse block i contains at least one pulse.
    /// This mask precedes spreading and is independent of output gain.
    pub fn collapse_mask(&self) -> u8 {
        self.collapse as u8
    }
    pub(crate) fn wide_collapse_mask(&self) -> u16 {
        self.collapse
    }
}

/// Decode one positive-pulse CELT PVQ band, atomically updating entropy.
/// Gain is Q31, output Q24. Blocks select equal contiguous short-block groups.
/// Range input may zero-extend; callers enforce the surrounding CELT bit budget.
pub fn decode_band(
    entropy: &mut RangeDecoder<'_>,
    codebook: PulseCodebook,
    spread: Spread,
    blocks: usize,
    gain: i32,
) -> Result<NormalizedBand, PvqError> {
    if !matches!(blocks, 1 | 2 | 4 | 8) {
        return Err(PvqError::InvalidBlocks);
    }
    decode_partition_band(entropy, codebook, spread, blocks, gain)
}
pub(crate) fn decode_partition_band(
    entropy: &mut RangeDecoder<'_>,
    codebook: PulseCodebook,
    spread: Spread,
    blocks: usize,
    gain: i32,
) -> Result<NormalizedBand, PvqError> {
    if codebook.pulses() == 0 {
        return Err(PvqError::NoPulses);
    }
    let n = codebook.dimensions();
    if !matches!(blocks, 1 | 2 | 4 | 8 | 16) || n % blocks != 0 {
        return Err(PvqError::InvalidBlocks);
    }
    if gain < 0 {
        return Err(PvqError::InvalidGain);
    }
    let mut next = entropy.clone();
    let pulses = codebook.decode(&mut next)?;
    let energy = pulses.squared_norm() as i32;
    let k = (31 - energy.leading_zeros() as i32) >> 1;
    let normalized = shift(energy, 2 * (k - 7) - 15);
    let scale = mul_q31(math::rsqrt_normalized_q31(normalized)?, gain);
    let mut result = NormalizedBand {
        values: [0; 176],
        length: n,
        collapse: 0,
    };
    for (target, &pulse) in result.values[..n].iter_mut().zip(pulses.values()) {
        let product = ((i64::from(pulse) * i64::from(scale)) >> 15) as i32;
        *target = shift(product, k + 15 - 24);
    }
    inverse_spread(&mut result.values[..n], blocks, codebook.pulses(), spread)?;
    for (i, block) in pulses.values().chunks_exact(n / blocks).enumerate() {
        if block.iter().any(|&p| p != 0) {
            result.collapse |= 1 << i;
        }
    }
    *entropy = next;
    Ok(result)
}
fn inverse_spread(
    x: &mut [i32],
    blocks: usize,
    pulses: usize,
    spread: Spread,
) -> Result<(), PvqError> {
    let n = x.len();
    if 2 * pulses >= n || spread == Spread::None {
        return Ok(());
    }
    let factor = match spread {
        Spread::None => 0,
        Spread::Light => 15,
        Spread::Normal => 10,
        Spread::Aggressive => 5,
    };
    let gain = mul_q31(
        32767 * n as i32,
        math::reciprocal((n + factor * pulses) as i32)?,
    );
    let theta = (((gain as i16 as i32) * (gain as i16 as i32)) >> 15) >> 1;
    let c = i32::from(math::cos_norm(theta));
    let s = i32::from(math::cos_norm(32767 - theta));
    let mut stride2 = 0;
    if n >= 8 * blocks {
        stride2 = 1;
        while (stride2 * stride2 + stride2) * blocks + (blocks >> 2) < n {
            stride2 += 1;
        }
    }
    for block in x.chunks_exact_mut(n / blocks) {
        if stride2 != 0 {
            rotate(block, stride2, s, c);
        }
        rotate(block, 1, c, s);
    }
    Ok(())
}
fn rotate(x: &mut [i32], stride: usize, c: i32, s: i32) {
    // Each rotation intentionally goes through rounded Q14 coordinates.
    for value in x.iter_mut() {
        *value = (*value + 512) >> 10;
    }
    for i in 0..x.len().saturating_sub(stride) {
        rotate_pair(x, i, stride, c, s);
    }
    for i in (0..x.len().saturating_sub(2 * stride)).rev() {
        rotate_pair(x, i, stride, c, s);
    }
    for value in x.iter_mut() {
        *value = value.wrapping_shl(10);
    }
}
fn rotate_pair(x: &mut [i32], i: usize, stride: usize, c: i32, s: i32) {
    let a = i32::from(x[i] as i16);
    let b = i32::from(x[i + stride] as i16);
    x[i + stride] = ((c * b + s * a + 16384) >> 15) as i16 as i32;
    x[i] = ((c * a - s * b + 16384) >> 15) as i16 as i32;
}
fn mul_q31(a: i32, b: i32) -> i32 {
    ((i64::from(a) * i64::from(b)) >> 31) as i32
}
fn shift(x: i32, amount: i32) -> i32 {
    if amount > 0 {
        x >> amount
    } else {
        x.wrapping_shl((-amount) as u32)
    }
}
