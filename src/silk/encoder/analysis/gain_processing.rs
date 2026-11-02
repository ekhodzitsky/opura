//! Quantized-predictor residual energy, gain soft limiting and NSQ lambda.
use super::{
    AnalysisError, GainQuantizer, LtpFrame, LtpResidual, QuantizedGains, ShapeFrame, SpectralFrame,
};
use crate::silk::{
    cng::sqrt_approx,
    encoder::math::{analysis_filter, energy, log2lin, sigmoid},
    gains::{FrameDuration, SignalType},
    pitch::SilkRate,
    synthesis::{mul_wb, mul_ww, round_shift, shift_saturated},
};
/// Per-subframe residual energies after the selected quantized LPC predictors.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResidualEnergies {
    rate: SilkRate,
    pub(super) count: usize,
    values: [i32; 4],
    q: [i32; 4],
}
impl ResidualEnergies {
    /// Measure stacked LTP-normalized blocks after quantized LPC filtering.
    /// Inverse gains must match those used for normalized_residual().
    pub fn analyze(
        rate: SilkRate,
        duration: FrameDuration,
        input: &LtpResidual,
        spectral: &SpectralFrame,
        inverse_gains_q16: &[i32],
    ) -> Result<Self, AnalysisError> {
        let khz = rate.khz() as usize;
        let count = if duration == FrameDuration::Ms10 {
            2
        } else {
            4
        };
        let order = if khz == 16 { 16 } else { 10 };
        let block = 5 * khz + order;
        if input.samples().len() != count * block
            || inverse_gains_q16.len() != count
            || spectral.quantized().spectrum_q15().len() != order
        {
            return Err(AnalysisError::InvalidLength);
        }
        if inverse_gains_q16.iter().any(|v| !(1..=16384).contains(v)) {
            return Err(AnalysisError::InvalidParameters);
        }
        let mut result = Self {
            rate,
            count,
            values: [0; 4],
            q: [0; 4],
        };
        let mut residual = [0i16; 192];
        for half in 0..count / 2 {
            analysis_filter(
                &mut residual[..2 * block],
                &input.samples()[half * 2 * block..(half + 1) * 2 * block],
                &spectral.predictors_q12()[half][..order],
            );
            for j in 0..2 {
                let k = half * 2 + j;
                let (power, shift) = energy(&residual[j * block + order..(j + 1) * block]);
                result.values[k] = power;
                result.q[k] = -(shift as i32);
            }
        }
        for (k, &inverse) in inverse_gains_q16.iter().enumerate() {
            let gain = 65536 / inverse;
            let lz1 = result.values[k].leading_zeros() - 1;
            let lz2 = gain.leading_zeros() - 1;
            let normalized = gain << lz2;
            let squared = ((i64::from(normalized) * i64::from(normalized)) >> 32) as i32;
            result.values[k] =
                ((i64::from(squared) * i64::from(result.values[k].wrapping_shl(lz1))) >> 32) as i32;
            result.q[k] += lz1 as i32 + 2 * lz2 as i32 - 64;
        }
        Ok(result)
    }
    /// Nonnegative energy mantissas, one per 5 ms subframe.
    pub fn values(&self) -> &[i32] {
        &self.values[..self.count]
    }
    /// Binary precision of each mantissa: physical energy = value * 2^(-q).
    pub fn precision(&self) -> &[i32] {
        &self.q[..self.count]
    }
}
/// Final gains and quantizer controls, before rate-control retry adjustment.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProcessedGains {
    quantized: QuantizedGains,
    unquantized: [i32; 4],
    offset: u8,
    lambda: u16,
    previous: u8,
}
impl ProcessedGains {
    /// Entropy gain indices and reconstructed NSQ Q16 gains.
    pub fn quantized(&self) -> &QuantizedGains {
        &self.quantized
    }
    /// Soft-limited Q16 gains retained for packet-budget retries.
    pub fn unquantized_q16(&self) -> &[i32] {
        &self.unquantized[..self.quantized.gains_q16().len()]
    }
    /// Final quantization offset, including the voiced gain/tilt override.
    pub fn offset(&self) -> u8 {
        self.offset
    }
    /// Positive NSQ rate/distortion tradeoff in Q10.
    pub fn lambda_q10(&self) -> u16 {
        self.lambda
    }
    /// Gain history before this frame, used to restore retry quantization.
    pub fn previous_index(&self) -> u8 {
        self.previous
    }
}
/// Gain history for one encoder channel. Errors preserve the entire history;
/// internal rate changes restart the native initial gain index of ten.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct GainProcessState {
    pub(super) rate: Option<SilkRate>,
    pub(super) quantizer: GainQuantizer,
}
impl GainProcessState {
    pub(in crate::silk::encoder) fn quantized_index(&self) -> u8 {
        self.quantizer.previous_index()
    }
    pub(in crate::silk::encoder) fn retry(
        &mut self,
        processed: &ProcessedGains,
        multipliers: &[i16; 4],
        conditional: bool,
    ) -> Result<QuantizedGains, AnalysisError> {
        let count = processed.quantized.gains_q16().len();
        let mut gains = [0i32; 4];
        for i in 0..count {
            gains[i] = shift_saturated(
                mul_wb(processed.unquantized[i], i32::from(multipliers[i])),
                8,
            );
        }
        let mut quantizer = GainQuantizer {
            previous: processed.previous,
        };
        let result = quantizer.quantize(
            if count == 2 {
                FrameDuration::Ms10
            } else {
                FrameDuration::Ms20
            },
            &gains[..count],
            conditional,
        )?;
        self.quantizer = quantizer;
        Ok(result)
    }
    pub(in crate::silk::encoder) fn hold_previous(
        &mut self,
        processed: &ProcessedGains,
        conditional: bool,
    ) -> QuantizedGains {
        let count = processed.quantized.gains_q16().len();
        let mut indices = [4; 4];
        if !conditional {
            indices[0] = processed.previous;
        }
        self.quantizer.previous = processed.previous;
        QuantizedGains {
            indices,
            gains: [crate::silk::gains::linear_gain(processed.previous); 4],
            count,
        }
    }
    #[cfg(test)]
    pub(in crate::silk::encoder) fn previous_index(&self) -> u8 {
        self.quantizer.previous_index()
    }
    /// Clear rate and gain-quantization history.
    pub fn reset(&mut self) {
        *self = Self::default();
    }
    /// Apply native prediction-dependent gain reduction, residual-energy soft
    /// limits, gain quantization, voiced offset selection and lambda calculation.
    pub fn process(
        &mut self,
        shape: &ShapeFrame,
        energies: &ResidualEnergies,
        ltp: &LtpFrame,
        conditional: bool,
    ) -> Result<ProcessedGains, AnalysisError> {
        if shape.rate != energies.rate
            || shape.count != energies.count
            || ltp.rate != shape.rate
            || ltp.count != shape.count
        {
            return Err(AnalysisError::InvalidLength);
        }
        let count = shape.count;
        let mut gains = shape.gains;
        if shape.signal == SignalType::Voiced {
            let reduction = -sigmoid(round_shift(ltp.prediction_gain_db_q7() - 1536, 4));
            for gain in &mut gains[..count] {
                *gain += mul_wb(*gain, reduction);
            }
        }
        let inverse_max = log2lin(mul_wb(8894 - i32::from(shape.options.snr_db_q7), 21627))
            / (5 * shape.rate.khz());
        for (k, current_gain) in gains.iter_mut().enumerate().take(count) {
            let mut part = mul_ww(energies.values[k], inverse_max);
            if part < 0 {
                return Err(AnalysisError::ArithmeticRange);
            }
            let q = energies.q[k];
            if q > 0 {
                part = if q >= 32 {
                    0
                } else {
                    round_shift(part, q as u32)
                };
            } else {
                let shift = (-q) as u32;
                part = if shift >= 31 || part >= i32::MAX >> shift {
                    i32::MAX
                } else {
                    part << shift
                };
            }
            let gain = *current_gain;
            let squared = part.saturating_add(((i64::from(gain) * i64::from(gain)) >> 32) as i32);
            *current_gain = if squared < 32767 {
                let exact = part.wrapping_shl(16).wrapping_add(mul_ww(gain, gain));
                if exact <= 0 {
                    return Err(AnalysisError::ArithmeticRange);
                }
                shift_saturated(sqrt_approx(exact).min(i32::MAX >> 8), 8)
            } else {
                shift_saturated(sqrt_approx(squared).min(i32::MAX >> 16), 16)
            };
            if *current_gain <= 0 {
                return Err(AnalysisError::ArithmeticRange);
            }
        }
        let mut staged = if self.rate == Some(shape.rate) {
            self.clone()
        } else {
            Self::default()
        };
        let previous = staged.quantizer.previous_index();
        let quantized = staged.quantizer.quantize(
            if count == 2 {
                FrameDuration::Ms10
            } else {
                FrameDuration::Ms20
            },
            &gains[..count],
            conditional,
        )?;
        let offset = if shape.signal == SignalType::Voiced {
            u8::from(
                ltp.prediction_gain_db_q7() + (i32::from(shape.options.activity.tilt_q15) >> 8)
                    <= 128,
            )
        } else {
            shape.offset
        };
        let quant_offset = if shape.signal == SignalType::Voiced {
            [32, 100][usize::from(offset)]
        } else {
            [100, 240][usize::from(offset)]
        };
        let states = match shape.options.complexity {
            0 | 1 => 1,
            2..=5 => 2,
            6..=7 => 3,
            _ => 4,
        };
        let lambda = 1229 - 50 * states
            + mul_wb(-52428, i32::from(shape.options.activity.probability_q8))
            + mul_wb(-409, shape.input_quality)
            + mul_wb(-818, shape.coding_quality)
            + mul_wb(52429, quant_offset);
        if !(1..2048).contains(&lambda) {
            return Err(AnalysisError::ArithmeticRange);
        }
        staged.rate = Some(shape.rate);
        *self = staged;
        Ok(ProcessedGains {
            quantized,
            unquantized: gains,
            offset,
            lambda: lambda as u16,
            previous,
        })
    }
}
