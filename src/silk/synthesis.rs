//! Loss-free single-channel SILK LTP/LPC synthesis at the internal sample rate.
//!
//! Adapted from Opus v1.6.1 decode_core, LPC_analysis_filter and Inlines.h;
//! see THIRD_PARTY_NOTICES.md. No PLC, comfort noise, stereo or resampling.
use super::{
    excitation::FrameExcitation,
    gains::{FrameGains, SignalType},
    nlsf::FrameNlsf,
    pitch::{FramePitch, SilkRate},
};
use std::fmt;

/// Incompatible decoded parameters; synthesis history remains unchanged.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum SynthesisError {
    /// Rate, frame length, filter order, signal type or subframe counts disagree.
    IncompatibleFrame,
}
impl fmt::Display for SynthesisError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("incompatible SILK synthesis parameters")
    }
}
impl std::error::Error for SynthesisError {}

/// One channel of signed 16-bit PCM at the internal SILK rate.
///
/// Direct synthesis returns samples before loss-processing glue. Channel loss
/// recovery additionally applies PLC/CNG; stereo and resampling are separate.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PcmFrame {
    pub(super) samples: [i16; 320],
    pub(super) length: usize,
    pub(super) rate: SilkRate,
}
impl PcmFrame {
    /// Samples at the internal SILK rate, not the Opus output rate.
    pub fn samples(&self) -> &[i16] {
        &self.samples[..self.length]
    }
    /// Internal sampling rate of these samples.
    pub fn rate(&self) -> SilkRate {
        self.rate
    }
}

/// Persistent synthesis history for one internal SILK channel.
///
/// Use separate state per channel. This stage accepts only loss-free frames;
/// callers must not represent a lost frame as a zero excitation frame.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SynthesisState {
    rate: Option<SilkRate>,
    pub(super) lpc: [i32; 16],
    pub(super) history: [i16; 320],
    pub(super) previous_gain: i32,
    pub(super) last_filter: [i16; 16],
    pub(super) after_loss: bool,
    pub(super) recovery_lag: Option<u16>,
}
impl Default for SynthesisState {
    fn default() -> Self {
        Self {
            rate: None,
            lpc: [0; 16],
            history: [0; 320],
            previous_gain: 65536,
            last_filter: [0; 16],
            after_loss: false,
            recovery_lag: None,
        }
    }
}

// Internal view of already validated geometry. Reference tests also exercise
// the arithmetic directly with independent coefficients and excitation.
struct Parameters<'a> {
    rate: SilkRate,
    gains: &'a [i32],
    filters: [&'a [i16]; 2],
    lags: &'a [u16],
    ltp: &'a [[i16; 5]],
    scale: i16,
    voiced: bool,
    interpolate: bool,
    excitation: &'a [i32],
}
impl SynthesisState {
    /// Forget all history, including the previous gain and rate.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Synthesize decoded parameters from the same internal frame and channel.
    ///
    /// Validates geometry and voiced pitch consistency before changing state.
    /// A rate change clears PCM/LPC history while retaining the prior gain, as
    /// in the reference decoder. NLSF-to-LPC conversion is included. This method
    /// cannot prove that separately decoded parameters came from the same data;
    /// the caller is responsible for their entropy ordering and provenance.
    /// Failure leaves this state unchanged. Complete decoder orchestration must
    /// also transact gain, NLSF, pitch and entropy state around this operation.
    pub fn synthesize(
        &mut self,
        rate: SilkRate,
        gains: &FrameGains,
        nlsf: &FrameNlsf,
        pitch: &FramePitch,
        excitation: &FrameExcitation,
    ) -> Result<PcmFrame, SynthesisError> {
        let khz = rate.khz() as usize;
        let count = gains.gains_q16().len();
        let voiced = gains.signal_type() == SignalType::Voiced;
        if !matches!(count, 2 | 4)
            || gains.gains_q16().iter().any(|&gain| gain <= 0)
            || pitch.lags().len() != count
            || pitch.coefficients_q14().len() != count
            || nlsf.current_q15().len() != if rate == SilkRate::Khz16 { 16 } else { 10 }
            || (count == 2 && nlsf.interpolation_q2() != 4)
            || excitation.samples_q14().len() != khz * 5 * count
            || voiced != pitch.lag_index().is_some()
            || (voiced
                && pitch
                    .lags()
                    .iter()
                    .any(|&lag| !(2 * khz..=18 * khz).contains(&usize::from(lag))))
        {
            return Err(SynthesisError::IncompatibleFrame);
        }
        let mut lpc = nlsf.to_lpc();
        if self.after_loss {
            lpc.expand_after_loss();
        }
        Ok(self.apply(Parameters {
            rate,
            gains: gains.gains_q16(),
            filters: [lpc.first_half_q12(), lpc.current_q12()],
            lags: pitch.lags(),
            ltp: pitch.coefficients_q14(),
            scale: pitch.scale_q14(),
            voiced,
            interpolate: nlsf.interpolation_q2() < 4,
            excitation: excitation.samples_q14(),
        }))
    }

