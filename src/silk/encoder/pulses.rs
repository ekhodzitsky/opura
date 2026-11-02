//! Native SILK shell-tree, least-significant-bit and sign encoding.
use super::{SignalType, signal_index};
use crate::{
    range::{EncoderError, RangeEncoder},
    silk::excitation::tables::*,
};

pub(super) fn encode(
    enc: &mut RangeEncoder,
    signal: SignalType,
    offset: u8,
    pulses: &[i8],
) -> Result<(), EncoderError> {
    // Validated caller geometry is 80/120/160/240/320. Zero padding is needed
    // only for 120 samples, and remains internal rather than mutating input.
    let blocks = pulses.len().div_ceil(16);
    let mut absolute = [0u16; 320];
    for (out, &pulse) in absolute.iter_mut().zip(pulses) {
        *out = i16::from(pulse).unsigned_abs();
    }
    let mut sums = [0usize; 20];
    let mut shifts = [0u8; 20];
    for block in 0..blocks {
        let values = &mut absolute[block * 16..(block + 1) * 16];
        loop {
            let mut combined = [0u16; 16];
            combined.copy_from_slice(values);
            let mut valid = true;
            let mut length = 8;
            for maximum in [8, 10, 12, 16] {
                for j in 0..length {
                    combined[j] = combined[2 * j] + combined[2 * j + 1];
                    valid &= combined[j] <= maximum;
                }
                length /= 2;
            }
            if valid {
                sums[block] = usize::from(combined[0]);
                break;
            }
            shifts[block] += 1;
            for value in values.iter_mut() {
                *value >>= 1;
            }
        }
    }
    let context = signal_index(signal) >> 1;
    let mut best = u32::MAX;
    let mut level = 0;
    for k in 0..9 {
        let mut cost = u32::from(RATE_COST[context * 9 + k]);
        for block in 0..blocks {
            let sum = if shifts[block] == 0 { sums[block] } else { 17 };
            cost += u32::from(SUM_COST[k * 18 + sum]);
        }
        if cost < best {
            best = cost;
            level = k;
        }
    }
    enc.encode_icdf(level, &RATES[context * 9..context * 9 + 9], 8)?;
    for block in 0..blocks {
        if shifts[block] == 0 {
            enc.encode_icdf(sums[block], &SUMS[level * 18..level * 18 + 18], 8)?;
        } else {
            enc.encode_icdf(17, &SUMS[level * 18..level * 18 + 18], 8)?;
            for _ in 1..shifts[block] {
                enc.encode_icdf(17, &SUMS[9 * 18..], 8)?;
            }
            enc.encode_icdf(sums[block], &SUMS[9 * 18..], 8)?;
        }
    }
    for block in 0..blocks {
        shell(enc, &absolute[block * 16..(block + 1) * 16], sums[block], 3)?;
    }
    for (block, &shift) in shifts[..blocks].iter().enumerate() {
        for j in block * 16..(block + 1) * 16 {
            let value = pulses.get(j).map_or(0, |&v| i16::from(v).unsigned_abs());
            for bit in (0..shift).rev() {
                enc.encode_icdf(usize::from((value >> bit) & 1), &[120, 0], 8)?;
            }
        }
    }
    let sign_context = 7 * (signal_index(signal) * 2 + usize::from(offset));
    for (block, &sum) in sums[..blocks].iter().enumerate() {
        let icdf = [SIGNS[sign_context + sum.min(6)], 0];
        for &pulse in &pulses[block * 16..((block + 1) * 16).min(pulses.len())] {
            if pulse != 0 {
                enc.encode_icdf(usize::from(pulse > 0), &icdf, 8)?;
            }
        }
    }
    Ok(())
}
fn shell(
    enc: &mut RangeEncoder,
    pulses: &[u16],
    sum: usize,
    level: usize,
) -> Result<(), EncoderError> {
    if sum == 0 {
        return Ok(());
    }
    let half = pulses.len() / 2;
    let left: usize = pulses[..half].iter().map(|&v| usize::from(v)).sum();
    let table = [&SHELL0, &SHELL1, &SHELL2, &SHELL3][level];
    let offset = usize::from(OFFSETS[sum]);
    enc.encode_icdf(left, &table[offset..offset + sum + 1], 8)?;
    if half > 1 {
        shell(enc, &pulses[..half], left, level - 1)?;
        shell(enc, &pulses[half..], sum - left, level - 1)?;
    }
    Ok(())
}
