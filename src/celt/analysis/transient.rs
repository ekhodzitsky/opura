//! Native tonal guard and forward/backward temporal masking analysis.
use super::{AnalysisError, FrameSize, math, transient_tables::*};

/// Stateless tone and temporal-mask diagnostics for one PCM analysis context.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TransientAnalysis {
    transient: bool,
    weak: bool,
    estimate: i16,
    channel: usize,
    frequency: i16,
    toneishness: i32,
    metric: i32,
}
impl TransientAnalysis {
    // Native TF gating reduces tone confidence when temporal masking is strong.
    pub(crate) fn tf_tonal(self) -> bool {
        self.toneishness
            .min((1 << 29) - (i32::from(self.estimate) << 15))
            >= TONE_GUARD
    }

    /// Native temporal-mask metric after the tonal guard (zero when suppressed).
    pub fn mask_metric(self) -> i32 {
        self.metric
    }
    /// Strong attack after the low-frequency tonal guard and weak policy.
    pub fn transient(self) -> bool {
        self.transient
    }
    /// Moderate attack suppressed by the optional weak-transient policy.
    pub fn weak_transient(self) -> bool {
        self.weak
    }
    /// Native temporal/VBR estimate in Q14.
    pub fn tf_estimate(self) -> i16 {
        self.estimate
    }
    /// Channel with the largest temporal-mask metric (first on a tie).
    pub fn channel(self) -> usize {
        self.channel
    }
    /// Estimated tone frequency in Q13 radians/sample, or -1 when unavailable.
    pub fn tone_frequency(self) -> i16 {
        self.frequency
    }
    /// Squared tone-pole radius in Q29; zero when no complex pole pair is found.
    pub fn toneishness(self) -> i32 {
        self.toneishness
    }
}
/// Analyze planar pre-emphasized PCM including 120 raw overlap samples/channel.
///
/// Input length is exactly `channels*(size.samples()+120)`, with one/two channels.
/// Coefficient magnitudes are <=2^28, covering signed16 PCM pre-emphasis. Context
/// is before any pitch prefilter. LM0 diagnostics are permitted; packet coding
/// still uses a long window at that duration. No allocation or input mutation
/// occurs. Unsupported geometry/magnitudes fail without a partial result.
pub fn analyze_transients(
    size: FrameSize,
    input: &[i32],
    channels: usize,
    allow_weak: bool,
) -> Result<TransientAnalysis, AnalysisError> {
    let n = size.samples() + 120;
    if !(1..=2).contains(&channels) || input.len() != channels * n {
        return Err(AnalysisError::InvalidGeometry);
    }
    let mut maximum = 0i32;
    for &x in input {
        let abs = i64::from(x).abs();
        if abs > 1 << 28 {
            return Err(AnalysisError::ArithmeticOverflow);
        }
        maximum = maximum.max(abs as i32);
    }
    let (frequency, toneishness) = tone(input, n, channels)?;
    let shift = (ilog(1 + maximum) - 14).max(0) as u32;
    let len2 = n / 2;
    let forward = if allow_weak { 5 } else { 4 };
    let mut metric = 0;
    let mut selected = 0;
    let mut tmp = [0i16; 1080];
    for c in 0..channels {
        let mut mem0 = 0;
        let mut mem1 = 0;
        for i in 0..n {
            let x = input[c * n + i] >> shift;
            let y = mem0 + x;
            mem0 = mem1 + y - 2 * x;
            mem1 = x - (y >> 1);
            tmp[i] = ((y + 2) >> 2).clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16;
        }
        tmp[..12].fill(0);
        let max = tmp[..n]
            .iter()
            .map(|&x| i32::from(x).abs())
            .max()
            .ok_or(AnalysisError::InvalidGeometry)?
            .max(1);
        let normalization = 14 - ilog(max);
        if normalization < 0 {
            return Err(AnalysisError::ArithmeticOverflow);
        }
        for x in &mut tmp[..n] {
            *x = (i32::from(*x) << normalization) as i16;
        }
        let mut mean = 0;
        mem0 = 0;
        for i in 0..len2 {
            let a = i32::from(tmp[2 * i]);
            let b = i32::from(tmp[2 * i + 1]);
            let energy = (a * a + b * b + 8) >> 4;
            mean += (energy + 2048) >> 12;
            mem0 += (energy - mem0 + (1 << (forward - 1))) >> forward;
            tmp[i] = ((mem0 + 2048) >> 12) as i16;
        }
        mem0 = 0;
        let mut max_energy = 0;
        for i in (0..len2).rev() {
            mem0 += ((i32::from(tmp[i]) << 4) - mem0 + 4) >> 3;
            tmp[i] = ((mem0 + 8) >> 4) as i16;
            max_energy = max_energy.max(i32::from(tmp[i]));
        }
        mean = math::sqrt(mean)? * math::sqrt(max_energy * (len2 as i32 >> 1))?;
        let norm = ((len2 as i32) << 20) / (1 + (mean >> 1));
        let mut unmask = 0;
        for i in (12..len2 - 5).step_by(4) {
            let at =
                ((i64::from(i32::from(tmp[i]) + 1) * i64::from(norm)) >> 15).clamp(0, 127) as usize;
            unmask += i32::from(INV[at]);
        }
        unmask = 64 * unmask * 4 / (6 * (len2 as i32 - 17));
        if unmask > metric {
            metric = unmask;
            selected = c;
        }
    }
    let mut transient = metric > 200;
    if toneishness > TONE_GUARD && i32::from(frequency) < TONE_GUARD_FREQ {
        transient = false;
        metric = 0;
    }
    let weak = allow_weak && transient && metric < 600;
    if weak {
        transient = false;
    }
    let max_tf = (math::sqrt(27 * metric)? - 42).max(0);
    let estimate = math::sqrt((((TF_SCALE * max_tf.min(163)) << 14) - TF_OFFSET).max(0))? as i16;
    Ok(TransientAnalysis {
        transient,
        weak,
        estimate,
        channel: selected,
        frequency,
        toneishness,
        metric,
    })
}
fn tone(input: &[i32], n: usize, channels: usize) -> Result<(i16, i32), AnalysisError> {
    let mut x = [0i16; 1080];
    for i in 0..n {
        let value = if channels == 2 {
            (input[i] >> 1) + (input[n + i] >> 1)
        } else {
            input[i]
        };
        x[i] = ((value + (1 << 13)) >> 14) as i16;
    }
    let mut energy = n as i32;
    for &x in &x[..n] {
        let x = i32::from(x);
        energy += (x * x) >> 10;
    }
    let shift = 5 - (28 - ilog(energy)) / 2;
    if shift > 0 {
        for x in &mut x[..n] {
            *x = ((i32::from(*x) + (1 << (shift - 1))) >> shift) as i16;
        }
    }
    let mut delay = 1;
    let mut coefficients = tone_lpc(&x[..n], delay)?;
    while delay <= 16 && coefficients.is_none_or(|c| c[0] > 1 << 29 && c[1] < 0) {
        delay *= 2;
        coefficients = tone_lpc(&x[..n], delay)?;
    }
    if let Some(c) = coefficients {
        if mul31(c[0], c[0]) + mul31(COMPLEX_SCALE, c[1]) < 0 {
            return Ok((
                ((acos(c[0] >> 1)? + delay as i32 / 2) / delay as i32) as i16,
                -c[1],
            ));
        }
    }
    Ok((-1, 0))
}
fn tone_lpc(x: &[i16], delay: usize) -> Result<Option<[i32; 2]>, AnalysisError> {
    let n = x.len();
    let mut r00 = 0;
    let mut r01 = 0;
    let mut r02 = 0;
    for i in 0..n - 2 * delay {
        let a = i32::from(x[i]);
        r00 += a * a;
        r01 += a * i32::from(x[i + delay]);
        r02 += a * i32::from(x[i + 2 * delay]);
    }
    let mut edges = 0;
    for i in 0..delay {
        edges += square(x[n + i - 2 * delay]) - square(x[i]);
    }
    let mut r11 = r00 + edges;
    edges = 0;
    for i in 0..delay {
        edges += square(x[n + i - delay]) - square(x[i + delay]);
    }
    let r22 = r11 + edges;
    edges = 0;
    for i in 0..delay {
        edges += i32::from(x[n + i - 2 * delay]) * i32::from(x[n + i - delay])
            - i32::from(x[i]) * i32::from(x[i + delay]);
    }
    let r12 = r01 + edges;
    r00 += r22;
    r01 += r12;
    r11 *= 2;
    r02 *= 2;
    // The backward sum makes r12 equal r01; r22 is no longer needed.
    let product = mul31(r00, r11);
    let den = product - mul31(r01, r01);
    if den <= product >> 10 {
        return Ok(None);
    }
    let num1 = mul31(r02, r11) - mul31(r01, r01);
    let side = if num1 >= den {
        1 << 29
    } else if num1 <= -den {
        -(1 << 29)
    } else {
        math::fractional_divide_q29(num1, den)?
    };
    let num0 = mul31(r00, r01) - mul31(r02, r01);
    let mid = if num0 >> 1 >= den {
        TONE_LIMIT
    } else if num0 >> 1 <= -den {
        -TONE_LIMIT
    } else {
        math::fractional_divide_wide_q29(num0, den)?
    };
    Ok(Some([mid, side]))
}
fn acos(x: i32) -> Result<i32, AnalysisError> {
    let absolute = x.abs();
    let x14 = absolute >> 15;
    let mut tmp = ((762 * x14) >> 14) - 3308;
    tmp = ((tmp * x14) >> 14) + 25726;
    tmp = (tmp * math::sqrt(((1 << 30) - (absolute << 1)).max(0))?) >> 16;
    Ok(if x < 0 { 25736 - tmp } else { tmp })
}
fn square(x: i16) -> i32 {
    i32::from(x) * i32::from(x)
}
fn mul31(a: i32, b: i32) -> i32 {
    ((i64::from(a) * i64::from(b)) >> 31) as i32
}
fn ilog(x: i32) -> i32 {
    31 - x.leading_zeros() as i32
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_acos_blocks_and_wide_q29_ratios_match() -> Result<(), Box<dyn std::error::Error>> {
        let mut counts = [0; 2];
        let mut tables = [false; 2];
        for line in include_str!("../../../tests/celt-transient-math-fixtures.txt").lines() {
            if line.starts_with('#') {
                continue;
            }
            let c: Vec<_> = line.split_whitespace().collect();
            if c[0] == "constants" {
                let native: Vec<i32> =
                    c[1..].iter().map(|x| x.parse()).collect::<Result<_, _>>()?;
                assert_eq!(
                    native,
                    [
                        TONE_LIMIT,
                        COMPLEX_SCALE,
                        TONE_GUARD,
                        TONE_GUARD_FREQ,
                        TF_SCALE,
                        TF_OFFSET
                    ]
                );
                tables[0] = true;
                continue;
            }
            if c[0] == "inverse" {
                let native: Vec<u8> = c[1..].iter().map(|x| x.parse()).collect::<Result<_, _>>()?;
                assert_eq!(native, INV);
                tables[1] = true;
                continue;
            }
            if c[0] == "acos" {
                let start = c[1].parse::<i32>()?;
                let step = c[2].parse::<i32>()?;
                let count = c[3].parse::<usize>()?;
                let mut hash = 14695981039346656037u64;
                for i in 0..count {
                    for b in
                        acos((i64::from(start) + i as i64 * i64::from(step)) as i32)?.to_le_bytes()
                    {
                        hash = (hash ^ u64::from(b)).wrapping_mul(1099511628211);
                    }
                }
                assert_eq!(hash, c[4].parse::<u64>()?);
                counts[0] += count;
            } else {
                assert_eq!(c[0], "divide");
                assert_eq!(
                    math::fractional_divide_wide_q29(c[1].parse()?, c[2].parse()?)?,
                    c[3].parse::<i32>()?
                );
                counts[1] += 1;
            }
        }
        assert_eq!(tables, [true, true]);
        assert_eq!(counts[0], 65537);
        assert_eq!(counts[1], 4290);
        for b in [-1, 0] {
            assert_eq!(
                math::fractional_divide_wide_q29(0, b),
                Err(math::MathError::InvalidDenominator)
            );
        }
        for a in [-4, 4] {
            assert_eq!(
                math::fractional_divide_wide_q29(a, 2),
                Err(math::MathError::NotFractional)
            );
        }
        assert_eq!(
            math::fractional_divide_q29(3, 2),
            Err(math::MathError::NotFractional)
        );
        Ok(())
    }
}