    pub(super) fn prepare_rate(&mut self, rate: SilkRate) {
        if self.rate != Some(rate) {
            self.lpc.fill(0);
            self.history.fill(0);
            self.recovery_lag = None;
            self.rate = Some(rate);
        }
    }

    fn apply(&mut self, p: Parameters<'_>) -> PcmFrame {
        let subframe = p.rate.khz() as usize * 5;
        let memory = subframe * 4;
        let length = p.excitation.len();
        self.prepare_rate(p.rate);
        let mut output = PcmFrame {
            samples: [0; 320],
            length,
            rate: p.rate,
        };
        let mut history = [0i16; 480];
        history[..memory].copy_from_slice(&self.history[..memory]);
        let mut ltp = [0i32; 640];
        let mut short = [0i32; 96];
        short[..16].copy_from_slice(&self.lpc);
        let mut residual = [0i32; 80];
        let mut cursor = memory;
        for (k, &gain) in p.gains.iter().enumerate() {
            let filter = p.filters[k / 2];
            let gain_adjustment = if gain == self.previous_gain {
                65536
            } else {
                divide_gain(self.previous_gain, gain)
            };
            if gain_adjustment != 65536 {
                for sample in &mut short[..16] {
                    *sample = mul_ww(gain_adjustment, *sample);
                }
            }
            self.previous_gain = gain;
            let recovery = self.after_loss && !p.voiced && k < 2;
            let recovery_lag = if recovery { self.recovery_lag } else { None };
            if p.voiced || recovery_lag.is_some() {
                let lag = usize::from(recovery_lag.unwrap_or(p.lags[k]));
                let coefficients = if recovery_lag.is_some() {
                    &[0, 0, 4096, 0, 0]
                } else {
                    &p.ltp[k]
                };
                if k == 0 || (k == 2 && p.interpolate) {
                    if k == 2 {
                        history[memory..memory + 2 * subframe]
                            .copy_from_slice(&output.samples[..2 * subframe]);
                    }
                    let mut inverse = inverse_gain(gain, 47);
                    if k == 0 {
                        inverse = mul_wb(inverse, i32::from(p.scale)).wrapping_shl(2);
                    }
                    // Rewhitening is an FIR over PCM history. Only lag + 2
                    // residuals feed LTP; no intermediate whitening buffer is needed.
                    // The validated lag leaves at least filter.len() prior samples.
                    for i in 0..lag + 2 {
                        let at = memory - lag - 2 + i + k * subframe;
                        let mut prediction = 0i32;
                        for (j, &coefficient) in filter.iter().enumerate() {
                            prediction = prediction.wrapping_add(
                                i32::from(history[at - j - 1]) * i32::from(coefficient),
                            );
                        }
                        let residual = (i32::from(history[at]) << 12).wrapping_sub(prediction);
                        let whitened = saturate16(round_shift(residual, 12));
                        ltp[cursor - lag - 2 + i] = mul_wb(inverse, i32::from(whitened));
                    }
                } else if gain_adjustment != 65536 {
                    for i in 0..lag + 2 {
                        ltp[cursor - i - 1] = mul_ww(gain_adjustment, ltp[cursor - i - 1]);
                    }
                }
                for (i, sample) in residual[..subframe].iter_mut().enumerate() {
                    let mut prediction = 2i32;
                    for (j, &coefficient) in coefficients.iter().enumerate() {
                        prediction = prediction.wrapping_add(mul_wb(
                            ltp[cursor - lag + 2 - j],
                            i32::from(coefficient),
                        ));
                    }
                    *sample =
                        p.excitation[k * subframe + i].wrapping_add(prediction.wrapping_shl(1));
                    ltp[cursor] = sample.wrapping_shl(1);
                    cursor += 1;
                }
            } else {
                residual[..subframe]
                    .copy_from_slice(&p.excitation[k * subframe..(k + 1) * subframe]);
            }
            let residual = &residual[..subframe];
            if filter.len() == 16 {
                synthesize_lpc::<16>(&mut short, residual, filter);
            } else {
                synthesize_lpc::<10>(&mut short, residual, filter);
            }
            let pcm = &mut output.samples[k * subframe..(k + 1) * subframe];
            for (sample, &value) in pcm.iter_mut().zip(&short[16..]) {
                *sample = saturate16(round_shift(mul_ww(value, gain >> 6), 8));
            }
            short.copy_within(subframe..subframe + 16, 0);
        }
        self.lpc.copy_from_slice(&short[..16]);
        self.last_filter.fill(0);
        self.last_filter[..p.filters[1].len()].copy_from_slice(p.filters[1]);
        self.after_loss = false;
        self.recovery_lag = None;
        self.history.copy_within(length..memory, 0);
        self.history[memory - length..memory].copy_from_slice(output.samples());
        output
    }
}

