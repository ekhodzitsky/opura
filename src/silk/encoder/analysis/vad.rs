//! Native four-band activity/noise estimation over internal-rate PCM.
use super::AnalysisError;
use crate::silk::{
    cng::sqrt_approx,
    encoder::math::lin2log,
    gains::FrameDuration,
    pitch::SilkRate,
    synthesis::{mul_wb, mul_ww, round_shift, saturate16},
};
/// Raw SILK VAD outputs, before outer encoder activity/DTX policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SpeechActivity {
    /// Estimated speech probability in Q8, 0..=255.
    pub probability_q8: u8,
    /// Spectral tilt in signed Q15.
    pub tilt_q15: i16,
    /// Four smoothed per-band quality estimates in Q15, 0..=32767.
    pub quality_bands_q15: [i16; 4],
}
/// Persistent VAD filter and noise-estimation state for one channel.
/// Like the native encoder, history is retained across duration/rate changes;
/// initialize/reset separately when starting a new encoder channel.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VoiceActivityDetector {
    pub(super) filters: [[i32; 2]; 3],
    pub(super) previous_energy: [i32; 4],
    pub(super) ratio: [i32; 4],
    pub(super) highpass: i16,
    pub(super) noise: [i32; 4],
    pub(super) inverse_noise: [i32; 4],
    pub(super) bias: [i32; 4],
    pub(super) counter: i32,
}
impl Default for VoiceActivityDetector {
    fn default() -> Self {
        let bias = [50, 25, 16, 12];
        let noise = bias.map(|v| 100 * v);
        let inverse_noise = noise.map(|v| i32::MAX / v);
        Self {
            filters: [[0; 2]; 3],
            previous_energy: [0; 4],
            ratio: [25600; 4],
            highpass: 0,
            noise,
            inverse_noise,
            bias,
            counter: 15,
        }
    }
}
impl VoiceActivityDetector {
    /// Restore initial filters, pink-noise bias and smoothing state.
    pub fn reset(&mut self) {
        *self = Self::default();
    }
    /// Analyze a complete internal 10/20 ms mono frame. Invalid length changes
    /// neither filter nor noise history. No external-rate conversion or DTX
    /// decision is performed here.
    pub fn analyze(
        &mut self,
        rate: SilkRate,
        duration: FrameDuration,
        input: &[i16],
    ) -> Result<SpeechActivity, AnalysisError> {
        let ms = if duration == FrameDuration::Ms10 {
            10
        } else {
            20
        };
        let length = rate.khz() as usize * ms;
        if input.len() != length {
            return Err(AnalysisError::InvalidLength);
        }
        let offsets = [0, length / 8 + length / 4, length / 2, 3 * length / 4];
        let mut bands = [0i16; 400];
        for (k, pair) in input.chunks_exact(2).enumerate() {
            let (low, high) = filter(&mut self.filters[0], pair[0], pair[1]);
            bands[k] = low;
            bands[offsets[3] + k] = high;
        }
        for (stage, n, high_start) in [(1, length / 2, offsets[2]), (2, length / 4, offsets[1])] {
            for k in 0..n / 2 {
                let (low, high) = filter(&mut self.filters[stage], bands[2 * k], bands[2 * k + 1]);
                bands[k] = low;
                bands[high_start + k] = high;
            }
        }
        let lowest = length / 8;
        bands[lowest - 1] >>= 1;
        let last = bands[lowest - 1];
        for i in (1..lowest).rev() {
            bands[i - 1] >>= 1;
            bands[i] = bands[i].wrapping_sub(bands[i - 1]);
        }
        bands[0] = bands[0].wrapping_sub(self.highpass);
        self.highpass = last;
        let mut energy = [0i32; 4];
        for b in 0..4 {
            let count = (length >> ((4 - b).min(3))) / 4;
            energy[b] = self.previous_energy[b];
            let mut sum = 0i32;
            for subframe in 0..4 {
                sum = 0;
                for &sample in
                    &bands[offsets[b] + subframe * count..offsets[b] + (subframe + 1) * count]
                {
                    let value = i32::from(sample >> 3);
                    sum += value * value;
                }
                energy[b] = energy[b].saturating_add(if subframe == 3 { sum >> 1 } else { sum });
            }
            self.previous_energy[b] = sum;
        }
        self.update_noise(&energy);
        let mut ratios = [0i32; 4];
        let mut sum = 0i32;
        let mut tilt = 0i32;
        for b in 0..4 {
            let speech = energy[b] - self.noise[b];
            if speech > 0 {
                ratios[b] = if (energy[b] as u32) & 0xff800000 == 0 {
                    (energy[b] << 8) / (self.noise[b] + 1)
                } else {
                    energy[b] / ((self.noise[b] >> 8) + 1)
                };
                let mut snr = lin2log(ratios[b]) - 1024;
                sum += snr * snr;
                if speech < 1 << 20 {
                    snr = mul_wb(sqrt_approx(speech) << 6, snr);
                }
                tilt = tilt.wrapping_add(mul_wb([30000, 6000, -12000, -12000][b], snr));
            } else {
                ratios[b] = 256;
            }
        }
        let snr_db = (3 * sqrt_approx(sum / 4)) as i16;
        let mut probability = sigmoid(mul_wb(45000, i32::from(snr_db)) - 128);
        let tilt = ((sigmoid(tilt) - 16384) << 1) as i16;
        let mut speech = 0i32;
        for (b, &band_energy) in energy.iter().enumerate() {
            speech += (b as i32 + 1) * ((band_energy - self.noise[b]) >> 4);
        }
        if ms == 20 {
            speech >>= 1;
        }
        if speech <= 0 {
            probability >>= 1;
        } else if speech < 16384 {
            probability = mul_wb(32768 + sqrt_approx(speech << 16), probability);
        }
        let activity = (probability >> 7).min(255) as u8;
        let mut smooth = mul_wb(4096, mul_wb(probability, probability));
        if ms == 10 {
            smooth >>= 1;
        }
        let mut quality = [0i16; 4];
        for b in 0..4 {
            self.ratio[b] = self.ratio[b].wrapping_add(mul_wb(ratios[b] - self.ratio[b], smooth));
            let snr = 3 * (lin2log(self.ratio[b]) - 1024);
            quality[b] = sigmoid((snr - 2048) >> 4) as i16;
        }
        Ok(SpeechActivity {
            probability_q8: activity,
            tilt_q15: tilt,
            quality_bands_q15: quality,
        })
    }
    fn update_noise(&mut self, energy: &[i32; 4]) {
        let minimum = if self.counter < 1000 {
            let value = 32767 / ((self.counter >> 4) + 1);
            self.counter += 1;
            value
        } else {
            0
        };
        for (k, &value) in energy.iter().enumerate() {
            let old = self.noise[k];
            let new = value.saturating_add(self.bias[k]);
            let inverse = i32::MAX / new;
            let coefficient = if new > old << 3 {
                128
            } else if new < old {
                1024
            } else {
                mul_wb(mul_ww(inverse, old), 2048)
            }
            .max(minimum);
            self.inverse_noise[k] = self.inverse_noise[k]
                .wrapping_add(mul_wb(inverse - self.inverse_noise[k], coefficient));
            self.noise[k] = (i32::MAX / self.inverse_noise[k]).min(0xffffff);
        }
    }
}
fn filter(state: &mut [i32; 2], even: i16, odd: i16) -> (i16, i16) {
    let a = i32::from(even) << 10;
    let difference = a.wrapping_sub(state[0]);
    let x = difference.wrapping_add(mul_wb(difference, -24290));
    let first = state[0].wrapping_add(x);
    state[0] = a.wrapping_add(x);
    let b = i32::from(odd) << 10;
    let difference = b.wrapping_sub(state[1]);
    let x = mul_wb(difference, 10788);
    let second = state[1].wrapping_add(x);
    state[1] = b.wrapping_add(x);
    (
        saturate16(round_shift(second.wrapping_add(first), 11)),
        saturate16(round_shift(second.wrapping_sub(first), 11)),
    )
}
fn sigmoid(value: i32) -> i32 {
    let magnitude = value.unsigned_abs();
    if magnitude >= 192 {
        return if value < 0 { 0 } else { 32767 };
    }
    let index = (magnitude >> 5) as usize;
    let delta = [237, 153, 73, 30, 12, 7][index] * (magnitude & 31) as i32;
    if value < 0 {
        [16384, 8812, 3906, 1554, 589, 219][index] - delta
    } else {
        [16384, 23955, 28861, 31213, 32178, 32548][index] + delta
    }
}
