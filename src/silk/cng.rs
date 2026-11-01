//! SILK comfort-noise estimation and mixing into concealed internal frames.
//!
//! Adapted from Opus v1.6.1 silk/CNG.c and Inlines.h; see
//! THIRD_PARTY_NOTICES.md. This is the CNG stage, not a PLC implementation.
use super::{
    excitation::FrameExcitation,
    gains::{FrameDuration, FrameGains, SignalType},
    lpc,
    nlsf::FrameNlsf,
    pitch::SilkRate,
    synthesis::{mul_wb, mul_ww, round_shift, saturate16, shift_saturated},
};
use std::fmt;

/// Invalid CNG input; history and output remain unchanged.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum CngError {
    /// Rate, LPC order, excitation length or gain geometry disagree.
    IncompatibleFrame,
    /// PLC noise scale must be 0..=16384 and previous Q16 gain positive.
    InvalidNoiseContext,
}
impl fmt::Display for CngError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::IncompatibleFrame => "incompatible SILK CNG frame geometry",
            Self::InvalidNoiseContext => "invalid SILK CNG PLC noise context",
        })
    }
}
impl std::error::Error for CngError {}

/// Persistent comfort-noise history for one internal SILK channel.
///
/// Observe every successfully decoded frame, including active frames. Then mix
/// into PLC output on each loss. This state alone cannot conceal speech or
/// restore the channel's prediction history after a loss.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CngState {
    rate: Option<SilkRate>,
    excitation: [i32; 320],
    spectrum: [i16; 16],
    synthesis: [i32; 16],
    gain: i32,
    seed: i32,
}
impl Default for CngState {
    fn default() -> Self {
        Self {
            rate: None,
            excitation: [0; 320],
            spectrum: [0; 16],
            synthesis: [0; 16],
            gain: 0,
            seed: 3176576,
        }
    }
}
impl CngState {
    /// Clear all estimation and synthesis history before a new stream.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Observe a good frame from the same rate/channel/entropy stream.
    ///
    /// Inactive frames update the smoothed spectrum, gain and excitation bank.
    /// Active frames retain these estimates. Both clear CNG synthesis history.
    /// Failure leaves this state unchanged. An enclosing frame transaction must
    /// commit this state together with channel decoding and PLC state.
    pub fn update(
        &mut self,
        rate: SilkRate,
        gains: &FrameGains,
        nlsf: &FrameNlsf,
        excitation: &FrameExcitation,
    ) -> Result<(), CngError> {
        let count = gains.gains_q16().len();
        let order = order(rate);
        if !matches!(count, 2 | 4)
            || gains.gains_q16().iter().any(|&gain| gain <= 0)
            || nlsf.current_q15().len() != order
            || excitation.samples_q14().len() != rate.khz() as usize * 5 * count
        {
            return Err(CngError::IncompatibleFrame);
        }
        self.observe(
            rate,
            gains.gains_q16(),
            nlsf.current_q15(),
            excitation.samples_q14(),
            gains.signal_type() == SignalType::Inactive,
        );
        Ok(())
    }

    /// Mix CNG into one 10/20 ms frame already produced by PLC, in place.
    ///
    /// `noise_scale_q14` and `previous_gain_q16` are the PLC random-noise scale
    /// after concealment and its last good subframe gain. They are not user
    /// volume controls. This stage subtracts the noise energy already supplied
    /// by PLC from the estimated background energy, filters random samples from
    /// its excitation bank, and adds with signed-16 saturation.
    ///
    /// Rate changes reset estimates and seed, retaining the excitation bank and
    /// filter memory as upstream does. Invalid inputs change neither history nor
    /// samples. Successful calls use fixed buffers and bounded loops.
    pub fn mix(
        &mut self,
        rate: SilkRate,
        duration: FrameDuration,
        noise_scale_q14: u16,
        previous_gain_q16: i32,
        pcm: &mut [i16],
    ) -> Result<(), CngError> {
        let length = rate.khz() as usize
            * if duration == FrameDuration::Ms10 {
                10
            } else {
                20
            };
        if pcm.len() != length {
            return Err(CngError::IncompatibleFrame);
        }
        if noise_scale_q14 > 16384 || previous_gain_q16 <= 0 {
            return Err(CngError::InvalidNoiseContext);
        }
        self.prepare(rate);
        self.add_noise(rate, i32::from(noise_scale_q14), previous_gain_q16, pcm);
        Ok(())
    }

