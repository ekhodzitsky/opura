//! Fixed-point warped correlations, reflection recursion and coefficient fitting.
use super::*;
pub(super) fn warped_correlation(input: &[i16], warp: i32, order: usize) -> ([i32; 25], i32) {
    let mut state = [0i32; 25];
    let mut correlation = [0i64; 25];
    for &sample in input {
        let mut a = i32::from(sample) << 13;
        for i in (0..order).step_by(2) {
            let b = state[i].wrapping_add(mul_wb(state[i + 1].wrapping_sub(a), warp));
            state[i] = a;
            correlation[i] += (i64::from(a) * i64::from(state[0])) >> 16;
            a = state[i + 1].wrapping_add(mul_wb(state[i + 2].wrapping_sub(b), warp));
            state[i + 1] = b;
            correlation[i + 1] += (i64::from(b) * i64::from(state[0])) >> 16;
        }
        state[order] = a;
        correlation[order] += (i64::from(a) * i64::from(state[0])) >> 16;
    }
    let shift = (correlation[0].leading_zeros() as i32 - 35).clamp(-22, 20);
    let mut result = [0; 25];
    for (out, &v) in result.iter_mut().zip(&correlation) {
        *out = if shift >= 0 {
            (v << shift) as i32
        } else {
            (v >> -shift) as i32
        };
    }
    (result, -(10 + shift))
}
pub(super) fn schur(output: &mut [i32], input: &[i32]) -> i32 {
    if input[0] <= 0 {
        return 0;
    }
    let order = output.len();
    let mut c = [[0i32; 2]; 25];
    for i in 0..=order {
        c[i] = [input[i]; 2];
    }
    for k in 0..order {
        if c[k + 1][0].unsigned_abs() >= c[0][1] as u32 {
            output[k] = if c[k + 1][0] > 0 { -64881 } else { 64881 };
            break;
        }
        let rc = divide_q(-c[k + 1][0], c[0][1], 31);
        output[k] = round_shift(rc, 15);
        for n in 0..order - k {
            let a = c[n + k + 1][0];
            let b = c[n][1];
            c[n + k + 1][0] =
                a.wrapping_add(((i64::from(b.wrapping_shl(1)) * i64::from(rc)) >> 32) as i32);
            c[n][1] = b.wrapping_add(((i64::from(a.wrapping_shl(1)) * i64::from(rc)) >> 32) as i32);
        }
    }
    c[0][1].max(1)
}
pub(super) fn fit(output: &mut [i16], coefficients: &mut [i32]) {
    for _ in 0..10 {
        let mut maximum = 0i32;
        let mut index = 0;
        for (i, &v) in coefficients.iter().enumerate() {
            if v.wrapping_abs() > maximum {
                maximum = v.wrapping_abs();
                index = i;
            }
        }
        maximum = round_shift(maximum, 11);
        if maximum <= 32767 {
            for (out, &v) in output.iter_mut().zip(coefficients.iter()) {
                *out = round_shift(v, 11) as i16;
            }
            return;
        }
        maximum = maximum.min(163838);
        let chirp = 65470 - ((maximum - 32767) << 14) / ((maximum * (index as i32 + 1)) >> 2);
        expand32(coefficients, chirp);
    }
    for (out, v) in output.iter_mut().zip(coefficients) {
        *out = saturate16(round_shift(*v, 11));
        *v = i32::from(*out) << 11;
    }
}
fn monic(coefficients: &mut [i32], warp: i32) -> Result<i32, AnalysisError> {
    for i in (1..coefficients.len()).rev() {
        coefficients[i - 1] = coefficients[i - 1].wrapping_add(mul_wb(coefficients[i], -warp));
    }
    let numerator = 65536 + mul_wb(-warp, warp);
    let denominator = (1 << 24) + mul_wb(coefficients[0], warp);
    if denominator <= 0 {
        return Err(AnalysisError::ArithmeticRange);
    }
    let gain = divide_q(numerator, denominator, 24);
    if gain <= 0 {
        return Err(AnalysisError::ArithmeticRange);
    }
    for v in coefficients {
        *v = mul_ww(gain, *v);
    }
    Ok(gain)
}
pub(super) fn limit_warped(coefficients: &mut [i32], warp: i32) -> Result<(), AnalysisError> {
    let mut gain = monic(coefficients, warp)?;
    let limit = 67092087 >> 4;
    for iteration in 0..10 {
        let mut maximum = 0u32;
        let mut index = 0;
        for (i, &v) in coefficients.iter().enumerate() {
            if v.unsigned_abs() > maximum {
                maximum = v.unsigned_abs();
                index = i;
            }
        }
        if maximum > i32::MAX as u32 {
            return Err(AnalysisError::ArithmeticRange);
        }
        let maximum = (maximum >> 4) as i32;
        if maximum <= limit {
            return Ok(());
        }
        for i in 1..coefficients.len() {
            coefficients[i - 1] = coefficients[i - 1].wrapping_add(mul_wb(coefficients[i], warp));
        }
        gain = inverse_gain(gain, 32);
        for v in coefficients.iter_mut() {
            *v = mul_ww(gain, *v);
        }
        let chirp = 64881
            - divide_q(
                mul_wb(maximum - limit, 819 + 102 * iteration),
                maximum * (index as i32 + 1),
                22,
            );
        expand32(coefficients, chirp);
        gain = monic(coefficients, warp)?;
    }
    Err(AnalysisError::ArithmeticRange)
}
