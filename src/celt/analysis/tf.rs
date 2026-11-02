//! Native L1/Haar temporal-frequency analysis and weighted binary path search.
use super::{AnalysisError, FrameSize};
use crate::celt::{bands, control_tables::TF_SELECT, output_tables::BANDS};

/// Immutable controls for native time/frequency resolution analysis.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TfAnalysisOptions {
    /// Short-window input; unavailable for 2.5ms frames.
    pub transient: bool,
    /// Cost of switching adjacent binary decisions (native units).
    pub lambda: u16,
    /// Temporal estimate in Q14, restricted to 0..16384.
    pub estimate: i16,
    /// Per-band perceptual weights; native neutral importance is 13.
    pub importance: [u8; 21],
}
impl Default for TfAnalysisOptions {
    fn default() -> Self {
        Self {
            transient: false,
            lambda: 80,
            estimate: 0,
            importance: [13; 21],
        }
    }
}
/// Binary TF writer decisions and pre-path Q1 resolution metrics.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TfAnalysis {
    decisions: [bool; 21],
    metrics: [i8; 21],
    select: bool,
    end: usize,
}
impl TfAnalysis {
    /// Binary decisions for TF signalling; entries after end are false.
    pub fn decisions(&self) -> &[bool; 21] {
        &self.decisions
    }
    /// Per-band best L1 levels in Q1, including narrow-band midpoint correction.
    /// Entries after end are zero.
    pub fn metrics(&self) -> &[i8; 21] {
        &self.metrics
    }
    /// Alternate native TF mapping; only selected for short-window input.
    pub fn select(&self) -> bool {
        self.select
    }
    /// Exclusive analyzed band end.
    pub fn end(&self) -> usize {
        self.end
    }
}
/// Analyze one selected channel of normalized Q24 CELT coefficients.
///
/// Input length is exactly `size.samples()`, with end in 1..21. Each active
/// band's squared norm must be <=5/4 in Q48, including zero vectors and ordinary
/// PCM band normalization. Only active bins/weights are used; tail bins may be
/// arbitrary. Native L1/Haar levels and weighted Viterbi ties are preserved.
/// No input mutation or heap allocation occurs, including on errors.
pub fn analyze_tf(
    size: FrameSize,
    normalized: &[i32],
    end: usize,
    options: TfAnalysisOptions,
) -> Result<TfAnalysis, AnalysisError> {
    if normalized.len() != size.samples() {
        return Err(AnalysisError::InvalidGeometry);
    }
    if !(1..=21).contains(&end) {
        return Err(AnalysisError::InvalidBands);
    }
    if !(0..=16384).contains(&options.estimate) || (options.transient && size == FrameSize::Ms2_5) {
        return Err(AnalysisError::InvalidTfParameters);
    }
    let lm = size.lm();
    let mut result = TfAnalysis {
        decisions: [false; 21],
        metrics: [0; 21],
        select: false,
        end,
    };
    let bias = bias(options.estimate);
    let mut tmp = [0; 176];
    let mut extra = [0; 176];
    for i in 0..end {
        let width = usize::from(BANDS[i + 1] - BANDS[i]);
        let n = width << lm;
        let input = &normalized[(usize::from(BANDS[i]) << lm)..(usize::from(BANDS[i + 1]) << lm)];
        let energy: i128 = input.iter().map(|&x| i128::from(x) * i128::from(x)).sum();
        if energy > 5 * (1i128 << 46) {
            return Err(AnalysisError::ArithmeticOverflow);
        }
        let values = &mut tmp[..n];
        values.copy_from_slice(input);
        let narrow = width == 1;
        let mut best = l1(values, if options.transient { lm as i32 } else { 0 }, bias);
        let mut level = 0i8;
        if options.transient && !narrow {
            extra[..n].copy_from_slice(values);
            bands::haar(&mut extra[..n], 1 << lm).map_err(|_| AnalysisError::InvalidGeometry)?;
            let cost = l1(&extra[..n], lm as i32 + 1, bias);
            if cost < best {
                best = cost;
                level = -1;
            }
        }
        let levels = lm + usize::from(!(options.transient || narrow));
        for k in 0..levels {
            bands::haar(values, 1 << k).map_err(|_| AnalysisError::InvalidGeometry)?;
            let b = if options.transient { lm - k - 1 } else { k + 1 };
            let cost = l1(values, b as i32, bias);
            if cost < best {
                best = cost;
                level = k as i8 + 1;
            }
        }
        let metric = if options.transient {
            2 * level
        } else {
            -2 * level
        };
        result.metrics[i] = if narrow && (metric == 0 || metric == -2 * lm as i8) {
            metric - 1
        } else {
            metric
        };
    }
    let (cost0, decisions0) = path(&result.metrics[..end], size, options, false);
    let (cost1, decisions1) = path(&result.metrics[..end], size, options, true);
    result.select = options.transient && cost1 < cost0;
    result.decisions = if result.select {
        decisions1
    } else {
        decisions0
    };
    Ok(result)
}
fn bias(estimate: i16) -> i32 {
    (1311 * (-4096).max(8192 - i32::from(estimate))) >> 14
}
fn l1(values: &[i32], level: i32, bias: i32) -> i32 {
    let sum: i32 = values.iter().map(|&x| (x >> 10).abs()).sum();
    // Bounded band norm keeps all Q14 coordinates in signed16 after Haar stages.
    sum + ((i64::from(level * bias) * i64::from(sum)) >> 15) as i32
}
fn path(
    metrics: &[i8],
    size: FrameSize,
    options: TfAnalysisOptions,
    select: bool,
) -> (i32, [bool; 21]) {
    let table = TF_SELECT[size.lm()];
    let base = 4 * usize::from(options.transient) + 2 * usize::from(select);
    let distortion = |i: usize, state: usize| {
        i32::from(options.importance[i])
            * (i32::from(metrics[i]) - 2 * i32::from(table[base + state])).abs()
    };
    let lambda = i32::from(options.lambda);
    let mut cost0 = distortion(0, 0);
    let mut cost1 = distortion(0, 1) + if options.transient { 0 } else { lambda };
    let mut path0 = [false; 21];
    let mut path1 = [false; 21];
    for i in 1..metrics.len() {
        // Native predecessor ties select state1 in both paths.
        path0[i] = cost0 >= cost1 + lambda;
        path1[i] = cost0 + lambda >= cost1;
        let next0 = cost0.min(cost1 + lambda) + distortion(i, 0);
        let next1 = (cost0 + lambda).min(cost1) + distortion(i, 1);
        cost0 = next0;
        cost1 = next1;
    }
    let mut decisions = [false; 21];
    let last = metrics.len() - 1;
    decisions[last] = cost0 >= cost1;
    for i in (0..last).rev() {
        decisions[i] = if decisions[i + 1] {
            path1[i + 1]
        } else {
            path0[i + 1]
        };
    }
    (cost0.min(cost1), decisions)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_bias_and_l1_scalars_match() -> Result<(), Box<dyn std::error::Error>> {
        let mut values = Vec::new();
        let mut estimates = 0;
        let mut vectors = 0;
        let mut scalars = 0;
        for line in include_str!("../../../tests/celt-tf-math-fixtures.txt").lines() {
            if line.starts_with('#') {
                continue;
            }
            let c: Vec<_> = line.split_whitespace().collect();
            match c[0] {
                "bias" => {
                    let start = c[1].parse::<i16>()?;
                    let count = c[2].parse::<usize>()?;
                    let mut hash = 14695981039346656037u64;
                    for i in 0..count {
                        for byte in bias(start + i as i16).to_le_bytes() {
                            hash = (hash ^ u64::from(byte)).wrapping_mul(1099511628211);
                        }
                    }
                    assert_eq!(hash, c[3].parse::<u64>()?);
                    estimates += count;
                }
                "vector" => {
                    values = c[2..].iter().map(|s| s.parse()).collect::<Result<_, _>>()?;
                    assert_eq!(values.len(), c[1].parse::<usize>()?);
                    vectors += 1;
                }
                "l1" => {
                    assert_eq!(
                        l1(&values, c[1].parse()?, c[2].parse()?),
                        c[3].parse::<i32>()?
                    );
                    scalars += 1;
                }
                _ => return Err("unknown TF scalar row".into()),
            }
        }
        assert_eq!((estimates, vectors, scalars), (16385, 96, 2400));
        Ok(())
    }
}
