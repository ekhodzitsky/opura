//! Fixed-point CELT autocorrelation, LPC and classic concealment filters.
//! Canonical scalar fast64 arithmetic follows celt_lpc.c.
//! Copyright (c) 2009-2010 Xiph.Org Foundation. See THIRD_PARTY_NOTICES.md.
use super::{
    math::{self, MathError},
    mdct_tables::WINDOW,
};
use std::fmt;
/// Invalid LPC geometry, correlation domain or scalar arithmetic.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum LpcError {
    /// Unsupported signal/order/window geometry.
    InvalidGeometry,
    /// ACF is outside the supported positive-energy/reflection domain.
    InvalidCorrelation,
    /// Scalar division failed (e.g. unstable reflection coefficient).
    Math(MathError),
}
impl fmt::Display for LpcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidGeometry => f.write_str("invalid CELT LPC geometry"),
            Self::InvalidCorrelation => f.write_str("invalid CELT autocorrelation domain"),
            Self::Math(e) => write!(f, "CELT LPC scalar arithmetic: {e}"),
        }
    }
}
impl std::error::Error for LpcError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Math(e) => Some(e),
            _ => None,
        }
    }
}
impl From<MathError> for LpcError {
    fn from(e: MathError) -> Self {
        Self::Math(e)
    }
}
/// Normalized autocorrelation and original energy scaling exponent.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Autocorrelation {
    values: [i32; 25],
    length: usize,
    shift: i32,
}
impl Autocorrelation {
    /// Lags 0..=requested_lag; lag0 is positive and normalized to [2^28,2^29).
    pub fn values(&self) -> &[i32] {
        &self.values[..self.length]
    }
    /// Scale exponent mapping normalized correlation back toward original energy.
    pub fn shift(&self) -> i32 {
        self.shift
    }
}
/// Analyze 3..2048 i16 samples at lag0..24, with at least three samples beyond
/// the lag. Optional standard120-sample end windows require length>=240.
/// No allocation; returned scaling/rounding follows the original scalar kernel.
pub fn autocorrelation(
    input: &[i16],
    lag: usize,
    windowed: bool,
) -> Result<Autocorrelation, LpcError> {
    let n = input.len();
    if !(3..=2048).contains(&n) || lag > 24 || lag + 3 > n || windowed && n < 240 {
        return Err(LpcError::InvalidGeometry);
    }
    let mut x = [0i16; 2048];
    x[..n].copy_from_slice(input);
    if windowed {
        for i in 0..120 {
            let w = i32::from(WINDOW[i]);
            x[i] = ((i32::from(input[i]) * w) >> 15) as i16;
            x[n - i - 1] = ((i32::from(input[n - i - 1]) * w) >> 15) as i16;
        }
    }
    let estimate_shift = ilog((n + n / 16) as i32);
    let mut estimate = 1 + (n as i32 * 128);
    for &v in &x[..n] {
        estimate += (i32::from(v) * i32::from(v)) >> estimate_shift;
    }
    estimate += estimate >> 7;
    let mut shift = ((ilog(estimate) - 30 + estimate_shift + 1) / 2).max(0);
    if shift > 0 {
        for v in &mut x[..n] {
            *v = ((i32::from(*v) + (1 << (shift - 1))) >> shift) as i16;
        }
    }
    let mut result = Autocorrelation {
        values: [0; 25],
        length: lag + 1,
        shift: 0,
    };
    for k in 0..=lag {
        let mut sum = 0i64;
        for i in k..n {
            sum += i64::from(x[i]) * i64::from(x[i - k]);
        }
        result.values[k] = sum as i32;
    }
    shift *= 2;
    if shift == 0 {
        result.values[0] += 1;
    }
    let energy = result.values[0];
    if energy < 268435456 {
        let extra = 28 - ilog(energy);
        for v in &mut result.values[..=lag] {
            *v = v.wrapping_shl(extra as u32);
        }
        shift -= extra;
    } else if energy >= 536870912 {
        let extra = if energy >= 1073741824 { 2 } else { 1 };
        for v in &mut result.values[..=lag] {
            *v >>= extra;
        }
        shift += extra;
    }
    result.shift = shift;
    Ok(result)
}
/// Levinson-Durbin with fast64 accumulation and Q25→Q12 bandwidth fitting.
/// Coefficient count is1..24, correlation has one extra lag, lag0 is0..2^30
/// and other lags' absolute values cannot exceed lag0. Unstable intermediate
/// reflections reject atomically. All-zero ACF clears coefficients. In the
/// reference ten-fit fallback only coefficient0 is replaced by4096; the other
/// output coordinates retain their incoming values, matching upstream exactly.
pub fn analyse(correlation: &[i32], coefficients: &mut [i16]) -> Result<(), LpcError> {
    let p = coefficients.len();
    if !(1..=24).contains(&p) || correlation.len() != p + 1 {
        return Err(LpcError::InvalidGeometry);
    }
    let ac = correlation;
    if !(0..=1 << 30).contains(&ac[0])
        || ac[1..]
            .iter()
            .any(|&v| i64::from(v).abs() > i64::from(ac[0]))
    {
        return Err(LpcError::InvalidCorrelation);
    }
    let mut lpc = [0i32; 24];
    let mut error = ac[0];
    if ac[0] != 0 {
        for i in 0..p {
            let mut acc = 0i64;
            for j in 0..i {
                acc = acc.wrapping_add(i64::from(lpc[j]) * i64::from(ac[i - j]));
            }
            let rr = ((acc >> 31) as i32).wrapping_add(ac[i + 1] >> 6);
            let r = -math::fractional_divide(rr.wrapping_shl(6), error)?;
            lpc[i] = r >> 6;
            for j in 0..(i + 1) >> 1 {
                let a = lpc[j];
                let b = lpc[i - 1 - j];
                lpc[j] = a.wrapping_add(mul31(r, b));
                lpc[i - 1 - j] = b.wrapping_add(mul31(r, a));
            }
            error = error.wrapping_sub(mul31(mul31(r, r), error));
            if error <= ac[0] >> 10 {
                break;
            }
        }
    }
    let mut fitted = false;
    for _ in 0..10 {
        let mut maxabs = 0i32;
        let mut index = 0;
        for (i, &value) in lpc[..p].iter().enumerate() {
            let abs = value.checked_abs().ok_or(LpcError::InvalidCorrelation)?;
            if abs > maxabs {
                maxabs = abs;
                index = i;
            }
        }
        let rounded = maxabs
            .checked_add(4096)
            .ok_or(LpcError::InvalidCorrelation)?
            >> 13;
        if rounded <= 32767 {
            fitted = true;
            break;
        }
        let max = rounded.min(163838);
        let mut chirp = 65470 - ((max - 32767) << 14) / ((max * (index as i32 + 1)) >> 2);
        let minus = chirp - 65536;
        for (i, value) in lpc[..p].iter_mut().enumerate() {
            *value = ((i64::from(chirp) * i64::from(*value)) >> 16) as i32;
            if i + 1 < p {
                chirp = chirp.wrapping_add(chirp.wrapping_mul(minus).wrapping_add(32768) >> 16);
            }
        }
    }
    if !fitted {
        coefficients[0] = 4096;
    } else {
        for (out, &value) in coefficients.iter_mut().zip(&lpc) {
            *out = (value.wrapping_add(4096) >> 13) as i16;
        }
    }
    Ok(())
}
/// Q12 FIR over i16 input with explicit leading history. Input length equals
/// output length plus coefficient count3..24; output length is1..1024.
/// Sums wrap signed32, then round/saturate to +/-32767, as the classic kernel.
/// Geometry errors preserve output.
pub fn fir(input: &[i16], coefficients: &[i16], output: &mut [i16]) -> Result<(), LpcError> {
    let p = coefficients.len();
    let n = output.len();
    if !(3..=24).contains(&p) || !(1..=1024).contains(&n) || input.len() != n + p {
        return Err(LpcError::InvalidGeometry);
    }
    for (i, out) in output.iter_mut().enumerate() {
        let mut sum = i32::from(input[p + i]) << 12;
        for j in 0..p {
            sum = sum.wrapping_add(i32::from(coefficients[j]) * i32::from(input[p + i - j - 1]));
        }
        *out = round_signal(sum);
    }
    Ok(())
}
/// Canonical four-sample IIR, in-place on Q12 i32 input/output. Order is4/8/12/
/// 16/20/24; length is order..1080 and divisible by4, matching classic PLC use.
/// Memory supplies newest-first i16 samples. The canonical kernel returns the
/// low16 bits of the last raw Q12 outputs in memory (not rounded samples).
/// Geometry errors preserve both signal and memory; all sums explicitly wrap.
pub fn iir(values: &mut [i32], coefficients: &[i16], memory: &mut [i16]) -> Result<(), LpcError> {
    let p = coefficients.len();
    let n = values.len();
    if !matches!(p, 4 | 8 | 12 | 16 | 20 | 24)
        || memory.len() != p
        || n < p
        || n > 1080
        || n % 4 != 0
    {
        return Err(LpcError::InvalidGeometry);
    }
    let mut history = [0i16; 24];
    for (h, &v) in history.iter_mut().zip(memory.iter()) {
        *h = v.wrapping_neg();
    }
    for value in values.iter_mut() {
        let mut sum = *value;
        for j in 0..p {
            sum = sum.wrapping_add(i32::from(coefficients[j]) * i32::from(history[j]));
        }
        history.copy_within(0..p - 1, 1);
        history[0] = round_signal(sum).wrapping_neg();
        *value = sum;
    }
    for (i, v) in memory.iter_mut().enumerate() {
        *v = values[n - 1 - i] as i16;
    }
    Ok(())
}
fn round_signal(x: i32) -> i16 {
    (x.wrapping_add(2048) >> 12).clamp(-32767, 32767) as i16
}
fn mul31(a: i32, b: i32) -> i32 {
    ((i64::from(a) * i64::from(b)) >> 31) as i32
}
fn ilog(x: i32) -> i32 {
    31 - x.leading_zeros() as i32
}
