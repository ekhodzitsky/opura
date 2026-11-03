//! Adaptive VOIP cutoff and native fixed-point input biquad.
//! Adapted from Opus1.6.1 HP_variable_cutoff.c and opus_encoder.c.
//! Copyright (c)2006-2011 Skype Limited; see THIRD_PARTY_NOTICES.md.
use super::{
    analysis::SpeechActivity,
    math::{lin2log, log2lin},
};
use crate::silk::{
    gains::SignalType,
    pitch::SilkRate,
    synthesis::{mul_wb, mul_ww, round_shift, saturate16},
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PitchCutoff {
    pub smooth: i32,
    lag: i32,
    signal: SignalType,
    quality: i16,
    activity: u8,
}
impl Default for PitchCutoff {
    fn default() -> Self {
        Self {
            smooth: initial_smoothing(),
            lag: 100,
            signal: SignalType::Inactive,
            quality: 0,
            activity: 0,
        }
    }
}
impl PitchCutoff {
    pub fn reset_prediction(&mut self) {
        self.lag = 100;
        self.signal = SignalType::Inactive;
    }
    pub fn advance(&mut self, rate: SilkRate) {
        if self.signal != SignalType::Voiced {
            return;
        }
        let frequency = ((rate.khz() * 1000) << 16) / self.lag;
        let mut pitch_log = lin2log(frequency) - (16 << 7);
        let quality = i32::from(self.quality);
        pitch_log += mul_wb(
            mul_wb(-quality << 2, quality),
            pitch_log - (lin2log(60 << 16) - (16 << 7)),
        );
        let mut delta = pitch_log - (self.smooth >> 8);
        if delta < 0 {
            delta *= 3;
        }
        delta = delta.clamp(-51, 51);
        self.smooth += mul_wb(i32::from(self.activity) * delta, 6554);
        self.smooth = self.smooth.clamp(initial_smoothing(), lin2log(100) << 8);
    }
    pub fn record(&mut self, lag: i32, signal: SignalType, activity: SpeechActivity) {
        self.lag = lag;
        self.signal = signal;
        self.quality = activity.quality_bands_q15[0];
        self.activity = activity.probability_q8;
    }
}
pub(crate) fn initial_smoothing() -> i32 {
    lin2log(60) << 8
}
pub(crate) fn smooth_cutoff(second: &mut i32, first: i32) -> i32 {
    *second += mul_wb(first - *second, 983);
    log2lin(*second >> 8)
}
// Geometry and the adaptive60..100Hz cutoff are validated by the stream owner.
pub(crate) fn filter(
    input: &[i16],
    output: &mut [i16],
    state: &mut [i32; 4],
    rate: u32,
    channels: usize,
    cutoff: i32,
) {
    let fc = 2471 * cutoff / (rate as i32 / 1000);
    let r = (1 << 28) - 471 * fc;
    let b = [r, -2 * r, r];
    let r22 = r >> 6;
    let a = [mul_ww(r22, mul_ww(fc, fc) - (2 << 22)), mul_ww(r22, r22)];
    let a = a.map(i32::wrapping_neg);
    for c in 0..channels {
        for i in (c..input.len()).step_by(channels) {
            let value = i32::from(input[i]);
            let y = state[2 * c]
                .wrapping_add(mul_wb(b[0], value))
                .wrapping_shl(2);
            state[2 * c] = state[2 * c + 1]
                .wrapping_add(round_shift(mul_wb(y, a[0] & 16383), 14))
                .wrapping_add(mul_wb(y, a[0] >> 14))
                .wrapping_add(mul_wb(b[1], value));
            state[2 * c + 1] = round_shift(mul_wb(y, a[1] & 16383), 14)
                .wrapping_add(mul_wb(y, a[1] >> 14))
                .wrapping_add(mul_wb(b[2], value));
            output[i] = saturate16(y.wrapping_add(16383) >> 14);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_adaptive_cutoff_and_biquad_states_match() -> Result<(), Box<dyn std::error::Error>>
    {
        let mut rows = 0;
        let mut changed = 0;
        for line in include_str!("../../../tests/opus-voip-highpass-fixtures.txt")
            .lines()
            .filter(|l| !l.starts_with('#'))
        {
            let f: Vec<_> = line.split_whitespace().collect();
            assert_eq!(f.len(), 24);
            let v = f[..23]
                .iter()
                .map(|s| s.parse::<i64>())
                .collect::<Result<Vec<_>, _>>()?;
            let rate = match v[3] {
                8 => SilkRate::Khz8,
                12 => SilkRate::Khz12,
                _ => SilkRate::Khz16,
            };
            let mut cutoff = PitchCutoff {
                smooth: v[8] as i32,
                lag: v[4] as i32,
                signal: match v[5] {
                    0 => SignalType::Inactive,
                    1 => SignalType::Unvoiced,
                    _ => SignalType::Voiced,
                },
                quality: v[6] as i16,
                activity: v[7] as u8,
            };
            cutoff.advance(rate);
            assert_eq!(cutoff.smooth, v[16] as i32, "HP1: {line}");
            let mut second = v[9] as i32;
            let hz = smooth_cutoff(&mut second, cutoff.smooth);
            assert_eq!(second, v[17] as i32, "HP2: {line}");
            assert_eq!(hz, v[18] as i32, "Hz: {line}");
            let count = v[0] as usize * v[2] as usize / 48000 * v[1] as usize;
            let mut seed = v[10] as u32;
            let input: Vec<i16> = (0..count)
                .map(|i| {
                    seed ^= seed << 13;
                    seed ^= seed >> 17;
                    seed ^= seed << 5;
                    match v[11] {
                        0 => 0,
                        1 => {
                            if i % 2 == 0 {
                                i16::MIN
                            } else {
                                i16::MAX
                            }
                        }
                        2 => (((seed & 65535) as i32 - 32768) >> 8) as i16,
                        _ => ((seed & 65535) as i32 - 32768) as i16,
                    }
                })
                .collect();
            let mut state = [v[12] as i32, v[13] as i32, v[14] as i32, v[15] as i32];
            let mut output = vec![0; count];
            filter(
                &input,
                &mut output,
                &mut state,
                v[0] as u32,
                v[1] as usize,
                hz,
            );
            assert_eq!(
                state,
                [v[19] as i32, v[20] as i32, v[21] as i32, v[22] as i32],
                "state: {line}"
            );
            let hash = output
                .iter()
                .flat_map(|x| x.to_le_bytes())
                .fold(14695981039346656037u64, |h, b| {
                    (h ^ u64::from(b)).wrapping_mul(1099511628211)
                });
            assert_eq!(hash, f[23].parse::<u64>()?, "PCM: {line}");
            changed += usize::from(cutoff.smooth != v[8] as i32);
            rows += 1;
        }
        assert_eq!(rows, 640);
        assert!(changed > 100);
        Ok(())
    }
}
