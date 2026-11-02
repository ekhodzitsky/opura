//! Native modified Burg predictor analysis and polynomial LPC-to-NLSF roots.
use super::AnalysisError;
use crate::silk::{
    cng::sqrt_approx,
    encoder::math::divide_q,
    gains::FrameDuration,
    nlsf::NlsfOrder,
    pitch::SilkRate,
    synthesis::{mul_wb, mul_ww, round_shift},
};

/// LPC analysis coefficients and normalized residual energy.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LpcAnalysis {
    order: NlsfOrder,
    coefficients: [i32; 16],
    energy: i32,
    energy_q: i32,
}
impl LpcAnalysis {
    /// Whitening coefficients in Q16, in native Burg sign convention.
    pub fn coefficients_q16(&self) -> &[i32] {
        &self.coefficients[..self.order.len()]
    }
    /// Residual energy and its binary Q exponent: physical energy is value/2^Q.
    pub fn residual_energy(&self) -> (i32, i32) {
        (self.energy, self.energy_q)
    }
    /// Convert a copy to Q15 spectral frequencies with bounded root recovery.
    pub fn frequencies_q15(&self) -> Result<[i16; 16], AnalysisError> {
        lpc_to_nlsf(self.order, self.coefficients_q16())
    }
}
/// Analyze gain-normalized/LTP-filtered PCM in stacked 5 ms subframe blocks.
/// Each block includes its own preceding LPC-order samples (10 at 8/12 kHz,
/// 16 at 16 kHz). `minimum_inverse_gain_q30` is 1..=2^30. A complete frame
/// therefore contains two/four blocks of `5*rate_khz + order` samples.
/// Fixed-point headroom errors are explicit; caller input is unchanged.
pub fn analyze_lpc(
    rate: SilkRate,
    duration: FrameDuration,
    input: &[i16],
    minimum_inverse_gain_q30: u32,
) -> Result<LpcAnalysis, AnalysisError> {
    let khz = rate.khz() as usize;
    let order = if khz == 16 {
        NlsfOrder::Order16
    } else {
        NlsfOrder::Order10
    };
    let count = if duration == FrameDuration::Ms10 {
        2
    } else {
        4
    };
    let length = khz * 5 + order.len();
    if input.len() != count * length {
        return Err(AnalysisError::InvalidLength);
    }
    if !(1..=1 << 30).contains(&minimum_inverse_gain_q30) {
        return Err(AnalysisError::InvalidParameters);
    }
    burg(order, input, length, count, minimum_inverse_gain_q30 as i32)
}
fn high(a: i32, b: i32) -> i32 {
    ((i64::from(a) * i64::from(b)) >> 32) as i32
}
fn dot(a: &[i16], b: &[i16]) -> i64 {
    a.iter()
        .zip(b)
        .map(|(&x, &y)| i64::from(x) * i64::from(y))
        .sum()
}
fn burg(
    order: NlsfOrder,
    input: &[i16],
    length: usize,
    subframes: usize,
    minimum: i32,
) -> Result<LpcAnalysis, AnalysisError> {
    let d = order.len();
    let sum = dot(input, input);
    let shifts = (36 - (sum as u64).leading_zeros() as i32).clamp(-16, 7);
    let initial = if shifts > 0 {
        sum >> shifts
    } else {
        sum << -shifts
    };
    if initial > i64::from(i32::MAX) {
        return Err(AnalysisError::ArithmeticRange);
    }
    let mut c0 = initial as i32;
    let mut first = [0i32; 16];
    let mut af = [0i32; 16];
    let mut forward = [0i32; 17];
    let mut backward = [0i32; 17];
    forward[0] = c0.wrapping_add(high(42950, c0)).wrapping_add(1);
    backward[0] = forward[0];
    for s in 0..subframes {
        let x = &input[s * length..(s + 1) * length];
        for n in 1..=d {
            let value = dot(&x[..length - n], &x[n..]);
            let value = if shifts > 0 {
                (value >> shifts) as i32
            } else {
                (value as i32).wrapping_shl((-shifts) as u32)
            };
            first[n - 1] = first[n - 1].wrapping_add(value);
        }
    }
    let mut last = first;
    let mut inverse = 1 << 30;
    let mut limited = false;
    for n in 0..d {
        for s in 0..subframes {
            let x = &input[s * length..(s + 1) * length];
            if shifts > -2 {
                let x1 = i32::from(x[n])
                    .wrapping_shl((16 - shifts) as u32)
                    .wrapping_neg();
                let x2 = i32::from(x[length - n - 1])
                    .wrapping_shl((16 - shifts) as u32)
                    .wrapping_neg();
                let mut a = i32::from(x[n]) << 9;
                let mut b = i32::from(x[length - n - 1]) << 9;
                for k in 0..n {
                    first[k] = first[k].wrapping_add(mul_wb(x1, i32::from(x[n - k - 1])));
                    last[k] = last[k].wrapping_add(mul_wb(x2, i32::from(x[length - n + k])));
                    a = a.wrapping_add(mul_wb(af[k], i32::from(x[n - k - 1])));
                    b = b.wrapping_add(mul_wb(af[k], i32::from(x[length - n + k])));
                }
                a = a.wrapping_neg().wrapping_shl((7 - shifts) as u32);
                b = b.wrapping_neg().wrapping_shl((7 - shifts) as u32);
                for k in 0..=n {
                    forward[k] = forward[k].wrapping_add(mul_wb(a, i32::from(x[n - k])));
                    backward[k] =
                        backward[k].wrapping_add(mul_wb(b, i32::from(x[length - n + k - 1])));
                }
            } else {
                let x1 = i32::from(x[n])
                    .wrapping_shl((-shifts) as u32)
                    .wrapping_neg();
                let x2 = i32::from(x[length - n - 1])
                    .wrapping_shl((-shifts) as u32)
                    .wrapping_neg();
                let mut a = i32::from(x[n]) << 17;
                let mut b = i32::from(x[length - n - 1]) << 17;
                for k in 0..n {
                    first[k] = first[k].wrapping_add(x1.wrapping_mul(i32::from(x[n - k - 1])));
                    last[k] = last[k].wrapping_add(x2.wrapping_mul(i32::from(x[length - n + k])));
                    let coefficient = round_shift(af[k], 8);
                    a = a.wrapping_add(i32::from(x[n - k - 1]).wrapping_mul(coefficient));
                    b = b.wrapping_add(i32::from(x[length - n + k]).wrapping_mul(coefficient));
                }
                a = a.wrapping_neg();
                b = b.wrapping_neg();
                for k in 0..=n {
                    forward[k] = forward[k].wrapping_add(mul_ww(
                        a,
                        i32::from(x[n - k]).wrapping_shl((-shifts - 1) as u32),
                    ));
                    backward[k] = backward[k].wrapping_add(mul_ww(
                        b,
                        i32::from(x[length - n + k - 1]).wrapping_shl((-shifts - 1) as u32),
                    ));
                }
            }
        }
        let mut a = first[n];
        let mut b = last[n];
        let mut numerator = 0i32;
        let mut energy = backward[0].wrapping_add(forward[0]);
        for k in 0..n {
            let head = af[k]
                .unsigned_abs()
                .leading_zeros()
                .saturating_sub(1)
                .min(7);
            let coefficient = af[k].wrapping_shl(head);
            let shift = 7 - head;
            a = a.wrapping_add(high(last[n - k - 1], coefficient).wrapping_shl(shift));
            b = b.wrapping_add(high(first[n - k - 1], coefficient).wrapping_shl(shift));
            numerator =
                numerator.wrapping_add(high(backward[n - k], coefficient).wrapping_shl(shift));
            energy = energy.wrapping_add(
                high(backward[k + 1].wrapping_add(forward[k + 1]), coefficient).wrapping_shl(shift),
            );
        }
        forward[n + 1] = a;
        backward[n + 1] = b;
        numerator = numerator.wrapping_add(b).wrapping_neg().wrapping_shl(1);
        let mut reflection = if energy > 0 && numerator.unsigned_abs() < (energy as u32) {
            divide_q(numerator, energy, 31)
        } else if numerator > 0 {
            i32::MAX
        } else {
            i32::MIN
        };
        let next = high(
            inverse,
            (1i32 << 30).wrapping_sub(high(reflection, reflection)),
        )
        .wrapping_shl(2);
        if next <= minimum {
            let remaining = (1i32 << 30).wrapping_sub(divide_q(minimum, inverse, 30));
            reflection = sqrt_approx(remaining);
            if reflection > 0 {
                reflection = ((reflection + remaining / reflection) >> 1).wrapping_shl(16);
                if numerator < 0 {
                    reflection = reflection.wrapping_neg();
                }
            }
            inverse = minimum;
            limited = true;
        } else {
            inverse = next;
        }
        for k in 0..n.div_ceil(2) {
            let a = af[k];
            let b = af[n - k - 1];
            af[k] = a.wrapping_add(high(b, reflection).wrapping_shl(1));
            af[n - k - 1] = b.wrapping_add(high(a, reflection).wrapping_shl(1));
        }
        af[n] = reflection >> 6;
        if limited {
            af[n + 1..d].fill(0);
            break;
        }
        for k in 0..=n + 1 {
            let a = forward[k];
            let b = backward[n + 1 - k];
            forward[k] = a.wrapping_add(high(b, reflection).wrapping_shl(1));
            backward[n + 1 - k] = b.wrapping_add(high(a, reflection).wrapping_shl(1));
        }
    }
    let mut coefficients = [0i32; 16];
    let energy = if limited {
        for k in 0..d {
            coefficients[k] = round_shift(af[k], 9).wrapping_neg();
        }
        for s in 0..subframes {
            let value = dot(
                &input[s * length..s * length + d],
                &input[s * length..s * length + d],
            );
            c0 = c0.wrapping_sub(if shifts > 0 {
                (value >> shifts) as i32
            } else {
                (value as i32).wrapping_shl((-shifts) as u32)
            });
        }
        high(inverse, c0).wrapping_shl(2)
    } else {
        let mut energy = forward[0];
        let mut sum = 1i32 << 16;
        for k in 0..d {
            let a = round_shift(af[k], 9);
            energy = energy.wrapping_add(mul_ww(forward[k + 1], a));
            sum = sum.wrapping_add(mul_ww(a, a));
            coefficients[k] = a.wrapping_neg();
        }
        energy.wrapping_add(mul_ww(high(42950, c0), sum.wrapping_neg()))
    };
    if energy < 0 {
        return Err(AnalysisError::ArithmeticRange);
    }
    Ok(LpcAnalysis {
        order,
        coefficients,
        energy,
        energy_q: -shifts,
    })
}
/// Convert order-10/16 Q16 whitening coefficients to Q15 NLSF frequencies.
/// Up to sixteen progressively stronger bandwidth expansions recover roots;
/// the reference white-spectrum fallback is used if convergence still fails.
pub fn lpc_to_nlsf(order: NlsfOrder, coefficients: &[i32]) -> Result<[i16; 16], AnalysisError> {
    if coefficients.len() != order.len() {
        return Err(AnalysisError::InvalidLength);
    }
    let mut copy = [0i32; 16];
    copy[..coefficients.len()].copy_from_slice(coefficients);
    roots(&mut copy[..coefficients.len()])
}
fn polynomial(coefficients: &[i32], x: i32) -> i32 {
    let mut value = coefficients[coefficients.len() - 1];
    for &coefficient in coefficients[..coefficients.len() - 1].iter().rev() {
        value = coefficient.wrapping_add(mul_ww(value, x << 4));
    }
    value
}
fn initialize(a: &[i32]) -> [[i32; 9]; 2] {
    let half = a.len() / 2;
    let mut result = [[0i32; 9]; 2];
    result[0][half] = 1 << 16;
    result[1][half] = 1 << 16;
    for k in 0..half {
        result[0][k] = a[half - k - 1].wrapping_neg().wrapping_sub(a[half + k]);
        result[1][k] = a[half - k - 1].wrapping_neg().wrapping_add(a[half + k]);
    }
    for k in (1..=half).rev() {
        result[0][k - 1] = result[0][k - 1].wrapping_sub(result[0][k]);
        result[1][k - 1] = result[1][k - 1].wrapping_add(result[1][k]);
    }
    for p in &mut result {
        for k in 2..=half {
            for n in (k + 1..=half).rev() {
                p[n - 2] = p[n - 2].wrapping_sub(p[n]);
            }
            p[k - 2] = p[k - 2].wrapping_sub(p[k].wrapping_shl(1));
        }
    }
    result
}
fn roots(a: &mut [i32]) -> Result<[i16; 16], AnalysisError> {
    use crate::silk::lpc::table::COS_Q12;
    let order = a.len();
    let half = order / 2;
    let mut polynomials = initialize(a);
    let mut frequencies = [0i16; 16];
    let mut which = 0;
    let mut low = i32::from(COS_Q12[0]);
    let mut ylow = polynomial(&polynomials[0][..=half], low);
    let mut root = 0;
    if ylow < 0 {
        root = 1;
        which = 1;
        ylow = polynomial(&polynomials[1][..=half], low);
    }
    let mut k = 1;
    let mut expansions = 0;
    let mut threshold = 0;
    loop {
        let mut high = i32::from(COS_Q12[k]);
        let mut yhigh = polynomial(&polynomials[which][..=half], high);
        if (ylow <= 0 && yhigh >= threshold) || (ylow >= 0 && yhigh <= -threshold) {
            threshold = i32::from(yhigh == 0);
            let mut fraction = -256i32;
            for m in 0..3 {
                let middle = round_shift(low + high, 1);
                let y = polynomial(&polynomials[which][..=half], middle);
                if (ylow <= 0 && y >= 0) || (ylow >= 0 && y <= 0) {
                    high = middle;
                    yhigh = y;
                } else {
                    low = middle;
                    ylow = y;
                    fraction += 128 >> m;
                }
            }
            let difference = ylow.wrapping_sub(yhigh);
            if ylow.unsigned_abs() < 65536 {
                let numerator = ylow.wrapping_shl(5).wrapping_add(difference >> 1);
                if difference != 0 {
                    fraction = fraction.wrapping_add(
                        numerator
                            .checked_div(difference)
                            .ok_or(AnalysisError::ArithmeticRange)?,
                    );
                }
            } else {
                fraction = fraction.wrapping_add(
                    ylow.checked_div(difference >> 5)
                        .ok_or(AnalysisError::ArithmeticRange)?,
                );
            }
            let frequency = ((k as i32).wrapping_shl(8).wrapping_add(fraction)).min(32767);
            if frequency < 0 {
                return Err(AnalysisError::ArithmeticRange);
            }
            frequencies[root] = frequency as i16;
            root += 1;
            if root >= order {
                return Ok(frequencies);
            }
            which = root & 1;
            low = i32::from(COS_Q12[k - 1]);
            ylow = (1 - (root as i32 & 2)) << 12;
        } else {
            k += 1;
            low = high;
            ylow = yhigh;
            threshold = 0;
            if k > 128 {
                expansions += 1;
                if expansions > 16 {
                    let step = 32768 / (order as i32 + 1);
                    for (j, value) in frequencies[..order].iter_mut().enumerate() {
                        *value = ((j as i32 + 1) * step) as i16;
                    }
                    return Ok(frequencies);
                }
                let mut chirp = 65536 - (1 << expansions);
                let delta = chirp - 65536;
                for (j, coefficient) in a.iter_mut().enumerate() {
                    *coefficient = mul_ww(chirp, *coefficient);
                    if j + 1 < order {
                        chirp += round_shift(chirp * delta, 16);
                    }
                }
                polynomials = initialize(a);
                which = 0;
                low = i32::from(COS_Q12[0]);
                ylow = polynomial(&polynomials[0][..=half], low);
                root = 0;
                if ylow < 0 {
                    frequencies[0] = 0;
                    root = 1;
                    which = 1;
                    ylow = polynomial(&polynomials[1][..=half], low);
                }
                k = 1;
            }
        }
    }
}
