//! Fixed-storage SILK noise-shaped scalar quantization with native arithmetic.
//!
//! Includes scalar and delayed-decision NSQ paths. The enclosing encoder supplies analyzed,
//! quantized predictor/gain and noise-shaping coefficients. PCM analysis and
//! complete packet orchestration remain separate encoder work.
//! Adapted from Opus v1.6.1 silk/NSQ.c and NSQ.h; see THIRD_PARTY_NOTICES.md.
use crate::silk::{
    gains::{FrameDuration, SignalType},
    pitch::SilkRate,
    synthesis::{divide_gain, inverse_gain, mul_wb, mul_ww, round_shift, saturate16},
};
use std::fmt;
mod delayed;

/// Analyzed predictor and shaping parameters for one 10/20 ms channel frame.
/// Arrays contain two/four subframes, using LPC order 10 at 8/12 kHz and 16
/// at 16 kHz. Unused array entries are ignored.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NsqParameters {
    /// Signal classification.
    pub signal: SignalType,
    /// Quantization offset selector, zero or one.
    pub offset: u8,
    /// Initial dither seed, 0..=3.
    pub seed: u8,
    /// NLSF interpolation factor 0..=4; four for a 10 ms frame.
    pub interpolation_q2: u8,
    /// First-/second-half LPC prediction coefficients in Q12.
    pub predictor_q12: [[i16; 16]; 2],
    /// Five Q14 LTP prediction coefficients per subframe.
    pub ltp_q14: [[i16; 5]; 4],
    /// Even AR shaping order, 2..=24.
    pub shaping_order: u8,
    /// AR noise-shaping coefficients in Q13.
    pub shaping_q13: [[i16; 24]; 4],
    /// Nonnegative harmonic noise-shaping gains in Q14.
    pub harmonic_q14: [i16; 4],
    /// Spectral tilt coefficients in Q14.
    pub tilt_q14: [i16; 4],
    /// Packed low-/high-half signed Q14 low-frequency shaping coefficients.
    pub low_frequency_q14: [i32; 4],
    /// Positive quantized Q16 subframe gains.
    pub gains_q16: [i32; 4],
    /// Subframe pitch lags in samples: 2..=18 ms, or zero for unvoiced frames.
    pub pitch: [u16; 4],
    /// Rate/distortion tradeoff in Q10, 0..=32767.
    pub lambda_q10: u16,
    /// LTP state scaling in Q14, 0..=16384; used for initial rewhitening.
    pub ltp_scale_q14: u16,
}
/// Invalid NSQ input; state remains unchanged.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum NsqError {
    /// Input length must match the internal rate and duration exactly.
    InvalidInputLength,
    /// A selector, order, gain, pitch or scaling parameter is outside its domain.
    InvalidParameters,
}
impl fmt::Display for NsqError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidInputLength => "NSQ PCM length does not match frame geometry",
            Self::InvalidParameters => "invalid SILK NSQ parameters",
        })
    }
}
impl std::error::Error for NsqError {}
/// Signed quantization pulses and the encoder's reconstructed internal PCM.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NsqFrame {
    pulses: [i8; 320],
    pcm: [i16; 320],
    length: usize,
    seed: u8,
}
impl NsqFrame {
    /// Initial dither seed selected by quantization; serialize this value.
    pub fn seed(&self) -> u8 {
        self.seed
    }
    /// Noise-shaped pulses ready for SILK shell/LSB/sign serialization.
    pub fn pulses(&self) -> &[i8] {
        &self.pulses[..self.length]
    }
    /// Local quantizer reconstruction before Opus buffering/output processing.
    pub fn reconstructed_pcm(&self) -> &[i16] {
        &self.pcm[..self.length]
    }
}
/// Persistent one-channel scalar/delayed-decision NSQ state, with no allocation.
/// Rate changes reset all prediction/shaping history. Duration changes retain it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoiseShapingQuantizer {
    rate: Option<SilkRate>,
    xq: [i16; 640],
    shaping_ltp: [i32; 640],
    lpc: [i32; 96],
    ar: [i32; 24],
    low_frequency: i32,
    difference: i32,
    previous_lag: usize,
    ltp_index: usize,
    shaping_index: usize,
    random: i32,
    previous_gain: i32,
    rewhite: bool,
}
impl Default for NoiseShapingQuantizer {
    fn default() -> Self {
        Self {
            rate: None,
            xq: [0; 640],
            shaping_ltp: [0; 640],
            lpc: [0; 96],
            ar: [0; 24],
            low_frequency: 0,
            difference: 0,
            previous_lag: 100,
            ltp_index: 0,
            shaping_index: 0,
            random: 0,
            previous_gain: 65536,
            rewhite: false,
        }
    }
}
impl NoiseShapingQuantizer {
    /// Clear all channel history, including the native initial gain/lag values.
    pub fn reset(&mut self) {
        *self = Self::default();
    }
    /// Quantize a complete internal PCM frame with already analyzed parameters.
    /// Validation precedes rate resets or mutation. This does not perform PCM
    /// analysis, entropy coding, bitrate search or delayed-decision quantization.
    pub fn quantize(
        &mut self,
        rate: SilkRate,
        duration: FrameDuration,
        input: &[i16],
        p: &NsqParameters,
    ) -> Result<NsqFrame, NsqError> {
        let (khz, subframes) = self.prepare(rate, duration, input, p)?;
        let mut frame = NsqFrame {
            pulses: [0; 320],
            pcm: [0; 320],
            length: input.len(),
            seed: p.seed,
        };
        self.process(khz, subframes, input, p, &mut frame);
        Ok(frame)
    }
    /// Quantize with 1..=4 delayed-decision states and signed Q16 warping.
    /// Returns the winning initial dither seed with the quantized output.
    /// Validation changes neither history nor rate configuration on failure.
    pub fn quantize_delayed(
        &mut self,
        rate: SilkRate,
        duration: FrameDuration,
        input: &[i16],
        p: &NsqParameters,
        states: u8,
        warping_q16: i16,
    ) -> Result<NsqFrame, NsqError> {
        if !(1..=4).contains(&states) {
            return Err(NsqError::InvalidParameters);
        }
        let (khz, subframes) = self.prepare(rate, duration, input, p)?;
        Ok(delayed::process(
            self,
            khz,
            subframes,
            input,
            p,
            usize::from(states),
            warping_q16,
        ))
    }
    fn prepare(
        &mut self,
        rate: SilkRate,
        duration: FrameDuration,
        input: &[i16],
        p: &NsqParameters,
    ) -> Result<(usize, usize), NsqError> {
        let khz = rate.khz() as usize;
        let subframes = if duration == FrameDuration::Ms10 {
            2
        } else {
            4
        };
        let length = subframes * 5 * khz;
        if input.len() != length {
            return Err(NsqError::InvalidInputLength);
        }
        if p.offset > 1
            || p.seed > 3
            || p.interpolation_q2 > 4
            || (subframes == 2 && p.interpolation_q2 != 4)
            || !(2..=24).contains(&p.shaping_order)
            || p.shaping_order % 2 != 0
            || p.lambda_q10 > 32767
            || p.ltp_scale_q14 > 16384
            || p.gains_q16[..subframes].iter().any(|&v| v <= 0)
            || p.harmonic_q14[..subframes].iter().any(|&v| v < 0)
            || p.pitch[..subframes].iter().any(|&v| {
                usize::from(v) > 18 * khz
                    || (v != 0 && usize::from(v) < 2 * khz)
                    || (p.signal == SignalType::Voiced && v == 0)
            })
        {
            return Err(NsqError::InvalidParameters);
        }
        if self.rate != Some(rate) {
            self.reset();
            self.rate = Some(rate);
        }
        Ok((khz, subframes))
    }
    fn process(
        &mut self,
        khz: usize,
        subframes: usize,
        input: &[i16],
        p: &NsqParameters,
        frame: &mut NsqFrame,
    ) {
        let memory = khz * 20;
        let sub_length = khz * 5;
        let order = if khz == 16 { 16 } else { 10 };
        let interpolation = usize::from(p.interpolation_q2 != 4);
        let mut lag = self.previous_lag;
        let mut ltp = [0i32; 640];
        let mut white = [0i16; 640];
        let mut scaled = [0i32; 80];
        self.random = i32::from(p.seed);
        self.shaping_index = memory;
        self.ltp_index = memory;
        for k in 0..subframes {
            let predictor = &p.predictor_q12[(k >> 1) | (1 - interpolation)];
            self.rewhite = false;
            if p.signal == SignalType::Voiced {
                lag = usize::from(p.pitch[k]);
                if k & (3 - 2 * interpolation) == 0 {
                    let start = memory - lag - order - 2;
                    analysis_filter(
                        &mut white[start..memory],
                        &self.xq[start + k * sub_length..memory + k * sub_length],
                        &predictor[..order],
                    );
                    self.rewhite = true;
                    self.ltp_index = memory;
                }
            }
            self.scale(
                k,
                memory,
                p,
                &input[k * sub_length..(k + 1) * sub_length],
                &mut scaled[..sub_length],
                &white,
                &mut ltp,
            );
            self.samples(
                k,
                order,
                lag,
                p,
                &scaled[..sub_length],
                predictor,
                &mut ltp,
                &mut frame.pulses[k * sub_length..(k + 1) * sub_length],
            );
        }
        frame.pcm[..frame.length].copy_from_slice(&self.xq[memory..memory + frame.length]);
        self.previous_lag = usize::from(p.pitch[subframes - 1]);
        self.xq.copy_within(frame.length..frame.length + memory, 0);
        self.shaping_ltp
            .copy_within(frame.length..frame.length + memory, 0);
    }
    #[allow(clippy::too_many_arguments)]
    fn scale(
        &mut self,
        k: usize,
        memory: usize,
        p: &NsqParameters,
        input: &[i16],
        scaled: &mut [i32],
        white: &[i16],
        ltp: &mut [i32],
    ) {
        let lag = usize::from(p.pitch[k]);
        let mut inverse = inverse_gain(p.gains_q16[k], 47);
        let inverse26 = round_shift(inverse, 5);
        for (out, &sample) in scaled.iter_mut().zip(input) {
            *out = mul_ww(i32::from(sample), inverse26);
        }
        if self.rewhite {
            if k == 0 {
                inverse = mul_wb(inverse, i32::from(p.ltp_scale_q14)).wrapping_shl(2);
            }
            for i in self.ltp_index - lag - 2..self.ltp_index {
                ltp[i] = mul_wb(inverse, i32::from(white[i]));
            }
        }
        if p.gains_q16[k] != self.previous_gain {
            let adjustment = divide_gain(self.previous_gain, p.gains_q16[k]);
            for value in &mut self.shaping_ltp[self.shaping_index - memory..self.shaping_index] {
                *value = mul_ww(adjustment, *value);
            }
            if p.signal == SignalType::Voiced && !self.rewhite {
                for value in &mut ltp[self.ltp_index - lag - 2..self.ltp_index] {
                    *value = mul_ww(adjustment, *value);
                }
            }
            self.low_frequency = mul_ww(adjustment, self.low_frequency);
            self.difference = mul_ww(adjustment, self.difference);
            for value in &mut self.lpc[..16] {
                *value = mul_ww(adjustment, *value);
            }
            for value in &mut self.ar {
                *value = mul_ww(adjustment, *value);
            }
            self.previous_gain = p.gains_q16[k];
        }
    }
    #[allow(clippy::too_many_arguments)]
    fn samples(
        &mut self,
        k: usize,
        order: usize,
        lag: usize,
        p: &NsqParameters,
        input: &[i32],
        predictor: &[i16; 16],
        ltp: &mut [i32],
        pulses: &mut [i8],
    ) {
        let harmonic = i32::from(p.harmonic_q14[k]);
        let packed = (harmonic >> 2) | ((harmonic >> 1) << 16);
        let offset = [[100, 240], [32, 100]][usize::from(p.signal == SignalType::Voiced)]
            [usize::from(p.offset)];
        for (i, (&sample, pulse)) in input.iter().zip(pulses).enumerate() {
            self.random = self.random.wrapping_mul(196314165).wrapping_add(907633515);
            let mut short = (order / 2) as i32;
            for (j, &coef) in predictor[..order].iter().enumerate() {
                short = short.wrapping_add(mul_wb(self.lpc[15 + i - j], i32::from(coef)));
            }
            let mut long = 0i32;
            if p.signal == SignalType::Voiced {
                long = 2;
                for j in 0..5 {
                    long = long.wrapping_add(mul_wb(
                        ltp[self.ltp_index - lag + 2 - j],
                        i32::from(p.ltp_q14[k][j]),
                    ));
                }
            }
            let mut ar = (usize::from(p.shaping_order) / 2) as i32;
            let mut previous = self.difference;
            for j in 0..usize::from(p.shaping_order) {
                let old = self.ar[j];
                self.ar[j] = previous;
                ar = ar.wrapping_add(mul_wb(previous, i32::from(p.shaping_q13[k][j])));
                previous = old;
            }
            ar = ar
                .wrapping_shl(1)
                .wrapping_add(mul_wb(self.low_frequency, i32::from(p.tilt_q14[k])));
            let low = mul_wb(
                self.shaping_ltp[self.shaping_index - 1],
                p.low_frequency_q14[k],
            )
            .wrapping_add(mul_wt(self.low_frequency, p.low_frequency_q14[k]));
            let mut prediction = short.wrapping_shl(2).wrapping_sub(ar).wrapping_sub(low);
            if lag > 0 {
                let at = self.shaping_index - lag + 1;
                let harmonic = mul_wb(
                    self.shaping_ltp[at].saturating_add(self.shaping_ltp[at - 2]),
                    packed,
                )
                .wrapping_add(mul_wt(self.shaping_ltp[at - 1], packed))
                .wrapping_shl(1);
                prediction = round_shift(
                    long.wrapping_sub(harmonic)
                        .wrapping_add(prediction.wrapping_shl(1)),
                    3,
                );
            } else {
                prediction = round_shift(prediction, 2);
            }
            let mut residual = sample.wrapping_sub(prediction);
            if self.random < 0 {
                residual = residual.wrapping_neg();
            }
            residual = residual.clamp(-(31 << 10), 30 << 10);
            let q = quantize_level(residual, offset, i32::from(p.lambda_q10));
            *pulse = round_shift(q, 10) as i8;
            let mut excitation = q.wrapping_shl(4);
            if self.random < 0 {
                excitation = excitation.wrapping_neg();
            }
            let excitation = excitation.wrapping_add(long.wrapping_shl(1));
            let reconstructed = excitation.wrapping_add(short.wrapping_shl(4));
            self.xq[self.shaping_index] =
                saturate16(round_shift(mul_ww(reconstructed, p.gains_q16[k] >> 6), 8));
            self.lpc[16 + i] = reconstructed;
            self.difference = reconstructed.wrapping_sub(sample.wrapping_shl(4));
            self.low_frequency = self.difference.wrapping_sub(ar.wrapping_shl(2));
            self.shaping_ltp[self.shaping_index] =
                self.low_frequency.wrapping_sub(low.wrapping_shl(2));
            ltp[self.ltp_index] = excitation.wrapping_shl(1);
            self.shaping_index += 1;
            self.ltp_index += 1;
            self.random = self.random.wrapping_add(i32::from(*pulse));
        }
        self.lpc.copy_within(input.len()..input.len() + 16, 0);
    }
}
fn mul_wt(a: i32, b: i32) -> i32 {
    ((i64::from(a) * i64::from(b >> 16)) >> 16) as i32
}
fn mul_bb(a: i32, b: i32) -> i32 {
    i32::from(a as i16) * i32::from(b as i16)
}
fn candidates(residual: i32, offset: i32, lambda: i32) -> [(i32, i32); 2] {
    let value = residual - offset;
    let mut level = value >> 10;
    if lambda > 2048 {
        let bias = lambda / 2 - 512;
        level = if value > bias {
            (value - bias) >> 10
        } else if value < -bias {
            (value + bias) >> 10
        } else if value < 0 {
            -1
        } else {
            0
        };
    }
    let (q1, q2, rate1, rate2) = if level > 0 {
        let q = (level << 10) - 80 + offset;
        (q, q + 1024, mul_bb(q, lambda), mul_bb(q + 1024, lambda))
    } else if level == 0 {
        (
            offset,
            offset + 944,
            mul_bb(offset, lambda),
            mul_bb(offset + 944, lambda),
        )
    } else if level == -1 {
        (
            offset - 944,
            offset,
            mul_bb(944 - offset, lambda),
            mul_bb(offset, lambda),
        )
    } else {
        let q = (level << 10) + 80 + offset;
        (q, q + 1024, mul_bb(-q, lambda), mul_bb(-q - 1024, lambda))
    };
    let error1 = residual - q1;
    let error2 = residual - q2;
    let cost1 = rate1.wrapping_add(mul_bb(error1, error1));
    let cost2 = rate2.wrapping_add(mul_bb(error2, error2));
    [(q1, cost1), (q2, cost2)]
}
fn quantize_level(residual: i32, offset: i32, lambda: i32) -> i32 {
    let [first, second] = candidates(residual, offset, lambda);
    if second.1 < first.1 {
        second.0
    } else {
        first.0
    }
}
fn analysis_filter(output: &mut [i16], input: &[i16], coefficients: &[i16]) {
    let order = coefficients.len();
    output[..order].fill(0);
    for i in order..input.len() {
        let mut sum = 0i32;
        for (j, &coefficient) in coefficients.iter().enumerate() {
            sum = sum.wrapping_add(i32::from(input[i - j - 1]) * i32::from(coefficient));
        }
        output[i] = saturate16(round_shift(
            (i32::from(input[i]) << 12).wrapping_sub(sum),
            12,
        ));
    }
}

#[cfg(test)]
#[path = "nsq_tests.rs"]
mod tests;
