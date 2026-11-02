//! Complete native LPC/NLSF spectral stage over normalized PCM subframes.
use super::super::math::energy;
use super::*;
use crate::silk::{encoder::math::analysis_filter, lpc::convert, nlsf::NlsfOrder, pitch::SilkRate};
/// Controls supplied by activity, pitch/LTP and quality analysis.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SpectralOptions {
    /// Inverse prediction-gain floor in Q30, 1..=2^30.
    pub minimum_inverse_gain_q30: u32,
    /// Voice activity in Q8, 0..=256.
    pub speech_activity_q8: u16,
    /// First-stage codebook survivors, 1..=32.
    pub survivors: u8,
    /// Whether to search first-/second-half NLSF interpolation on 20 ms frames.
    pub allow_interpolation: bool,
    /// Signal classification used for the first-stage entropy rate context.
    pub signal: crate::silk::gains::SignalType,
}
/// Spectral indices and ready-to-use NSQ predictors for one channel frame.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpectralFrame {
    quantized: QuantizedNlsf,
    predictors: [[i16; 16]; 2],
    interpolation: u8,
}
impl SpectralFrame {
    /// Full quantized spectrum, codebook path and rate/distortion objective.
    pub fn quantized(&self) -> &QuantizedNlsf {
        &self.quantized
    }
    /// First-/second-half Q12 LPC predictors. Only the internal order is active.
    pub fn predictors_q12(&self) -> &[[i16; 16]; 2] {
        &self.predictors
    }
    /// Chosen interpolation factor; four disables interpolation.
    pub fn interpolation_q2(&self) -> u8 {
        self.interpolation
    }
}
/// Persistent quantized spectral history for one encoder channel.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SpectralState {
    pub(super) rate: Option<SilkRate>,
    pub(super) previous: [i16; 16],
}
impl SpectralState {
    /// Clear interpolation history.
    pub fn reset(&mut self) {
        *self = Self::default();
    }
    /// Analyze normalized/LTP-filtered stacked PCM blocks as in [`analyze_lpc`],
    /// choose interpolation, and quantize the NLSFs and predictor coefficients.
    /// Rate changes suppress prior interpolation, including 8/12 kHz switches.
    /// Failures leave spectral history unchanged; PCM normalization, pitch/LTP
    /// analysis and packet orchestration are supplied by the enclosing encoder.
    pub fn analyze(
        &mut self,
        rate: SilkRate,
        duration: FrameDuration,
        input: &[i16],
        options: SpectralOptions,
    ) -> Result<SpectralFrame, AnalysisError> {
        if options.speech_activity_q8 > 256 || !(1..=32).contains(&options.survivors) {
            return Err(AnalysisError::InvalidParameters);
        }
        let full = analyze_lpc(rate, duration, input, options.minimum_inverse_gain_q30)?;
        let order = if rate == SilkRate::Khz16 {
            NlsfOrder::Order16
        } else {
            NlsfOrder::Order10
        };
        let count = order.len();
        let length = rate.khz() as usize * 5 + count;
        let mut factor = 4;
        let mut spectrum = [0i16; 16];
        if options.allow_interpolation && self.rate == Some(rate) && duration == FrameDuration::Ms20
        {
            let last = analyze_lpc(
                rate,
                FrameDuration::Ms10,
                &input[2 * length..],
                options.minimum_inverse_gain_q30,
            )?;
            let (mut best, mut best_q) = full.residual_energy();
            let (last_energy, last_q) = last.residual_energy();
            let shift = last_q - best_q;
            if shift >= 0 {
                if shift < 32 {
                    best = best.wrapping_sub(last_energy >> shift);
                }
            } else {
                best = (best >> -shift).wrapping_sub(last_energy);
                best_q = last_q;
            }
            spectrum = last.frequencies_q15()?;
            let mut residual = [0i16; 192];
            for candidate in (0..4).rev() {
                let interpolated = interpolate(&self.previous, &spectrum, count, candidate);
                let predictor = convert(&interpolated[..count]);
                analysis_filter(
                    &mut residual[..2 * length],
                    &input[..2 * length],
                    &predictor[..count],
                );
                let (mut e0, q0) = energy(&residual[count..length]);
                let (mut e1, q1) = energy(&residual[count + length..2 * length]);
                let exponent = if q0 >= q1 {
                    e1 >>= q0 - q1;
                    -(q0 as i32)
                } else {
                    e0 >>= q1 - q0;
                    -(q1 as i32)
                };
                let sum = e0.wrapping_add(e1);
                let shift = exponent - best_q;
                let lower = if shift >= 0 {
                    (sum >> shift) < best
                } else {
                    -shift < 32 && sum < (best >> -shift)
                };
                if lower {
                    best = sum;
                    best_q = exponent;
                    factor = candidate;
                }
            }
        }
        if factor == 4 {
            spectrum = full.frequencies_q15()?;
        }
        let mut weight = laroia_weights(order, &spectrum[..count])?;
        if factor < 4 {
            let previous = interpolate(&self.previous, &spectrum, count, factor);
            let first = laroia_weights(order, &previous[..count])?;
            let square = i32::from(factor) * i32::from(factor) * 2048;
            for j in 0..count {
                weight[j] =
                    (i32::from(weight[j] >> 1) + ((i32::from(first[j]) * square) >> 16)) as i16;
            }
        }
        let mut mu = 3146 + mul_wb(-268434, i32::from(options.speech_activity_q8));
        if duration == FrameDuration::Ms10 {
            mu += mu >> 1;
        }
        let quantized = quantize_nlsf(
            order,
            &spectrum[..count],
            &weight[..count],
            mu as u16,
            usize::from(options.survivors),
            options.signal,
        )?;
        spectrum[..count].copy_from_slice(quantized.spectrum_q15());
        let second = quantized.predictor_q12();
        let first = if factor < 4 {
            let interpolated = interpolate(&self.previous, &spectrum, count, factor);
            convert(&interpolated[..count])
        } else {
            second
        };
        self.rate = Some(rate);
        self.previous = spectrum;
        Ok(SpectralFrame {
            quantized,
            predictors: [first, second],
            interpolation: factor,
        })
    }
}
fn interpolate(previous: &[i16; 16], current: &[i16; 16], count: usize, factor: u8) -> [i16; 16] {
    let mut result = [0i16; 16];
    for j in 0..count {
        result[j] = (i32::from(previous[j])
            + ((i32::from(factor) * (i32::from(current[j]) - i32::from(previous[j]))) >> 2))
            as i16;
    }
    result
}
