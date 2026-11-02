//! Stateful native pulse-sparsity analysis, hysteresis and high-frequency history.
use super::{AnalysisError, FrameSize};
use crate::celt::{output_tables::BANDS, pvq::Spread};

/// Immutable normalized-spectrum context for pulse spreading analysis.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SpreadingOptions {
    /// One/two planar channels.
    pub channels: usize,
    /// Exclusive band end in 1..21.
    pub end: usize,
    /// Per-band masking weights in 0..32; inactive entries are ignored.
    pub weights: [u8; 21],
    /// Update the high-frequency average and pitch tapset suggestion.
    pub update_hf: bool,
}
impl Default for SpreadingOptions {
    fn default() -> Self {
        Self {
            channels: 1,
            end: 21,
            weights: [32; 21],
            update_hf: false,
        }
    }
}
/// Checked recursive sparsity/HF histories and last effective spread decision.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SpreadingState {
    average: u16,
    hf: u8,
    tapset: u8,
    decision: Spread,
}
impl Default for SpreadingState {
    fn default() -> Self {
        Self {
            average: 256,
            hf: 0,
            tapset: 0,
            decision: Spread::Normal,
        }
    }
}
impl SpreadingState {
    /// Restore checked history: average 0..768, HF 0..64 and tapset 0..2.
    pub fn from_history(
        average: u16,
        hf: u8,
        tapset: u8,
        decision: Spread,
    ) -> Result<Self, AnalysisError> {
        if average > 768 || hf > 64 || tapset > 2 {
            return Err(AnalysisError::InvalidSpreadParameters);
        }
        Ok(Self {
            average,
            hf,
            tapset,
            decision,
        })
    }
    /// Native recursive sparsity average, in 0..768.
    pub fn tonal_average(self) -> u16 {
        self.average
    }
    /// Native recursive high-frequency average, in 0..64.
    pub fn hf_average(self) -> u8 {
        self.hf
    }
    /// Suggested native pitch-filter tapset in 0..2.
    pub fn tapset(self) -> u8 {
        self.tapset
    }
    /// Last effective pulse-spreading decision.
    pub fn decision(self) -> Spread {
        self.decision
    }
    /// Restore native initial histories and Normal decision.
    pub fn reset(&mut self) {
        *self = Self::default();
    }
    /// Analyze planar normalized Q24 spectra and commit all histories together.
    ///
    /// Input length is exactly `channels*size.samples()`. Each active band's
    /// squared norm must be <=5/4 Q48, including zero vectors. Only active bins
    /// and weights are inspected. If the last band has <=8 coefficients, return
    /// [`Spread::None`] and retain numeric histories. Otherwise at least one eligible band
    /// (>8 coefficients) must have positive weight. Unsupported contexts fail
    /// without changing state or immutable input. All storage is fixed.
    pub fn process(
        &mut self,
        size: FrameSize,
        normalized: &[i32],
        options: SpreadingOptions,
    ) -> Result<Spread, AnalysisError> {
        let channels = options.channels;
        let end = options.end;
        let n = size.samples();
        let lm = size.lm();
        if !(1..=2).contains(&channels) || normalized.len() != channels * n {
            return Err(AnalysisError::InvalidGeometry);
        }
        if !(1..=21).contains(&end) {
            return Err(AnalysisError::InvalidBands);
        }
        if options.weights[..end].iter().any(|&w| w > 32) {
            return Err(AnalysisError::InvalidSpreadParameters);
        }
        for c in 0..channels {
            for i in 0..end {
                let input = &normalized[c * n + (usize::from(BANDS[i]) << lm)
                    ..c * n + (usize::from(BANDS[i + 1]) << lm)];
                let energy: i128 = input.iter().map(|&x| i128::from(x) * i128::from(x)).sum();
                if energy > 5 * (1i128 << 46) {
                    return Err(AnalysisError::ArithmeticOverflow);
                }
            }
        }
        if (usize::from(BANDS[end] - BANDS[end - 1]) << lm) <= 8 {
            self.decision = Spread::None;
            return Ok(Spread::None);
        }
        let mut sum = 0u32;
        let mut weight = 0u32;
        let mut hf_sum = 0u32;
        for c in 0..channels {
            for i in 0..end {
                let width = usize::from(BANDS[i + 1] - BANDS[i]) << lm;
                if width <= 8 {
                    continue;
                }
                let input = &normalized[c * n + (usize::from(BANDS[i]) << lm)
                    ..c * n + (usize::from(BANDS[i + 1]) << lm)];
                let mut counts = [0u32; 3];
                for &value in input {
                    let x = value >> 10;
                    let energy = ((x * x) >> 15) * width as i32;
                    for (count, threshold) in counts.iter_mut().zip([2048, 512, 128]) {
                        *count += u32::from(energy < threshold);
                    }
                }
                if i > 17 {
                    hf_sum += 32 * (counts[1] + counts[0]) / width as u32;
                }
                let sparse: u32 = counts
                    .iter()
                    .map(|&count| u32::from(2 * count >= width as u32))
                    .sum();
                sum += sparse * u32::from(options.weights[i]);
                weight += u32::from(options.weights[i]);
            }
        }
        if weight == 0 {
            return Err(AnalysisError::InvalidSpreadParameters);
        }
        let mut next = *self;
        if options.update_hf {
            // Nonzero HF counts imply end>=19, so the native divisor is positive.
            if hf_sum != 0 {
                hf_sum /= (channels * (end - 17)) as u32;
            }
            next.hf = ((u32::from(self.hf) + hf_sum) >> 1) as u8;
            let adjusted = i32::from(next.hf)
                + if self.tapset == 2 {
                    4
                } else if self.tapset == 0 {
                    -4
                } else {
                    0
                };
            next.tapset = if adjusted > 22 {
                2
            } else if adjusted > 18 {
                1
            } else {
                0
            };
        }
        let measured = (sum << 8) / weight;
        next.average = ((measured + u32::from(self.average)) >> 1) as u16;
        let hysteresis =
            (3 * u32::from(next.average) + ((3 - self.decision as u32) << 7) + 64 + 2) >> 2;
        next.decision = if hysteresis < 80 {
            Spread::Aggressive
        } else if hysteresis < 256 {
            Spread::Normal
        } else if hysteresis < 384 {
            Spread::Light
        } else {
            Spread::None
        };
        *self = next;
        Ok(next.decision)
    }
    pub(crate) fn effective_decision(&mut self, decision: Spread) {
        self.decision = decision;
    }
}
