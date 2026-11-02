//! Fixed-point CELT scalar math following Opus v1.6.1 mathops.c/mathops.h.
//!
//! Q formats are part of each operation's contract. Results reproduce reference
//! approximations, including rounding, truncation and saturation, rather than
//! correctly rounded real arithmetic. See THIRD_PARTY_NOTICES.md.
// Copyright (c) 2002-2008 Jean-Marc Valin; 2007-2008 CSIRO;
// 2007-2009 Xiph.Org Foundation; 2024 Arm Limited. See THIRD_PARTY_NOTICES.md.
use std::fmt;

/// Input outside a fixed-point operation's documented domain.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum MathError {
    /// The scalar operation requires nonnegative input.
    NegativeInput,
    /// Reciprocal/division denominator must be positive.
    InvalidDenominator,
    /// Input lies outside the operation-specific normalized range.
    NotNormalized,
    /// Fractional division requires numerator magnitude no greater than denominator.
    NotFractional,
}
impl fmt::Display for MathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NegativeInput => "CELT scalar input must be nonnegative",
            Self::InvalidDenominator => "CELT reciprocal/division denominator must be positive",
            Self::NotNormalized => "CELT input lies outside the required normalized range",
            Self::NotFractional => "CELT fractional numerator magnitude exceeds denominator",
        })
    }
}
impl std::error::Error for MathError {}

/// Exact floor(sqrt(x)) for any unsigned 32-bit integer, including zero.
/// Uses at most 16 iterations and no floating-point arithmetic.
pub fn integer_sqrt(mut x: u32) -> u32 {
    if x == 0 {
        return 0;
    }
    let mut shift = (31 - x.leading_zeros()) / 2;
    let mut bit = 1 << shift;
    let mut root = 0;
    loop {
        let trial = ((root << 1) + bit) << shift;
        if trial <= x {
            root += bit;
            x -= trial;
        }
        if shift == 0 {
            return root;
        }
        bit >>= 1;
        shift -= 1;
    }
}

/// Reference square-root approximation: QX input to Q(X/2) output.
/// Returns zero for zero and clamps inputs >= 2^30 to 32767.
pub fn sqrt(x: i32) -> Result<i32, MathError> {
    if x < 0 {
        return Err(MathError::NegativeInput);
    }
    if x == 0 {
        return Ok(0);
    }
    if x >= 1 << 30 {
        return Ok(32767);
    }
    let k = (ilog(x) >> 1) - 7;
    let n = (vshift(x, 2 * k) - 32768) as i16;
    let mut polynomial = 336;
    for coefficient in [-1002, 1592, -2901, 11574] {
        polynomial = add16(coefficient, mul_q15(n as i32, polynomial));
    }
    Ok(vshift(23171 + mul_q15(n as i32, polynomial), 7 - k))
}

/// Approximate reciprocal square root: normalized Q16 [16384,65536) to Q14.
pub fn rsqrt_normalized(x: i32) -> Result<i16, MathError> {
    if !(16384..65536).contains(&x) {
        return Err(MathError::NotNormalized);
    }
    let n = (x - 32768) as i16 as i32;
    let r = add16(23557, mul_q15(n, add16(-13490, mul_q15(n, 6713))));
    let r2 = mul_q15(r, r);
    let y = ((add16(mul_q15(r2, n), r2) - 16384) << 1) as i16 as i32;
    Ok(add16(r, mul_q15(r, mul_q15(y, mul_q15(y, 12288) - 16384))) as i16)
}

/// Refined reciprocal square root: normalized Q31 [2^29,2^31) to Q29.
/// Uses the Q14 estimate followed by the reference fixed-point Newton step.
pub fn rsqrt_normalized_q31(x: i32) -> Result<i32, MathError> {
    if x < 1 << 29 {
        return Err(MathError::NotNormalized);
    }
    let root = i32::from(rsqrt_normalized(x >> 15)?) << 15;
    let mut tmp = mul_q31(root, root);
    tmp = mul_q31(1 << 30, tmp);
    tmp = mul_q31(x, tmp);
    Ok(mul_q31(root, 201326592 - tmp).wrapping_shl(4))
}
fn mul_q31(a: i32, b: i32) -> i32 {
    ((i64::from(a) * i64::from(b)) >> 31) as i32
}

