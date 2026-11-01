//! Classic CELT split arithmetic and band-vector reconstruction primitives.
//! These operations support forthcoming recursive spectral decoding, not packets.
//! Copyright (c) 2007-2008 CSIRO; 2007-2009 Xiph.Org Foundation;
//! 2008-2009 Gregory Maxwell. See THIRD_PARTY_NOTICES.md.
use super::{
    band_tables::{HAAR_Q31, MIN_STEREO_Q28, ORDER},
    math::{self, MathError},
};
use std::fmt;

/// Invalid band geometry, coded angle, gain or vector domain.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum BandError {
    /// Band vectors need 1..176 samples with matching/power-of-two block geometry.
    InvalidGeometry,
    /// Coded theta requires endpoints0/16384 or an interior angle64..16320.
    InvalidAngle,
    /// Log-ratio sine and cosine must be strictly positive Q15 values.
    InvalidTrig,
    /// Q31 gain must be nonnegative.
    InvalidGain,
    /// Vector exceeds the documented normalization/merge energy domain.
    InvalidVector,
    /// Scalar normalization failed.
    Math(MathError),
}
impl fmt::Display for BandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidGeometry => f.write_str("invalid CELT band-vector geometry"),
            Self::InvalidAngle => f.write_str("invalid classic coded CELT theta"),
            Self::InvalidTrig => f.write_str("CELT log2tan needs positive sine/cosine"),
            Self::InvalidGain => f.write_str("CELT band gain must be nonnegative"),
            Self::InvalidVector => f.write_str("CELT band vector exceeds normalization domain"),
            Self::Math(e) => write!(f, "CELT band scalar math: {e}"),
        }
    }
}
impl std::error::Error for BandError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Math(e) => Some(e),
            _ => None,
        }
    }
}
impl From<MathError> for BandError {
    fn from(e: MathError) -> Self {
        Self::Math(e)
    }
}

/// Bit-exact coded-theta cosine/sine Q15 gains, including original endpoints.
/// Theta uses the Q14 quarter-circle scale: 0..16384. Classic qn<=256 makes
/// valid interior angles64..16320; smaller interior values reject.
pub fn split_gains(theta: u16) -> Result<[i16; 2], BandError> {
    match theta {
        0 => Ok([32767, 0]),
        16384 => Ok([0, 32767]),
        64..=16320 => Ok([
            bitexact_cos(theta as i32),
            bitexact_cos(16384 - i32::from(theta)),
        ]),
        _ => Err(BandError::InvalidAngle),
    }
}
fn bitexact_cos(x: i32) -> i16 {
    let x2 = ((4096 + x * x) >> 13) as i16 as i32;
    (1 + (32767 - x2) + frac(x2, -7651 + frac(x2, 8277 + frac(-626, x2)))) as i16
}
/// Bit-exact log2(sine/cosine) split-allocation approximation in Q11.
/// Both Q15 arguments must be positive; endpoints are handled by the caller.
pub fn log2_tan(sine: i16, cosine: i16) -> Result<i32, BandError> {
    if sine <= 0 || cosine <= 0 {
        return Err(BandError::InvalidTrig);
    }
    let ls = 32 - (sine as u32).leading_zeros() as i32;
    let lc = 32 - (cosine as u32).leading_zeros() as i32;
    let sine = i32::from(sine) << (15 - ls);
    let cosine = i32::from(cosine) << (15 - lc);
    Ok((ls - lc) * 2048 + frac(sine, frac(sine, -2597) + 7932)
        - frac(cosine, frac(cosine, -2597) + 7932))
}
fn frac(a: i32, b: i32) -> i32 {
    (16384 + i32::from(a as i16) * i32::from(b as i16)) >> 15
}

/// Reorganize frequency-interleaved blocks into time order, optionally applying
/// the standard Hadamard order. Stride1/2/4/8/16 divides the band length;
/// Hadamard ordering requires stride>=2. Errors preserve the input.
pub fn deinterleave(values: &mut [i32], stride: usize, hadamard: bool) -> Result<(), BandError> {
    permutation(values, stride, hadamard, false)
}
/// Inverse of deinterleave, with the same validated geometry and exact copying.
pub fn interleave(values: &mut [i32], stride: usize, hadamard: bool) -> Result<(), BandError> {
    permutation(values, stride, hadamard, true)
}
fn permutation(
    values: &mut [i32],
    stride: usize,
    hadamard: bool,
    inverse: bool,
) -> Result<(), BandError> {
    let n = values.len();
    if !(1..=176).contains(&n)
        || !matches!(stride, 1 | 2 | 4 | 8 | 16)
        || n % stride != 0
        || hadamard && stride == 1
    {
        return Err(BandError::InvalidGeometry);
    }
    let per = n / stride;
    let mut scratch = [0i32; 176];
    for i in 0..stride {
        let order = if hadamard {
            usize::from(ORDER[stride - 2 + i])
        } else {
            i
        };
        for j in 0..per {
            if inverse {
                scratch[j * stride + i] = values[order * per + j];
            } else {
                scratch[order * per + j] = values[j * stride + i];
            }
        }
    }
    values.copy_from_slice(&scratch[..n]);
    Ok(())
}
/// One reference Haar stage on an even per-stride length, with power-of-two
/// stride and total1..176. Coefficient products truncate before wrapping sums.
pub fn haar(values: &mut [i32], stride: usize) -> Result<(), BandError> {
    let n = values.len();
    if !(1..=176).contains(&n)
        || !stride.is_power_of_two()
        || stride > n
        || n % stride != 0
        || (n / stride) % 2 != 0
    {
        return Err(BandError::InvalidGeometry);
    }
    for i in 0..stride {
        for j in 0..n / stride / 2 {
            let at = stride * 2 * j + i;
            let a = mul31(HAAR_Q31, values[at]);
            let b = mul31(HAAR_Q31, values[at + stride]);
            values[at] = a.wrapping_add(b);
            values[at + stride] = a.wrapping_sub(b);
        }
    }
    Ok(())
}

