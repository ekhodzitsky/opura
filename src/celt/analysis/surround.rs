//! Persistent surround masking analysis, before per-stream CELT allocation.
//! Copyright (c) 2007-2008 CSIRO; 2007-2011 Xiph.Org Foundation;
//! 2008 Gregory Maxwell. Written by Jean-Marc Valin and Gregory Maxwell.
//! Q24 log-sum return/offset corrections are documented in the validation record.
//! See THIRD_PARTY_NOTICES.md. This stage alone does not enable surround encoding.
use super::{AnalysisError, analyze_bands, analyze_frame};
use crate::celt::{
    FrameSize,
    input::{InputRate, PreemphasisState},
    math,
    output_tables::{BANDS, MEANS},
};

const LOGSUM: [i32; 17] = [
    8388608, 4907022, 2700528, 1425434, 733691, 372406, 187635, 94181, 47183, 0, 0, 0, 0, 0, 0, 0,
    0,
];

/// Mean-adjusted channel energies minus the native spatial masking estimate.
///
/// Each channel has21 signed Q24 values, in the same Vorbis order as input.
/// A layout's LFE channel receives exactly zero masks, while its preemphasis
/// and overlap histories still advance. These are analysis values, not positive
/// dynamic allocation requests or an encoded packet.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SurroundMask {
    channels: Vec<[i32; 21]>,
}
impl SurroundMask {
    /// Complete channel-major Q24 masks.
    pub fn channels(&self) -> &[[i32; 21]] {
        &self.channels
    }
    /// One Vorbis-order channel, or `None` if outside the layout.
    pub fn channel(&self, index: usize) -> Option<&[i32; 21]> {
        self.channels.get(index)
    }
}