/// Approximate cos(pi*x/2): periodic Q15 angle to Q15 amplitude.
/// The period is 131072; cardinal values are exactly 0 or +/-32767.
pub fn cos_norm(x: i32) -> i16 {
    let mut x = x & 0x1ffff;
    if x > 65536 {
        x = 131072 - x;
    }
    if x & 0x7fff == 0 {
        return if x & 0xffff != 0 {
            0
        } else if x != 0 {
            -32767
        } else {
            32767
        };
    }
    if x < 32768 {
        cosine_quadrant(x)
    } else {
        -cosine_quadrant(65536 - x)
    }
}
fn cosine_quadrant(x: i32) -> i16 {
    let x2 = mul_p15(x, x) as i16 as i32;
    (1 + (32767 - x2 + mul_p15(x2, -7651 + mul_p15(x2, 8277 + mul_p15(-626, x2)))).min(32766))
        as i16
}

/// Approximate log2(x): nonnegative Q14 input to Q10 output.
/// Zero returns the reference sentinel -32767.
pub fn log2(x: i32) -> Result<i16, MathError> {
    if x < 0 {
        return Err(MathError::NegativeInput);
    }
    if x == 0 {
        return Ok(-32767);
    }
    let i = ilog(x);
    let n = (vshift(x, i - 15) - 32768 - 16384) as i16 as i32;
    let mut polynomial = -1401;
    for coefficient in [2545, -5217, 15746, -6793] {
        polynomial = add16(coefficient, mul_q15(n, polynomial));
    }
    Ok((((i - 13) << 10) + (polynomial >> 4)) as i16)
}

/// Approximate 2^x: Q10 exponent to Q16 output.
/// Integer exponents below -15 return zero, above 14 return 0x7f000000.
pub fn exp2(x: i16) -> i32 {
    let integer = i32::from(x) >> 10;
    if integer > 14 {
        return 0x7f000000;
    }
    if integer < -15 {
        return 0;
    }
    vshift(
        exp2_frac((i32::from(x) - (integer << 10)) as i16),
        -integer - 2,
    )
}

// Classic Q10 fractional exponent polynomial, including rounded endpoint 1024.
pub(super) fn exp2_frac(x: i16) -> i32 {
    let frac = (i32::from(x) << 4) as i16 as i32;
    add16(
        16383,
        mul_q15(
            frac,
            add16(22804, mul_q15(frac, add16(14819, mul_q15(10204, frac)))),
        ),
    )
}

/// Approximate reciprocal: positive Q15 input to Q16 output.
pub fn reciprocal(x: i32) -> Result<i32, MathError> {
    if x <= 0 {
        return Err(MathError::InvalidDenominator);
    }
    Ok(reciprocal_positive(x))
}
fn reciprocal_positive(x: i32) -> i32 {
    let i = ilog(x);
    let n = (vshift(x, i - 15) - 32768) as i16 as i32;
    vshift(reciprocal_normalized_q15(n), i - 16)
}
fn reciprocal_normalized_q15(n: i32) -> i32 {
    let mut r = add16(30840, mul_q15(-15420, n));
    r = (r - mul_q15(r, add16(mul_q15(r, n), add16(r, -32768)))) as i16 as i32;
    (r - add16(1, mul_q15(r, add16(mul_q15(r, n), add16(r, -32768))))) as i16 as i32
}

/// Refined reference square root: QX input to Q(X/2 + 16) output.
/// Returns zero for zero, rejects negatives and clamps inputs >=2^30 to i32::MAX.
pub fn sqrt_extended(x: i32) -> Result<i32, MathError> {
    if x < 0 {
        return Err(MathError::NegativeInput);
    }
    if x == 0 {
        return Ok(0);
    }
    if x >= 1 << 30 {
        return Ok(i32::MAX);
    }
    let k = ilog(x) >> 1;
    let normalized = vshift(x, 2 * (k - 14) - 1);
    let root = mul_q31(rsqrt_normalized_q31(normalized)?, normalized);
    Ok(if k < 12 {
        root.wrapping_add(1 << (11 - k)) >> (12 - k)
    } else {
        root.wrapping_shl((k - 12) as u32)
    })
}

