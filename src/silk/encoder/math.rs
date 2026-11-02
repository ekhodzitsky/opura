//! Shared fixed-point encoder analysis arithmetic; callers validate domains.
use crate::silk::synthesis::{mul_wb, round_shift, saturate16, shift_saturated};
// Nonnegative input; native log(0) sentinel is -128. Normalization preserves
// the seven bits below the leading one.
pub(super) fn lin2log(value: i32) -> i32 {
    let zeros = value.leading_zeros();
    let fraction = ((value as u32).wrapping_shl(zeros) >> 24) & 127;
    let fraction = fraction as i32;
    (31 - zeros as i32) * 128 + fraction + ((fraction * (128 - fraction) * 179) >> 16)
}
// Signed native reciprocal-refined division. Denominator is nonzero, precision
// is 0..=31. Encoder callers keep ratios representable or accept saturation.
pub(super) fn divide_q(numerator: i32, denominator: i32, precision: u32) -> i32 {
    if numerator == 0 {
        return 0;
    }
    let a_head = numerator.unsigned_abs().leading_zeros().saturating_sub(1);
    let b_head = denominator.unsigned_abs().leading_zeros().saturating_sub(1);
    let a = numerator.wrapping_shl(a_head);
    let b = denominator.wrapping_shl(b_head);
    let inverse = (i32::MAX >> 2) / (b >> 16);
    let initial = mul_wb(a, inverse);
    let residual =
        a.wrapping_sub((((i64::from(b) * i64::from(initial)) >> 32) as i32).wrapping_shl(3));
    let result = initial.wrapping_add(mul_wb(residual, inverse));
    let shift = 29 + a_head as i32 - b_head as i32 - precision as i32;
    if shift <= -32 {
        if result > 0 {
            i32::MAX
        } else if result < 0 {
            i32::MIN
        } else {
            0
        }
    } else if shift < 0 {
        shift_saturated(result, (-shift) as u32)
    } else if shift < 32 {
        result >> shift
    } else {
        0
    }
}

pub(super) fn analysis_filter(output: &mut [i16], input: &[i16], coefficients: &[i16]) {
    let order = coefficients.len();
    output[..order].fill(0);
    for i in order..input.len() {
        let mut sum = 0i32;
        for (j, &coefficient) in coefficients.iter().enumerate() {
            sum = sum.wrapping_add(i32::from(input[i - j - 1]) * i32::from(coefficient));
        }
        output[i] = saturate16(round_shift(
            (i32::from(input[i]) << 12).wrapping_sub(sum),
            12,
        ));
    }
}

// Pairwise shifting (rather than one final shift of a 64-bit sum) preserves
// reference rounding, which can decide the selected interpolation factor.
pub(super) fn energy(input: &[i16]) -> (i32, u32) {
    let mut shift = 31 - (input.len() as u32).leading_zeros();
    let accumulate = |shift: u32, start: u32| {
        input.chunks(2).fold(start, |sum, pair| {
            let value = pair.iter().fold(0u32, |sum, &x| {
                sum.wrapping_add((i32::from(x) * i32::from(x)) as u32)
            });
            sum.wrapping_add(value >> shift)
        })
    };
    let first = accumulate(shift, input.len() as u32);
    shift = (shift as i32 + 3 - first.leading_zeros() as i32).max(0) as u32;
    (accumulate(shift, 0) as i32, shift)
}
