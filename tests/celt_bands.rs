//! Original coded angles, permutations, Haar, fold normalization and stereo merge.
use opura::celt::bands::{self, BandError};

#[test]
fn original_scalar_and_vector_outputs_match() -> Result<(), Box<dyn std::error::Error>> {
    let mut counts = [0; 5];
    for (line_no, line) in include_str!("celt-band-fixtures.txt").lines().enumerate() {
        if line.starts_with('#') {
            continue;
        }
        let fields: Vec<_> = line.split_whitespace().collect();
        match fields[0] {
            "theta" => {
                let angle = fields[1].parse()?;
                let expected = [fields[2].parse()?, fields[3].parse()?];
                assert_eq!(bands::split_gains(angle)?, expected, "theta {angle}");
                counts[0] += 1;
            }
            "log" => {
                assert_eq!(
                    bands::log2_tan(fields[1].parse()?, fields[2].parse()?)?,
                    fields[3].parse()?,
                    "log line {line_no}"
                );
                counts[1] += 1;
            }
            "logblock" => {
                let start: i16 = fields[1].parse()?;
                let count: i16 = fields[2].parse()?;
                let step: i16 = fields[3].parse()?;
                let sine = fields[4].parse()?;
                let mut hash = 14695981039346656037u64;
                for i in 0..count {
                    for byte in bands::log2_tan(sine, start + i * step)?.to_le_bytes() {
                        hash = (hash ^ u64::from(byte)).wrapping_mul(1099511628211);
                    }
                }
                assert_eq!(hash, fields[5].parse()?, "logblock sine {sine}");
                counts[1] += usize::try_from(count)?;
            }
            "vector" => {
                let op = fields[1].parse::<usize>()?;
                let n = fields[2].parse::<usize>()?;
                let stride = fields[3].parse()?;
                let hadamard = fields[4] == "1";
                let gain = fields[5].parse()?;
                let values: Vec<i32> = fields[6..]
                    .iter()
                    .map(|s| s.parse())
                    .collect::<Result<_, _>>()?;
                let mut actual = values[..n].to_vec();
                match op {
                    0 => bands::deinterleave(&mut actual, stride, hadamard)?,
                    1 => bands::interleave(&mut actual, stride, hadamard)?,
                    2 => bands::haar(&mut actual, stride)?,
                    3 => bands::renormalize(&mut actual, gain)?,
                    _ => panic!("fixture op"),
                }
                assert_eq!(
                    actual,
                    values[n..],
                    "vector line {line_no} op{op} n{n} stride{stride} gain{gain}"
                );
                if op <= 1 {
                    if op == 0 {
                        bands::interleave(&mut actual, stride, hadamard)?;
                    } else {
                        bands::deinterleave(&mut actual, stride, hadamard)?;
                    }
                    assert_eq!(actual, values[..n], "permutation inverse");
                }
                counts[2 + usize::from(op == 3)] += 1;
            }
            "merge" => {
                let n = fields[1].parse::<usize>()?;
                let mid = fields[2].parse()?;
                let values: Vec<i32> = fields[3..]
                    .iter()
                    .map(|s| s.parse())
                    .collect::<Result<_, _>>()?;
                let mut left = values[..n].to_vec();
                let mut right = values[n..2 * n].to_vec();
                bands::stereo_merge(&mut left, &mut right, mid)?;
                assert_eq!(left, values[2 * n..3 * n], "merge left line {line_no}");
                assert_eq!(right, values[3 * n..], "merge right line {line_no}");
                counts[4] += 1;
            }
            _ => panic!("fixture row"),
        }
    }
    assert_eq!(counts, [16259, 68644, 1531, 420, 400]);
    Ok(())
}

#[test]
fn invalid_domains_preserve_vectors() {
    for theta in [1, 63, 16321, 16383, 16385, u16::MAX] {
        assert_eq!(bands::split_gains(theta), Err(BandError::InvalidAngle));
    }
    for (s, c) in [(0, 1), (1, 0), (-1, 100), (32767, -32768)] {
        assert_eq!(bands::log2_tan(s, c), Err(BandError::InvalidTrig));
    }
    for n in [0, 1, 3, 8, 176, 177] {
        for stride in [0, 1, 2, 3, 4, 8, 16, 32, 64, 128, usize::MAX] {
            for had in [false, true] {
                let valid = (1..=176).contains(&n)
                    && matches!(stride, 1 | 2 | 4 | 8 | 16)
                    && n % stride == 0
                    && (!had || stride > 1);
                let original = vec![123; n];
                for inverse in [false, true] {
                    let mut actual = original.clone();
                    let result = if inverse {
                        bands::interleave(&mut actual, stride, had)
                    } else {
                        bands::deinterleave(&mut actual, stride, had)
                    };
                    assert_eq!(result.is_ok(), valid);
                    if !valid {
                        assert_eq!(actual, original);
                    }
                }
            }
            let valid = (1..=176).contains(&n)
                && stride.is_power_of_two()
                && stride <= n
                && n % stride == 0
                && (n / stride) % 2 == 0;
            let mut values = vec![123; n];
            let original = values.clone();
            assert_eq!(bands::haar(&mut values, stride).is_ok(), valid);
            if !valid {
                assert_eq!(values, original);
            }
        }
    }
    for (values, gain, error) in [
        (vec![], 1, BandError::InvalidGeometry),
        (vec![1; 177], 1, BandError::InvalidGeometry),
        (vec![1], -1, BandError::InvalidGain),
        (vec![1, i32::MAX], 1, BandError::InvalidVector),
        (vec![32767 << 10; 3], 1, BandError::InvalidVector),
        (vec![-32768 << 10; 2], 1, BandError::InvalidVector),
    ] {
        let mut actual = values.clone();
        assert_eq!(bands::renormalize(&mut actual, gain), Err(error));
        assert_eq!(actual, values);
    }
    for (left, right, mid, error) in [
        (vec![], vec![], 1, BandError::InvalidGeometry),
        (vec![1], vec![1; 2], 1, BandError::InvalidGeometry),
        (vec![1; 177], vec![1; 177], 1, BandError::InvalidGeometry),
        (vec![1], vec![1], -1, BandError::InvalidGain),
        (vec![i32::MAX], vec![1], 1, BandError::InvalidVector),
        (vec![1], vec![i32::MIN], 1, BandError::InvalidVector),
    ] {
        let mut x = left.clone();
        let mut y = right.clone();
        assert_eq!(bands::stereo_merge(&mut x, &mut y, mid), Err(error));
        assert_eq!(x, left);
        assert_eq!(y, right);
    }
}
