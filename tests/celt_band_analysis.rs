//! Original band amplitudes/normalized vectors/log energies and refined math.
use opura::celt::{
    FrameSize,
    analysis::{self, AnalysisError},
    input::{InputRate, PreemphasisState},
    math::{self, MathError},
};
const SIZES: [FrameSize; 4] = [
    FrameSize::Ms2_5,
    FrameSize::Ms5,
    FrameSize::Ms10,
    FrameSize::Ms20,
];

fn scalar(op: u8, value: i32) -> Result<i32, MathError> {
    match op {
        0 => math::sqrt_extended(value),
        1 => math::reciprocal_normalized_q31(value),
        _ => panic!("invalid fixture operation"),
    }
}
#[test]
fn refined_scalar_dense_blocks_and_wide_boundaries_match() -> Result<(), Box<dyn std::error::Error>>
{
    let mut coverage = [0usize; 2];
    for line in include_str!("celt-analysis-math-fixtures.txt").lines() {
        if line.starts_with('#') {
            continue;
        }
        let cols: Vec<_> = line.split_whitespace().collect();
        let op = cols[1].parse::<u8>()?;
        if cols[0] == "block" {
            let start = cols[2].parse::<i32>()?;
            let step = cols[3].parse::<i32>()?;
            let count = cols[4].parse::<usize>()?;
            let mut fingerprint = 14695981039346656037u64;
            for i in 0..count {
                let value = start + i as i32 * step;
                for byte in scalar(op, value)?.to_le_bytes() {
                    fingerprint = (fingerprint ^ u64::from(byte)).wrapping_mul(1099511628211);
                }
            }
            assert_eq!(fingerprint, cols[5].parse::<u64>()?);
            coverage[op as usize] += count;
        } else {
            let value = cols[2].parse::<i32>()?;
            assert_eq!(
                scalar(op, value)?,
                cols[3].parse::<i32>()?,
                "op{op} value{value}"
            );
            coverage[op as usize] += 1;
        }
    }
    assert_eq!(coverage, [69834, 34877]);
    for value in [i32::MIN, -1] {
        assert_eq!(math::sqrt_extended(value), Err(MathError::NegativeInput));
    }
    for value in [i32::MIN, -1, 0, 1, (1 << 30) - 1] {
        assert_eq!(
            math::reciprocal_normalized_q31(value),
            Err(MathError::NotNormalized)
        );
    }
    assert_eq!(math::sqrt_extended(0)?, 0);
    assert_eq!(math::sqrt_extended(1)?, 65536);
    assert_eq!(math::sqrt_extended(1 << 30)?, i32::MAX);
    assert_eq!(math::sqrt_extended(i32::MAX)?, i32::MAX);
    Ok(())
}

#[test]
fn all_original_band_outputs_and_inactive_storage_match() -> Result<(), Box<dyn std::error::Error>>
{
    let mut rows = 0;
    let mut tiny = false;
    let mut normal = false;
    let mut tails = false;
    for (index, line) in include_str!("celt-band-analysis-fixtures.txt")
        .lines()
        .enumerate()
    {
        if line.starts_with('#') {
            continue;
        }
        let cols: Vec<_> = line.split_whitespace().collect();
        assert_eq!(cols[0], "frame");
        let lm = cols[1].parse::<usize>()?;
        let channels = cols[2].parse::<usize>()?;
        let effective_end = cols[3].parse::<usize>()?;
        let end = cols[4].parse::<usize>()?;
        let n = SIZES[lm].samples();
        let count = n * channels;
        let input: Vec<i32> = cols[5..5 + count]
            .iter()
            .map(|s| s.parse())
            .collect::<Result<_, _>>()?;
        let expected: Vec<i32> = cols[5 + count..]
            .iter()
            .map(|s| s.parse())
            .collect::<Result<_, _>>()?;
        assert_eq!(expected.len(), 42 * channels + count);
        let before = input.clone();
        let result = analysis::analyze_bands(SIZES[lm], &input, channels, effective_end, end)?;
        assert_eq!(input, before);
        assert_eq!(
            (
                result.size(),
                result.channels(),
                result.effective_end(),
                result.end()
            ),
            (SIZES[lm], channels, effective_end, end)
        );
        for channel in 0..channels {
            let amplitude = result.amplitudes(channel).ok_or("missing amplitudes")?;
            let normalized = result
                .normalized(channel)
                .ok_or("missing normalized channel")?;
            let logs = result.log_energies(channel).ok_or("missing logs")?;
            assert_eq!(
                amplitude.as_slice(),
                &expected[channel * 21..(channel + 1) * 21],
                "amplitude row{index}"
            );
            assert_eq!(
                normalized,
                &expected[21 * channels + channel * n..21 * channels + (channel + 1) * n],
                "normalized row{index}"
            );
            assert_eq!(
                logs.as_slice(),
                &expected[21 * channels + count + channel * 21
                    ..21 * channels + count + (channel + 1) * 21],
                "logs row{index}"
            );
            tiny |= amplitude[..effective_end]
                .iter()
                .any(|&x| (2..10).contains(&x));
            normal |= amplitude[..effective_end].iter().any(|&x| x >= 10);
            assert!(normalized.iter().all(|&x| i64::from(x).abs() <= 16777220));
        }
        for channel in [channels, usize::MAX] {
            assert_eq!(result.amplitudes(channel), None);
            assert_eq!(result.normalized(channel), None);
            assert_eq!(result.log_energies(channel), None);
        }
        tails |= effective_end < end;
        rows += 1;
    }
    assert_eq!(rows, 640);
    assert!(tiny && normal && tails);
    Ok(())
}

