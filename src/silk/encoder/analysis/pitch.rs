//! LPC whitening and multirate three-stage pitch analysis.
use super::AnalysisError;
use crate::silk::{
    encoder::math::{analysis_filter, divide_q},
    gains::{FrameDuration, SignalType},
    lpc::expand_i16,
    pitch::SilkRate,
    synthesis::{mul_wb, saturate16},
};
mod search;
/// Activity and computational budget for one pitch analysis frame.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PitchOptions {
    /// Native encoder complexity, 0..=10.
    pub complexity: u8,
    /// VAD decision for this frame; inactive frames skip the pitch search.
    pub active: bool,
    /// Smoothed speech probability in Q8, 0..=256.
    pub speech_activity_q8: u16,
    /// VAD input spectral tilt in Q15.
    pub input_tilt_q15: i16,
}
/// Whitened context and quantizable pitch contour for one internal frame.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PitchFrame {
    pub(super) residual: [i16; 672],
    pub(super) length: usize,
    pub(super) count: usize,
    pub(super) signal: SignalType,
    pub(super) lags: [u16; 4],
    index: u16,
    contour: u8,
    correlation: i32,
    gain: i32,
}
impl PitchFrame {
    /// LPC residual including 20 ms history, current frame and 2 ms lookahead.
    pub fn residual(&self) -> &[i16] {
        &self.residual[..self.length]
    }
    /// Inactive, unvoiced or voiced decision.
    pub fn signal(&self) -> SignalType {
        self.signal
    }
    /// Pitch lag per 5 ms subframe; zero when not voiced.
    pub fn lags(&self) -> &[u16] {
        &self.lags[..self.count]
    }
    /// Absolute base lag index for SILK entropy serialization.
    pub fn lag_index(&self) -> u16 {
        self.index
    }
    /// Rate-/duration-dependent pitch contour codebook index.
    pub fn contour_index(&self) -> u8 {
        self.contour
    }
    /// Normalized long-term correlation in Q15.
    pub fn correlation_q15(&self) -> i32 {
        self.correlation
    }
    /// LPC prediction gain in Q16, before pitch analysis.
    pub fn prediction_gain_q16(&self) -> i32 {
        self.gain
    }
}
/// Pitch history for one channel. Rate changes restart voiced prediction;
/// duration and complexity changes retain it. Input context is caller-owned.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PitchState {
    pub(super) rate: Option<SilkRate>,
    pub(super) lag: u16,
    pub(super) signal: SignalType,
    pub(super) correlation: i32,
}
impl Default for PitchState {
    fn default() -> Self {
        Self {
            rate: None,
            lag: 100,
            signal: SignalType::Inactive,
            correlation: 0,
        }
    }
}
impl PitchState {
    /// Discard previous pitch and correlation history.
    pub fn reset(&mut self) {
        *self = Self::default();
    }
    /// Analyze internal-rate PCM: exactly 20 ms history + 10/20 ms current
    /// frame + 2 ms lookahead. The first frame after reset/rate change still
    /// whitens PCM, but suppresses voiced classification. Errors preserve state.
    pub fn analyze(
        &mut self,
        rate: SilkRate,
        duration: FrameDuration,
        input: &[i16],
        options: PitchOptions,
    ) -> Result<PitchFrame, AnalysisError> {
        let khz = rate.khz() as usize;
        let count = if duration == FrameDuration::Ms10 {
            2
        } else {
            4
        };
        let length = (22 + count * 5) * khz;
        if input.len() != length {
            return Err(AnalysisError::InvalidLength);
        }
        if options.complexity > 10 || options.speech_activity_q8 > 256 {
            return Err(AnalysisError::InvalidParameters);
        }
        let (complexity, threshold, order): (usize, i32, usize) = match options.complexity {
            0 | 2 => (0, 52429, 6),
            1 | 3 => (1, 49807, 8),
            4..=5 => (1, 48497, 10),
            6..=7 => (1, 47186, 12),
            _ => (2, 45875, 16),
        };
        let order = order.min(if rate == SilkRate::Khz16 { 16 } else { 10 });
        let lookahead = 2 * khz;
        let window_length = (4 + count * 5) * khz;
        let start = length - window_length;
        let mut window = [0i16; 384];
        sine_window(
            &mut window[..lookahead],
            &input[start..start + lookahead],
            false,
        );
        window[lookahead..window_length - lookahead]
            .copy_from_slice(&input[start + lookahead..length - lookahead]);
        sine_window(
            &mut window[window_length - lookahead..window_length],
            &input[length - lookahead..],
            true,
        );
        let ac = crate::celt::lpc::autocorrelation(&window[..window_length], order, false)
            .map_err(|_| AnalysisError::ArithmeticRange)?;
        let mut correlation = [0i32; 17];
        correlation[..=order].copy_from_slice(ac.values());
        correlation[0] += mul_wb(correlation[0], 66) + 1;
        let mut reflection = [0i16; 16];
        let residual_energy = schur(&mut reflection[..order], &correlation[..=order]);
        let gain = divide_q(correlation[0], residual_energy.max(1), 16);
        let mut a = [0i32; 16];
        for k in 0..order {
            let rc = i32::from(reflection[k]);
            for n in 0..k.div_ceil(2) {
                let left = a[n];
                let right = a[k - n - 1];
                a[n] = left.wrapping_add(mul_wb(right.wrapping_shl(1), rc));
                a[k - n - 1] = right.wrapping_add(mul_wb(left.wrapping_shl(1), rc));
            }
            a[k] = -(rc << 9);
        }
        let mut coefficients = [0i16; 16];
        for i in 0..order {
            coefficients[i] = saturate16(a[i] >> 12);
        }
        expand_i16(&mut coefficients[..order], 64881);
        let mut result = PitchFrame {
            residual: [0; 672],
            length,
            count,
            signal: if options.active {
                SignalType::Unvoiced
            } else {
                SignalType::Inactive
            },
            lags: [0; 4],
            index: 0,
            contour: 0,
            correlation: 0,
            gain,
        };
        analysis_filter(
            &mut result.residual[..length],
            input,
            &coefficients[..order],
        );
        if options.active && self.rate == Some(rate) {
            let threshold2 = 4915 - 32 * order as i32
                + mul_wb(-209714, i32::from(options.speech_activity_q8))
                - 1228 * i32::from(self.signal == SignalType::Voiced)
                + mul_wb(-1637, i32::from(options.input_tilt_q15));
            search::analyze(
                &mut result,
                khz,
                complexity,
                threshold,
                i32::from(saturate16(threshold2)),
                self.lag,
                self.correlation,
            );
        }
        self.rate = Some(rate);
        self.lag = result.lags[count - 1];
        self.signal = result.signal;
        self.correlation = result.correlation;
        Ok(result)
    }
}
// Validated pitch/noise windows have length 16..=120 and a multiple of four.
fn sine_window(output: &mut [i16], input: &[i16], falling: bool) {
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
fn schur(output: &mut [i16], input: &[i32]) -> i32 {
    let order = output.len();
    let zeros = input[0].leading_zeros();
    let mut c = [[0i32; 2]; 17];
    for i in 0..=order {
        let value = if zeros < 2 {
            input[i] >> 1
        } else {
            input[i].wrapping_shl(zeros - 2)
        };
        c[i] = [value; 2];
    }
    for k in 0..order {
        if c[k + 1][0].unsigned_abs() >= c[0][1] as u32 {
            output[k] = if c[k + 1][0] > 0 { -32440 } else { 32440 };
            break;
        }
        let rc = i32::from(saturate16(-(c[k + 1][0] / (c[0][1] >> 15).max(1))));
        output[k] = rc as i16;
        for n in 0..order - k {
            let a = c[n + k + 1][0];
            let b = c[n][1];
            c[n + k + 1][0] = a.wrapping_add(mul_wb(b.wrapping_shl(1), rc));
            c[n][1] = b.wrapping_add(mul_wb(a.wrapping_shl(1), rc));
        }
    }
    c[0][1].max(1)
}

#[cfg(test)]
mod tests;
