//! Scalar fixed-point native CELT variable-rate target and budget histories.
use crate::celt::{FrameSize, analysis::AnalysisError, math, output_tables::BANDS};

/// Nominal CELT variable-rate controls for one frame.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VbrOptions {
    /// Nominal TOC-inclusive bits/s; at least24 bits/frame, at most1500000.
    /// Native per-channel and physical-capacity limits can reduce the target.
    pub bitrate: u32,
    /// Enable native reservoir/drift feedback and the initial constrained cap.
    pub constrained: bool,
    /// Hard payload capacity excluding the TOC,2..=1275 bytes.
    pub payload_bytes: usize,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct VbrRate {
    pub bitrate: i32,
    pub constrained: bool,
    pub capacity: usize,
}
impl VbrRate {
    pub fn q3(self, size: FrameSize) -> i32 {
        // This API owns the TOC, like native CELT with signalling enabled.
        // Reservoir accounting must cover payload bits only.
        (self.bitrate / (400 >> size.lm()) - 8) * 8
    }
    pub fn equivalent(self, size: FrameSize, channels: usize) -> i32 {
        (self.capacity as i32 * 8 * (400 >> size.lm())).min(self.bitrate)
            - (40 * channels as i32 + 20) * ((400 >> size.lm()) - 50)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct VbrState {
    pub reservoir: i32,
    pub drift: i32,
    pub offset: i32,
    pub count: u16,
    pub spectral_average: i32,
}
#[derive(Clone, Copy)]
pub(super) struct VbrTarget {
    pub size: FrameSize,
    pub channels: usize,
    pub coded_bands: usize,
    pub intensity: usize,
    pub stereo_saving: u16,
    pub intended_boost: i32,
    pub tf_estimate: i16,
    pub max_depth: i32,
    pub temporal: i32,
    pub equivalent_rate: i32,
}
#[derive(Clone, Copy)]
pub(super) struct VbrFrame {
    pub rate: VbrRate,
    pub target: VbrTarget,
    pub capacity: usize,
    pub tell_q3: u32,
    pub actual_boost: u32,
    pub silence: bool,
}
fn q15(a: i32, b: i32) -> i32 {
    ((i64::from(a) * i64::from(b)) >> 15) as i32
}
impl VbrTarget {
    pub fn compute(self, base: i32, constrained: bool) -> i32 {
        let lm = self.size.lm();
        let coded = if self.coded_bands == 0 {
            21
        } else {
            self.coded_bands
        };
        let mut bins = i32::from(BANDS[coded]) << lm;
        if self.channels == 2 {
            bins += i32::from(BANDS[self.intensity.min(coded)]) << lm;
        }
        let mut target = base;
        if self.channels == 2 {
            let stereo_bands = self.intensity.min(coded);
            let dof = (i32::from(BANDS[stereo_bands]) << lm) - stereo_bands as i32;
            let fraction = 26214 * dof / bins;
            let saving = i32::from(self.stereo_saving.min(256));
            target -= q15(fraction, target).min(((saving - 26) * (dof << 3)) >> 8);
        }
        target += self.intended_boost - (19 << lm);
        target += q15(i32::from(self.tf_estimate) - 721, target) * 2;
        let bins = i32::from(BANDS[19]) << lm;
        let depth = (q15(self.channels as i32 * bins * 8, self.max_depth) >> 9).max(target >> 2);
        target = target.min(depth);
        if constrained {
            target = base + q15(21955, target - base);
        }
        if self.tf_estimate < 3277 {
            let amount = (3329 * (96000 - self.equivalent_rate).clamp(0, 32000)) >> 15;
            let factor = ((self.temporal >> 14) * amount) >> 10;
            target += q15(factor, target);
        }
        target.min(2 * base)
    }
}
impl VbrState {
    pub fn capacity(self, size: FrameSize, rate: VbrRate, capacity: usize) -> usize {
        if rate.constrained {
            (((i64::from(rate.q3(size)) * 2 - i64::from(self.reservoir)) >> 6).max(2) as usize)
                .min(capacity)
        } else {
            capacity
        }
    }
    pub fn temporal(
        &mut self,
        logs: &[[i32; 21]; 2],
        channels: usize,
        start: usize,
        end: usize,
        size: FrameSize,
        transient: bool,
    ) -> i32 {
        let offset = if transient {
            (size.lm() as i32) << 18
        } else {
            0
        };
        let mut follow = -(10 << 19);
        let mut total = 0i64;
        for i in start..end {
            follow = (follow - (1 << 19)).max((logs[0][i] >> 5) - offset);
            if channels == 2 {
                follow = follow.max((logs[1][i] >> 5) - offset);
            }
            total += i64::from(follow);
        }
        let average = (total / (end - start) as i64) * 32;
        let delta = (average - i64::from(self.spectral_average)).clamp(-(3 << 23), 3 << 24) as i32;
        self.spectral_average += q15(655, delta);
        delta
    }
    pub fn finish(&mut self, frame: VbrFrame) -> Result<usize, AnalysisError> {
        let VbrFrame {
            rate,
            target,
            capacity,
            tell_q3,
            actual_boost,
            silence,
        } = frame;
        let lm = target.size.lm();
        let vbr_rate = rate.q3(target.size);
        let mut base = vbr_rate - ((40 * target.channels as i32 + 20) << 3);
        if rate.constrained {
            base += self.offset >> (3 - lm);
        }
        let requested = target.compute(base, rate.constrained) + tell_q3 as i32;
        let maximum = capacity.min(1275 >> (3 - lm));
        let minimum = (tell_q3 + actual_boost).div_ceil(64) + 2;
        let mut bytes = (((requested + 32) >> 6).max(minimum as i32) as usize).min(maximum);
        let mut delta = requested - vbr_rate;
        if silence {
            bytes = 2;
            delta = 0;
        }
        let count = (self.count + 1).min(970);
        let alpha = if self.count < 970 {
            math::reciprocal(i32::from(count + 20) << 16).map_err(AnalysisError::Math)?
        } else {
            33
        };
        let mut next = *self;
        next.count = count;
        if rate.constrained {
            let reservoir = i64::from(self.reservoir) + bytes as i64 * 64 - i64::from(vbr_rate);
            let error =
                (i64::from(delta) << (3 - lm)) - i64::from(self.offset) - i64::from(self.drift);
            let drift = i64::from(self.drift) + ((i64::from(alpha) * error) >> 15);
            next.drift = i32::try_from(drift).map_err(|_| AnalysisError::ArithmeticOverflow)?;
            next.offset = next
                .drift
                .checked_neg()
                .ok_or(AnalysisError::ArithmeticOverflow)?;
            if reservoir < 0 {
                if !silence {
                    bytes += (-reservoir / 64) as usize;
                }
                next.reservoir = 0;
            } else {
                next.reservoir =
                    i32::try_from(reservoir).map_err(|_| AnalysisError::ArithmeticOverflow)?;
            }
        }
        *self = next;
        Ok(bytes.min(maximum))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unchanged_native_temporal_spectrum_history_matches() -> Result<(), Box<dyn std::error::Error>>
    {
        let mut state = VbrState::default();
        let mut rows = 0;
        for line in include_str!("../../tests/celt-encoder-temporal-vbr-fixtures.txt")
            .lines()
            .filter(|l| !l.starts_with('#'))
        {
            let values = line
                .split_whitespace()
                .map(str::parse)
                .collect::<Result<Vec<i32>, _>>()?;
            assert_eq!(values.len(), 51);
            if values[1] == 0 {
                state = VbrState::default();
            }
            let size = [
                FrameSize::Ms2_5,
                FrameSize::Ms5,
                FrameSize::Ms10,
                FrameSize::Ms20,
            ][values[3] as usize];
            let mut logs = [[0; 21]; 2];
            logs[0].copy_from_slice(&values[7..28]);
            logs[1].copy_from_slice(&values[28..49]);
            let temporal = state.temporal(
                &logs,
                values[2] as usize,
                values[4] as usize,
                values[5] as usize,
                size,
                values[6] != 0,
            );
            assert_eq!(
                (temporal, state.spectral_average),
                (values[49], values[50]),
                "{line}"
            );
            rows += 1;
        }
        assert_eq!(rows, 1024);
        Ok(())
    }
    #[test]
    fn unchanged_native_target_capacity_and_state_match() -> Result<(), Box<dyn std::error::Error>>
    {
        let mut state = VbrState::default();
        let mut rows = 0;
        for line in include_str!("../../tests/celt-encoder-vbr-fixtures.txt")
            .lines()
            .filter(|l| !l.starts_with('#'))
        {
            let values = line
                .split_whitespace()
                .map(str::parse)
                .collect::<Result<Vec<i32>, _>>()?;
            let [
                _,
                frame,
                lm,
                channels,
                capacity,
                bitrate,
                constrained,
                precap,
                coded,
                intensity,
                saving,
                tf,
                depth,
                temporal,
                equivalent,
                intended,
                actual,
                tell,
                silence,
                expected_target,
                bytes,
                reservoir,
                drift,
                offset,
                count,
            ]: [i32; 25] = values.try_into().map_err(|_| "bad fixture row")?;
            if frame == 0 {
                state = VbrState::default();
            }
            let size = [
                FrameSize::Ms2_5,
                FrameSize::Ms5,
                FrameSize::Ms10,
                FrameSize::Ms20,
            ][lm as usize];
            let rate = VbrRate {
                bitrate,
                constrained: constrained != 0,
                capacity: capacity as usize,
            };
            let target = VbrTarget {
                size,
                channels: channels as usize,
                coded_bands: coded as usize,
                intensity: intensity as usize,
                stereo_saving: saving as u16,
                intended_boost: intended,
                tf_estimate: tf as i16,
                max_depth: depth,
                temporal,
                equivalent_rate: equivalent,
            };
            assert_eq!(
                rate.equivalent(size, channels as usize),
                equivalent,
                "{line}"
            );
            assert_eq!(
                state.capacity(size, rate, capacity as usize),
                precap as usize,
                "{line}"
            );
            let base = rate.q3(size) - ((40 * channels + 20) << 3)
                + if rate.constrained {
                    state.offset >> (3 - lm)
                } else {
                    0
                };
            assert_eq!(
                target.compute(base, rate.constrained),
                expected_target,
                "{line}"
            );
            let result = state
                .finish(VbrFrame {
                    rate,
                    target,
                    capacity: precap as usize,
                    tell_q3: tell as u32,
                    actual_boost: actual as u32,
                    silence: silence != 0,
                })
                .map(|n| n as i32)
                .map(|n| {
                    (
                        n,
                        state.reservoir,
                        state.drift,
                        state.offset,
                        i32::from(state.count),
                    )
                });
            assert_eq!(result?, (bytes, reservoir, drift, offset, count), "{line}");
            rows += 1;
        }
        assert_eq!(rows, 8192);
        Ok(())
    }
}
