//! Stateful perceptual noise-shaping analysis from internal PCM and pitch.
use super::{AnalysisError, PitchFrame, SpeechActivity};
use crate::silk::{
    cng::sqrt_approx,
    encoder::math::{divide_q, energy, expand32, lin2log, log2lin, sigmoid, sine_window},
    gains::{FrameDuration, SignalType},
    pitch::SilkRate,
    synthesis::{inverse_gain, mul_wb, mul_ww, round_shift, saturate16, shift_saturated},
};
mod filters;
/// Per-frame quality and complexity controls for noise shaping.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ShapeOptions {
    /// Native noise-shaping complexity, 0..=10.
    pub complexity: u8,
    /// Target coding SNR in dB Q7, 0..=8192 (64 dB).
    pub snr_db_q7: u16,
    /// Activity, spectral tilt and band qualities from VAD.
    pub activity: SpeechActivity,
    /// Disable variable-rate activity-dependent SNR reduction.
    pub constant_bitrate: bool,
}
/// Noise shaping, initial gains and quality controls for prediction and NSQ.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShapeFrame {
    pub(super) rate: SilkRate,
    pub(super) count: usize,
    pub(super) signal: SignalType,
    pub(super) options: ShapeOptions,
    pub(super) order: usize,
    pub(super) warping: i16,
    pub(super) ar: [[i16; 24]; 4],
    pub(super) gains: [i32; 4],
    pub(super) low_frequency: [i32; 4],
    pub(super) harmonic: [i16; 4],
    pub(super) tilt: [i16; 4],
    pub(super) offset: u8,
    pub(super) input_quality: i32,
    pub(super) coding_quality: i32,
}
impl ShapeFrame {
    /// Shaping filter order, 12..=24, depending on complexity.
    pub fn order(&self) -> usize {
        self.order
    }
    /// NSQ warping in Q16. Analysis uses a slightly quality-adjusted value.
    pub fn warping_q16(&self) -> i16 {
        self.warping
    }
    /// Per-subframe Q13 AR coefficients; only the first order() taps are active.
    pub fn coefficients_q13(&self) -> &[[i16; 24]] {
        &self.ar[..self.count]
    }
    /// Positive initial Q16 gains, before prediction/gain soft limiting.
    pub fn gains_q16(&self) -> &[i32] {
        &self.gains[..self.count]
    }
    /// Packed Q14 low-frequency shaping coefficients.
    pub fn low_frequency_q14(&self) -> &[i32] {
        &self.low_frequency[..self.count]
    }
    /// Smoothed Q14 harmonic shaping strength for each subframe.
    pub fn harmonic_q14(&self) -> &[i16] {
        &self.harmonic[..self.count]
    }
    /// Smoothed Q14 noise tilt for each subframe.
    pub fn tilt_q14(&self) -> &[i16] {
        &self.tilt[..self.count]
    }
    /// Initial quantization offset. Gain processing may override voiced offsets.
    pub fn offset(&self) -> u8 {
        self.offset
    }
    /// Combined quality of the two lowest VAD bands in Q14.
    pub fn input_quality_q14(&self) -> i32 {
        self.input_quality
    }
    /// Target-SNR-derived coding quality in Q14.
    pub fn coding_quality_q14(&self) -> i32 {
        self.coding_quality
    }
    /// Derive native LPC gain floor, spectrum survivors and interpolation policy
    /// from this frame's quality/complexity and quantized LTP prediction gain.
    pub fn spectral_options(
        &self,
        ltp: &super::LtpFrame,
        first: bool,
    ) -> Result<super::SpectralOptions, AnalysisError> {
        if self.rate != ltp.rate || self.count != ltp.count {
            return Err(AnalysisError::InvalidLength);
        }
        let minimum = if first {
            10737418
        } else {
            let numerator = log2lin(2048 + mul_wb(ltp.prediction_gain_db_q7(), 21845));
            let denominator = mul_ww(10000, 65536 + mul_wb(196608, self.coding_quality));
            divide_q(numerator, denominator, 14)
        };
        if !(1..=1 << 30).contains(&minimum) {
            return Err(AnalysisError::ArithmeticRange);
        }
        let complexity = self.options.complexity;
        let survivors = match complexity {
            0 | 2 => 2,
            1 => 3,
            3 => 4,
            4..=5 => 6,
            6..=7 => 8,
            _ => 16,
        };
        Ok(super::SpectralOptions {
            minimum_inverse_gain_q30: minimum as u32,
            speech_activity_q8: u16::from(self.options.activity.probability_q8),
            survivors,
            allow_interpolation: complexity >= 4,
            signal: self.signal,
        })
    }
    /// Native gain normalization for the LTP/LPC prediction stage: return
    /// inverse Q16 gains with one-quarter maximum and a floor of 100.
    pub fn inverse_gains_q16(&self) -> [i32; 4] {
        let minimum = self
            .gains_q16()
            .iter()
            .copied()
            .fold(i32::MAX >> 6, i32::min);
        let mut inverse = [0; 4];
        for (out, &gain) in inverse.iter_mut().zip(self.gains_q16()) {
            *out = divide_q(minimum, gain, 14).max(100);
        }
        inverse
    }
}
/// Smoothing state for one channel. Internal rate changes clear the filters;
/// duration/complexity changes preserve smoothing. Failures preserve all state.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ShapeState {
    pub(super) rate: Option<SilkRate>,
    pub(super) harmonic: i32,
    pub(super) tilt: i32,
}
impl ShapeState {
    /// Clear harmonic/tilt smoothing and internal-rate history.
    pub fn reset(&mut self) {
        *self = Self::default();
    }
    /// Analyze exactly 20 ms history + current 10/20 ms + shape lookahead PCM.
    /// Lookahead is 3 ms at complexity0/2 and 5 ms otherwise. Pitch must describe
    /// the same rate/duration frame. This stage commits only after all filters fit.
    pub fn analyze(
        &mut self,
        rate: SilkRate,
        duration: FrameDuration,
        input: &[i16],
        pitch: &PitchFrame,
        options: ShapeOptions,
    ) -> Result<ShapeFrame, AnalysisError> {
        let khz = rate.khz() as usize;
        let count = if duration == FrameDuration::Ms10 {
            2
        } else {
            4
        };
        if options.complexity > 10
            || options.snr_db_q7 > 8192
            || options.activity.quality_bands_q15.iter().any(|&v| v < 0)
        {
            return Err(AnalysisError::InvalidParameters);
        }
        let lookahead = if matches!(options.complexity, 0 | 2) {
            3 * khz
        } else {
            5 * khz
        };
        if input.len() != (20 + 5 * count) * khz + lookahead
            || pitch.length != (22 + 5 * count) * khz
            || pitch.count != count
        {
            return Err(AnalysisError::InvalidLength);
        }
        let (order, warping) = match options.complexity {
            0 | 2 => (12, 0),
            1 | 3 => (14, 0),
            4..=5 => (16, khz * 983),
            6..=7 => (20, khz * 983),
            _ => (24, khz * 983),
        };
        let input_quality = (i32::from(options.activity.quality_bands_q15[0])
            + i32::from(options.activity.quality_bands_q15[1]))
            >> 2;
        let coding_quality = sigmoid(round_shift(i32::from(options.snr_db_q7) - 2560, 4)) >> 1;
        let mut result = ShapeFrame {
            rate,
            count,
            signal: pitch.signal,
            options,
            order,
            warping: warping as i16,
            ar: [[0; 24]; 4],
            gains: [0; 4],
            low_frequency: [0; 4],
            harmonic: [0; 4],
            tilt: [0; 4],
            offset: 0,
            input_quality,
            coding_quality,
        };
        let mut snr = i32::from(options.snr_db_q7);
        let activity = i32::from(options.activity.probability_q8);
        if !options.constant_bitrate {
            let background = 256 - activity;
            let background = mul_wb(background << 8, background);
            snr += mul_wb(
                -8 * background,
                mul_wb(16384 + input_quality, coding_quality),
            );
        }
        if pitch.signal == SignalType::Voiced {
            snr += mul_wb(512, pitch.correlation_q15());
        } else {
            snr += mul_wb(
                3072 + mul_wb(-104858, i32::from(options.snr_db_q7)),
                16384 - input_quality,
            );
        }
        if pitch.signal != SignalType::Voiced {
            let mut variation = 0;
            let mut previous = 0;
            let residual = &pitch.residual()[20 * khz..(20 + 5 * count) * khz];
            let segments = 5 * count / 2;
            for (k, segment) in residual.chunks_exact(2 * khz).enumerate() {
                let (power, shift) = energy(segment);
                let log = lin2log(power + ((2 * khz) as i32 >> shift));
                if k > 0 {
                    variation += (log - previous).abs();
                }
                previous = log;
            }
            result.offset = u8::from(variation <= 77 * (segments - 1) as i32);
        }
        let strength = mul_wb(pitch.prediction_gain_q16(), 66);
        let bandwidth = divide_q(61604, 65536 + mul_ww(strength, strength), 16);
        let adjusted_warping = if warping > 0 {
            warping as i32 + mul_wb(coding_quality, 2621)
        } else {
            0
        };
        let window_length = 5 * khz + 2 * lookahead;
        let flat = 3 * khz;
        let slope = (window_length - flat) / 2;
        let mut window = [0i16; 240];
        for k in 0..count {
            let start = 20 * khz - lookahead + k * 5 * khz;
            let x = &input[start..start + window_length];
            sine_window(&mut window[..slope], &x[..slope], false);
            window[slope..slope + flat].copy_from_slice(&x[slope..slope + flat]);
            sine_window(
                &mut window[slope + flat..window_length],
                &x[slope + flat..],
                true,
            );
            let (mut correlation, scale) = if warping > 0 {
                filters::warped_correlation(&window[..window_length], adjusted_warping, order)
            } else {
                let ac = crate::celt::lpc::autocorrelation(&window[..window_length], order, false)
                    .map_err(|_| AnalysisError::ArithmeticRange)?;
                let mut values = [0i32; 25];
                values[..=order].copy_from_slice(ac.values());
                (values, ac.shift())
            };
            correlation[0] = correlation[0].wrapping_add(mul_wb(correlation[0] >> 4, 31).max(1));
            let mut reflection = [0i32; 24];
            let mut power = filters::schur(&mut reflection[..order], &correlation[..=order]);
            let mut a = [0i32; 24];
            for j in 0..order {
                for n in 0..j.div_ceil(2) {
                    let left = a[n];
                    let right = a[j - n - 1];
                    a[n] = left.wrapping_add(mul_ww(right, reflection[j]));
                    a[j - n - 1] = right.wrapping_add(mul_ww(left, reflection[j]));
                }
                a[j] = -(reflection[j] << 8);
            }
            let mut q = -scale;
            if q & 1 != 0 {
                q -= 1;
                power >>= 1;
            }
            q >>= 1;
            let mut gain = shift_saturated(sqrt_approx(power), (16 - q) as u32);
            if warping > 0 {
                let mut denominator = a[order - 1];
                for i in (0..order - 1).rev() {
                    denominator = a[i].wrapping_add(mul_wb(denominator, -adjusted_warping));
                }
                denominator = (1 << 24) + mul_wb(denominator, adjusted_warping);
                if denominator <= 0 {
                    return Err(AnalysisError::ArithmeticRange);
                }
                let multiplier = inverse_gain(denominator, 40);
                gain = if gain < 16384 {
                    mul_ww(gain, multiplier)
                } else {
                    let half = mul_ww(round_shift(gain, 1), multiplier);
                    if half >= i32::MAX >> 1 {
                        i32::MAX
                    } else {
                        half.wrapping_shl(1)
                    }
                };
            }
            expand32(&mut a[..order], bandwidth);
            if warping > 0 {
                filters::limit_warped(&mut a[..order], adjusted_warping)?;
                for (out, &v) in result.ar[k][..order].iter_mut().zip(&a[..order]) {
                    *out = saturate16(round_shift(v, 11));
                }
            } else {
                filters::fit(&mut result.ar[k][..order], &mut a[..order]);
            }
            result.gains[k] = gain;
        }
        let multiplier = log2lin(2048 - mul_wb(snr, 10486));
        let floor = log2lin(2048 + mul_wb(2 * 128, 10486));
        for gain in &mut result.gains[..count] {
            *gain = mul_ww(*gain, multiplier).saturating_add(floor);
            if *gain <= 0 {
                return Err(AnalysisError::ArithmeticRange);
            }
        }
        let strength = (64
            * (4096
                + mul_wb(
                    4096,
                    i32::from(options.activity.quality_bands_q15[0]) - 32768,
                ))
            * activity)
            >> 8;
        let target_tilt;
        if pitch.signal == SignalType::Voiced {
            for k in 0..count {
                let b = 3277 / khz as i32 + 49152 / i32::from(pitch.lags()[k]);
                result.low_frequency[k] = (16384 - b - mul_wb(strength, b)).wrapping_shl(16)
                    | i32::from((b - 16384) as u16);
            }
            target_tilt = -16384 - mul_wb(49152, mul_wb(5872026, activity));
        } else {
            let b = 21299 / khz as i32;
            let low = (16384 - b - mul_wb(strength, mul_wb(39322, b))).wrapping_shl(16)
                | i32::from((b - 16384) as u16);
            result.low_frequency[..count].fill(low);
            target_tilt = -16384;
        }
        let target_harmonic = if pitch.signal == SignalType::Voiced {
            let gain = 19661
                + mul_wb(
                    65536 - mul_wb(262144 - (coding_quality << 4), input_quality),
                    13107,
                );
            mul_wb(gain << 1, sqrt_approx(pitch.correlation_q15() << 15))
        } else {
            0
        };
        let mut staged = if self.rate == Some(rate) {
            self.clone()
        } else {
            Self::default()
        };
        for k in 0..4 {
            staged.harmonic += mul_wb(target_harmonic - staged.harmonic, 26214);
            staged.tilt += mul_wb(target_tilt - staged.tilt, 26214);
            result.harmonic[k] = round_shift(staged.harmonic, 2) as i16;
            result.tilt[k] = round_shift(staged.tilt, 2) as i16;
        }
        staged.rate = Some(rate);
        *self = staged;
        Ok(result)
    }
}