/// Refined reciprocal: normalized Q31 [2^30,2^31) input to Q30 output.
/// Preserves native Q15 initialization and the full-width Newton correction.
pub fn reciprocal_normalized_q31(x: i32) -> Result<i32, MathError> {
    if x < 1 << 30 {
        return Err(MathError::NotNormalized);
    }
    let estimate = reciprocal_normalized_q15((x >> 15) - 32768) << 16;
    let error = mul_q31(estimate, x).wrapping_sub(1 << 30);
    Ok(estimate.wrapping_sub(mul_q31(error, estimate).wrapping_shl(1).wrapping_add(1)))
}

/// Reference atan(x)*2/pi: normalized Q30 input [-1,1] to Q30 angle.
/// Endpoints return exactly +/-0.5 in Q30; zero returns zero.
pub fn atan_norm(x: i32) -> Result<i32, MathError> {
    if !(-(1 << 30)..=1 << 30).contains(&x) {
        return Err(MathError::NotNormalized);
    }
    if x == 1 << 30 {
        return Ok(1 << 29);
    }
    if x == -(1 << 30) {
        return Ok(-(1 << 29));
    }
    let doubled = x.wrapping_shl(1);
    let square = mul_q31(doubled, x);
    let mut value = mul_q31(square, -598602432);
    for coefficient in [
        1583306112i32,
        -1985085440,
        1682636672,
        -1200579328,
        857391616,
        -715791936,
    ] {
        value = mul_q31(square, coefficient.wrapping_add(value));
    }
    Ok(mul_q31(1367130551, x.wrapping_add(mul_q31(doubled, value))))
}
/// Reference atan2(y,x)*2/pi for nonnegative same-scale magnitudes.
/// Returns a Q30 angle in `[0,1]`. Both-zero returns zero; negative inputs fail.
pub fn atan2_norm(y: i32, x: i32) -> Result<i32, MathError> {
    if x < 0 || y < 0 {
        return Err(MathError::NegativeInput);
    }
    if x == 0 && y == 0 {
        return Ok(0);
    }
    if y < x {
        atan_norm(fractional_divide(y, x)? >> 1)
    } else {
        Ok((1 << 30) - atan_norm(fractional_divide(x, y)? >> 1)?)
    }
}

/// Approximate a/b in Q29, for b>0 and -b<=a<=b.
/// Preserves reference reciprocal refinement instead of using integer division.
pub fn fractional_divide_q29(a: i32, b: i32) -> Result<i32, MathError> {
    if b <= 0 {
        return Err(MathError::InvalidDenominator);
    }
    if i64::from(a).abs() > i64::from(b) {
        return Err(MathError::NotFractional);
    }
    let shift = ilog(b) - 29;
    let a = vshift(a, shift);
    let b = vshift(b, shift);
    let rounded_b = (b + 32768) >> 16;
    let rcp = ((reciprocal_positive(rounded_b) + 4) >> 3) as i16 as i32;
    let result = ((i64::from(rcp) * i64::from(a)) >> 15) as i32;
    let remainder = ((a + 2) >> 2) - ((i64::from(result) * i64::from(b)) >> 31) as i32;
    Ok(result
        .wrapping_add((((i64::from(rcp) * i64::from(remainder)) >> 15) as i32).wrapping_shl(2)))
}

/// Approximate a/b in saturated Q31, for b>0 and -b<=a<=b.
/// Saturates to +/-2147483647, matching the reference negative endpoint.
pub fn fractional_divide(a: i32, b: i32) -> Result<i32, MathError> {
    let result = fractional_divide_q29(a, b)?;
    Ok(if result >= 1 << 29 {
        i32::MAX
    } else if result <= -(1 << 29) {
        -i32::MAX
    } else {
        result << 2
    })
}

fn ilog(x: i32) -> i32 {
    31 - x.leading_zeros() as i32
}
fn vshift(x: i32, shift: i32) -> i32 {
    if shift > 0 {
        x >> shift
    } else {
        x.wrapping_shl((-shift) as u32)
    }
}
fn add16(a: i32, b: i32) -> i32 {
    (a as i16).wrapping_add(b as i16) as i32
}
fn mul_q15(a: i32, b: i32) -> i32 {
    (i32::from(a as i16) * i32::from(b as i16)) >> 15
}
fn mul_p15(a: i32, b: i32) -> i32 {
    (16384 + i32::from(a as i16) * i32::from(b as i16)) >> 15
}