/// Stateful3..8channel surround preemphasis, overlap and spatial masking.
///
/// The rate and Vorbis channel layout are immutable. New/reset state has zero
/// PCM histories. Analysis follows the original fixed-point arithmetic with two
/// Q24 width/scale corrections: retain the log-sum return in32 bits and convert
/// the channel-count logarithm from Q10 to Q24 before adding it to masks. The
/// optional oracle records pristine and corrected output separately.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SurroundState {
    rate: InputRate,
    preemphasis: Vec<PreemphasisState>,
    overlap: Vec<[i32; 120]>,
}
impl SurroundState {
    /// Create a standard Vorbis surround layout at one of five input rates.
    /// Mono/stereo use their existing non-surround pipeline and are rejected.
    pub fn new(rate: InputRate, channels: usize) -> Result<Self, AnalysisError> {
        if !(3..=8).contains(&channels) {
            return Err(AnalysisError::InvalidGeometry);
        }
        Ok(Self {
            rate,
            preemphasis: vec![PreemphasisState::default(); channels],
            overlap: vec![[0; 120]; channels],
        })
    }
    /// Immutable PCM input rate.
    pub fn input_rate(&self) -> InputRate {
        self.rate
    }
    /// Immutable channel count.
    pub fn channels(&self) -> usize {
        self.preemphasis.len()
    }
    /// Clear PCM histories, retaining input rate and layout.
    pub fn reset(&mut self) {
        for preemphasis in &mut self.preemphasis {
            preemphasis.reset();
        }
        self.overlap.fill([0; 120]);
    }
    /// Analyze exactly one interleaved Vorbis-order signed16 packet of PCM.
    ///
    /// `duration48` is120/240/480/960/1920/2880/3840/4800/5760 samples per channel
    /// on the48kHz clock. Input length is `duration48/rate.factor()*channels`.
    /// Long packets retain the maximum amplitude in each band over20ms windows,
    /// then apply spectral spreading and left/center/right masking. Low-rate
    /// spectra receive native scaling/cutoff before energy analysis.
    ///
    /// Input is bounded by5760 samples/channel and eight channels; the largest
    /// scratch context holds5880 signed32 values, including the overlap.
    /// Any invalid geometry or transform/arithmetic error preserves the complete
    /// state. Successful analysis advances every channel, including LFE. The
    /// enclosing encoder must skip this stage for restricted-SILK and own its
    /// transaction together with constituent encoders; this API emits no audio.
    pub fn analyze(
        &mut self,
        duration48: usize,
        pcm: &[i16],
    ) -> Result<SurroundMask, AnalysisError> {
        if !matches!(
            duration48,
            120 | 240 | 480 | 960 | 1920 | 2880 | 3840 | 4800 | 5760
        ) || pcm.len() != duration48 / self.rate.factor() * self.channels()
        {
            return Err(AnalysisError::InvalidGeometry);
        }
        let mut staged = self.clone();
        let result = staged.analyze_inner(duration48, pcm)?;
        *self = staged;
        Ok(result)
    }
    fn analyze_inner(
        &mut self,
        duration: usize,
        pcm: &[i16],
    ) -> Result<SurroundMask, AnalysisError> {
        let size = match duration {
            120 => FrameSize::Ms2_5,
            240 => FrameSize::Ms5,
            480 => FrameSize::Ms10,
            _ => FrameSize::Ms20,
        };
        let n = size.samples();
        let channels = self.channels();
        let roles: &[usize] = match channels {
            3 => &[1, 2, 3],
            4 => &[1, 3, 1, 3],
            5 => &[1, 2, 3, 1, 3],
            6 => &[1, 2, 3, 1, 3, 0],
            7 => &[1, 2, 3, 1, 3, 2, 0],
            8 => &[1, 2, 3, 1, 3, 1, 3, 0],
            _ => return Err(AnalysisError::InvalidGeometry),
        };
        let mut result = SurroundMask {
            channels: vec![[0; 21]; channels],
        };
        let mut spatial = [[-(28 << 24); 21]; 3];
        let mut context = vec![0; duration + 120];
        for (channel, &role) in roles.iter().enumerate() {
            let mono: Vec<i16> = pcm
                .chunks_exact(channels)
                .map(|frame| frame[channel])
                .collect();
            context[..120].copy_from_slice(&self.overlap[channel]);
            for (index, block) in mono.chunks_exact(n / self.rate.factor()).enumerate() {
                self.preemphasis[channel]
                    .process(
                        size,
                        self.rate,
                        block,
                        1,
                        0,
                        &mut context[120 + index * n..120 + (index + 1) * n],
                    )
                    .map_err(|_| AnalysisError::InvalidGeometry)?;
            }
            let mut amplitudes = [0; 21];
            for offset in (0..duration).step_by(n) {
                let spectrum = analyze_frame(
                    size,
                    &context[offset..offset + n + 120],
                    1,
                    1,
                    self.rate,
                    false,
                )?;
                let bands = analyze_bands(size, spectrum.values(), 1, 21, 21)?;
                let measured = bands.amplitudes(0).ok_or(AnalysisError::InvalidGeometry)?;
                for (maximum, &value) in amplitudes.iter_mut().zip(measured) {
                    *maximum = (*maximum).max(value);
                }
            }
            let logs = &mut result.channels[channel];
            for (band, (&amplitude, log)) in amplitudes.iter().zip(logs.iter_mut()).enumerate() {
                *log = (i32::from(math::log2(amplitude)?) << 14) - (i32::from(MEANS[band]) << 20)
                    + (2 << 24);
            }
            for band in 1..21 {
                logs[band] = logs[band].max(logs[band - 1] - (1 << 24));
            }
            for band in (0..20).rev() {
                logs[band] = logs[band].max(logs[band + 1] - (2 << 24));
            }
            for (band, &log) in logs.iter().enumerate() {
                match role {
                    1 | 3 => spatial[role - 1][band] = log_sum(spatial[role - 1][band], log),
                    2 => {
                        spatial[0][band] = log_sum(spatial[0][band], log - (1 << 23));
                        spatial[2][band] = log_sum(spatial[2][band], log - (1 << 23));
                    }
                    _ => {}
                }
            }
            self.overlap[channel].copy_from_slice(&context[duration..]);
        }
        for band in 0..21 {
            spatial[1][band] = spatial[0][band].min(spatial[2][band]);
        }
        let channel_offset = (i32::from(math::log2((2 << 14) / (channels as i32 - 1))?) << 14) >> 1;
        for mask in &mut spatial {
            for log in mask {
                *log += channel_offset;
            }
        }
        for (&role, logs) in roles.iter().zip(&mut result.channels) {
            if role == 0 {
                logs.fill(0);
            } else {
                for (log, mask) in logs.iter_mut().zip(spatial[role - 1]) {
                    *log -= mask;
                }
            }
        }
        Ok(result)
    }
}
fn log_sum(a: i32, b: i32) -> i32 {
    let maximum = a.max(b);
    let diff = (a - b).abs();
    if diff >= 8 << 24 {
        return maximum;
    }
    let low = (diff >> 23) as usize;
    let fraction = (diff - ((low as i32) << 23)) >> 8;
    maximum
        + LOGSUM[low]
        + ((i64::from(fraction) * i64::from(LOGSUM[low + 1] - LOGSUM[low])) >> 15) as i32
}