#[test]
fn malformed_and_unrepresentable_active_domains_preserve_input() -> Result<(), AnalysisError> {
    for size in SIZES {
        let n = size.samples();
        let mut input = vec![0; n];
        for (channels, length) in [
            (0, 0),
            (3, n * 3),
            (usize::MAX, 0),
            (1, n - 1),
            (1, n + 1),
            (2, n * 2 - 1),
        ] {
            assert_eq!(
                analysis::analyze_bands(size, &vec![0; length], channels, 21, 21),
                Err(AnalysisError::InvalidGeometry)
            );
        }
        for (effective_end, end) in [
            (0, 0),
            (0, 21),
            (1, 0),
            (22, 22),
            (2, 1),
            (usize::MAX, 21),
            (1, usize::MAX),
        ] {
            assert_eq!(
                analysis::analyze_bands(size, &input, 1, effective_end, end),
                Err(AnalysisError::InvalidBands)
            );
        }
        for value in [i32::MIN, i32::MAX] {
            input[0] = value;
            let before = input.clone();
            assert_eq!(
                analysis::analyze_bands(size, &input, 1, 21, 21),
                Err(AnalysisError::ArithmeticOverflow)
            );
            assert_eq!(input, before);
        }
        input.fill(0);
        input[78 * (n / 120)..100 * (n / 120)].fill(1 << 30);
        let before = input.clone();
        assert_eq!(
            analysis::analyze_bands(size, &input, 1, 21, 21),
            Err(AnalysisError::ArithmeticOverflow)
        );
        assert_eq!(input, before);
        input.fill(i32::MIN);
        input[..n / 120].fill(0);
        let result = analysis::analyze_bands(size, &input, 1, 1, 21)?;
        assert!(
            result
                .normalized(0)
                .is_some_and(|x| x.iter().all(|&v| v == 0))
        );
        assert!(
            result
                .log_energies(0)
                .is_some_and(|x| x[1..].iter().all(|&v| v == -(14 << 24)))
        );
    }
    Ok(())
}

#[test]
fn pcm_derived_long_short_and_low_rate_spectra_fit_band_domain()
-> Result<(), Box<dyn std::error::Error>> {
    for rate in [
        InputRate::Khz8,
        InputRate::Khz12,
        InputRate::Khz16,
        InputRate::Khz24,
        InputRate::Khz48,
    ] {
        for size in SIZES {
            let n = size.samples();
            let count = n / rate.factor();
            for pattern in 0..4 {
                let pcm: Vec<i16> = (0..count * 2)
                    .map(|i| match pattern {
                        0 => 32767,
                        1 => -32768,
                        2 => {
                            if i % 2 == 0 {
                                32767
                            } else {
                                -32768
                            }
                        }
                        _ => ((i as i16).wrapping_mul(7659)).wrapping_add(12345),
                    })
                    .collect();
                let mut context = vec![0; 2 * (n + 120)];
                for channel in 0..2 {
                    PreemphasisState::default().process(
                        size,
                        rate,
                        &pcm,
                        2,
                        channel,
                        &mut context[channel * (n + 120) + 120..(channel + 1) * (n + 120)],
                    )?;
                }
                for short in [false, true] {
                    for channels in [1, 2] {
                        let spectrum =
                            analysis::analyze_frame(size, &context, 2, channels, rate, short)?;
                        let result =
                            analysis::analyze_bands(size, spectrum.values(), channels, 21, 21)?;
                        assert_eq!(result.channels(), channels);
                        for c in 0..channels {
                            assert!(
                                result
                                    .amplitudes(c)
                                    .is_some_and(|a| a.iter().all(|&v| v > 0))
                            );
                        }
                    }
                }
            }
        }
    }
    Ok(())
}
