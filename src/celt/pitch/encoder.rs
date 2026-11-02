//! Original encoder pitch search and subharmonic period/gain refinement.
use super::{PitchError, ilog, mul15, search, whiten};
use crate::celt::{FrameSize, math};

/// Checked previous comb period/gain used by the native continuity model.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PitchHistory {
    period: u16,
    gain: u16,
}
impl PitchHistory {
    /// Period 15..1022, gain 0..24576 Q15; initial period zero requires zero gain.
    pub fn new(period: u16, gain: u16) -> Result<Self, PitchError> {
        if gain > 24576 || !(period == 0 && gain == 0 || (15..=1022).contains(&period)) {
            return Err(PitchError::InvalidHistory);
        }
        Ok(Self { period, gain })
    }
    /// Previous 48 kHz period, initially zero.
    pub fn period(self) -> u16 {
        self.period
    }
    /// Previous nonnegative Q15 gain, initially zero.
    pub fn gain(self) -> u16 {
        self.gain
    }
}
/// Fixed-storage native encoder search diagnostics before outer prefilter policy.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EncoderPitch {
    size: FrameSize,
    coarse: u16,
    period: u16,
    gain: i16,
    whitened: [i16; 992],
}
impl EncoderPitch {
    /// Coarse 48 kHz period before subharmonic/doubling refinement.
    pub fn coarse_period(&self) -> u16 {
        self.coarse
    }
    /// Refined 48 kHz period 15..1023, before outer prefilter clamp to 1022.
    pub fn period(&self) -> u16 {
        self.period
    }
    /// Raw refined Q15 gain, before the outer 0.7/loss/threshold/quantization policy.
    pub fn gain(&self) -> i16 {
        self.gain
    }
    /// Factor-two lowpass/LPC4/FIR5 whitened signal, before search-only scaling.
    pub fn whitened(&self) -> &[i16] {
        &self.whitened[..(1024 + self.size.samples()) / 2]
    }
}
/// Analyze immutable raw pre-emphasized mono/stereo histories plus one frame.
/// Each channel is exactly 1024+size.samples() Q12 values in +/-536870911.
/// This standalone stage does not filter samples or update stream history.
/// Final inner-product/energy accumulations must fit signed32; unsupported
/// accumulations reject explicitly. Fixed storage, no heap allocation.
pub fn encoder_pitch(
    size: FrameSize,
    left: &[i32],
    right: Option<&[i32]>,
    previous: PitchHistory,
) -> Result<EncoderPitch, PitchError> {
    let n = size.samples();
    if left.len() != 1024 + n || right.is_some_and(|r| r.len() != 1024 + n) {
        return Err(PitchError::InvalidEncoderGeometry);
    }
    if left
        .iter()
        .chain(right.into_iter().flatten())
        .any(|v| !(-536870911..=536870911).contains(v))
    {
        return Err(PitchError::InvalidSignal);
    }
    let mut result = EncoderPitch {
        size,
        coarse: 0,
        period: 0,
        gain: 0,
        whitened: [0; 992],
    };
    let lp = &mut result.whitened[..(1024 + n) / 2];
    whiten(left, right, lp)?;
    result.coarse = (1024 - search(lp, 512, n, 979)) as u16;
    let (period, gain) = remove_doubling(lp, n, result.coarse, previous)?;
    result.period = period;
    result.gain = gain;
    Ok(result)
}
fn inner(left: &[i16], right: &[i16]) -> Result<i32, PitchError> {
    let sum: i64 = left
        .iter()
        .zip(right)
        .map(|(&l, &r)| i64::from(l) * i64::from(r))
        .sum();
    i32::try_from(sum).map_err(|_| PitchError::ArithmeticOverflow)
}
fn gain(xy: i32, xx: i32, yy: i32) -> Result<i16, PitchError> {
    if xy == 0 || xx == 0 || yy == 0 {
        return Ok(0);
    }
    if xx < 0 || yy < 0 {
        return Err(PitchError::ArithmeticOverflow);
    }
    let sx = ilog(xx) - 14;
    let sy = ilog(yy) - 14;
    let mut shift = sx + sy;
    let mut x2y2 = (vshift(xx, sx) as i16 as i32 * vshift(yy, sy) as i16 as i32) >> 14;
    if shift & 1 != 0 {
        if x2y2 < 32768 {
            x2y2 <<= 1;
            shift -= 1;
        } else {
            x2y2 >>= 1;
            shift += 1;
        }
    }
    let den = math::rsqrt_normalized(x2y2).map_err(PitchError::Math)?;
    let g = mul15(i32::from(den), xy);
    Ok(vshift(g, (shift >> 1) - 1).clamp(-32767, 32767) as i16)
}
fn vshift(value: i32, shift: i32) -> i32 {
    if shift > 0 {
        value >> shift
    } else {
        value.wrapping_shl((-shift) as u32)
    }
}
fn remove_doubling(
    values: &[i16],
    n: usize,
    coarse: u16,
    previous: PitchHistory,
) -> Result<(u16, i16), PitchError> {
    const SECOND: [usize; 16] = [0, 0, 3, 2, 3, 2, 5, 2, 3, 2, 3, 2, 5, 2, 3, 2];
    let n = n / 2;
    let base = 512;
    let t0 = (usize::from(coarse) / 2).min(511);
    let prev = usize::from(previous.period) / 2;
    let x = &values[base..base + n];
    let xx = inner(x, x)?;
    let xy = inner(x, &values[base - t0..base - t0 + n])?;
    let mut lookup = [0i32; 513];
    lookup[0] = xx;
    let mut yy = xx;
    for i in 1..=512 {
        let a = i32::from(values[base - i]);
        let b = i32::from(values[base + n - i]);
        yy = i32::try_from(
            i64::from(yy) + i64::from(a) * i64::from(a) - i64::from(b) * i64::from(b),
        )
        .map_err(|_| PitchError::ArithmeticOverflow)?;
        lookup[i] = yy.max(0);
    }
    let mut best_xy = xy;
    let mut best_yy = lookup[t0];
    let g0 = gain(xy, xx, best_yy)?;
    let mut g = g0;
    let mut t = t0;
    for (k, &second) in SECOND.iter().enumerate().skip(2) {
        let t1 = (2 * t0 + k) / (2 * k);
        if t1 < 7 {
            break;
        }
        let t1b = if k == 2 {
            if t1 + t0 > 512 { t0 } else { t0 + t1 }
        } else {
            (2 * second * t0 + k) / (2 * k)
        };
        let xy = inner(x, &values[base - t1..base - t1 + n])?
            .wrapping_add(inner(x, &values[base - t1b..base - t1b + n])?)
            >> 1;
        let yy = lookup[t1].wrapping_add(lookup[t1b]) >> 1;
        let g1 = gain(xy, xx, yy)?;
        let continuity = if t1.abs_diff(prev) <= 1 {
            i32::from(previous.gain)
        } else if t1.abs_diff(prev) <= 2 && 5 * k * k < t0 {
            i32::from(previous.gain) >> 1
        } else {
            0
        };
        let mut threshold = 9830.max(mul15(22938, i32::from(g0)) - continuity);
        if t1 < 21 {
            threshold = 13107.max(mul15(27853, i32::from(g0)) - continuity);
        }
        // The original later `else if T1<2*minperiod` is unreachable after
        // `T1<3*minperiod`; retain its effective ordering, not a new threshold.
        if i32::from(g1) > threshold {
            best_xy = xy;
            best_yy = yy;
            t = t1;
            g = g1;
        }
    }
    best_xy = best_xy.max(0);
    let pg = if best_yy <= best_xy {
        32767
    } else {
        math::fractional_divide(
            best_xy,
            best_yy
                .checked_add(1)
                .ok_or(PitchError::ArithmeticOverflow)?,
        )
        .map_err(PitchError::Math)?
            >> 16
    };
    let mut corr = [0i32; 3];
    for (k, v) in corr.iter_mut().enumerate() {
        *v = inner(x, &values[base - (t + k - 1)..base - (t + k - 1) + n])?;
    }
    let offset = if corr[2].wrapping_sub(corr[0]) > mul15(22938, corr[1].wrapping_sub(corr[0])) {
        1
    } else if corr[0].wrapping_sub(corr[2]) > mul15(22938, corr[1].wrapping_sub(corr[2])) {
        -1
    } else {
        0
    };
    Ok((
        ((2 * t) as i32 + offset).max(15) as u16,
        pg.min(i32::from(g)) as i16,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_isolated_refinement_matches() -> Result<(), Box<dyn std::error::Error>> {
        let mut count = 0;
        for line in include_str!("../../../tests/celt-pitch-refinement-fixtures.txt")
            .lines()
            .filter(|l| !l.starts_with('#'))
        {
            let f: Vec<_> = line.split_whitespace().collect();
            assert_eq!(f[0], "refine");
            assert_eq!(f.len(), 12);
            let v: Vec<i32> = f[1..].iter().map(|s| s.parse()).collect::<Result<_, _>>()?;
            let n = 120usize << v[0];
            let length = (1024 + n) / 2;
            let period = v[1];
            let shape = v[2];
            let amplitude = v[3];
            let offset = v[4];
            let mut seed = v[5] as u32;
            let mut input = [0i16; 992];
            for (i, value) in input[..length].iter_mut().enumerate() {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                let phase = (i as i32 + offset) % period;
                let base = (i64::from(amplitude)
                    * i64::from(4 * phase.min(period - phase) - period)
                    / i64::from(period)) as i32;
                let noise = ((i64::from(seed as i32) * i64::from(amplitude)) >> 31) as i32;
                *value = match shape {
                    0 => base,
                    1 => {
                        if phase < period / 2 {
                            amplitude
                        } else {
                            -amplitude
                        }
                    }
                    2 => {
                        if phase == 0 {
                            amplitude
                        } else {
                            0
                        }
                    }
                    3 => {
                        (i64::from(amplitude) * i64::from(2 * phase - period) / i64::from(period))
                            as i32
                    }
                    4 => noise,
                    5 => 0,
                    6 => amplitude,
                    7 => {
                        if i % 2 != 0 {
                            amplitude
                        } else {
                            -amplitude
                        }
                    }
                    16 => {
                        if phase == 0 || phase == (period + 1) / 3 {
                            amplitude
                        } else {
                            0
                        }
                    }
                    _ => {
                        let weight = shape - 7;
                        (base * weight + noise * (8 - weight)) / 8
                    }
                } as i16;
            }
            let before = input;
            let result = remove_doubling(
                &input[..length],
                n,
                v[6] as u16,
                PitchHistory::new(v[7] as u16, v[8] as u16)?,
            )?;
            assert_eq!(
                [i32::from(result.1), i32::from(result.0)],
                v[9..11],
                "{line}"
            );
            assert_eq!(input, before);
            count += 1;
        }
        eprintln!("isolated refinement contexts{count}");
        assert_eq!(count, 203729);
        Ok(())
    }
    #[test]
    fn unsupported_accumulators_and_normalization_reject() {
        let values = [32767i16; 992];
        let before = values;
        assert_eq!(
            inner(&values[..480], &values[..480]),
            Err(PitchError::ArithmeticOverflow)
        );
        assert_eq!(
            remove_doubling(&values, 960, 1024, PitchHistory::default()),
            Err(PitchError::ArithmeticOverflow)
        );
        let mut rolling = values;
        rolling[512..].fill(0);
        assert_eq!(
            remove_doubling(&rolling, 960, 48, PitchHistory::default()),
            Err(PitchError::ArithmeticOverflow)
        );
        assert_eq!(gain(1, -1, 1), Err(PitchError::ArithmeticOverflow));
        assert_eq!(gain(1, 1, -1), Err(PitchError::ArithmeticOverflow));
        assert_eq!(values, before);
    }
    #[test]
    fn original_normalized_pitch_gains_match() -> Result<(), Box<dyn std::error::Error>> {
        let mut count = 0;
        for line in include_str!("../../../tests/celt-encoder-pitch-math-fixtures.txt")
            .lines()
            .filter(|l| !l.starts_with('#'))
        {
            let f: Vec<_> = line.split_whitespace().collect();
            assert_eq!(f[0], "gain");
            assert_eq!(f.len(), 5);
            let v: Vec<i32> = f[1..].iter().map(|s| s.parse()).collect::<Result<_, _>>()?;
            assert_eq!(i32::from(gain(v[0], v[1], v[2])?), v[3], "{line}");
            count += 1;
        }
        assert!(count > 50000);
        Ok(())
    }
}
