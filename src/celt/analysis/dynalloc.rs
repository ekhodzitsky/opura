//! Native perceptual masking/follower analysis for CELT dynamic bit requests.
use super::{AnalysisError, FrameSize, dynalloc_tables::*};
use crate::celt::{
    allocation_tables::LOG_N,
    energy::BandLayout,
    math,
    output_tables::{BANDS, MEANS},
};

/// Immutable Q24 band logs for current, comparison and previous frames.
#[derive(Clone, Copy, Debug)]
pub struct DynallocInput<'a> {
    /// Current measured mean-adjusted log energies.
    pub current: &'a [[i32; 21]; 2],
    /// Comparison logs; native transient long-window logs include LM/2 scaling.
    pub comparison: &'a [[i32; 21]; 2],
    /// Previous quantized energy histories, before current coarse quantization.
    pub previous: &'a [[i32; 21]; 2],
}
/// Immutable native dynamic-allocation analysis controls.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DynallocOptions {
    /// Effective payload budget in bytes, restricted to 2..1275.
    pub payload_bytes: u16,
    /// Input precision in bits, restricted to 8..24.
    pub lsb_depth: u8,
    /// Short-window context; unavailable at 2.5ms.
    pub transient: bool,
    /// Whether variable-rate analysis is requested.
    pub vbr: bool,
    /// Constrained-VBR policy (effective when VBR is enabled).
    pub constrained_vbr: bool,
    /// LFE disables positive requests but still measures masking and depth.
    pub lfe: bool,
    /// Surround boosts in Q24, each selected value restricted to 0..4.
    pub surround: [i32; 21],
    /// Tone frequency in Q13 radians/sample, from -1 (unavailable) to 25736.
    pub tone_frequency: i16,
    /// Adjusted tone confidence in Q29, restricted to 0..1.
    pub toneishness: i32,
}
impl Default for DynallocOptions {
    fn default() -> Self {
        Self {
            payload_bytes: 128,
            lsb_depth: 16,
            transient: false,
            vbr: false,
            constrained_vbr: false,
            lfe: false,
            surround: [0; 21],
            tone_frequency: -1,
            toneishness: 0,
        }
    }
}
/// Fixed-storage dynamic requests, perceptual weights and signal depth.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DynallocAnalysis {
    requests: [i32; 21],
    boosts: [u8; 21],
    importance: [u8; 21],
    spread: [u8; 21],
    depth: i32,
    total: i32,
}
impl DynallocAnalysis {
    /// Exact native raw requests, including the CBR/CVBR cap-hit convention.
    /// A negative cap remainder means no positive boost flags.
    /// Entries outside the selected interval are zero.
    pub fn requested_boosts(&self) -> &[i32; 21] {
        &self.requests
    }
    /// Requests clipped to 0..255 for the allocation writer. Standard native caps
    /// admit at most 66 positive symbols, so this preserves every encoded symbol.
    pub fn boosts(&self) -> &[u8; 21] {
        &self.boosts
    }
    /// Native perceptual TF weights (13..208), zero outside selected bands.
    pub fn importance(&self) -> &[u8; 21] {
        &self.importance
    }
    /// Native masking/spreading weights (1..32), zero outside selected bands.
    pub fn spread_weights(&self) -> &[u8; 21] {
        &self.spread
    }
    /// Maximum current signal depth above the native noise floor, in Q24.
    pub fn max_depth(&self) -> i32 {
        self.depth
    }
    /// Intended total boost in eighth-bit units (before entropy/cap clipping).
    pub fn total_boost(&self) -> i32 {
        self.total
    }
}
/// Analyze native CELT dynamic-allocation requests without mutating any input.
///
/// Layout must be nonempty with end>=3 (median windows). All three log arrays
/// inspect active channels and bins0..end, with each log restricted to [-32,32]
/// in Q24; inactive channels/tails are ignored. Surround controls are inspected
/// only in selected bands. Other bounds are documented on [`DynallocOptions`].
/// Native noise floors, stereo followers, median filters, tone compensation,
/// importance ordering and VBR/CBR caps are preserved. Output and scratch have
/// fixed storage, with no heap allocation or partial result on errors.
pub fn analyze_dynalloc(
    size: FrameSize,
    layout: BandLayout,
    input: DynallocInput<'_>,
    options: DynallocOptions,
) -> Result<DynallocAnalysis, AnalysisError> {
    let range = layout.bands();
    let end = range.end;
    let channels = layout.channels();
    let lm = size.lm();
    if range.is_empty() || end < 3 {
        return Err(AnalysisError::InvalidBands);
    }
    if !(2..=1275).contains(&options.payload_bytes)
        || !(8..=24).contains(&options.lsb_depth)
        || (options.transient && size == FrameSize::Ms2_5)
        || !(-1..=25736).contains(&options.tone_frequency)
        || !(0..=1 << 29).contains(&options.toneishness)
        || range
            .clone()
            .any(|i| !(0..=4 << 24).contains(&options.surround[i]))
    {
        return Err(AnalysisError::InvalidDynallocParameters);
    }
    for logs in [input.current, input.comparison, input.previous] {
        for channel in &logs[..channels] {
            if channel[..end]
                .iter()
                .any(|&x| !(-(32 << 24)..=32 << 24).contains(&x))
            {
                return Err(AnalysisError::ArithmeticOverflow);
            }
        }
    }
    let mut result = DynallocAnalysis {
        requests: [0; 21],
        boosts: [0; 21],
        importance: [0; 21],
        spread: [0; 21],
        depth: INITIAL_DEPTH,
        total: 0,
    };
    let mut floor = [0; 21];
    for i in 0..end {
        floor[i] = (1 << 20) * i32::from(LOG_N[i])
            + (1 << 23)
            + (9 - i32::from(options.lsb_depth)) * (1 << 24)
            - (i32::from(MEANS[i]) << 20)
            + NOISE_SLOPE * (i as i32 + 5) * (i as i32 + 5);
        for channel in &input.current[..channels] {
            result.depth = result.depth.max(channel[i] - floor[i]);
        }
    }
    let mut mask = [0; 21];
    let mut signal = [0; 21];
    for i in 0..end {
        mask[i] = input.current[0][i] - floor[i];
        if channels == 2 {
            mask[i] = mask[i].max(input.current[1][i] - floor[i]);
        }
    }
    signal[..end].copy_from_slice(&mask[..end]);
    for i in 1..end {
        mask[i] = mask[i].max(mask[i - 1] - (2 << 24));
    }
    for i in (0..end - 1).rev() {
        mask[i] = mask[i].max(mask[i + 1] - (3 << 24));
    }
    for i in range.clone() {
        let smr = signal[i] - 0.max(result.depth - (12 << 24)).max(mask[i]);
        let clamped = smr.clamp(-(5 << 24), 0);
        let shift = -((clamped + (1 << 23)) >> 24);
        result.spread[i] = (32u32 >> shift) as u8;
    }
    if usize::from(options.payload_bytes) < 30 + 5 * lm || options.lfe {
        result.importance[range].fill(13);
        return Ok(result);
    }
    let mut followers = [[0; 21]; 2];
    let mut last = 0;
    for (c, follower) in followers[..channels].iter_mut().enumerate() {
        let mut comparison = input.comparison[c];
        if lm == 0 {
            for (i, value) in comparison[..8.min(end)].iter_mut().enumerate() {
                *value = (*value).max(input.previous[c][i]);
            }
        }
        follower[0] = comparison[0];
        for i in 1..end {
            if comparison[i] > comparison[i - 1] + (1 << 23) {
                last = i;
            }
            follower[i] = (follower[i - 1] + (3 << 23)).min(comparison[i]);
        }
        for i in (0..last).rev() {
            follower[i] = follower[i].min((follower[i + 1] + (2 << 24)).min(comparison[i]));
        }
        for i in 2..end - 2 {
            follower[i] = follower[i].max(median::<5>(&comparison[i - 2..i + 3]) - (1 << 24));
        }
        let first = median::<3>(&comparison[..3]) - (1 << 24);
        follower[0] = follower[0].max(first);
        follower[1] = follower[1].max(first);
        let tail = median::<3>(&comparison[end - 3..end]) - (1 << 24);
        follower[end - 2] = follower[end - 2].max(tail);
        follower[end - 1] = follower[end - 1].max(tail);
        for i in 0..end {
            follower[i] = follower[i].max(floor[i]);
        }
    }
    for i in range.clone() {
        let residual = if channels == 2 {
            followers[1][i] = followers[1][i].max(followers[0][i] - (4 << 24));
            followers[0][i] = followers[0][i].max(followers[1][i] - (4 << 24));
            ((input.current[0][i] - followers[0][i]).max(0)
                + (input.current[1][i] - followers[1][i]).max(0))
                >> 1
        } else {
            (input.current[0][i] - followers[0][i]).max(0)
        };
        followers[0][i] = residual.max(options.surround[i]);
        result.importance[i] = importance(followers[0][i]);
    }
    let limited = !options.vbr || (options.constrained_vbr && !options.transient);
    for i in range.clone() {
        if (!options.vbr || options.constrained_vbr) && !options.transient {
            followers[0][i] >>= 1;
        }
        if i < 8 {
            followers[0][i] *= 2;
        }
        if i >= 12 {
            followers[0][i] >>= 1;
        }
    }
    if options.toneishness > super::transient_tables::TONE_GUARD {
        let bin = (i32::from(options.tone_frequency) * TONE_BIN_Q9 + (1 << 21)) >> 22;
        for i in range.clone() {
            for (distance, gain) in [(0, 2 << 24), (1, 1 << 24), (2, 1 << 24), (3, 1 << 23)] {
                if bin >= i32::from(BANDS[i]) - distance
                    && bin <= i32::from(BANDS[i + 1]) + distance
                {
                    followers[0][i] += gain;
                }
            }
        }
        if bin >= i32::from(BANDS[end]) {
            followers[0][end - 1] += 2 << 24;
            followers[0][end - 2] += 1 << 24;
        }
    }
    for i in range.clone() {
        let follower = followers[0][i].min(4 << 24) >> 8;
        let width = (channels as i32 * i32::from(BANDS[i + 1] - BANDS[i])) << lm;
        let (boost, bits) = if width < 6 {
            let boost = follower >> 16;
            (boost, boost * width * 8)
        } else if width > 48 {
            let boost = (follower * 8) >> 16;
            (boost, boost * width)
        } else {
            let boost = (follower * width / 6) >> 16;
            (boost, boost * 48)
        };
        if limited && (result.total + bits) >> 6 > 2 * i32::from(options.payload_bytes) / 3 {
            let cap = (2 * i32::from(options.payload_bytes) / 3) << 6;
            result.requests[i] = cap - result.total;
            result.total = cap;
            break;
        }
        result.requests[i] = boost;
        result.total += bits;
    }
    // Native cap rounding can leave a negative raw request; j<request then
    // emits no positive flags. Large counts stop at the standard cap first.
    for i in range {
        result.boosts[i] = result.requests[i].clamp(0, 255) as u8;
    }
    Ok(result)
}
fn median<const N: usize>(values: &[i32]) -> i32 {
    let mut sorted = [0; N];
    sorted.copy_from_slice(values);
    sorted.sort_unstable();
    sorted[N / 2]
}

