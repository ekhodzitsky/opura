//! Native recursive mono shape encoding over standard pulse caches.
use super::*;
use crate::celt::math;

/// Encode a normalized mono partition and return its resynthesized shape.
///
/// Controls use the same geometry/budget/fold domains as [`decode_partition`].
/// Input must match the standard dimension and have Q24 squared norm at most 2
/// (`sum(x*x)<=2^49`). `avoid_split_noise` enables native endpoint protection
/// when an interior angle would allocate more bits than one side can receive.
/// No allocation or caller input mutation occurs. The full writer, remaining
/// budget and folding seed commit together only after complete success.
pub fn encode_partition(
    entropy: &mut RangeEncoder,
    state: &mut PartitionState,
    controls: PartitionControls,
    input: &[i32],
    fold: Option<&[i32]>,
    avoid_split_noise: bool,
) -> Result<Partition, PartitionError> {
    let n = validate_context(
        entropy.capacity() as u32 * 8,
        entropy.tell(),
        state,
        controls,
        fold,
        2,
    )?;
    if entropy.is_finished() {
        return Err(EncoderError::Finished.into());
    }
    if input.len() != n {
        return Err(PartitionError::InvalidInput);
    }
    let mut energy = 0i64;
    for &value in input {
        energy = energy
            .checked_add(i64::from(value) * i64::from(value))
            .ok_or(PartitionError::InvalidInput)?;
        if energy > 1 << 49 {
            return Err(PartitionError::InvalidInput);
        }
    }
    let mut writer = entropy.clone();
    let mut shared = state.clone();
    let mut result = Partition {
        values: [0; 176],
        length: n,
        collapse: 0,
    };
    result.values[..n].copy_from_slice(input);
    let mut encoder = Encoder {
        entropy: &mut writer,
        state: &mut shared,
        band: controls.band,
        spread: controls.spread,
        avoid_split_noise,
        intensity: 21,
        disable_inv: false,
        tf_change: 0,
        amplitudes: [0; 2],
        theta_round: 0,
    };
    result.collapse = encoder.partition(
        &mut result.values[..n],
        fold,
        Node {
            bits: controls.bits,
            blocks: controls.blocks,
            lm: controls.lm,
            gain: controls.gain,
            fill: u32::from(controls.fill),
        },
    )? as u16;
    *entropy = writer;
    *state = shared;
    Ok(result)
}
pub(super) struct Encoder<'a> {
    pub(super) entropy: &'a mut RangeEncoder,
    pub(super) state: &'a mut PartitionState,
    pub(super) band: usize,
    pub(super) spread: Spread,
    pub(super) avoid_split_noise: bool,
    pub(super) intensity: usize,
    pub(super) disable_inv: bool,
    pub(super) tf_change: i8,
    pub(super) amplitudes: [i32; 2],
    pub(super) theta_round: i8,
}
impl Encoder<'_> {
    pub(super) fn partition(
        &mut self,
        x: &mut [i32],
        fold: Option<&[i32]>,
        node: Node,
    ) -> Result<u32, PartitionError> {
        let Node {
            mut bits,
            mut blocks,
            mut lm,
            gain,
            mut fill,
        } = node;
        let cache = cache(self.band, lm);
        let original_blocks = blocks;
        let n = x.len();
        if lm != -1 && bits > i32::from(cache[usize::from(cache[0])]) + 12 && n > 2 {
            let half = n / 2;
            lm -= 1;
            if blocks == 1 {
                fill = (fill & 1) | (fill << 1);
            }
            blocks = (blocks + 1) >> 1;
            let (left, right) = x.split_at_mut(half);
            let split = self.theta(
                left,
                right,
                &mut bits,
                &mut fill,
                Theta {
                    n: half,
                    blocks,
                    original_blocks,
                    lm,
                    stereo: false,
                },
            )?;
            let mut delta = split.delta;
            if original_blocks > 1 && (split.angle & 0x3fff) != 0 {
                if split.angle > 8192 {
                    delta -= delta >> (4 - lm);
                } else {
                    delta = 0.min(delta + ((half as i32 * 8) >> (5 - lm)));
                }
            }
            let mut mid_bits = 0.max(bits.min((bits - delta) / 2));
            let mut side_bits = bits - mid_bits;
            self.state.remaining -= split.cost;
            let before = self.state.remaining;
            let (left, right) = x.split_at_mut(half);
            let (fold_left, fold_right) = if let Some(fold) = fold {
                (Some(&fold[..half]), Some(&fold[half..]))
            } else {
                (None, None)
            };
            let mgain = mul31(gain, i32::from(split.gains[0]) << 16);
            let sgain = mul31(gain, i32::from(split.gains[1]) << 16);
            let cm;
            if mid_bits >= side_bits {
                cm = self.partition(
                    left,
                    fold_left,
                    Node {
                        bits: mid_bits,
                        blocks,
                        lm,
                        gain: mgain,
                        fill,
                    },
                )?;
                let rebalance = mid_bits - (before - self.state.remaining);
                if rebalance > 24 && split.angle != 0 {
                    side_bits += rebalance - 24;
                }
                Ok(cm
                    | (self.partition(
                        right,
                        fold_right,
                        Node {
                            bits: side_bits,
                            blocks,
                            lm,
                            gain: sgain,
                            fill: fill >> blocks,
                        },
                    )? << (original_blocks >> 1)))
            } else {
                cm = self.partition(
                    right,
                    fold_right,
                    Node {
                        bits: side_bits,
                        blocks,
                        lm,
                        gain: sgain,
                        fill: fill >> blocks,
                    },
                )? << (original_blocks >> 1);
                let rebalance = side_bits - (before - self.state.remaining);
                if rebalance > 24 && split.angle != 16384 {
                    mid_bits += rebalance - 24;
                }
                Ok(cm
                    | self.partition(
                        left,
                        fold_left,
                        Node {
                            bits: mid_bits,
                            blocks,
                            lm,
                            gain: mgain,
                            fill,
                        },
                    )?)
            }
        } else {
            let mut q = bits_to_pulses(cache, bits);
            let mut cost = pulse_cost(cache, q);
            self.state.remaining -= cost;
            while self.state.remaining < 0 && q > 0 {
                self.state.remaining += cost;
                q -= 1;
                cost = pulse_cost(cache, q);
                self.state.remaining -= cost;
            }
            if q > 0 {
                let pulses = if q < 8 {
                    q
                } else {
                    (8 + (q & 7)) << ((q >> 3) - 1)
                };
                let band = pvq::encode_partition_band(
                    self.entropy,
                    PulseCodebook::new(n, pulses)?,
                    x,
                    self.spread,
                    blocks,
                    gain,
                )?;
                x.copy_from_slice(band.values());
                Ok(u32::from(band.wide_collapse_mask()))
            } else {
                fill_partition(x, fold, self.state, blocks, gain, fill)
            }
        }
    }
    pub(super) fn theta(
        &mut self,
        left: &mut [i32],
        right: &mut [i32],
        bits: &mut i32,
        fill: &mut u32,
        t: Theta,
    ) -> Result<Split, PartitionError> {
        let Theta {
            n,
            blocks,
            original_blocks,
            lm,
            stereo,
        } = t;
        let pulse_cap = i32::from(LOG_N[self.band]) + i32::from(lm) * 8;
        let offset = (pulse_cap >> 1) - if stereo && n == 2 { 16 } else { 4 };
        let mut qn = compute_qn(n, *bits, offset, pulse_cap, stereo);
        if stereo && self.band >= self.intensity {
            qn = 1;
        }
        let raw = measure_angle(left, right, stereo)?;
        let before = self.entropy.tell_frac();
        let mut inv = false;
        let angle;
        if qn != 1 {
            let mut index = (u32::from(raw) * qn + 8192) >> 14;
            if stereo && self.theta_round != 0 {
                let bias = if raw > 8192 {
                    32767 / qn as i32
                } else {
                    -32767 / qn as i32
                };
                let down = ((i32::from(raw) * qn as i32 + bias) >> 14).clamp(0, qn as i32 - 1);
                index = (down + i32::from(self.theta_round > 0)) as u32;
            } else if !stereo && self.avoid_split_noise && index > 0 && index < qn {
                let angle = (index * 16384 / qn) as u16;
                let gains = bands::split_gains(angle)?;
                let delta = delta(n, angle, gains)?;
                if delta > *bits {
                    index = qn;
                } else if delta < -*bits {
                    index = 0;
                }
            }
            if stereo && n > 2 {
                let q = qn as usize;
                let mut cdf = [0u16; 258];
                for i in 0..=q {
                    cdf[i + 1] = cdf[i] + if i <= q / 2 { 3 } else { 1 };
                }
                self.entropy.encode_cdf(index as usize, &cdf[..q + 2])?;
            } else if original_blocks > 1 || stereo {
                self.entropy.encode_uint(index, qn + 1)?;
            } else {
                let q = qn as usize;
                let mut cdf = [0u16; 258];
                for i in 0..=q {
                    cdf[i + 1] = cdf[i] + (if i <= q / 2 { i + 1 } else { q + 1 - i }) as u16;
                }
                self.entropy.encode_cdf(index as usize, &cdf[..q + 2])?;
            }
            angle = (index * 16384 / qn) as u16;
            if stereo {
                if angle == 0 {
                    intensity_mix(left, right, self.amplitudes)?;
                } else {
                    stereo_split(left, right);
                }
            }
        } else if stereo {
            inv = raw > 8192 && !self.disable_inv;
            if inv {
                for y in right.iter_mut() {
                    *y = y.wrapping_neg();
                }
            }
            intensity_mix(left, right, self.amplitudes)?;
            if *bits > 16 && self.state.remaining > 16 {
                self.entropy.encode_bit_logp(inv, 2)?;
            } else {
                inv = false;
            }
            if self.disable_inv {
                inv = false;
            }
            angle = 0;
        } else {
            // Every standard recursive mono split has qn>=2 (finite table test).
            return Err(PartitionError::InvalidBudget);
        }
        let cost = (self.entropy.tell_frac() - before) as i32;
        *bits -= cost;
        let gains = bands::split_gains(angle)?;
        let delta = delta(n, angle, gains)?;
        if angle == 0 {
            *fill &= (1 << blocks) - 1;
        } else if angle == 16384 {
            *fill &= ((1 << blocks) - 1) << blocks;
        }
        Ok(Split {
            inv,
            angle,
            gains,
            delta,
            cost,
        })
    }
}
fn measure_angle(left: &[i32], right: &[i32], stereo: bool) -> Result<u16, PartitionError> {
    let energy = |values: &[i32]| -> Result<i32, PartitionError> {
        let mut sum = 0i64;
        for &value in values {
            sum = sum
                .checked_add(i64::from(value) * i64::from(value))
                .ok_or(PartitionError::InvalidInput)?;
        }
        i32::try_from(sum >> 20).map_err(|_| PartitionError::InvalidInput)
    };
    let (mid_energy, side_energy) = if stereo {
        let mut mid = 0i64;
        let mut side = 0i64;
        for (&x, &y) in left.iter().zip(right) {
            let m = (x + y + 1024) >> 11;
            let s = (x - y + 1024) >> 11;
            mid += i64::from(m) * i64::from(m);
            side += i64::from(s) * i64::from(s);
        }
        (
            i32::try_from(mid).map_err(|_| PartitionError::InvalidInput)?,
            i32::try_from(side).map_err(|_| PartitionError::InvalidInput)?,
        )
    } else {
        (energy(left)?, energy(right)?)
    };
    let mid = math::sqrt_extended(mid_energy)?;
    let side = math::sqrt_extended(side_energy)?;
    Ok((math::atan2_norm(side, mid)? >> 16) as u16)
}
fn stereo_split(left: &mut [i32], right: &mut [i32]) {
    for (x, y) in left.iter_mut().zip(right.iter_mut()) {
        let l = mul31(crate::celt::band_tables::HAAR_Q31, *x);
        let r = mul31(crate::celt::band_tables::HAAR_Q31, *y);
        *x = l.wrapping_add(r);
        *y = r.wrapping_sub(l);
    }
}
fn intensity_mix(
    left: &mut [i32],
    right: &[i32],
    amplitudes: [i32; 2],
) -> Result<(), PartitionError> {
    let max = amplitudes[0].max(amplitudes[1]);
    let log = if max > 0 {
        31 - max.leading_zeros() as i32
    } else {
        0
    };
    let shift = log - 13;
    let weights = amplitudes.map(|x| if shift > 0 { x >> shift } else { x << -shift });
    let norm = 1 + math::sqrt(1 + weights[0] * weights[0] + weights[1] * weights[1])?;
    let weights = weights.map(|x| (x.min(norm - 1) << 15) / norm);
    for (x, &y) in left.iter_mut().zip(right) {
        *x = (((i64::from(weights[0]) * i64::from(*x)) >> 15) as i32)
            .wrapping_add(((i64::from(weights[1]) * i64::from(y)) >> 15) as i32);
    }
    Ok(())
}
fn delta(n: usize, angle: u16, gains: [i16; 2]) -> Result<i32, PartitionError> {
    Ok(if angle == 0 {
        -16384
    } else if angle == 16384 {
        16384
    } else {
        (16384
            + (((n - 1) << 7) as i16 as i32) * bands::log2_tan(gains[1], gains[0])? as i16 as i32)
            >> 15
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stereo_native_scalar_mixing_matches() -> Result<(), Box<dyn std::error::Error>> {
        let mut count = [0; 2];
        for line in include_str!("../../../tests/celt-stereo-math-fixtures.txt").lines() {
            if line.starts_with('#') {
                continue;
            }
            let c: Vec<_> = line.split_whitespace().collect();
            if c[0] == "split" {
                let mut x = [c[1].parse::<i32>()?];
                let mut y = [c[2].parse::<i32>()?];
                stereo_split(&mut x, &mut y);
                assert_eq!(x[0], c[3].parse::<i32>()?);
                assert_eq!(y[0], c[4].parse::<i32>()?);
                count[0] += 1;
            } else {
                assert_eq!(c[0], "intensity");
                let amplitudes = [c[1].parse()?, c[2].parse()?];
                let mut x = [c[3].parse()?];
                let y = [c[4].parse()?];
                intensity_mix(&mut x, &y, amplitudes)?;
                assert_eq!(x[0], c[5].parse::<i32>()?);
                count[1] += 1;
            }
        }
        assert_eq!(count, [4352, 1024]);
        Ok(())
    }
    #[test]
    fn every_standard_split_can_signal_an_angle() {
        let mut count = 0;
        for band in 0..21 {
            for lm in 0..=3 {
                let n = usize::from(BANDS[band + 1] - BANDS[band]) << lm;
                if n <= 2 {
                    continue;
                }
                let values = cache(band, lm as i8);
                let minimum = i32::from(values[usize::from(values[0])]) + 13;
                let child_lm = lm as i8 - 1;
                let pulse_cap = i32::from(LOG_N[band]) + i32::from(child_lm) * 8;
                let qn = compute_qn(n / 2, minimum, (pulse_cap >> 1) - 4, pulse_cap, false);
                assert!(qn >= 2, "band{band} lm{lm} minimum{minimum}");
                assert!(compute_qn(n / 2, 81600, (pulse_cap >> 1) - 4, pulse_cap, false) <= 256);
                count += 1;
            }
        }
        assert_eq!(count, 64);
    }
}
