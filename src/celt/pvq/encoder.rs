//! Native classic PVQ pyramid/greedy pulse selection and band encoding.
use super::{NormalizedBand, PvqError, Spread, reconstruct, spread_vector};
use crate::{
    celt::{math, pulses::PulseCodebook},
    range::RangeEncoder,
};

/// Quantize a normalized Q24 band and encode its integer pulse vector atomically.
///
/// Input length must match the positive-pulse codebook; squared norm is at most 2
/// (the sum of signed-32 coefficient squares is <=2^49). Blocks are 1/2/4/8 and
/// divide the dimension. Gain is nonnegative Q31 and affects reconstruction,
/// not pulse search. Returns native resynthesized Q24 coordinates and collapse
/// mask. Input remains unchanged; every error preserves the entire writer.
/// No allocation, QEXT refinement or recursive band orchestration is supplied.
pub fn encode_band(
    entropy: &mut RangeEncoder,
    codebook: PulseCodebook,
    input: &[i32],
    spread: Spread,
    blocks: usize,
    gain: i32,
) -> Result<NormalizedBand, PvqError> {
    if !matches!(blocks, 1 | 2 | 4 | 8) {
        return Err(PvqError::InvalidBlocks);
    }
    encode_partition_band(entropy, codebook, input, spread, blocks, gain)
}
pub(crate) fn encode_partition_band(
    entropy: &mut RangeEncoder,
    codebook: PulseCodebook,
    input: &[i32],
    spread: Spread,
    blocks: usize,
    gain: i32,
) -> Result<NormalizedBand, PvqError> {
    if codebook.pulses() == 0 {
        return Err(PvqError::NoPulses);
    }
    let n = codebook.dimensions();
    if !matches!(blocks, 1 | 2 | 4 | 8 | 16) || n % blocks != 0 {
        return Err(PvqError::InvalidBlocks);
    }
    if gain < 0 {
        return Err(PvqError::InvalidGain);
    }
    if input.len() != n {
        return Err(PvqError::InvalidInput);
    }
    squared_sum(input, 1 << 49).ok_or(PvqError::InvalidInput)?;
    let mut work = [0i32; 176];
    work[..n].copy_from_slice(input);
    spread_vector(&mut work[..n], blocks, codebook.pulses(), spread, false)?;
    let selection = search(&mut work[..n], codebook.pulses())?;
    // Reconstruction is staged before the single atomic entropy operation.
    let result = reconstruct(
        &selection.values[..n],
        selection.energy,
        codebook.pulses(),
        spread,
        blocks,
        gain,
    )?;
    codebook.encode(entropy, &selection.values[..n])?;
    Ok(result)
}
struct Selection {
    values: [i16; 176],
    energy: u32,
}
fn squared_sum(input: &[i32], limit: i64) -> Option<i64> {
    let mut sum = 0i64;
    for &value in input {
        sum = sum.checked_add(i64::from(value) * i64::from(value))?;
        if sum > limit {
            return None;
        }
    }
    Some(sum)
}
fn search(x: &mut [i32], pulses: usize) -> Result<Selection, PvqError> {
    let n = x.len();
    // Native celt_inner_prod_norm_shift returns signed-32 energy; the following
    // 1+energy must remain positive before its integer logarithm.
    let sum = squared_sum(x, i64::from(i32::MAX - 1) << 20).ok_or(PvqError::ArithmeticOverflow)?;
    let energy = (sum >> 20) as i32;
    let shift = (((31 - (energy + 1).leading_zeros() as i32) + 1) / 2 - 4).max(0) as u32;
    let mut negative = [false; 176];
    for (i, value) in x.iter_mut().enumerate() {
        if shift > 0 {
            *value = value.wrapping_add(1 << (shift - 1)) >> shift;
        }
        let scaled = i16::try_from(*value).map_err(|_| PvqError::ArithmeticOverflow)?;
        if scaled == i16::MIN {
            return Err(PvqError::ArithmeticOverflow);
        }
        negative[i] = scaled < 0;
        *value = i32::from(scaled).abs();
    }
    let mut iy = [0i32; 176];
    let mut y = [0i32; 176];
    let mut xy = 0i32;
    let mut yy = 0i32;
    let mut left = pulses;
    if pulses > n / 2 {
        let mut sum = x.iter().copied().sum::<i32>();
        if sum <= pulses as i32 {
            x.fill(0);
            x[0] = 1 << 14;
            sum = 1 << 14;
        }
        let reciprocal = ((pulses as i64 * i64::from(math::reciprocal(sum)?)) >> 16) as i16 as i32;
        for i in 0..n {
            let value = (x[i] * reciprocal) >> 15;
            if value < 0 || value as usize > left {
                return Err(PvqError::ArithmeticOverflow);
            }
            iy[i] = value;
            y[i] = 2 * value;
            yy += value * value;
            xy += x[i] * value;
            left -= value as usize;
        }
    }
    if left > n + 3 {
        yy += (left * left) as i32 + left as i32 * y[0];
        iy[0] += left as i32;
        left = 0;
    }
    // Q14 coefficients fit signed16 and the validated L1 pulse total is<=128:
    // xy<=32767*128, y<=256 and yy<=16384. Score products fit signed32;
    // explicit wrapping retains the reference arithmetic and strict ties.
    for i in 0..left {
        let rshift = 32 - ((pulses - left + i + 1) as u32).leading_zeros();
        yy += 1;
        let score = |j: usize| {
            let correlation = ((xy + x[j]) >> rshift) as i16 as i32;
            ((correlation * correlation) >> 15, yy + y[j])
        };
        let (mut best_num, mut best_den) = score(0);
        let mut best = 0;
        for j in 1..n {
            let (numerator, denominator) = score(j);
            if best_den.wrapping_mul(numerator) > denominator.wrapping_mul(best_num) {
                best_num = numerator;
                best_den = denominator;
                best = j;
            }
        }
        xy += x[best];
        yy += y[best];
        y[best] += 2;
        iy[best] += 1;
    }
    let mut result = Selection {
        values: [0; 176],
        energy: 0,
    };
    for i in 0..n {
        let value = if negative[i] { -iy[i] } else { iy[i] };
        result.values[i] = value as i16;
        result.energy += (value * value) as u32;
    }
    if yy <= 0 || result.energy != yy as u32 {
        return Err(PvqError::ArithmeticOverflow);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{celt::pvq::decode_partition_band, range::RangeDecoder};
    #[test]
    fn original_quantized_pulses_resynthesis_masks_and_bytes_match()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut rows = 0;
        let mut wide_mask = false;
        let mut gains = [false; 3];
        let mut spreads = [false; 4];
        let mut block_counts = [false; 5];
        let mut prefixes = [false; 3];
        for (index, line) in include_str!("../../../tests/celt-pvq-encoder-fixtures.txt")
            .lines()
            .enumerate()
        {
            if line.starts_with('#') {
                continue;
            }
            let c: Vec<_> = line.split_whitespace().collect();
            assert_eq!(c[0], "band");
            let n = c[1].parse::<usize>()?;
            let k = c[2].parse::<usize>()?;
            let spread_index = c[3].parse::<usize>()?;
            let spread = [
                Spread::None,
                Spread::Light,
                Spread::Normal,
                Spread::Aggressive,
            ][spread_index];
            let blocks = c[4].parse::<usize>()?;
            let gain = c[5].parse::<i32>()?;
            let prefix_bits = c[6].parse::<u32>()?;
            let prefix_value = c[7].parse::<u32>()?;
            let data: Vec<u8> = (0..c[8].len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&c[8][i..i + 2], 16))
                .collect::<Result<_, _>>()?;
            let mask = c[9].parse::<u16>()?;
            let expected = (
                c[10].parse::<u32>()?,
                c[11].parse::<u32>()?,
                c[12].parse::<u32>()?,
            );
            let energy = c[13].parse::<u32>()?;
            assert_eq!(c.len(), 14 + 3 * n);
            let input: Vec<i32> = c[14..14 + n]
                .iter()
                .map(|v| v.parse())
                .collect::<Result<_, _>>()?;
            let pulses: Vec<i16> = c[14 + n..14 + 2 * n]
                .iter()
                .map(|v| v.parse())
                .collect::<Result<_, _>>()?;
            let reconstructed: Vec<i32> = c[14 + 2 * n..]
                .iter()
                .map(|v| v.parse())
                .collect::<Result<_, _>>()?;
            let before = input.clone();
            let book = PulseCodebook::new(n, k)?;
            let mut writer = RangeEncoder::new(64)?;
            if prefix_bits > 0 {
                writer.encode_uint(prefix_value, 1 << prefix_bits)?;
            }
            let result = encode_partition_band(&mut writer, book, &input, spread, blocks, gain)?;
            assert_eq!(result.values(), reconstructed, "resynth row{index}");
            assert_eq!(result.wide_collapse_mask(), mask, "mask row{index}");
            assert_eq!(input, before);
            assert_eq!(
                (writer.range(), writer.tell(), writer.tell_frac()),
                expected,
                "entropy row{index}"
            );
            writer.shrink(data.len())?;
            assert_eq!(writer.finish()?, data, "bytes row{index}");
            let mut decoder = RangeDecoder::new(&data);
            if prefix_bits > 0 {
                assert_eq!(decoder.decode_uint(1 << prefix_bits)?, prefix_value);
            }
            let selected = book.decode(&mut decoder)?;
            assert_eq!(selected.values(), pulses, "pulses row{index}");
            assert_eq!(selected.squared_norm(), energy);
            assert_eq!(
                (decoder.range(), decoder.tell(), decoder.tell_frac()),
                expected
            );
            let mut decoder = RangeDecoder::new(&data);
            if prefix_bits > 0 {
                decoder.decode_uint(1 << prefix_bits)?;
            }
            let decoded = decode_partition_band(&mut decoder, book, spread, blocks, gain)?;
            assert_eq!(decoded, result);
            wide_mask |= mask > 255;
            spreads[spread_index] = true;
            block_counts[blocks.trailing_zeros() as usize] = true;
            gains[match gain {
                0 => 0,
                1073741824 => 1,
                _ => 2,
            }] = true;
            prefixes[match prefix_bits {
                0 => 0,
                3 => 1,
                _ => 2,
            }] = true;
            rows += 1;
        }
        assert_eq!(rows, 16368);
        assert!(wide_mask);
        assert!(
            gains
                .iter()
                .chain(spreads.iter())
                .chain(block_counts.iter())
                .chain(prefixes.iter())
                .all(|&x| x)
        );
        Ok(())
    }
}
