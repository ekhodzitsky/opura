//! Fixed-point CELT math against independent pinned source and integer bounds.
use opura::celt::math::{self, MathError};

fn evaluate(op: u8, x: i64, y: i32) -> Result<i32, MathError> {
    Ok(match op {
        0 => math::integer_sqrt(x as u32) as i32,
        1 => math::sqrt(x as i32)?,
        2 => i32::from(math::rsqrt_normalized(x as i32)?),
        3 => i32::from(math::cos_norm(x as i32)),
        4 => i32::from(math::log2(x as i32)?),
        5 => math::exp2(x as i16),
        6 => math::reciprocal(x as i32)?,
        7 => math::fractional_divide_q29(x as i32, y)?,
        8 => math::fractional_divide(x as i32, y)?,
        _ => panic!("unknown operation"),
    })
}

#[test]
fn every_scalar_result_and_exhaustive_block_matches_reference()
-> Result<(), Box<dyn std::error::Error>> {
    let mut coverage = [0; 9];
    for (line_index, line) in include_str!("celt-math-fixtures.txt").lines().enumerate() {
        if line.starts_with('#') {
            continue;
        }
        let row: Vec<_> = line.split_whitespace().collect();
        let op = row[1].parse::<u8>()?;
        if row[0] == "block" {
            let start = row[2].parse::<i64>()?;
            let count = row[3].parse::<i64>()?;
            let mut hash = 14695981039346656037u64;
            for x in start..start + count {
                for byte in evaluate(op, x, 0)?.to_le_bytes() {
                    hash = (hash ^ u64::from(byte)).wrapping_mul(1099511628211);
                }
            }
            assert_eq!(
                hash,
                row[4].parse::<u64>()?,
                "block line {line_index} op {op} start {start}"
            );
            coverage[op as usize] += count;
        } else {
            assert_eq!(row[0], "case");
            let x = row[2].parse()?;
            let y = row[3].parse()?;
            assert_eq!(
                evaluate(op, x, y)?,
                row[4].parse::<i32>()?,
                "case line {line_index} op {op} x {x} y {y}"
            );
            coverage[op as usize] += 1;
        }
    }
    assert!(coverage.iter().all(|n| *n > 2000));
    assert_eq!(coverage[2], 49152);
    assert_eq!(coverage[5], 65536);
    Ok(())
}

#[test]
fn exact_integer_sqrt_brackets_every_square_and_wide_values() {
    fn verify(x: u32) {
        let root = u64::from(math::integer_sqrt(x));
        assert!(root * root <= u64::from(x), "sqrt lower bound {x}");
        assert!(
            (root + 1) * (root + 1) > u64::from(x),
            "sqrt upper bound {x}"
        );
    }
    for root in 0..=65535u32 {
        let square = root * root;
        verify(square);
        verify(square.saturating_sub(1));
        verify(square.saturating_add(1));
    }
    let mut random = 0x5351_5254u32;
    for _ in 0..65536 {
        random ^= random << 13;
        random ^= random >> 17;
        random ^= random << 5;
        verify(random);
    }
    verify(u32::MAX);
}

#[test]
fn domains_cardinals_periodicity_and_saturation_are_explicit() {
    for invalid in [i32::MIN, -1] {
        assert_eq!(math::sqrt(invalid), Err(MathError::NegativeInput));
        assert_eq!(math::log2(invalid), Err(MathError::NegativeInput));
    }
    for invalid in [i32::MIN, -1, 0] {
        assert_eq!(
            math::reciprocal(invalid),
            Err(MathError::InvalidDenominator)
        );
        assert_eq!(
            math::fractional_divide(0, invalid),
            Err(MathError::InvalidDenominator)
        );
    }
    for invalid in [i32::MIN, 0, 16383, 65536, i32::MAX] {
        assert_eq!(
            math::rsqrt_normalized(invalid),
            Err(MathError::NotNormalized)
        );
    }
    for (a, b) in [(2, 1), (-2, 1), (i32::MIN, i32::MAX)] {
        assert_eq!(
            math::fractional_divide_q29(a, b),
            Err(MathError::NotFractional)
        );
        assert_eq!(math::fractional_divide(a, b), Err(MathError::NotFractional));
    }
    for angle in [i32::MIN, -65536, -32768, 0, 32768, 65536, 98304, i32::MAX] {
        assert_eq!(
            math::cos_norm(angle),
            math::cos_norm(angle.wrapping_add(131072))
        );
        assert_eq!(math::cos_norm(angle), math::cos_norm(angle.wrapping_neg()));
    }
    assert_eq!(math::cos_norm(0), 32767);
    assert_eq!(math::cos_norm(32768), 0);
    assert_eq!(math::cos_norm(65536), -32767);
    assert_eq!(math::exp2(i16::MIN), 0);
    assert_eq!(math::exp2(i16::MAX), 0x7f000000);
    assert_eq!(math::sqrt(i32::MAX), Ok(32767));
    assert_eq!(math::log2(0), Ok(-32767));
}
