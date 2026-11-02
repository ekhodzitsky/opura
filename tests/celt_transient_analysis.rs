//! Pinned fixed-point tone and temporal mask analysis.
use opura::celt::{
    FrameSize,
    analysis::{self, AnalysisError},
};
const SIZES: [FrameSize; 4] = [
    FrameSize::Ms2_5,
    FrameSize::Ms5,
    FrameSize::Ms10,
    FrameSize::Ms20,
];
#[test]
fn original_tonal_guard_masking_weak_and_tf_diagnostics_match()
-> Result<(), Box<dyn std::error::Error>> {
    let mut lines = include_str!("celt-transient-analysis-fixtures.txt")
        .lines()
        .filter(|l| !l.starts_with('#'));
    let mut rows = 0;
    let mut strong = 0;
    let mut weak = 0;
    let mut tonal = 0;
    while let Some(line) = lines.next() {
        let c: Vec<_> = line.split_whitespace().collect();
        assert_eq!(c[0], "frame");
        let size = SIZES[c[1].parse::<usize>()?];
        let channels = c[2].parse::<usize>()?;
        let input: Vec<i32> = c[3..].iter().map(|s| s.parse()).collect::<Result<_, _>>()?;
        let before = input.clone();
        for allow in [false, true] {
            let c: Vec<i64> = lines
                .next()
                .ok_or("analysis")?
                .split_whitespace()
                .skip(1)
                .map(|s| s.parse())
                .collect::<Result<_, _>>()?;
            assert_eq!(c.len(), 8);
            assert_eq!(c[0], i64::from(allow));
            let result = analysis::analyze_transients(size, &input, channels, allow)?;
            assert_eq!(
                [
                    i64::from(result.transient()),
                    i64::from(result.weak_transient()),
                    i64::from(result.tf_estimate()),
                    result.channel() as i64,
                    i64::from(result.tone_frequency()),
                    i64::from(result.toneishness()),
                    i64::from(result.mask_metric())
                ],
                c[1..],
                "analysis frame{rows} weak{allow}"
            );
            assert_eq!(input, before);
            strong += usize::from(result.transient());
            weak += usize::from(result.weak_transient());
            tonal += usize::from(result.toneishness() > 1 << 28);
        }
        rows += 1;
    }
    eprintln!("analysis frames{rows} strong{strong} weak{weak} tonal{tonal}");
    assert_eq!(rows, 1788);
    assert!(strong > 100);
    assert!(weak > 10);
    assert!(tonal > 100);
    Ok(())
}
#[test]
fn invalid_shapes_and_nonphysical_magnitudes_reject_without_input_changes()
-> Result<(), Box<dyn std::error::Error>> {
    for channels in [0, 3, usize::MAX] {
        assert_eq!(
            analysis::analyze_transients(FrameSize::Ms20, &[], channels, false),
            Err(AnalysisError::InvalidGeometry)
        );
    }
    for size in SIZES {
        for channels in [1, 2] {
            let n = channels * (size.samples() + 120);
            let input = vec![0; n];
            for len in [0, n - 1, n + 1] {
                assert_eq!(
                    analysis::analyze_transients(size, &vec![0; len], channels, false),
                    Err(AnalysisError::InvalidGeometry)
                );
            }
            for value in [i32::MIN, -(1 << 28) - 1, (1 << 28) + 1, i32::MAX] {
                let mut bad = input.clone();
                bad[n / 2] = value;
                let before = bad.clone();
                assert_eq!(
                    analysis::analyze_transients(size, &bad, channels, false),
                    Err(AnalysisError::ArithmeticOverflow)
                );
                assert_eq!(bad, before);
            }
            for value in [-(1 << 28), 1 << 28] {
                analysis::analyze_transients(size, &vec![value; n], channels, false)?;
            }
        }
    }
    Ok(())
}

#[test]
fn bounded_random_contexts_are_deterministic_and_keep_physical_diagnostics()
-> Result<(), Box<dyn std::error::Error>> {
    let mut seed = 0x544f4e45u32;
    for size in SIZES {
        for channels in [1, 2] {
            for _ in 0..64 {
                let values: Vec<i32> = (0..channels * (size.samples() + 120))
                    .map(|_| {
                        seed ^= seed << 13;
                        seed ^= seed >> 17;
                        seed ^= seed << 5;
                        (seed as i32) >> 3
                    })
                    .collect();
                for allow in [false, true] {
                    let result = analysis::analyze_transients(size, &values, channels, allow)?;
                    assert_eq!(
                        analysis::analyze_transients(size, &values, channels, allow)?,
                        result
                    );
                    assert!(result.channel() < channels);
                    assert!(!(result.transient() && result.weak_transient()));
                    assert!((0..=1 << 29).contains(&result.toneishness()));
                    assert!((-1..=25736).contains(&result.tone_frequency()));
                    assert!((0..=16384).contains(&result.tf_estimate()));
                    assert!(result.mask_metric() >= 0);
                }
            }
        }
    }
    Ok(())
}