fn importance(follower: i32) -> u8 {
    let exponent = (follower.min(4 << 24) + (1 << 13)) >> 14;
    ((13 * math::exp2(exponent as i16) + (1 << 15)) >> 16) as u8
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_importance_rounding_medians_and_constants_match()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut counts = [0; 2];
        let mut constants = false;
        for line in include_str!("../../../tests/celt-dynalloc-math-fixtures.txt").lines() {
            if line.starts_with('#') {
                continue;
            }
            let c: Vec<_> = line.split_whitespace().collect();
            match c[0] {
                "constants" => {
                    let values: Vec<i32> =
                        c[1..].iter().map(|s| s.parse()).collect::<Result<_, _>>()?;
                    assert_eq!(values, [INITIAL_DEPTH, NOISE_SLOPE, TONE_BIN_Q9]);
                    constants = true;
                }
                "importance" => {
                    assert_eq!(i32::from(importance(c[1].parse()?)), c[2].parse::<i32>()?);
                    counts[0] += 1;
                }
                "median" => {
                    let n = c[1].parse::<usize>()?;
                    let values: Vec<i32> = c[2..2 + n]
                        .iter()
                        .map(|s| s.parse())
                        .collect::<Result<_, _>>()?;
                    let result = match n {
                        3 => median::<3>(&values),
                        5 => median::<5>(&values),
                        _ => return Err("unknown median geometry".into()),
                    };
                    assert_eq!(result, c[2 + n].parse::<i32>()?);
                    counts[1] += 1;
                }
                _ => return Err("unknown dynamic scalar row".into()),
            }
        }
        assert!(constants);
        assert_eq!(counts[1], 270);
        assert_eq!(counts[0], 24579);
        Ok(())
    }
}
