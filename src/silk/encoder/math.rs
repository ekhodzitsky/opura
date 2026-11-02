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

// Validated pitch/noise windows have length 16..=120 and a multiple of four.
pub(super) fn sine_window(output: &mut [i16], input: &[i16], falling: bool) {
    const FREQ: [i32; 27] = [
        12111, 9804, 8235, 7100, 6239, 5565, 5022, 4575, 4202, 3885, 3612, 3375, 3167, 2984, 2820,
        2674, 2542, 2422, 2313, 2214, 2123, 2038, 1961, 1889, 1822, 1760, 1702,
    ];
    let length = input.len();
    let f = FREQ[length / 4 - 4];
    let c = mul_wb(f, -f);
    let (mut s0, mut s1) = if falling {
        (65536, 65536 + (c >> 1) + (length as i32 >> 4))
    } else {
        (0, f + (length as i32 >> 3))
    };
    for k in (0..length).step_by(4) {
        output[k] = mul_wb((s0 + s1) >> 1, i32::from(input[k])) as i16;
        output[k + 1] = mul_wb(s1, i32::from(input[k + 1])) as i16;
        s0 = (mul_wb(s1, c) + 2 * s1 - s0 + 1).min(65536);
        output[k + 2] = mul_wb((s0 + s1) >> 1, i32::from(input[k + 2])) as i16;
        output[k + 3] = mul_wb(s0, i32::from(input[k + 3])) as i16;
        s1 = (mul_wb(s0, c) + 2 * s0 - s1).min(65536);
    }
}

pub(super) fn sigmoid(value: i32) -> i32 {
    let magnitude = value.unsigned_abs();
    if magnitude >= 192 {
        return if value < 0 { 0 } else { 32767 };
    }
    let index = (magnitude >> 5) as usize;
    let delta = [237, 153, 73, 30, 12, 7][index] * (magnitude & 31) as i32;
    if value < 0 {
        [16384, 8812, 3906, 1554, 589, 219][index] - delta
    } else {
        [16384, 23955, 28861, 31213, 32178, 32548][index] + delta
    }
}

pub(super) fn log2lin(log: i32) -> i32 {
    if log < 0 {
        return 0;
    }
    if log >= 3967 {
        return i32::MAX;
    }
    let base = 1i32 << (log >> 7);
    let f = log & 127;
    let delta = f + mul_wb(f * (128 - f), -174);
    if log < 2048 {
        base + ((base * delta) >> 7)
    } else {
        base + (base >> 7) * delta
    }
}

pub(super) fn expand32(coefficients: &mut [i32], mut chirp: i32) {
    let delta = chirp - 65536;
    for coefficient in coefficients {
        *coefficient = crate::silk::synthesis::mul_ww(chirp, *coefficient);
        chirp = chirp.wrapping_add(round_shift(chirp.wrapping_mul(delta), 16));
    }
}