/// Per-stream CELT controls derived from one/two channels' surround masks.
/// This is the original mask reduction, without the spectral-stage corrections.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SurroundAllocation {
    dynamic: [i32; 21],
    trim: i32,
    masking: i32,
}
impl SurroundAllocation {
    /// Nonnegative Q24 dynamic-allocation requests. Unselected bands are zero.
    pub fn dynamic(&self) -> &[i32; 21] {
        &self.dynamic
    }
    /// Signed Q24 tilt, for [`super::TrimOptions::surround_trim`].
    pub fn trim(&self) -> i32 {
        self.trim
    }
    /// Signed Q24 masking offset consumed by native surround VBR targeting.
    pub fn masking(&self) -> i32 {
        self.masking
    }
}

/// Reduce one/two coded channels' spatial masks to CELT allocation controls.
///
/// `last_coded_bands` is0..21; zero/one use the native minimum of two measured
/// bands. Selected values must be in[-32,32] Q24. Later bands are ignored, even
/// if outside this domain, and outputs there are zero. Inputs are never changed.
///
/// This stage applies only when the encoder has a surround mask and is neither
/// LFE nor Hybrid. Absence of a mask must bypass it: an explicitly zero mask has
/// native positive VBR offset and is not equivalent to no surround context.
pub fn analyze_surround_allocation(
    masks: &[[i32; 21]],
    last_coded_bands: usize,
) -> Result<SurroundAllocation, AnalysisError> {
    if !(1..=2).contains(&masks.len()) {
        return Err(AnalysisError::InvalidGeometry);
    }
    if last_coded_bands > 21 {
        return Err(AnalysisError::InvalidBands);
    }
    let end = last_coded_bands.max(2);
    if masks.iter().any(|m| {
        m[..end]
            .iter()
            .any(|&v| !(-(32 << 24)..=32 << 24).contains(&v))
    }) {
        return Err(AnalysisError::ArithmeticOverflow);
    }
    let mut average = 0;
    let mut diff = 0;
    let mut count = 0;
    for mask in masks {
        for (band, &value) in mask[..end].iter().enumerate() {
            let mut value = value.clamp(-(2 << 24), 1 << 22);
            if value > 0 {
                value >>= 1;
            }
            let value = value >> 14;
            let width = i32::from(BANDS[band + 1] - BANDS[band]);
            average += value * width;
            count += width;
            diff += value * (1 + 2 * band as i32 - end as i32);
        }
    }
    average = ((average / count) << 14) + 3355443;
    diff = ((diff * 6 / (masks.len() * (end - 1) * (end + 1) * end) as i32) << 14) >> 1;
    diff = diff.clamp(-520094, 520094);
    let mut midband = 0;
    while BANDS[midband + 1] < BANDS[end] / 2 {
        midband += 1;
    }
    let mut dynamic = [0; 21];
    let mut count_dynamic = 0;
    for (band, request) in dynamic[..end].iter_mut().enumerate() {
        let line = average + diff * (band as i32 - midband as i32);
        let unmask = if masks.len() == 2 {
            masks[0][band].max(masks[1][band])
        } else {
            masks[0][band]
        }
        .min(0)
            - line;
        if unmask > 1 << 22 {
            *request = unmask - (1 << 22);
            count_dynamic += 1;
        }
    }
    if count_dynamic >= 3 {
        average += 1 << 22;
        if average > 0 {
            average = 0;
            diff = 0;
            dynamic.fill(0);
        } else {
            for value in &mut dynamic[..end] {
                *value = (*value - (1 << 22)).max(0);
            }
        }
    }
    Ok(SurroundAllocation {
        dynamic,
        trim: 64 * diff,
        masking: average + 3355443,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
    fn signal(
        rate: InputRate,
        channels: usize,
        duration: usize,
        step: usize,
        kind: usize,
    ) -> Vec<i16> {
        let count = duration / rate.factor();
        let mut result = Vec::new();
        let mut seed = 6716u32.wrapping_add(37 * step as u32);
        for frame in 0..count {
            for channel in 0..channels {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                let phase = (frame + step * 13 + channel * 37) % (31 + 11 * channel);
                let triangle = (phase as i32 - (15 + 5 * channel as i32)) * 173;
                let value = match kind {
                    0 => 0,
                    1 => {
                        if frame == count / 2 + channel {
                            30000
                        } else {
                            0
                        }
                    }
                    2 => (seed >> 16) as i32 - 32768,
                    3 => {
                        if phase % 9 < 4 {
                            20000
                        } else {
                            -20000
                        }
                    }
                    4 => {
                        if channel == 1 {
                            triangle
                        } else {
                            0
                        }
                    }
                    5 => {
                        if (frame + channel) % 2 == 0 {
                            -32768
                        } else {
                            32767
                        }
                    }
                    6 => triangle >> 10,
                    _ => triangle + ((seed >> 24) as i32 - 128) * 3,
                };
                result.push(value as i16);
            }
        }
        result
    }
    fn hash(bytes: impl Iterator<Item = u8>) -> u64 {
        bytes.fold(14695981039346656037, |h, b| {
            (h ^ u64::from(b)).wrapping_mul(1099511628211)
        })
    }
    #[test]
    fn original_corrected_q24_surround_masks_and_pcm_histories_match() -> Result<()> {
        let mut state = None;
        let mut rows = 0;
        let mut differences = 0;
        for line in include_str!("../../../tests/surround-analysis-fixtures.txt").lines() {
            let fields: Vec<_> = line.split_whitespace().collect();
            if fields.is_empty() || fields[0] == "#" {
                continue;
            }
            match fields[0] {
                "case" => {
                    let rate = match fields[1] {
                        "8000" => InputRate::Khz8,
                        "12000" => InputRate::Khz12,
                        "16000" => InputRate::Khz16,
                        "24000" => InputRate::Khz24,
                        "48000" => InputRate::Khz48,
                        _ => return Err("unexpected fixture rate".into()),
                    };
                    state = Some(SurroundState::new(rate, fields[2].parse()?)?);
                }
                "frame" => {
                    let state = state.as_mut().ok_or("fixture state absent")?;
                    let duration = fields[1].parse()?;
                    let reset: u8 = fields[2].parse()?;
                    let step = fields[3].parse()?;
                    let kind = fields[4].parse()?;
                    let input_hash: u64 = fields[5].parse()?;
                    let overlap_hash: u64 = fields[6].parse()?;
                    let pristine_hash: u64 = fields[7].parse()?;
                    let expected: Vec<i32> = fields[8..]
                        .iter()
                        .map(|s| s.parse())
                        .collect::<std::result::Result<_, _>>()?;
                    assert_eq!(expected.len(), 22 * state.channels());
                    let pcm = signal(state.rate, state.channels(), duration, step, kind);
                    assert_eq!(
                        hash(pcm.iter().flat_map(|x| x.to_le_bytes())),
                        input_hash,
                        "input row{rows}"
                    );
                    if reset != 0 {
                        state.reset();
                    }
                    let masks = state.analyze(duration, &pcm)?;
                    let actual: Vec<i32> = masks.channels().iter().flatten().copied().collect();
                    assert_eq!(actual, expected[state.channels()..], "masks row{rows}");
                    let preemph: Vec<i32> = state
                        .preemphasis
                        .iter()
                        .map(PreemphasisState::memory)
                        .collect();
                    assert_eq!(preemph, expected[..state.channels()], "preemph row{rows}");
                    assert_eq!(
                        hash(state.overlap.iter().flatten().flat_map(|x| x.to_le_bytes())),
                        overlap_hash,
                        "overlap row{rows}"
                    );
                    differences += usize::from(
                        hash(actual.iter().flat_map(|x| x.to_le_bytes())) != pristine_hash,
                    );
                    if state.channels() >= 6 {
                        assert_eq!(masks.channel(state.channels() - 1), Some(&[0; 21]));
                    }
                    rows += 1;
                }
                _ => return Err("unexpected surround fixture record".into()),
            }
        }
        assert_eq!(rows, 2160);
        assert_eq!(differences, 2160);
        Ok(())
    }
    #[test]
    fn surround_analysis_bounds_reset_and_lfe_histories_are_transactional() -> Result<()> {
        for n in [0, 1, 2, 9, 255, usize::MAX] {
            assert_eq!(
                SurroundState::new(InputRate::Khz48, n),
                Err(AnalysisError::InvalidGeometry)
            );
        }
        for channels in 3..=8 {
            for rate in [
                InputRate::Khz8,
                InputRate::Khz12,
                InputRate::Khz16,
                InputRate::Khz24,
                InputRate::Khz48,
            ] {
                let mut state = SurroundState::new(rate, channels)?;
                let pcm = signal(rate, channels, 960, 7, 7);
                let initial = state.analyze(960, &pcm)?;
                let saved = state.clone();
                for duration in [0, 1, 119, 121, 1000, 5761, usize::MAX] {
                    assert_eq!(
                        state.analyze(duration, &pcm),
                        Err(AnalysisError::InvalidGeometry)
                    );
                    assert_eq!(state, saved);
                }
                assert_eq!(
                    state.analyze(960, &pcm[1..]),
                    Err(AnalysisError::InvalidGeometry)
                );
                assert_eq!(state, saved);
                assert!(state.overlap.iter().all(|c| c.iter().any(|&x| x != 0)));
                assert!(initial.channel(channels).is_none());
                state.reset();
                assert_eq!(state, SurroundState::new(rate, channels)?);
                assert_eq!(state.analyze(960, &pcm)?, initial);
            }
        }
        assert_eq!(log_sum(0, 0), 1 << 23);
        assert_eq!(log_sum(8 << 24, 0), 8 << 24);
        Ok(())
    }

    #[test]
    fn unchanged_native_surround_allocation_controls_match() -> Result<()> {
        let mut rows = 0;
        let mut boosted = 0;
        let mut tilted = 0;
        for line in include_str!("../../../tests/surround-allocation-fixtures.txt")
            .lines()
            .filter(|l| !l.starts_with('#'))
        {
            let values: Vec<i32> = line
                .split_whitespace()
                .map(str::parse)
                .collect::<std::result::Result<_, _>>()?;
            let channels = values[0] as usize;
            assert_eq!(values.len(), 25 + 21 * channels);
            let end = 2 + 21 * channels;
            let mut masks = [[0; 21]; 2];
            for (mask, source) in masks[..channels]
                .iter_mut()
                .zip(values[2..end].chunks_exact(21))
            {
                mask.copy_from_slice(source);
            }
            let expected = analyze_surround_allocation(&masks[..channels], values[1] as usize)?;
            assert_eq!(
                (expected.trim(), expected.masking()),
                (values[end], values[end + 1]),
                "controls row{rows}"
            );
            assert_eq!(
                expected.dynamic().as_slice(),
                &values[end + 2..],
                "dynamic row{rows}"
            );
            assert!(
                expected
                    .dynamic()
                    .iter()
                    .all(|&v| (0..=4 << 24).contains(&v))
            );
            boosted += usize::from(expected.dynamic().iter().any(|&v| v > 0));
            tilted += usize::from(expected.trim() != 0);
            rows += 1;
        }
        assert_eq!(rows, 2816);
        assert!(boosted > 0 && tilted > 0);
        Ok(())
    }

    #[test]
    fn surround_allocation_validates_selected_bands_and_distinguishes_zero_mask() -> Result<()> {
        let masks = [[0; 21]; 2];
        let zero = analyze_surround_allocation(&masks, 0)?;
        assert_eq!(zero.dynamic(), &[0; 21]);
        assert_eq!(zero.trim(), 0);
        assert_eq!(zero.masking(), 2 * 3355443);
        assert_eq!(
            analyze_surround_allocation(&[], 0),
            Err(AnalysisError::InvalidGeometry)
        );
        assert_eq!(
            analyze_surround_allocation(&[[0; 21]; 3], 0),
            Err(AnalysisError::InvalidGeometry)
        );
        assert_eq!(
            analyze_surround_allocation(&masks, usize::MAX),
            Err(AnalysisError::InvalidBands)
        );
        let mut bad = masks;
        bad[0][1] = i32::MIN;
        assert_eq!(
            analyze_surround_allocation(&bad, 0),
            Err(AnalysisError::ArithmeticOverflow)
        );
        bad[0][1] = 0;
        bad[0][2..].fill(i32::MIN);
        assert_eq!(analyze_surround_allocation(&bad, 1)?, zero);
        Ok(())
    }
}