fn synthesize_lpc<const ORDER: usize>(short: &mut [i32; 96], residual: &[i32], filter: &[i16]) {
    let coefficients: [i32; ORDER] = std::array::from_fn(|j| i32::from(filter[j]));
    // pending[j] holds the contribution of past samples to output j steps ahead.
    // Shift each product to Q10 before accumulating it, as in the direct LPC sum.
    let mut pending = [0i32; ORDER];
    let push_sample = |pending: &mut [i32; ORDER], value| {
        for j in 0..ORDER - 1 {
            pending[j] = pending[j + 1].wrapping_add(mul_wb(value, coefficients[j]));
        }
        pending[ORDER - 1] = mul_wb(value, coefficients[ORDER - 1]);
    };
    for &value in &short[16 - ORDER..16] {
        push_sample(&mut pending, value);
    }
    for (i, &sample) in residual.iter().enumerate() {
        let prediction = pending[0].wrapping_add((ORDER / 2) as i32);
        let value = sample.saturating_add(shift_saturated(prediction, 4));
        push_sample(&mut pending, value);
        short[16 + i] = value;
    }
}

pub(super) fn mul_wb(a: i32, b: i32) -> i32 {
    ((i64::from(a) * i64::from(b as i16)) >> 16) as i32
}
pub(super) fn mul_ww(a: i32, b: i32) -> i32 {
    ((i64::from(a) * i64::from(b)) >> 16) as i32
}
pub(crate) fn round_shift(value: i32, bits: u32) -> i32 {
    ((i64::from(value) + (1i64 << (bits - 1))) >> bits) as i32
}
pub(super) fn saturate16(value: i32) -> i16 {
    value.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16
}
pub(super) fn shift_saturated(value: i32, bits: u32) -> i32 {
    value.clamp(i32::MIN >> bits, i32::MAX >> bits) << bits
}

// Reference reciprocal refinement: ordinary integer division is not bit-exact.
pub(super) fn inverse_gain(denominator: i32, fraction_bits: u32) -> i32 {
    let headroom = denominator.leading_zeros() - 1;
    let normalized = denominator << headroom;
    let inverse = (i32::MAX >> 2) / (normalized >> 16);
    let initial = inverse << 16;
    let error = ((1 << 29) - mul_wb(normalized, inverse)).wrapping_shl(3);
    let result = initial.wrapping_add(mul_ww(error, inverse));
    let shift = 61 - headroom as i32 - fraction_bits as i32;
    if shift <= 0 {
        shift_saturated(result, (-shift) as u32)
    } else {
        result >> shift
    }
}
pub(super) fn divide_gain(numerator: i32, denominator: i32) -> i32 {
    let a_head = numerator.leading_zeros() - 1;
    let b_head = denominator.leading_zeros() - 1;
    let a = numerator << a_head;
    let b = denominator << b_head;
    let inverse = (i32::MAX >> 2) / (b >> 16);
    let initial = mul_wb(a, inverse);
    let residual =
        a.wrapping_sub((((i64::from(b) * i64::from(initial)) >> 32) as i32).wrapping_shl(3));
    let result = initial.wrapping_add(mul_wb(residual, inverse));
    let shift = 13 + a_head as i32 - b_head as i32;
    if shift < 0 {
        shift_saturated(result, (-shift) as u32)
    } else if shift < 32 {
        result >> shift
    } else {
        0
    }
}

#[cfg(test)]
#[path = "synthesis_tests.rs"]
mod tests;
