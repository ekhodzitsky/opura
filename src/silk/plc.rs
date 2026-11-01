//! Fixed-point SILK packet-loss concealment and energy glue.
//!
//! Adapted from Opus v1.6.1 silk/PLC.c, sum_sqr_shift.c and bwexpander.c;
//! see THIRD_PARTY_NOTICES.md. Neural PLC is outside this implementation.
use super::{
    cng::sqrt_approx,
    excitation::FrameExcitation,
    gains::{FrameDuration, FrameGains, SignalType},
    lpc::{expand_i16, inverse_gain as lpc_inverse_gain},
    pitch::{FramePitch, SilkRate},
    synthesis::{
        PcmFrame, SynthesisState, inverse_gain, mul_wb, mul_ww, round_shift, saturate16,
        shift_saturated,
    },
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct PlcState {
    rate: Option<SilkRate>,
    pitch_q8: i32,
    ltp: [i16; 5],
    filter: [i16; 16],
    last_lost: bool,
    seed: i32,
    pub(super) noise_scale: i16,
    energy: i32,
    energy_shift: u32,
    ltp_scale: i16,
    pub(super) gains: [i32; 2],
    count: usize,
    subframe: usize,
    excitation: [i32; 320],
    signal: SignalType,
    pub(super) losses: u32,
    first: bool,
}
impl Default for PlcState {
    fn default() -> Self {
        Self {
            rate: None,
            pitch_q8: 0,
            ltp: [0; 5],
            filter: [0; 16],
            last_lost: false,
            seed: 0,
            noise_scale: 0,
            energy: 0,
            energy_shift: 0,
            ltp_scale: 0,
            gains: [65536; 2],
            count: 2,
            subframe: 20,
            excitation: [0; 320],
            signal: SignalType::Inactive,
            losses: 0,
            first: true,
        }
    }
}
impl PlcState {
    fn prepare(&mut self, rate: SilkRate, length: usize) {
        if self.rate != Some(rate) {
            self.pitch_q8 = (length as i32) << 7;
            self.gains = [65536; 2];
            self.subframe = 20;
            self.count = 2;
            self.signal = SignalType::Inactive;
            self.first = true;
            self.rate = Some(rate);
        }
    }

    // All components and last_filter have passed channel synthesis validation.
    pub(super) fn observe(
        &mut self,
        rate: SilkRate,
        gains: &FrameGains,
        pitch: &FramePitch,
        excitation: &FrameExcitation,
        last_filter: &[i16; 16],
    ) {
        self.prepare(rate, excitation.samples_q14().len());
        self.signal = gains.signal_type();
        let count = gains.gains_q16().len();
        let subframe = rate.khz() as usize * 5;
        let mut ltp_gain = 0;
        if self.signal == SignalType::Voiced {
            for j in 0..count {
                if j * subframe >= usize::from(pitch.lags()[count - 1]) {
                    break;
                }
                let index = count - 1 - j;
                let gain = pitch.coefficients_q14()[index]
                    .iter()
                    .map(|&v| i32::from(v))
                    .sum::<i32>();
                if gain > ltp_gain {
                    ltp_gain = gain;
                    self.pitch_q8 = i32::from(pitch.lags()[index]) << 8;
                }
            }
            self.ltp = [0, 0, ltp_gain as i16, 0, 0];
            if ltp_gain < 11469 {
                let scale = (11469 << 10) / ltp_gain.max(1);
                for coefficient in &mut self.ltp {
                    *coefficient = ((i32::from(*coefficient) * scale) >> 10) as i16;
                }
            } else if ltp_gain > 15565 {
                let scale = (15565 << 14) / ltp_gain;
                for coefficient in &mut self.ltp {
                    *coefficient = ((i32::from(*coefficient) * scale) >> 14) as i16;
                }
            }
        } else {
            self.pitch_q8 = (rate.khz() * 18) << 8;
            self.ltp.fill(0);
        }
        let order = if rate == SilkRate::Khz16 { 16 } else { 10 };
        self.filter[..order].copy_from_slice(&last_filter[..order]);
        self.ltp_scale = pitch.scale_q14();
        self.gains.copy_from_slice(&gains.gains_q16()[count - 2..]);
        self.subframe = subframe;
        self.count = count;
        self.excitation[..excitation.samples_q14().len()].copy_from_slice(excitation.samples_q14());
        self.losses = 0;
        self.first = false;
    }

    pub(super) fn conceal(
        &mut self,
        rate: SilkRate,
        duration: FrameDuration,
        synthesis: &mut SynthesisState,
    ) -> PcmFrame {
        let count = if duration == FrameDuration::Ms10 {
            2
        } else {
            4
        };
        let subframe = rate.khz() as usize * 5;
        let length = count * subframe;
        let memory = rate.khz() as usize * 20;
        let order = if rate == SilkRate::Khz16 { 16 } else { 10 };
        self.prepare(rate, length);
        synthesis.prepare_rate(rate);
        if self.first {
            self.filter.fill(0);
        }
        let gains = [self.gains[0] >> 6, self.gains[1] >> 6];
        // The energy comparison uses current geometry, while the random bank
        // starts use the last good geometry saved by PLC_update (also after
        // duration changes). Both are intentional reference behaviors.
        let mut energies = [(0, 0); 2];
        let mut scaled = [0i16; 80];
        for k in 0..2 {
            for (i, sample) in scaled[..subframe].iter_mut().enumerate() {
                *sample = saturate16(
                    mul_ww(self.excitation[i + (k + count - 2) * subframe], gains[k]) >> 8,
                );
            }
            energies[k] = sum_squares(&scaled[..subframe]);
        }
        let first_energy_lower =
            (energies[0].0 >> energies[1].1) < (energies[1].0 >> energies[0].1);
        let bank_end = if first_energy_lower {
            (self.count - 1) * self.subframe
        } else {
            self.count * self.subframe
        };
        let bank_start = bank_end.saturating_sub(128);
        let attenuation = usize::from(self.losses != 0);
        let harmonic_gain = [32440, 31130][attenuation];
        let mut random_gain = if self.signal == SignalType::Voiced {
            [31130, 26214][attenuation]
        } else {
            [32440, 29491][attenuation]
        };
        expand_i16(&mut self.filter[..order], 64881);
        let mut noise_scale = i32::from(self.noise_scale);
        if self.losses == 0 {
            noise_scale = 16384;
            if self.signal == SignalType::Voiced {
                for &coefficient in &self.ltp {
                    noise_scale -= i32::from(coefficient);
                }
                noise_scale = noise_scale.max(3277);
                noise_scale = (noise_scale * i32::from(self.ltp_scale)) >> 14;
            } else {
                let inverse = lpc_inverse_gain(&self.filter[..order]);
                let down_scale = inverse.clamp((1 << 30) >> 8, (1 << 30) >> 3) << 3;
                random_gain = mul_wb(down_scale, random_gain) >> 14;
            }
        }
        let mut lag = round_shift(self.pitch_q8, 8) as usize;
        let start = memory - lag - order - 2;
        let mut ltp = [0i32; 640];
        let inverse = inverse_gain(self.gains[1], 46).min(i32::MAX >> 1);
        for (i, sample) in synthesis.history[..memory]
            .iter()
            .enumerate()
            .skip(start + order)
        {
            let mut prediction = 0i32;
            for (j, &coefficient) in self.filter[..order].iter().enumerate() {
                prediction = prediction
                    .wrapping_add(i32::from(synthesis.history[i - j - 1]) * i32::from(coefficient));
            }
            let residual = (i32::from(*sample) << 12).wrapping_sub(prediction);
            ltp[i] = mul_wb(inverse, i32::from(saturate16(round_shift(residual, 12))));
        }
        let mut cursor = memory;
        for _ in 0..count {
            for _ in 0..subframe {
                let mut prediction = 2i32;
                for (j, &coefficient) in self.ltp.iter().enumerate() {
                    prediction = prediction
                        .wrapping_add(mul_wb(ltp[cursor - lag + 2 - j], i32::from(coefficient)));
                }
                self.seed = self.seed.wrapping_mul(196314165).wrapping_add(907633515);
                let index = ((self.seed >> 25) & 127) as usize;
                ltp[cursor] = prediction
                    .wrapping_add(mul_wb(self.excitation[bank_start + index], noise_scale))
                    .wrapping_shl(2);
                cursor += 1;
            }
            for coefficient in &mut self.ltp {
                *coefficient = ((harmonic_gain * i32::from(*coefficient)) >> 15) as i16;
            }
            noise_scale = (noise_scale * random_gain) >> 15;
            self.pitch_q8 += mul_wb(self.pitch_q8, 655);
            self.pitch_q8 = self.pitch_q8.min((rate.khz() * 18) << 8);
            lag = round_shift(self.pitch_q8, 8) as usize;
        }
        let mut output = PcmFrame {
            samples: [0; 320],
            length,
            rate,
        };
        ltp[memory - 16..memory].copy_from_slice(&synthesis.lpc);
        for i in 0..length {
            let mut prediction = (order / 2) as i32;
            for (j, &coefficient) in self.filter[..order].iter().enumerate() {
                prediction = prediction
                    .wrapping_add(mul_wb(ltp[memory + i - j - 1], i32::from(coefficient)));
            }
            ltp[memory + i] = ltp[memory + i].saturating_add(shift_saturated(prediction, 4));
            output.samples[i] = saturate16(round_shift(mul_ww(ltp[memory + i], gains[1]), 8));
        }
        synthesis
            .lpc
            .copy_from_slice(&ltp[memory + length - 16..memory + length]);
        synthesis.history.copy_within(length..memory, 0);
        synthesis.history[memory - length..memory].copy_from_slice(output.samples());
        synthesis.after_loss = true;
        synthesis.recovery_lag = if self.signal == SignalType::Voiced {
            Some(lag as u16)
        } else {
            None
        };
        self.noise_scale = noise_scale as i16;
        self.losses = self.losses.saturating_add(1);
        output
    }

    // Run after CNG on both losses and good frames. This alters output, not the
    // PCM history used by subsequent synthesis, matching decode_frame ordering.
    pub(super) fn glue(&mut self, pcm: &mut [i16]) {
        if self.losses != 0 {
            (self.energy, self.energy_shift) = sum_squares(pcm);
            self.last_lost = true;
        } else {
            if self.last_lost {
                let (mut energy, shift) = sum_squares(pcm);
                if shift > self.energy_shift {
                    self.energy >>= shift - self.energy_shift;
                } else {
                    energy >>= self.energy_shift - shift;
                }
                if energy > self.energy {
                    let leading = self.energy.leading_zeros() - 1;
                    self.energy <<= leading;
                    energy >>= 24u32.saturating_sub(leading);
                    let fraction = self.energy / energy.max(1);
                    let mut gain = sqrt_approx(fraction) << 4;
                    let slope = ((65536 - gain) / pcm.len() as i32) << 2;
                    for sample in pcm {
                        *sample = mul_wb(gain, i32::from(*sample)) as i16;
                        gain += slope;
                        if gain > 65536 {
                            break;
                        }
                    }
                }
            }
            self.last_lost = false;
        }
    }
}

// Pairwise rounding in both passes matters; summing in i64 and shifting once
// gives different PLC energy comparisons and recovery fade-in gains.
fn sum_squares(samples: &[i16]) -> (i32, u32) {
    let initial_shift = 31 - (samples.len() as u32).leading_zeros();
    let sum = |shift: u32| -> i32 {
        samples
            .chunks(2)
            .map(|pair| {
                let mut energy = (i32::from(pair[0]) * i32::from(pair[0])) as u32;
                if pair.len() == 2 {
                    energy += (i32::from(pair[1]) * i32::from(pair[1])) as u32;
                }
                (energy >> shift) as i32
            })
            .sum()
    };
    let conservative = samples.len() as i32 + sum(initial_shift);
    let shift = (initial_shift + 3).saturating_sub(conservative.leading_zeros());
    (sum(shift), shift)
}

#[cfg(test)]
impl PlcState {
    pub(super) fn reference_state(&self) -> Vec<i32> {
        let mut out = vec![self.pitch_q8];
        out.extend(self.ltp.map(i32::from));
        out.extend(self.filter.map(i32::from));
        out.extend([
            i32::from(self.last_lost),
            self.seed,
            i32::from(self.noise_scale),
            self.energy,
            self.energy_shift as i32,
            i32::from(self.ltp_scale),
        ]);
        out.extend(self.gains);
        out.extend([
            self.rate.map_or(0, SilkRate::khz),
            self.count as i32,
            self.subframe as i32,
            match self.signal {
                SignalType::Inactive => 0,
                SignalType::Unvoiced => 1,
                SignalType::Voiced => 2,
            },
            self.losses as i32,
            i32::from(self.first),
        ]);
        out.extend(self.excitation);
        out
    }
}