/// Legacy CELT folded/noise-vector normalization to nonnegative Q31 gain.
/// Rounded Q14 coordinates must fit i16, and one plus their squared sum must
/// fit positive i32. This includes the standard decoder's noise/folded inputs.
/// All validation/scalar work precedes writes; errors leave the vector unchanged.
pub fn renormalize(values: &mut [i32], gain: i32) -> Result<(), BandError> {
    if !(1..=176).contains(&values.len()) {
        return Err(BandError::InvalidGeometry);
    }
    if gain < 0 {
        return Err(BandError::InvalidGain);
    }
    let mut q14 = [0i16; 176];
    let mut energy = 1i64;
    for (i, &value) in values.iter().enumerate() {
        let rounded = (i64::from(value) + 512) >> 10;
        q14[i] = i16::try_from(rounded).map_err(|_| BandError::InvalidVector)?;
        energy += rounded * rounded;
        if energy > i64::from(i32::MAX) {
            return Err(BandError::InvalidVector);
        }
    }
    let energy = energy as i32;
    let k = (31 - energy.leading_zeros() as i32) >> 1;
    let normalized = shift(energy, 2 * (k - 7));
    let g = mul31(i32::from(math::rsqrt_normalized(normalized)?), gain) as i16;
    for (value, &rounded) in values.iter_mut().zip(&q14) {
        let product = i32::from(g) * i32::from(rounded);
        *value = (((product + (1 << k)) >> (k + 1)) as i16 as i32) << 10;
    }
    Ok(())
}
/// Reconstruct/renormalize decoded stereo from normalized Q24 mid/side vectors.
/// Mid gain is nonnegative Q31; each input vector's Q48 squared norm is <=2.
/// Near-cancelled output energy copies mid to both channels as upstream does.
/// Invalid domains/scalar errors preserve both complete vectors.
pub fn stereo_merge(left: &mut [i32], right: &mut [i32], mid: i32) -> Result<(), BandError> {
    if !(1..=176).contains(&left.len()) || left.len() != right.len() {
        return Err(BandError::InvalidGeometry);
    }
    if mid < 0 {
        return Err(BandError::InvalidGain);
    }
    merge_energy(left)?;
    let right_energy = merge_energy(right)?;
    let dot = left
        .iter()
        .zip(right.iter())
        .map(|(&x, &y)| i64::from(x) * i64::from(y))
        .sum::<i64>();
    let product = mul31(mid, (dot >> 20) as i32);
    let side = (right_energy >> 20) as i32;
    let base = mul31(mid, mid) >> 3;
    let el = base
        .wrapping_add(side)
        .wrapping_sub(product.wrapping_shl(1));
    let er = base
        .wrapping_add(side)
        .wrapping_add(product.wrapping_shl(1));
    if el < MIN_STEREO_Q28 || er < MIN_STEREO_Q28 {
        right.copy_from_slice(left);
        return Ok(());
    }
    let kl = (31 - el.leading_zeros() as i32) >> 1;
    let kr = (31 - er.leading_zeros() as i32) >> 1;
    let lgain = math::rsqrt_normalized_q31(shift(el, 2 * kl - 29))?;
    let rgain = math::rsqrt_normalized_q31(shift(er, 2 * kr - 29))?;
    for (left, right) in left.iter_mut().zip(right.iter_mut()) {
        let l = mul31(mid, *left);
        let r = *right;
        *left = shift(mul31(lgain, l.wrapping_sub(r)), kl.max(7) - 15);
        *right = shift(mul31(rgain, l.wrapping_add(r)), kr.max(7) - 15);
    }
    Ok(())
}
fn merge_energy(values: &[i32]) -> Result<i64, BandError> {
    let mut sum = 0i64;
    for &value in values {
        sum += i64::from(value) * i64::from(value);
        if sum > 2i64 << 48 {
            return Err(BandError::InvalidVector);
        }
    }
    Ok(sum)
}
fn mul31(a: i32, b: i32) -> i32 {
    ((i64::from(a) * i64::from(b)) >> 31) as i32
}
fn shift(x: i32, amount: i32) -> i32 {
    if amount > 0 {
        x >> amount
    } else {
        x.wrapping_shl((-amount) as u32)
    }
}
