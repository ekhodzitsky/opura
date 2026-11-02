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
struct Encoder<'a> {
    entropy: &'a mut RangeEncoder,
    state: &'a mut PartitionState,
    band: usize,
    spread: Spread,
    avoid_split_noise: bool,
}
impl Encoder<'_> {
    fn partition(
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
            let split = self.theta(
                &x[..half],
                &x[half..],
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
    fn theta(
        &mut self,
        left: &[i32],
        right: &[i32],
        bits: &mut i32,
        fill: &mut u32,
        t: Theta,
    ) -> Result<Split, PartitionError> {
        let Theta {
            n,
            blocks,
            original_blocks,
            lm,
            stereo: _,
        } = t;
        let pulse_cap = i32::from(LOG_N[self.band]) + i32::from(lm) * 8;
        let qn = compute_qn(n, *bits, (pulse_cap >> 1) - 4, pulse_cap, false);
        // Every standard split's minimum cache budget yields qn>=2, verified
        // over the complete finite geometry table. No uncoded mono angle path.
        if qn == 1 {
            return Err(PartitionError::InvalidBudget);
        }
        let raw = measure_angle(left, right)?;
        let mut index = (u32::from(raw) * qn + 8192) >> 14;
        if self.avoid_split_noise && index > 0 && index < qn {
            let angle = (index * 16384 / qn) as u16;
            let gains = bands::split_gains(angle)?;
            let delta = delta(n, angle, gains)?;
            if delta > *bits {
                index = qn;
            } else if delta < -*bits {
                index = 0;
            }
        }
        let before = self.entropy.tell_frac();
        if original_blocks > 1 {
            self.entropy.encode_uint(index, qn + 1)?;
        } else {
            let q = qn as usize;
            let mut cdf = [0u16; 258];
            for i in 0..=q {
                cdf[i + 1] = cdf[i] + (if i <= q / 2 { i + 1 } else { q + 1 - i }) as u16;
            }
            self.entropy.encode_cdf(index as usize, &cdf[..q + 2])?;
        }
        let angle = (index * 16384 / qn) as u16;
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
            inv: false,
            angle,
            gains,
            delta,
            cost,
        })
    }
}
fn measure_angle(left: &[i32], right: &[i32]) -> Result<u16, PartitionError> {
    let energy = |values: &[i32]| -> Result<i32, PartitionError> {
        let mut sum = 0i64;
        for &value in values {
            sum = sum
                .checked_add(i64::from(value) * i64::from(value))
                .ok_or(PartitionError::InvalidInput)?;
        }
        i32::try_from(sum >> 20).map_err(|_| PartitionError::InvalidInput)
    };
    let mid = math::sqrt_extended(energy(left)?)?;
    let side = math::sqrt_extended(energy(right)?)?;
    Ok((math::atan2_norm(side, mid)? >> 16) as u16)
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
