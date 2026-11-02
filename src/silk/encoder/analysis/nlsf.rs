//! Original weighted predictive VQ and four-state residual NLSF trellis.
use super::AnalysisError;
use crate::silk::{
    encoder::math::{divide_q, lin2log},
    gains::SignalType,
    lpc,
    nlsf::{NlsfOrder, stabilize},
};
/// Quantized normalized spectrum and the complete codebook path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QuantizedNlsf {
    order: NlsfOrder,
    indices: [i8; 17],
    spectrum: [i16; 16],
    cost: i32,
}
impl QuantizedNlsf {
    /// First-stage index followed by signed residual indices.
    pub fn indices(&self) -> &[i8] {
        &self.indices[..=self.order.len()]
    }
    /// Stabilized quantized Q15 frequencies.
    pub fn spectrum_q15(&self) -> &[i16] {
        &self.spectrum[..self.order.len()]
    }
    /// Native rate/distortion objective in Q25, not a perceptual quality score.
    pub fn cost_q25(&self) -> i32 {
        self.cost
    }
    /// Convert quantized frequencies to stable Q12 prediction coefficients.
    pub fn predictor_q12(&self) -> [i16; 16] {
        lpc::convert(self.spectrum_q15())
    }
}
/// Compute positive Q2 Laroia spectral weights from an order-10/16 spectrum.
/// Nonpositive spacings use the original one-unit floor. Input is unchanged.
pub fn laroia_weights(order: NlsfOrder, spectrum: &[i16]) -> Result<[i16; 16], AnalysisError> {
    let count = order.len();
    if spectrum.len() != count {
        return Err(AnalysisError::InvalidLength);
    }
    if spectrum.iter().any(|&v| v < 0) {
        return Err(AnalysisError::InvalidParameters);
    }
    let mut weights = [0; 16];
    for i in 0..count {
        let before = if i == 0 {
            0
        } else {
            i32::from(spectrum[i - 1])
        };
        let after = if i + 1 == count {
            32768
        } else {
            i32::from(spectrum[i + 1])
        };
        let current = i32::from(spectrum[i]);
        weights[i] = ((1 << 17) / (current - before).max(1) + (1 << 17) / (after - current).max(1))
            .min(32767) as i16;
    }
    Ok(weights)
}
/// Stabilize and quantize a spectrum using native weighted predictive search.
/// `weights` are positive Q2 values, `mu_q20` is 0..=32767, and 1..=32 first-stage
/// survivors are retained. All buffers are fixed; errors do not modify input.
pub fn quantize_nlsf(
    order: NlsfOrder,
    spectrum: &[i16],
    weights: &[i16],
    mu_q20: u16,
    survivors: usize,
    signal: SignalType,
) -> Result<QuantizedNlsf, AnalysisError> {
    let count = order.len();
    if spectrum.len() != count || weights.len() != count {
        return Err(AnalysisError::InvalidLength);
    }
    if !(1..=32).contains(&survivors) || mu_q20 > 32767 || weights.iter().any(|&v| v <= 0) {
        return Err(AnalysisError::InvalidParameters);
    }
    let cb = order.codebook();
    let mut input = [0i16; 16];
    input[..count].copy_from_slice(spectrum);
    stabilize(&mut input[..count], cb.delta);
    let mut errors = [0i32; 32];
    let mut sorted = [0usize; 32];
    for k in 0..32 {
        sorted[k] = k;
        let mut previous = 0i32;
        let mut sum = 0i32;
        for j in (0..count).rev() {
            let difference = i32::from(input[j]) - (i32::from(cb.cb1[k * count + j]) << 7);
            let weighted = difference * i32::from(cb.weights[k * count + j]);
            sum = sum.wrapping_add(weighted.wrapping_sub(previous >> 1).wrapping_abs());
            previous = weighted;
        }
        errors[k] = sum;
    }
    // Stable insertion order preserves the original first-stage tie behavior.
    for k in 1..32 {
        let key = sorted[k];
        let mut j = k;
        while j > 0 && errors[sorted[j - 1]] > errors[key] {
            sorted[j] = sorted[j - 1];
            j -= 1;
        }
        sorted[j] = key;
    }
    let mut costs = [0i32; 32];
    let mut paths = [[0i8; 16]; 32];
    for s in 0..survivors {
        let first = sorted[s];
        let mut residual = [0i16; 16];
        let mut adjusted = [0i16; 16];
        for j in 0..count {
            let weight = i32::from(cb.weights[first * count + j]);
            let difference = i32::from(input[j]) - (i32::from(cb.cb1[first * count + j]) << 7);
            residual[j] = ((difference * weight) >> 14) as i16;
            adjusted[j] = divide_q(i32::from(weights[j]), weight * weight, 21) as i16;
        }
        let (entropy, prediction) = cb.unpack(first, count);
        costs[s] = trellis(
            &mut paths[s],
            &residual[..count],
            &adjusted,
            &prediction,
            &entropy,
            cb.rates,
            cb.step,
            cb.inverse_step,
            i32::from(mu_q20),
        );
        if costs[s] < 0 || paths[s][..count].iter().any(|&v| !(-10..=10).contains(&v)) {
            return Err(AnalysisError::ArithmeticRange);
        }
        let context = if signal == SignalType::Voiced { 32 } else { 0 };
        let probability = if first == 0 {
            256 - i32::from(cb.icdf1[context])
        } else {
            i32::from(cb.icdf1[context + first - 1]) - i32::from(cb.icdf1[context + first])
        };
        let bits = 1024 - lin2log(probability);
        costs[s] = costs[s].wrapping_add(bits * i32::from(mu_q20 >> 2));
    }
    let mut best = 0;
    for s in 1..survivors {
        if costs[s] < costs[best] {
            best = s;
        }
    }
    if costs[best] < 0 {
        return Err(AnalysisError::ArithmeticRange);
    }
    let first = sorted[best];
    let mut indices = [0i8; 17];
    indices[0] = first as i8;
    indices[1..=count].copy_from_slice(&paths[best][..count]);
    let (_, prediction) = cb.unpack(first, count);
    let result = cb.reconstruct(&indices[..=count], &prediction);
    Ok(QuantizedNlsf {
        order,
        indices,
        spectrum: result,
        cost: costs[best],
    })
}
#[allow(clippy::too_many_arguments)]
fn trellis(
    indices: &mut [i8; 16],
    input: &[i16],
    weights: &[i16; 16],
    prediction: &[u8; 16],
    entropy: &[usize; 16],
    rates: &[u8],
    step: i32,
    inverse: i16,
    mu: i32,
) -> i32 {
    let mut out0 = [0i16; 20];
    let mut out1 = [0i16; 20];
    for i in -10i32..10 {
        for (index, table) in [(i, &mut out0), (i + 1, &mut out1)] {
            let value = index * 1024 - index.signum() * 102;
            table[(i + 10) as usize] = ((value * step) >> 16) as i16;
        }
    }
    let mut states = 1;
    let mut cost = [0i32; 8];
    let mut previous = [0i16; 8];
    let mut path = [[0i8; 16]; 4];
    for i in (0..input.len()).rev() {
        let table = &rates[entropy[i]..entropy[i] + 9];
        for j in 0..states {
            let predicted = (i32::from(prediction[i]) * i32::from(previous[j])) >> 8;
            let residual = input[i].wrapping_sub(predicted as i16);
            let index = ((i32::from(inverse) * i32::from(residual)) >> 16).clamp(-10, 9);
            path[j][i] = index as i8;
            let first = out0[(index + 10) as usize].wrapping_add(predicted as i16);
            let second = out1[(index + 10) as usize].wrapping_add(predicted as i16);
            previous[j] = first;
            previous[j + states] = second;
            let (rate0, rate1) = if index >= 3 {
                if index == 3 {
                    (i32::from(table[7]), 280)
                } else {
                    let rate = 108 + 43 * index;
                    (rate, rate + 43)
                }
            } else if index <= -4 {
                if index == -4 {
                    (280, i32::from(table[1]))
                } else {
                    let rate = 108 - 43 * index;
                    (rate, rate - 43)
                }
            } else {
                (
                    i32::from(table[(index + 4) as usize]),
                    i32::from(table[(index + 5) as usize]),
                )
            };
            let old = cost[j];
            let difference0 = i32::from(input[i].wrapping_sub(first));
            let difference1 = i32::from(input[i].wrapping_sub(second));
            cost[j] = old
                .wrapping_add((difference0 * difference0).wrapping_mul(i32::from(weights[i])))
                .wrapping_add(mu * rate0);
            cost[j + states] = old
                .wrapping_add((difference1 * difference1).wrapping_mul(i32::from(weights[i])))
                .wrapping_add(mu * rate1);
        }
        if states <= 2 {
            for j in 0..states {
                path[j + states][i] = path[j][i] + 1;
            }
            states *= 2;
            for j in states..4 {
                path[j][i] = path[j - states][i];
            }
        } else {
            let mut order = [0usize; 4];
            let mut minimum = [0i32; 4];
            let mut maximum = [0i32; 4];
            for j in 0..4 {
                if cost[j] > cost[j + 4] {
                    cost.swap(j, j + 4);
                    previous.swap(j, j + 4);
                    order[j] = j + 4;
                } else {
                    order[j] = j;
                }
                minimum[j] = cost[j];
                maximum[j] = cost[j + 4];
            }
            // Each replacement consumes a distinct loser: at most four steps.
            for _ in 0..4 {
                let mut min_max = i32::MAX;
                let mut max_min = 0;
                let mut losing = 0;
                let mut winning = 0;
                for j in 0..4 {
                    if min_max > maximum[j] {
                        min_max = maximum[j];
                        losing = j;
                    }
                    if max_min < minimum[j] {
                        max_min = minimum[j];
                        winning = j;
                    }
                }
                if min_max >= max_min {
                    break;
                }
                order[winning] = order[losing] ^ 4;
                cost[winning] = cost[losing + 4];
                previous[winning] = previous[losing + 4];
                minimum[winning] = 0;
                maximum[losing] = i32::MAX;
                path[winning] = path[losing];
            }
            for j in 0..4 {
                path[j][i] += (order[j] >> 2) as i8;
            }
        }
    }
    let mut best = 0;
    for j in 1..8 {
        if cost[j] < cost[best] {
            best = j;
        }
    }
    indices[..input.len()].copy_from_slice(&path[best & 3][..input.len()]);
    indices[0] += (best >> 2) as i8;
    cost[best]
}
