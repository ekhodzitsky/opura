//! Three-stage pitch search with fixed storage and cached fine-lag products.
use super::PitchFrame;
use crate::silk::{
    encoder::math::{divide_q, energy, lin2log},
    gains::SignalType,
    pitch::tables::{LAGS10, LAGS10_NB, LAGS20, LAGS20_NB, SEARCH10, SEARCH20},
    synthesis::{mul_wb, round_shift, saturate16},
};
fn dot(a: &[i16], b: &[i16]) -> i32 {
    a.iter().zip(b).fold(0i32, |sum, (&a, &b)| {
        sum.wrapping_add(i32::from(a) * i32::from(b))
    })
}
fn square(v: i16) -> i32 {
    i32::from(v) * i32::from(v)
}
fn down2(output: &mut [i16], input: &[i16]) {
    let mut s = [0i32; 2];
    for (out, pair) in output.iter_mut().zip(input.chunks_exact(2)) {
        let a = i32::from(pair[0]) << 10;
        let y = a.wrapping_sub(s[0]);
        let x = y.wrapping_add(mul_wb(y, -25727));
        let first = s[0].wrapping_add(x);
        s[0] = a.wrapping_add(x);
        let a = i32::from(pair[1]) << 10;
        let y = a.wrapping_sub(s[1]);
        let x = mul_wb(y, 9872);
        let sum = first.wrapping_add(s[1]).wrapping_add(x);
        s[1] = a.wrapping_add(x);
        *out = saturate16(round_shift(sum, 11));
    }
}
fn down_two_thirds(output: &mut [i16], input: &[i16]) {
    // The longest 12 kHz pitch context is 480 samples: one native AR2 batch.
    let mut buffer = [0i32; 484];
    let mut s = [0i32; 2];
    for (i, &sample) in input.iter().enumerate() {
        let value = s[0].wrapping_add(i32::from(sample) << 8);
        buffer[i + 4] = value;
        let scaled = value.wrapping_shl(2);
        s[0] = s[1].wrapping_add(mul_wb(scaled, -2797));
        s[1] = mul_wb(scaled, -6507);
    }
    for (i, pair) in output.chunks_exact_mut(2).enumerate() {
        let b = &buffer[3 * i..];
        let first = mul_wb(b[0], 4697)
            .wrapping_add(mul_wb(b[1], 10739))
            .wrapping_add(mul_wb(b[2], 8276))
            .wrapping_add(mul_wb(b[3], 1567));
        let second = mul_wb(b[1], 1567)
            .wrapping_add(mul_wb(b[2], 8276))
            .wrapping_add(mul_wb(b[3], 10739))
            .wrapping_add(mul_wb(b[4], 4697));
        pair[0] = saturate16(round_shift(first, 6));
        pair[1] = saturate16(round_shift(second, 6));
    }
}
pub(super) fn analyze(
    result: &mut PitchFrame,
    khz: usize,
    complexity: usize,
    threshold1: i32,
    threshold2: i32,
    previous_lag: u16,
    previous_correlation: i32,
) {
    let count = result.count;
    let length = (20 + 5 * count) * khz;
    let input = &result.residual[..length];
    let (power, scale) = energy(input);
    let shift = scale as i32 + 3 - power.leading_zeros() as i32;
    let shift = if shift > 0 { (shift + 1) / 2 } else { 0 };
    let mut frame = [0i16; 640];
    for (out, &x) in frame[..length].iter_mut().zip(input) {
        *out = x >> shift;
    }
    let length8 = (20 + 5 * count) * 8;
    let length4 = length8 / 2;
    let mut frame8 = [0i16; 320];
    match khz {
        16 => down2(&mut frame8[..length8], &frame[..length]),
        12 => down_two_thirds(&mut frame8[..length8], &frame[..length]),
        _ => frame8[..length8].copy_from_slice(&frame[..length]),
    }
    let mut frame4 = [0i16; 160];
    down2(&mut frame4[..length4], &frame8[..length8]);
    for i in (1..length4).rev() {
        frame4[i] = saturate16(i32::from(frame4[i]) + i32::from(frame4[i - 1]));
    }
    let mut coarse = [[0i16; 65]; 2];
    for (k, row) in coarse.iter_mut().enumerate().take(count / 2) {
        let target = 80 + k * 40;
        let t = &frame4[target..target + 40];
        let mut normalizer = dot(t, t)
            + dot(
                &frame4[target - 8..target - 8 + 40],
                &frame4[target - 8..target - 8 + 40],
            )
            + 160000;
        for lag in 8..=72 {
            let basis = target - lag;
            if lag > 8 {
                normalizer += square(frame4[basis]) - square(frame4[basis + 40]);
            }
            row[lag - 8] = divide_q(dot(t, &frame4[basis..basis + 40]), normalizer, 14) as i16;
        }
    }
    let mut scores = [(i16::MIN, 0usize); 8];
    let top = 4 + 2 * complexity;
    for lag in 8..=72 {
        let sum = if count == 4 {
            i32::from(coarse[0][lag - 8]) + i32::from(coarse[1][lag - 8])
        } else {
            2 * i32::from(coarse[0][lag - 8])
        };
        let score = (sum + mul_wb(sum, -(lag as i32) << 4)) as i16;
        // Strict insertion retains native ordering when correlation ties.
        for j in 0..top {
            if score > scores[j].0 {
                for i in (j + 1..top).rev() {
                    scores[i] = scores[i - 1];
                }
                scores[j] = (score, lag * 2);
                break;
            }
        }
    }
    if scores[0].0 < 3277 {
        return;
    }
    let threshold = mul_wb(threshold1, i32::from(scores[0].0));
    let mut mask = [0i16; 134];
    for &(score, lag) in &scores[..top] {
        if i32::from(score) > threshold {
            mask[lag - 13] = 1;
        }
    }
    for i in (16..147).rev() {
        mask[i - 13] += mask[i - 14] + mask[i - 15];
    }
    let mut candidates = [0usize; 24];
    let mut candidate_count = 0;
    for i in 16..=143 {
        if mask[i + 1 - 13] > 0 {
            candidates[candidate_count] = i;
            candidate_count += 1;
        }
    }
    for i in (16..147).rev() {
        mask[i - 13] += mask[i - 14] + mask[i - 15] + mask[i - 16];
    }
    let mut c = [[0i16; 132]; 4];
    for (k, row) in c.iter_mut().enumerate().take(count) {
        let target = 160 + k * 40;
        let t = &frame8[target..target + 40];
        let target_energy = dot(t, t) + 1;
        for i in 16..147 {
            if mask[i - 13] == 0 {
                continue;
            }
            let lag = i - 2;
            let b = &frame8[target - lag..target - lag + 40];
            let cross = dot(t, b);
            if cross > 0 {
                row[lag - 14] = divide_q(cross, target_energy + dot(b, b), 14) as i16;
            }
        }
    }
    let (cb, stride, search): (&[i8], usize, usize) = if count == 4 {
        (
            &LAGS20_NB,
            11,
            if khz == 8 && complexity > 0 { 11 } else { 3 },
        )
    } else {
        (&LAGS10_NB, 3, 3)
    };
    let previous_lag = match khz {
        12 => 2 * usize::from(previous_lag) / 3,
        16 => usize::from(previous_lag) / 2,
        _ => usize::from(previous_lag),
    };
    let previous_log = if previous_lag > 0 {
        lin2log(previous_lag as i32)
    } else {
        0
    };
    let mut best_biased = i32::MIN;
    let mut best = i32::MIN;
    let mut lag = 0;
    let mut contour = 0;
    for &d in &candidates[..candidate_count] {
        let mut local = i32::MIN;
        let mut local_contour = 0;
        for j in 0..search {
            let mut sum = 0;
            for k in 0..count {
                let sublag = (d as i32 + i32::from(cb[k * stride + j]) - 14) as usize;
                sum += i32::from(c[k][sublag]);
            }
            if sum > local {
                local = sum;
                local_contour = j;
            }
        }
        let log = lin2log(d as i32);
        let mut biased = local - ((count as i32 * 1638 * log) >> 7);
        if previous_lag > 0 {
            let delta = log - previous_log;
            let square = (delta * delta) >> 7;
            let bias = (count as i32 * 1638 * previous_correlation) >> 15;
            biased -= bias * square / (square + 64);
        }
        if biased > best_biased
            && local > count as i32 * threshold2
            && LAGS20_NB[local_contour] <= 16
        {
            best_biased = biased;
            best = local;
            lag = d;
            contour = local_contour;
        }
    }
    if lag == 0 {
        return;
    }
    result.correlation = (best / count as i32) << 2;
    if khz == 8 {
        for k in 0..count {
            result.lags[k] =
                (lag as i32 + i32::from(cb[k * stride + contour])).clamp(16, 144) as u16;
        }
        result.index = (lag - 16) as u16;
        result.contour = contour as u8;
    } else {
        fine(result, &frame[..length], khz, complexity, lag);
    }
    result.signal = SignalType::Voiced;
}
fn fine(result: &mut PitchFrame, frame: &[i16], khz: usize, complexity: usize, lag8: usize) {
    let count = result.count;
    let sf = 5 * khz;
    let minimum = 2 * khz;
    let maximum = 18 * khz - 1;
    let lag = (if khz == 12 { lag8 * 3 / 2 } else { lag8 * 2 }).clamp(minimum, maximum);
    let start = lag.saturating_sub(2).max(minimum);
    let end = (lag + 2).min(maximum);
    let (cb, stride, search, ranges): (&[i8], usize, usize, &[i8]) = if count == 4 {
        (
            &LAGS20,
            34,
            [16, 24, 34][complexity],
            &SEARCH20[complexity * 8..complexity * 8 + 8],
        )
    } else {
        (&LAGS10, 12, 12, &SEARCH10)
    };
    let mut cross = [[[0i32; 5]; 34]; 4];
    let mut energies = [[[0i32; 5]; 34]; 4];
    for k in 0..count {
        let target = 20 * khz + k * sf;
        let t = &frame[target..target + sf];
        let low = i32::from(ranges[k * 2]);
        let high = i32::from(ranges[k * 2 + 1]);
        let base = (target as i32 - start as i32 - low) as usize;
        let mut correlations = [0i32; 22];
        let mut powers = [0i32; 22];
        let mut power = dot(&frame[base..base + sf], &frame[base..base + sf]);
        for i in 0..=(high - low) as usize {
            let b = base - i;
            correlations[i] = dot(t, &frame[b..b + sf]);
            if i > 0 {
                power -= square(frame[base + sf - i]);
                power = power.saturating_add(square(frame[base - i]));
            }
            powers[i] = power;
        }
        for j in 0..search {
            let index = (i32::from(cb[k * stride + j]) - low) as usize;
            cross[k][j].copy_from_slice(&correlations[index..index + 5]);
            energies[k][j].copy_from_slice(&powers[index..index + 5]);
        }
    }
    let t = &frame[20 * khz..20 * khz + count * sf];
    let target_energy = dot(t, t) + 1;
    let contour_bias = 1638 / lag as i32;
    let mut best = i32::MIN;
    let mut chosen = lag;
    let mut contour = 0;
    for d in start..=end {
        for (j, &first_offset) in LAGS20.iter().enumerate().take(search) {
            let mut corr = 0i32;
            let mut energy = target_energy;
            for k in 0..count {
                corr = corr.wrapping_add(cross[k][j][d - start]);
                energy = energy.wrapping_add(energies[k][j][d - start]);
            }
            let score = if corr > 0 {
                mul_wb(divide_q(corr, energy, 14), 32767 - contour_bias * j as i32)
            } else {
                0
            };
            // Native upper-bound checks deliberately use the 20 ms first row
            // even for 10 ms frames; actual output uses the selected codebook.
            if score > best && d as i32 + i32::from(first_offset) <= maximum as i32 {
                best = score;
                chosen = d;
                contour = j;
            }
        }
    }
    for k in 0..count {
        result.lags[k] = (chosen as i32 + i32::from(cb[k * stride + contour]))
            .clamp(minimum as i32, (18 * khz) as i32) as u16;
    }
    result.index = (chosen - minimum) as u16;
    result.contour = contour as u8;
}