    fn prepare(&mut self, rate: SilkRate) {
        if self.rate != Some(rate) {
            let count = order(rate);
            let step = 32767 / (count + 1) as i16;
            for (i, frequency) in self.spectrum[..count].iter_mut().enumerate() {
                *frequency = step * (i + 1) as i16;
            }
            self.gain = 0;
            self.seed = 3176576;
            self.rate = Some(rate);
        }
    }

    // Geometry is checked by update; oracle tests use independent literal data.
    fn observe(
        &mut self,
        rate: SilkRate,
        gains: &[i32],
        spectrum: &[i16],
        excitation: &[i32],
        inactive: bool,
    ) {
        self.prepare(rate);
        let count = order(rate);
        if inactive {
            for (smoothed, &current) in self.spectrum[..count].iter_mut().zip(spectrum) {
                *smoothed = (i32::from(*smoothed)
                    + mul_wb(i32::from(current) - i32::from(*smoothed), 16348))
                    as i16;
            }
            // Strict comparison retains the first subframe in a gain tie.
            let mut loudest = 0;
            for i in 1..gains.len() {
                if gains[i] > gains[loudest] {
                    loudest = i;
                }
            }
            let subframe = rate.khz() as usize * 5;
            self.excitation
                .copy_within(0..(gains.len() - 1) * subframe, subframe);
            self.excitation[..subframe]
                .copy_from_slice(&excitation[loudest * subframe..(loudest + 1) * subframe]);
            for &gain in gains {
                self.gain += mul_wb(gain - self.gain, 4634);
                if mul_ww(self.gain, 46396) > gain {
                    self.gain = gain;
                }
            }
        }
        self.synthesis[..count].fill(0);
    }

    fn add_noise(&mut self, rate: SilkRate, noise_scale: i32, previous_gain: i32, pcm: &mut [i16]) {
        let mut gain = mul_ww(noise_scale, previous_gain);
        gain = if gain >= 1 << 21 || self.gain > 1 << 23 {
            let noise_energy = (gain >> 16) * (gain >> 16);
            let energy =
                ((self.gain >> 16) * (self.gain >> 16)).wrapping_sub(noise_energy.wrapping_shl(5));
            sqrt_approx(energy).wrapping_shl(16)
        } else {
            let noise_energy = mul_ww(gain, gain);
            let energy = mul_ww(self.gain, self.gain).wrapping_sub(noise_energy.wrapping_shl(5));
            sqrt_approx(energy).wrapping_shl(8)
        };
        let gain_q10 = gain >> 6;
        let count = order(rate);
        let filter = lpc::convert(&self.spectrum[..count]);
        let mut mask = 255;
        while mask > pcm.len() {
            mask >>= 1;
        }
        let mut signal = [0i32; 336];
        signal[..16].copy_from_slice(&self.synthesis);
        for (i, sample) in pcm.iter_mut().enumerate() {
            self.seed = self.seed.wrapping_mul(196314165).wrapping_add(907633515);
            let index = ((self.seed >> 24) as usize) & mask;
            let mut prediction = (count / 2) as i32;
            for (j, &coefficient) in filter[..count].iter().enumerate() {
                prediction =
                    prediction.wrapping_add(mul_wb(signal[16 + i - j - 1], i32::from(coefficient)));
            }
            signal[16 + i] = self.excitation[index].saturating_add(shift_saturated(prediction, 4));
            *sample =
                sample.saturating_add(saturate16(round_shift(mul_ww(signal[16 + i], gain_q10), 8)));
        }
        self.synthesis
            .copy_from_slice(&signal[pcm.len()..pcm.len() + 16]);
    }
}
fn order(rate: SilkRate) -> usize {
    if rate == SilkRate::Khz16 { 16 } else { 10 }
}

// Matches the bounded CLZ/fraction approximation in SILK Inlines.h, including
// returning zero for negative residual energy rather than taking its absolute.
pub(super) fn sqrt_approx(value: i32) -> i32 {
    if value <= 0 {
        return 0;
    }
    let leading = value.leading_zeros();
    let fraction = (value as u32).rotate_right(24u32.wrapping_sub(leading)) & 127;
    let initial = (if leading & 1 != 0 { 32768 } else { 46214 }) >> (leading / 2);
    initial + mul_wb(initial, 213 * fraction as i32)
}

#[cfg(test)]
#[path = "cng_tests.rs"]
mod tests;
