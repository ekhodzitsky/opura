//! Native encoder pitch policy and separate-source comb prefilter.
//! Follows pinned celt_encoder.c::run_prefilter and celt.c::comb_filter.
//! Existing pitch/comb notices in THIRD_PARTY_NOTICES.md apply.
use super::{
    FrameSize,
    mdct_tables::WINDOW,
    pitch::{self, PitchError, PitchHistory},
    postfilter::{PostfilterError, PostfilterParams, Tapset},
};
use std::fmt;

/// Invalid prefilter geometry/signal/options or a constituent analysis failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum PrefilterError {
    /// Channel count must be one/two and planar input must contain channels*N.
    InvalidGeometry,
    /// Raw Q12 samples must fit +/-536870911.
    InvalidSignal,
    /// Options exceed their documented domains.
    InvalidOptions,
    /// Pitch search/refinement failed.
    Pitch(PitchError),
    /// Comb parameter validation failed.
    Parameters(PostfilterError),
}
impl fmt::Display for PrefilterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidGeometry => f.write_str("invalid CELT prefilter geometry"),
            Self::InvalidSignal => f.write_str("CELT prefilter signal exceeds saturation domain"),
            Self::InvalidOptions => f.write_str("invalid CELT prefilter options"),
            Self::Pitch(e) => write!(f, "CELT prefilter pitch: {e}"),
            Self::Parameters(e) => write!(f, "CELT prefilter parameters: {e}"),
        }
    }
}
impl std::error::Error for PrefilterError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Pitch(e) => Some(e),
            Self::Parameters(e) => Some(e),
            _ => None,
        }
    }
}
/// Native prefilter policy inputs. Outer packet resource gating is caller-owned.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PrefilterOptions {
    /// Allow new pitch gain; disabled frames still ramp out old filtering.
    pub enabled: bool,
    /// Native complexity 0..10; ordinary search requires at least five.
    pub complexity: u8,
    /// Available payload bytes 0..1275, used by gain thresholds.
    pub available_bytes: u16,
    /// Temporal estimate 0..16384 Q14.
    pub tf_estimate: i16,
    /// Tone frequency -1..25736 Q13; -1 denotes no detected tone.
    pub tone_frequency: i16,
    /// Adjusted tone pole radius 0..2^29 Q29.
    pub toneishness: i32,
    /// Packet-loss percentage 0..100, used by ordinary pitch gain attenuation.
    pub loss_rate: u8,
    /// Requested current comb shape.
    pub tapset: Tapset,
}
impl Default for PrefilterOptions {
    fn default() -> Self {
        Self {
            enabled: true,
            complexity: 10,
            available_bytes: 128,
            tf_estimate: 0,
            tone_frequency: -1,
            toneishness: 0,
            loss_rate: 0,
            tapset: Tapset::Set0,
        }
    }
}
/// Fixed-storage filtered planar MDCT context, with 120 preceding samples/channel.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PrefilteredFrame {
    size: FrameSize,
    channels: usize,
    values: [i32; 2160],
    parameters: PostfilterParams,
    qgain: u8,
    active: bool,
}
impl PrefilteredFrame {
    /// Transform duration.
    pub fn size(&self) -> FrameSize {
        self.size
    }
    /// Planar channel count.
    pub fn channels(&self) -> usize {
        self.channels
    }
    /// Channels*(N+120) filtered context values in Q12.
    pub fn values(&self) -> &[i32] {
        &self.values[..self.channels * (self.size.samples() + 120)]
    }
    /// Internal native pitch/gain/tap, retaining period even when gain is zero.
    pub fn parameters(&self) -> PostfilterParams {
        self.parameters
    }
    /// Transmitted gain index 0..7 when active, zero when inactive.
    pub fn quantized_gain(&self) -> u8 {
        self.qgain
    }
    /// Whether a nonzero new prefilter survived gain thresholds/cancellation.
    pub fn active(&self) -> bool {
        self.active
    }
}
/// Raw encoder pitch history and separate filtered transform overlap.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PrefilterState {
    channels: usize,
    raw: [[i32; 1024]; 2],
    overlap: [[i32; 120]; 2],
    parameters: PostfilterParams,
}
impl PrefilterState {
    /// Start a one/two-channel stream with native zero histories.
    pub fn new(channels: usize) -> Result<Self, PrefilterError> {
        if !(1..=2).contains(&channels) {
            return Err(PrefilterError::InvalidGeometry);
        }
        Ok(Self {
            channels,
            raw: [[0; 1024]; 2],
            overlap: [[0; 120]; 2],
            parameters: PostfilterParams::default(),
        })
    }
    /// Fixed raw/filtered channel count.
    pub fn channels(&self) -> usize {
        self.channels
    }
    /// Internal previous pitch/gain/tap, initially all zero.
    pub fn parameters(&self) -> PostfilterParams {
        self.parameters
    }
    /// Last 1024 raw pre-emphasized samples for an active channel.
    pub fn raw_history(&self, channel: usize) -> Option<&[i32; 1024]> {
        if channel < self.channels {
            Some(&self.raw[channel])
        } else {
            None
        }
    }
    /// Last 120 filtered samples for an active channel.
    pub fn overlap(&self, channel: usize) -> Option<&[i32; 120]> {
        if channel < self.channels {
            Some(&self.overlap[channel])
        } else {
            None
        }
    }
    /// Clear histories, retaining channel count.
    pub fn reset(&mut self) {
        self.raw = [[0; 1024]; 2];
        self.overlap = [[0; 120]; 2];
        self.parameters = PostfilterParams::default();
    }
    /// Filter immutable planar current Q12 samples, returning the full MDCT context.
    /// Exact length is channels*size.samples(); every value is +/-536870911.
    /// Options are checked even when their branch is inactive. All errors preserve
    /// complete state/input. Fixed storage; no allocation or feedback lag reads.
    pub fn process(
        &mut self,
        size: FrameSize,
        input: &[i32],
        options: PrefilterOptions,
    ) -> Result<PrefilteredFrame, PrefilterError> {
        self.process_with(size, input, options, pitch::encoder_pitch)
    }
    fn process_with(
        &mut self,
        size: FrameSize,
        input: &[i32],
        options: PrefilterOptions,
        analyze: impl FnOnce(
            FrameSize,
            &[i32],
            Option<&[i32]>,
            PitchHistory,
        ) -> Result<pitch::EncoderPitch, PitchError>,
    ) -> Result<PrefilteredFrame, PrefilterError> {
        let n = size.samples();
        let c = self.channels;
        if input.len() != c * n {
            return Err(PrefilterError::InvalidGeometry);
        }
        if input.iter().any(|v| !(-536870911..=536870911).contains(v)) {
            return Err(PrefilterError::InvalidSignal);
        }
        if options.complexity > 10
            || options.available_bytes > 1275
            || !(0..=16384).contains(&options.tf_estimate)
            || !(-1..=25736).contains(&options.tone_frequency)
            || !(0..=1 << 29).contains(&options.toneishness)
            || options.loss_rate > 100
        {
            return Err(PrefilterError::InvalidOptions);
        }
        let mut joined = [[0; 1984]; 2];
        for (channel, source) in joined[..c].iter_mut().enumerate() {
            source[..1024].copy_from_slice(&self.raw[channel]);
            source[1024..1024 + n].copy_from_slice(&input[channel * n..(channel + 1) * n]);
        }
        let (period, mut gain) = if options.enabled && options.toneishness > 531502208 {
            let mut frequency = i32::from(options.tone_frequency);
            if frequency >= 25736 {
                frequency = 25736 - frequency;
            }
            let mut multiple = 1;
            while frequency >= multiple * 3195 {
                multiple += 1;
            }
            let period = if frequency > 50 {
                ((51472 * multiple + frequency / 2) / frequency).min(1022)
            } else {
                15
            };
            (period as u16, 24576)
        } else if options.enabled && options.complexity >= 5 {
            let previous = PitchHistory::new(self.parameters.period(), self.parameters.gain())
                .map_err(PrefilterError::Pitch)?;
            let analyzed = analyze(
                size,
                &joined[0][..1024 + n],
                if c == 2 {
                    Some(&joined[1][..1024 + n])
                } else {
                    None
                },
                previous,
            )
            .map_err(PrefilterError::Pitch)?;
            let mut gain = mul(22938, i32::from(analyzed.gain()));
            if options.loss_rate > 2 {
                gain >>= 1;
            }
            if options.loss_rate > 4 {
                gain >>= 1;
            }
            if options.loss_rate > 8 {
                gain = 0;
            }
            (analyzed.period().min(1022), gain)
        } else {
            (15, 0)
        };
        let mut threshold = 6554;
        if period.abs_diff(self.parameters.period()) * 10 > period {
            threshold += 6554;
            if options.tf_estimate > 16056 {
                gain = 0;
            }
        }
        if options.available_bytes < 25 {
            threshold += 3277;
        }
        if options.available_bytes < 35 {
            threshold += 3277;
        }
        if self.parameters.gain() > 13107 {
            threshold -= 3277;
        }
        if self.parameters.gain() > 18022 {
            threshold -= 3277;
        }
        threshold = threshold.max(6554);
        let mut active = gain >= threshold;
        let mut qgain = 0;
        if active {
            if (gain - i32::from(self.parameters.gain())).abs() < 3277 {
                gain = i32::from(self.parameters.gain());
            }
            qgain = (((gain + 1536) >> 10) / 3 - 1).clamp(0, 7) as u8;
            gain = 3072 * (i32::from(qgain) + 1);
        } else {
            gain = 0;
        }
        let mut parameters = PostfilterParams::new(period, gain as u16, options.tapset)
            .map_err(PrefilterError::Parameters)?;
        let mut result = PrefilteredFrame {
            size,
            channels: c,
            values: [0; 2160],
            parameters,
            qgain,
            active,
        };
        let mut before = [0i32; 2];
        let mut after = [0i32; 2];
        for channel in 0..c {
            let output = &mut result.values[channel * (n + 120)..(channel + 1) * (n + 120)];
            output[..120].copy_from_slice(&self.overlap[channel]);
            comb(
                &joined[channel],
                &mut output[120..],
                self.parameters,
                parameters,
            );
            before[channel] = input[channel * n..(channel + 1) * n]
                .iter()
                .map(|v| (v >> 12).abs())
                .sum();
            after[channel] = output[120..].iter().map(|v| (v >> 12).abs()).sum();
        }
        let cancel = if c == 2 {
            // Original opus_val16 thresholds narrow after both wide products.
            let thresholds = [
                (mul(mul(8192, gain), before[0]) + mul(328, before[1])) as i16 as i32,
                (mul(mul(8192, gain), before[1]) + mul(328, before[0])) as i16 as i32,
            ];
            after[0] - before[0] > thresholds[0]
                || after[1] - before[1] > thresholds[1]
                || before[0] - after[0] < thresholds[0] && before[1] - after[1] < thresholds[1]
        } else {
            after[0] > before[0]
        };
        if cancel {
            active = false;
            qgain = 0;
            parameters = PostfilterParams::new(period, 0, options.tapset)
                .map_err(PrefilterError::Parameters)?;
            for (channel, source) in joined[..c].iter().enumerate() {
                let output =
                    &mut result.values[channel * (n + 120) + 120..(channel + 1) * (n + 120)];
                output.copy_from_slice(&input[channel * n..(channel + 1) * n]);
                comb(source, &mut output[..120], self.parameters, parameters);
            }
        }
        for channel in 0..c {
            self.raw[channel].copy_within(n..1024, 0);
            self.raw[channel][1024 - n..].copy_from_slice(&input[channel * n..(channel + 1) * n]);
            self.overlap[channel].copy_from_slice(
                &result.values[channel * (n + 120) + n..(channel + 1) * (n + 120)],
            );
        }
        self.parameters = parameters;
        result.parameters = parameters;
        result.qgain = qgain;
        result.active = active;
        Ok(result)
    }
}
fn mul(coefficient: i32, value: i32) -> i32 {
    ((i64::from(coefficient as i16) * i64::from(value)) >> 15) as i32
}
fn comb(source: &[i32; 1984], output: &mut [i32], old: PostfilterParams, new: PostfilterParams) {
    if old.gain() == 0 && new.gain() == 0 {
        output.copy_from_slice(&source[1024..1024 + output.len()]);
        return;
    }
    let old_period = usize::from(old.period().max(15));
    let new_period = usize::from(new.period().max(15));
    let old_gains = old
        .tapset()
        .gains()
        .map(|g| (-i32::from(old.gain()) * g + 16384) >> 15);
    let new_gains = new
        .tapset()
        .gains()
        .map(|g| (-i32::from(new.gain()) * g + 16384) >> 15);
    let overlap = if old == new { 0 } else { 120 };
    for (i, value) in output.iter_mut().enumerate() {
        let at = 1024 + i;
        let mut sum = source[at];
        if i < overlap {
            let weight = mul(i32::from(WINDOW[i]), i32::from(WINDOW[i]));
            for (period, gains, w) in [
                (old_period, old_gains, 32767 - weight),
                (new_period, new_gains, weight),
            ] {
                let delayed = at - period;
                let terms = [
                    source[delayed],
                    source[delayed + 1] + source[delayed - 1],
                    source[delayed + 2] + source[delayed - 2],
                ];
                for j in 0..3 {
                    sum += mul(mul(w, gains[j]), terms[j]);
                }
            }
            *value = (sum - 3).clamp(-536870911, 536870911);
        } else if new.gain() != 0 {
            let delayed = at - new_period;
            let terms = [
                source[delayed],
                source[delayed + 1] + source[delayed - 1],
                source[delayed + 2] + source[delayed - 2],
            ];
            for j in 0..3 {
                sum += mul(new_gains[j], terms[j]);
            }
            *value = (sum - 1).clamp(-536870911, 536870911);
        } else {
            *value = source[at];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn injected_pitch_failure_preserves_all_histories_and_input()
    -> Result<(), Box<dyn std::error::Error>> {
        for channels in [1, 2] {
            let mut state = PrefilterState::new(channels)?;
            let input: Vec<_> = (0..960 * channels)
                .map(|i| {
                    i32::from(super::super::math::cos_norm((i % 960) as i32 * 131072 / 60)) * 2048
                })
                .collect();
            let options = PrefilterOptions {
                toneishness: 1 << 29,
                tone_frequency: 858,
                ..PrefilterOptions::default()
            };
            state.process(FrameSize::Ms20, &input, options)?;
            assert!(state.parameters.gain() > 0);
            let before = state.clone();
            let original = input.clone();
            let mut called = false;
            assert_eq!(
                state.process_with(
                    FrameSize::Ms20,
                    &input,
                    PrefilterOptions::default(),
                    |size, left, right, previous| {
                        called = true;
                        assert_eq!(size, FrameSize::Ms20);
                        assert_eq!(&left[..1024], &before.raw[0]);
                        assert_eq!(&left[1024..], &input[..960]);
                        assert_eq!(right.is_some(), channels == 2);
                        assert_eq!(previous.period(), before.parameters.period());
                        assert_eq!(previous.gain(), before.parameters.gain());
                        Err(PitchError::ArithmeticOverflow)
                    }
                ),
                Err(PrefilterError::Pitch(PitchError::ArithmeticOverflow))
            );
            assert!(called);
            assert_eq!(state, before);
            assert_eq!(input, original);
            let mut reference = before;
            assert_eq!(
                state.process(FrameSize::Ms20, &input, PrefilterOptions::default())?,
                reference.process(FrameSize::Ms20, &input, PrefilterOptions::default())?
            );
            assert_eq!(state, reference);
        }
        Ok(())
    }
}
