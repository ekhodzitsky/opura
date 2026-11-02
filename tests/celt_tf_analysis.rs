//! Original native time-frequency analysis, including path and level ties.
use opura::celt::{
    FrameSize,
    analysis::{AnalysisError, TfAnalysisOptions, analyze_tf},
};
const SIZES: [FrameSize; 4] = [
    FrameSize::Ms2_5,
    FrameSize::Ms5,
    FrameSize::Ms10,
    FrameSize::Ms20,
];
#[test]
fn original_l1_haar_metrics_select_and_weighted_paths_match()
-> Result<(), Box<dyn std::error::Error>> {
    let mut lines = include_str!("celt-tf-analysis-fixtures.txt")
        .lines()
        .filter(|l| !l.starts_with('#'));
    assert_eq!(lines.next(), Some("constant 1311"));
    let mut frames = 0;
    let mut analyses = 0;
    let mut selected = 0;
    let mut positive = 0;
    let mut negative = 0;
    while let Some(line) = lines.next() {
        let f: Vec<_> = line.split_whitespace().collect();
        assert_eq!(f[0], "frame");
        let size = SIZES[f[1].parse::<usize>()?];
        let end = f[2].parse::<usize>()?;
        let options = f[3].parse::<usize>()?;
        let input: Vec<i32> = f[4..].iter().map(|s| s.parse()).collect::<Result<_, _>>()?;
        let original = input.clone();
        for _ in 0..options {
            let f: Vec<i32> = lines
                .next()
                .ok_or("TF row")?
                .split_whitespace()
                .skip(1)
                .map(|s| s.parse())
                .collect::<Result<_, _>>()?;
            assert_eq!(f.len(), 67);
            let controls = TfAnalysisOptions {
                transient: f[0] != 0,
                lambda: u16::try_from(f[1])?,
                estimate: i16::try_from(f[2])?,
                importance: f[3..24]
                    .iter()
                    .map(|&x| u8::try_from(x))
                    .collect::<Result<Vec<_>, _>>()?
                    .try_into()
                    .map_err(|_| "weights")?,
            };
            let result = analyze_tf(size, &input, end, controls)?;
            assert_eq!(result.end(), end);
            assert_eq!(result.select(), f[24] != 0, "select frame{frames}");
            for i in 0..21 {
                assert_eq!(
                    i32::from(result.metrics()[i]),
                    f[25 + i],
                    "metric frame{frames} band{i} controls{controls:?}"
                );
                assert_eq!(
                    result.decisions()[i],
                    f[46 + i] != 0,
                    "decision frame{frames} band{i} controls{controls:?}"
                );
                positive += usize::from(result.metrics()[i] > 0);
                negative += usize::from(result.metrics()[i] < 0);
            }
            assert_eq!(input, original);
            selected += usize::from(result.select());
            analyses += 1;
        }
        frames += 1;
    }
    eprintln!(
        "TF frames{frames} analyses{analyses} alternate{selected} positive{positive} negative{negative}"
    );
    assert_eq!((frames, analyses), (672, 7056));
    assert!(selected > 0 && positive > 0 && negative > 0);
    Ok(())
}
#[test]
fn invalid_inputs_and_exact_norm_boundary_preserve_immutable_context()
-> Result<(), Box<dyn std::error::Error>> {
    let options = TfAnalysisOptions::default();
    for size in SIZES {
        let input = vec![0; size.samples()];
        for count in [0, size.samples() - 1, size.samples() + 1] {
            assert_eq!(
                analyze_tf(size, &vec![0; count], 21, options),
                Err(AnalysisError::InvalidGeometry)
            );
        }
        for end in [0, 22, usize::MAX] {
            assert_eq!(
                analyze_tf(size, &input, end, options),
                Err(AnalysisError::InvalidBands)
            );
        }
        for estimate in [-1, 16385, i16::MIN, i16::MAX] {
            assert_eq!(
                analyze_tf(
                    size,
                    &input,
                    21,
                    TfAnalysisOptions {
                        estimate,
                        ..options
                    }
                ),
                Err(AnalysisError::InvalidTfParameters)
            );
        }
        for bad in [i32::MIN, i32::MAX, 1 << 25] {
            let mut invalid = input.clone();
            invalid[0] = bad;
            let before = invalid.clone();
            assert_eq!(
                analyze_tf(size, &invalid, 21, options),
                Err(AnalysisError::ArithmeticOverflow)
            );
            assert_eq!(invalid, before);
        }
    }
    assert_eq!(
        analyze_tf(
            FrameSize::Ms2_5,
            &[0; 120],
            21,
            TfAnalysisOptions {
                transient: true,
                ..options
            }
        ),
        Err(AnalysisError::InvalidTfParameters)
    );
    let mut input = [0; 960];
    input[0] = 1 << 24;
    input[1] = 1 << 23;
    for transient in [false, true] {
        analyze_tf(
            FrameSize::Ms20,
            &input,
            21,
            TfAnalysisOptions {
                transient,
                ..options
            },
        )?;
    }
    input[1] += 1;
    let before = input;
    assert_eq!(
        analyze_tf(FrameSize::Ms20, &input, 21, options),
        Err(AnalysisError::ArithmeticOverflow)
    );
    assert_eq!(input, before);
    Ok(())
}
