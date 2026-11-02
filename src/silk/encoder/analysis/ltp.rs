//! Correlation-weighted five-tap LTP codebook search and residual filtering.
use super::{AnalysisError, PitchFrame};
use crate::silk::{
    encoder::math::{energy, lin2log, log2lin},
    gains::SignalType,
    pitch::{SilkRate, tables::*},
    synthesis::{mul_wb, round_shift, saturate16},
};
/// Quantized long-term prediction for a 10/20 ms internal frame.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LtpFrame {
    pub(super) rate: SilkRate,
    pub(super) count: usize,
    lags: [u16; 4],
    coefficients: [[i16; 5]; 4],
    indices: [u8; 4],
    periodicity: u8,
    prediction_gain: i32,
}
impl LtpFrame {
    /// Five Q14 prediction coefficients per 5 ms subframe.
    pub fn coefficients_q14(&self) -> &[[i16; 5]] {
        &self.coefficients[..self.count]
    }
    /// Selected codebook vector per subframe.
    pub fn indices(&self) -> &[u8] {
        &self.indices[..self.count]
    }
    /// Shared LTP codebook selector, 0..=2.
    pub fn periodicity_index(&self) -> u8 {
        self.periodicity
    }
    /// Native estimated LTP prediction gain in dB Q7.
    pub fn prediction_gain_db_q7(&self) -> i32 {
        self.prediction_gain
    }
    /// Form gain-normalized LPC blocks from raw PCM with the same history /
    /// current / lookahead geometry as pitch analysis. Each block contains an
    /// LPC-order prefix followed by 5 ms. Inverse gains are Q16, 1..=16384,
    /// matching the quarter-scale headroom of the encoder prediction stage.
    /// Unvoiced blocks use gain scaling without long-term prediction.
    pub fn normalized_residual(
        &self,
        input: &[i16],
        inverse_gains_q16: &[i32],
    ) -> Result<LtpResidual, AnalysisError> {
        let khz = self.rate.khz() as usize;
        let sf = 5 * khz;
        let order = if khz == 16 { 16 } else { 10 };
        if input.len() != (22 + 5 * self.count) * khz || inverse_gains_q16.len() != self.count {
            return Err(AnalysisError::InvalidLength);
        }
        if inverse_gains_q16.iter().any(|v| !(1..=16384).contains(v)) {
            return Err(AnalysisError::InvalidParameters);
        }
        let mut result = LtpResidual {
            samples: [0; 384],
            length: (sf + order) * self.count,
        };
        for (k, &gain) in inverse_gains_q16.iter().enumerate() {
            let start = 20 * khz - order + k * sf;
            let lag = usize::from(self.lags[k]);
            for i in 0..sf + order {
                let at = start + i;
                let mut predicted = 0i32;
                if lag != 0 {
                    for j in 0..5 {
                        predicted = predicted.wrapping_add(
                            i32::from(input[at - lag + 2 - j]) * i32::from(self.coefficients[k][j]),
                        );
                    }
                }
                let residual = saturate16(i32::from(input[at]) - round_shift(predicted, 14));
                result.samples[k * (sf + order) + i] = mul_wb(gain, i32::from(residual)) as i16;
            }
        }
        Ok(result)
    }
}
/// Stacked normalized PCM for the stateful LPC/NLSF stage.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LtpResidual {
    samples: [i16; 384],
    length: usize,
}
impl LtpResidual {
    /// 2/4 blocks of (5 ms + LPC-order) samples, in chronological order.
    pub fn samples(&self) -> &[i16] {
        &self.samples[..self.length]
    }
}
/// Cumulative LTP gain limiter for one encoder channel.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LtpState {
    pub(super) rate: Option<SilkRate>,
    pub(super) sum_log_gain: i32,
}
impl LtpState {
    /// Clear cumulative prediction gain and rate history.
    pub fn reset(&mut self) {
        *self = Self::default();
    }
    /// Fit/quantize the LPC residual from pitch analysis. An explicit history reset
    /// clears cumulative gain before searching, as does an internal rate change.
    /// Independent packet coding alone does not require this reset.
    /// Inactive/unvoiced frames clear the limiter and return zero LTP taps.
    pub fn analyze(&mut self, pitch: &PitchFrame, reset_gain_history: bool) -> LtpFrame {
        let khz = (pitch.length / (22 + pitch.count * 5)) as i32;
        let rate = match khz {
            8 => SilkRate::Khz8,
            12 => SilkRate::Khz12,
            _ => SilkRate::Khz16,
        };
        let mut result = LtpFrame {
            rate,
            count: pitch.count,
            lags: pitch.lags,
            coefficients: [[0; 5]; 4],
            indices: [0; 4],
            periodicity: 0,
            prediction_gain: 0,
        };
        let previous = if reset_gain_history || self.rate != Some(rate) {
            0
        } else {
            self.sum_log_gain
        };
        self.rate = Some(rate);
        if pitch.signal != SignalType::Voiced {
            self.sum_log_gain = 0;
            return result;
        }
        let mut xx = [[0i32; 25]; 4];
        let mut xt = [[0i32; 5]; 4];
        correlations(
            &pitch.residual[..pitch.length],
            khz as usize,
            &pitch.lags[..pitch.count],
            &mut xx,
            &mut xt,
        );
        self.sum_log_gain = quantize(&mut result, &xx, &xt, previous);
        result
    }
}
fn product(a: i16, b: i16, shift: u32) -> i32 {
    (i32::from(a) * i32::from(b)) >> shift
}
fn correlations(
    input: &[i16],
    khz: usize,
    lags: &[u16],
    matrices: &mut [[i32; 25]; 4],
    vectors: &mut [[i32; 5]; 4],
) {
    let sf = 5 * khz;
    for (k, &lag) in lags.iter().enumerate() {
        let at = 20 * khz + k * sf;
        let base = at - usize::from(lag) - 2;
        let x = &input[base..base + sf + 4];
        let t = &input[at..at + sf + 5];
        let (mut target_energy, target_shift) = energy(t);
        let (mut total, shift) = energy(x);
        let matrix = &mut matrices[k];
        // Diagonals retain the pairwise energy rounding of sum_sqr_shift;
        // recomputing five independent dots would change the native fit.
        let mut power = total;
        for &sample in &x[..4] {
            power -= product(sample, sample, shift);
        }
        matrix[0] = power;
        for j in 1..5 {
            power -= product(x[4 + sf - j], x[4 + sf - j], shift);
            power += product(x[4 - j], x[4 - j], shift);
            matrix[j * 5 + j] = power;
        }
        for lag in 1..5 {
            let mut value = 0i32;
            for i in 0..sf {
                value = value.wrapping_add(product(x[4 + i], x[4 - lag + i], shift));
            }
            matrix[lag] = value;
            matrix[lag * 5] = value;
            for j in 1..5 - lag {
                value = value
                    .wrapping_sub(product(x[4 + sf - j], x[4 - lag + sf - j], shift))
                    .wrapping_add(product(x[4 - j], x[4 - lag - j], shift));
                matrix[j * 5 + j + lag] = value;
                matrix[(j + lag) * 5 + j] = value;
            }
        }
        let common = shift.max(target_shift);
        if common > shift {
            for v in matrix.iter_mut() {
                *v >>= common - shift;
            }
            total >>= common - shift;
        }
        target_energy >>= common - target_shift;
        for (lag, v) in vectors[k].iter_mut().enumerate() {
            for i in 0..sf {
                *v = v.wrapping_add(product(x[4 - lag + i], t[i], common));
            }
        }
        let normalizer = (1 + mul_wb(total, 1966)).max(target_energy);
        for v in matrix.iter_mut().chain(vectors[k].iter_mut()) {
            *v = ((i64::from(*v) << 17) / i64::from(normalizer)) as i32;
        }
    }
}
fn quantize(result: &mut LtpFrame, xx: &[[i32; 25]; 4], xt: &[[i32; 5]; 4], previous: i32) -> i32 {
    let mut best_cost = i32::MAX;
    let mut best_sum = 0;
    let mut final_energy = 0;
    for periodicity in 0..3 {
        let (codebook, bits, gains): (&[i8], &[u8], &[u8]) = match periodicity {
            0 => (&COEFFICIENTS0, &BITS0, &EFFECTIVE_GAIN0),
            1 => (&COEFFICIENTS1, &BITS1, &EFFECTIVE_GAIN1),
            _ => (&COEFFICIENTS2, &BITS2, &EFFECTIVE_GAIN2),
        };
        let mut sum = previous;
        let mut energy = 0i32;
        let mut cost = 0i32;
        let mut indices = [0u8; 4];
        for k in 0..result.count {
            let maximum = log2lin(5333 - sum + 896) - 51;
            let mut best = i32::MAX;
            let mut residual = i32::MAX;
            let mut gain = 0;
            for (j, vector) in codebook.chunks_exact(5).enumerate() {
                let penalty = (i32::from(gains[j]) - maximum).max(0).wrapping_shl(11);
                let mut error = 32801i32;
                for i in 0..5 {
                    let mut row = xt[k][i].wrapping_shl(7).wrapping_neg();
                    for (n, &coefficient) in vector.iter().enumerate().skip(i + 1) {
                        row =
                            row.wrapping_add(xx[k][i * 5 + n].wrapping_mul(i32::from(coefficient)));
                    }
                    row = row
                        .wrapping_shl(1)
                        .wrapping_add(xx[k][i * 5 + i].wrapping_mul(i32::from(vector[i])));
                    error = error.wrapping_add(mul_wb(row, i32::from(vector[i])));
                }
                if error >= 0 {
                    let penalized = error.wrapping_add(penalty);
                    let bits_cost = (5 * result.rate.khz()) * (lin2log(penalized) - (15 << 7))
                        + i32::from(bits[j]) * 4;
                    if bits_cost <= best {
                        best = bits_cost;
                        residual = penalized;
                        gain = i32::from(gains[j]);
                        indices[k] = j as u8;
                    }
                }
            }
            // Native ADD_POS_SAT32 saturates any sign-bit result, including
            // negative high-rate costs, rather than signed saturating_add.
            energy = positive_add(energy, residual);
            cost = positive_add(cost, best);
            sum = (sum + lin2log(51 + gain) - 896).max(0);
        }
        final_energy = energy;
        if cost <= best_cost {
            best_cost = cost;
            best_sum = sum;
            result.indices = indices;
            result.periodicity = periodicity;
        }
    }
    let codebook: &[i8] = match result.periodicity {
        0 => &COEFFICIENTS0,
        1 => &COEFFICIENTS1,
        _ => &COEFFICIENTS2,
    };
    for k in 0..result.count {
        for j in 0..5 {
            result.coefficients[k][j] =
                i16::from(codebook[usize::from(result.indices[k]) * 5 + j]) << 7;
        }
    }
    // The reference reports the final searched codebook's residual energy,
    // independently of which codebook won the rate/distortion search.
    final_energy >>= if result.count == 2 { 1 } else { 2 };
    result.prediction_gain = -3 * (lin2log(final_energy) - (15 << 7));
    best_sum
}
fn positive_add(a: i32, b: i32) -> i32 {
    let sum = a.wrapping_add(b);
    if sum < 0 { i32::MAX } else { sum }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::silk::encoder::analysis::{PitchOptions, PitchState};
    type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
    fn signal(v: i32) -> SignalType {
        match v {
            0 => SignalType::Inactive,
            1 => SignalType::Unvoiced,
            _ => SignalType::Voiced,
        }
    }
    #[test]
    fn public_native_ltp_fit_and_normalized_pcm_match() -> Result<()> {
        let mut pitch = None;
        let mut calls = 0;
        let mut selected = [0usize; 3];
        for (line_number, line) in
            include_str!("../../../../tests/silk-pitch-analysis-fixtures.txt")
                .lines()
                .enumerate()
        {
            if line.starts_with('#') {
                continue;
            }
            let fields: Vec<_> = line.split_whitespace().collect();
            let v: Vec<i32> = fields[1..]
                .iter()
                .map(|s| s.parse())
                .collect::<std::result::Result<_, _>>()?;
            if fields[0] == "pitch" {
                let rate = match v[0] {
                    8 => SilkRate::Khz8,
                    12 => SilkRate::Khz12,
                    _ => SilkRate::Khz16,
                };
                let count = v[1] as usize;
                let length = v[15] as usize;
                let mut state = PitchState {
                    rate: if v[5] != 0 { None } else { Some(rate) },
                    lag: v[8] as u16,
                    signal: signal(v[7]),
                    correlation: v[9],
                };
                let input: Vec<i16> = v[16 + count..16 + count + length]
                    .iter()
                    .map(|&v| v as i16)
                    .collect();
                pitch = Some(state.analyze(
                    rate,
                    if count == 2 {
                        crate::silk::gains::FrameDuration::Ms10
                    } else {
                        crate::silk::gains::FrameDuration::Ms20
                    },
                    &input,
                    PitchOptions {
                        complexity: v[2] as u8,
                        active: v[6] != 0,
                        speech_activity_q8: v[3] as u16,
                        input_tilt_q15: v[4] as i16,
                    },
                )?);
                continue;
            }
            let pitch = pitch.as_ref().ok_or("missing pitch trace")?;
            let count = v[4] as usize;
            let khz = pitch.length / (22 + pitch.count * 5);
            let mut xx = [[0i32; 25]; 4];
            let mut xt = [[0i32; 5]; 4];
            correlations(pitch.residual(), khz, pitch.lags(), &mut xx, &mut xt);
            let start = 5 + count + count * 5;
            assert_eq!(
                xx[..count].iter().flatten().copied().collect::<Vec<_>>(),
                v[start..start + count * 25],
                "matrix line {}",
                line_number + 1
            );
            assert_eq!(
                xt[..count].iter().flatten().copied().collect::<Vec<_>>(),
                v[start + count * 25..start + count * 30],
                "vector line {}",
                line_number + 1
            );
            let rate = match khz {
                8 => SilkRate::Khz8,
                12 => SilkRate::Khz12,
                _ => SilkRate::Khz16,
            };
            let mut state = LtpState {
                rate: Some(rate),
                sum_log_gain: v[0],
            };
            let frame = state.analyze(pitch, false);
            assert_eq!(
                i32::from(frame.periodicity_index()),
                v[3],
                "codebook line {}",
                line_number + 1
            );
            assert_eq!(
                frame.indices(),
                v[5..5 + count].iter().map(|&v| v as u8).collect::<Vec<_>>(),
                "indices line {}",
                line_number + 1
            );
            assert_eq!(state.sum_log_gain, v[1], "sum line {}", line_number + 1);
            assert_eq!(
                frame.prediction_gain_db_q7(),
                v[2],
                "gain line {}",
                line_number + 1
            );
            assert_eq!(
                frame
                    .coefficients_q14()
                    .iter()
                    .flatten()
                    .copied()
                    .collect::<Vec<_>>(),
                v[5 + count..start]
                    .iter()
                    .map(|&v| v as i16)
                    .collect::<Vec<_>>()
            );
            let at = start + count * 30;
            let inverse = &v[at..at + count];
            let input: Vec<i16> = v[at + count..at + count + pitch.length]
                .iter()
                .map(|&v| v as i16)
                .collect();
            let normalized = frame.normalized_residual(&input, inverse)?;
            assert_eq!(
                normalized.samples(),
                v[at + count + pitch.length..]
                    .iter()
                    .map(|&v| v as i16)
                    .collect::<Vec<_>>(),
                "normalized PCM line {}",
                line_number + 1
            );
            assert_eq!(
                frame.normalized_residual(&input[..input.len() - 1], inverse),
                Err(AnalysisError::InvalidLength)
            );
            assert_eq!(
                frame.normalized_residual(&input, &vec![0; count]),
                Err(AnalysisError::InvalidParameters)
            );
            let mut fresh = LtpState::default();
            assert_eq!(state.analyze(pitch, true), fresh.analyze(pitch, false));
            assert_eq!(state, fresh);
            selected[v[3] as usize] += 1;
            calls += 1;
        }
        assert_eq!(calls, 47);
        assert!(selected.iter().all(|&n| n > 0));
        Ok(())
    }
    #[test]
    fn unvoiced_frames_scale_without_prediction_and_reset_gain_history() -> Result<()> {
        assert_eq!(lin2log(0), -128);
        let mut pitch = PitchState::default();
        let mut state = LtpState {
            rate: Some(SilkRate::Khz16),
            sum_log_gain: 4000,
        };
        let input: Vec<i16> = (0..672)
            .map(|i| if i % 2 == 0 { i16::MIN } else { i16::MAX })
            .collect();
        let frame = pitch.analyze(
            SilkRate::Khz16,
            crate::silk::gains::FrameDuration::Ms20,
            &input,
            PitchOptions {
                complexity: 10,
                active: false,
                speech_activity_q8: 0,
                input_tilt_q15: 0,
            },
        )?;
        let ltp = state.analyze(&frame, false);
        assert_eq!(state.sum_log_gain, 0);
        assert!(ltp.coefficients_q14().iter().flatten().all(|&v| v == 0));
        let normalized = ltp.normalized_residual(&input, &[16384, 8192, 100, 1])?;
        for (k, &gain) in [16384, 8192, 100, 1].iter().enumerate() {
            for i in 0..96 {
                assert_eq!(
                    normalized.samples()[k * 96 + i],
                    mul_wb(gain, i32::from(input[320 - 16 + k * 80 + i])) as i16
                );
            }
        }
        state.reset();
        assert_eq!(state, LtpState::default());
        Ok(())
    }
}
