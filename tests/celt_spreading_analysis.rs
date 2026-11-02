//! Independent original pulse-spreading state and recursive history evidence.
use opura::celt::{
    FrameSize,
    analysis::{AnalysisError, SpreadingOptions, SpreadingState},
    pvq::Spread,
};
const SIZES: [FrameSize; 4] = [
    FrameSize::Ms2_5,
    FrameSize::Ms5,
    FrameSize::Ms10,
    FrameSize::Ms20,
];
const SPREAD: [Spread; 4] = [
    Spread::None,
    Spread::Light,
    Spread::Normal,
    Spread::Aggressive,
];
#[test]
fn original_distributions_hysteresis_hf_and_tapset_histories_match()
-> Result<(), Box<dyn std::error::Error>> {
    let mut lines = include_str!("celt-spreading-analysis-fixtures.txt")
        .lines()
        .filter(|l| !l.starts_with('#'));
    let defaults: Vec<i32> = lines
        .next()
        .ok_or("defaults")?
        .split_whitespace()
        .skip(1)
        .map(|s| s.parse())
        .collect::<Result<_, _>>()?;
    let initial = SpreadingState::default();
    assert_eq!(
        [
            initial.decision() as i32,
            i32::from(initial.tonal_average()),
            i32::from(initial.hf_average()),
            i32::from(initial.tapset())
        ],
        defaults.as_slice()
    );
    let mut frames = 0;
    let mut analyses = 0;
    let mut decisions = [0; 4];
    let mut hf_changes = 0;
    let mut taps = [0; 3];
    while let Some(line) = lines.next() {
        let f: Vec<_> = line.split_whitespace().collect();
        assert_eq!(f[0], "frame");
        let size = SIZES[f[1].parse::<usize>()?];
        let channels = f[2].parse::<usize>()?;
        let end = f[3].parse::<usize>()?;
        let count = f[4].parse::<usize>()?;
        let input: Vec<i32> = f[5..].iter().map(|s| s.parse()).collect::<Result<_, _>>()?;
        let original = input.clone();
        for case in 0..count {
            let d: Vec<i32> = lines
                .next()
                .ok_or("spread row")?
                .split_whitespace()
                .skip(1)
                .map(|s| s.parse())
                .collect::<Result<_, _>>()?;
            assert_eq!(d.len(), 30);
            let mut state = SpreadingState::from_history(
                u16::try_from(d[1])?,
                u8::try_from(d[2])?,
                u8::try_from(d[3])?,
                SPREAD[usize::try_from(d[4])?],
            )?;
            let weights: [u8; 21] = d[5..26]
                .iter()
                .map(|&x| u8::try_from(x))
                .collect::<Result<Vec<_>, _>>()?
                .try_into()
                .map_err(|_| "weights")?;
            let result = state.process(
                size,
                &input,
                SpreadingOptions {
                    channels,
                    end,
                    weights,
                    update_hf: d[0] != 0,
                },
            )?;
            assert_eq!(
                [
                    result as i32,
                    i32::from(state.tonal_average()),
                    i32::from(state.hf_average()),
                    i32::from(state.tapset())
                ],
                d[26..30],
                "frame{frames} case{case}"
            );
            assert_eq!(state.decision(), result);
            assert_eq!(input, original);
            decisions[result as usize] += 1;
            taps[usize::from(state.tapset())] += 1;
            hf_changes += usize::from(i32::from(state.hf_average()) != d[2]);
            analyses += 1;
        }
        frames += 1;
    }
    eprintln!(
        "spreading frames{frames} analyses{analyses} decisions{decisions:?} taps{taps:?} HFchanges{hf_changes}"
    );
    assert_eq!((frames, analyses), (2019, 41486));
    assert!(decisions.iter().all(|&n| n > 100));
    assert!(taps.iter().all(|&n| n > 100));
    assert!(hf_changes > 100);
    Ok(())
}

#[test]
fn checked_histories_invalid_contexts_and_late_norm_errors_are_atomic()
-> Result<(), Box<dyn std::error::Error>> {
    for average in [769, u16::MAX] {
        assert_eq!(
            SpreadingState::from_history(average, 0, 0, Spread::Normal),
            Err(AnalysisError::InvalidSpreadParameters)
        );
    }
    for hf in [65, u8::MAX] {
        assert_eq!(
            SpreadingState::from_history(0, hf, 0, Spread::Normal),
            Err(AnalysisError::InvalidSpreadParameters)
        );
    }
    for tap in [3, u8::MAX] {
        assert_eq!(
            SpreadingState::from_history(0, 0, tap, Spread::Normal),
            Err(AnalysisError::InvalidSpreadParameters)
        );
    }
    for size in SIZES {
        let n = size.samples();
        let mut state = SpreadingState::from_history(768, 64, 2, Spread::Aggressive)?;
        let before = state;
        for channels in [0, 3, usize::MAX] {
            assert_eq!(
                state.process(
                    size,
                    &[],
                    SpreadingOptions {
                        channels,
                        ..SpreadingOptions::default()
                    }
                ),
                Err(AnalysisError::InvalidGeometry)
            );
            assert_eq!(state, before);
        }
        for end in [0, 22, usize::MAX] {
            assert_eq!(
                state.process(
                    size,
                    &vec![0; n],
                    SpreadingOptions {
                        end,
                        ..SpreadingOptions::default()
                    }
                ),
                Err(AnalysisError::InvalidBands)
            );
            assert_eq!(state, before);
        }
        for count in [0, n - 1, n + 1] {
            assert_eq!(
                state.process(size, &vec![0; count], SpreadingOptions::default()),
                Err(AnalysisError::InvalidGeometry)
            );
            assert_eq!(state, before);
        }
        let mut weights = [32; 21];
        weights[20] = 33;
        assert_eq!(
            state.process(
                size,
                &vec![0; n],
                SpreadingOptions {
                    weights,
                    ..SpreadingOptions::default()
                }
            ),
            Err(AnalysisError::InvalidSpreadParameters)
        );
        assert_eq!(state, before);
        for channels in [1, 2] {
            for bad in [i32::MIN, i32::MAX, 1 << 25] {
                let mut input = vec![0; channels * n];
                input[(channels - 1) * n + 100 * (size.samples() / 120) - 1] = bad;
                let original = input.clone();
                assert_eq!(
                    state.process(
                        size,
                        &input,
                        SpreadingOptions {
                            channels,
                            ..SpreadingOptions::default()
                        }
                    ),
                    Err(AnalysisError::ArithmeticOverflow)
                );
                assert_eq!(state, before);
                assert_eq!(input, original);
            }
        }
        assert_eq!(
            state.process(
                size,
                &vec![0; n],
                SpreadingOptions {
                    weights: [0; 21],
                    ..SpreadingOptions::default()
                }
            ),
            Err(AnalysisError::InvalidSpreadParameters)
        );
        assert_eq!(state, before);
        state.reset();
        assert_eq!(state, SpreadingState::default());
    }
    let mut state = SpreadingState::from_history(768, 64, 2, Spread::Aggressive)?;
    assert_eq!(
        state.process(
            FrameSize::Ms2_5,
            &[0; 120],
            SpreadingOptions {
                end: 1,
                weights: [0; 21],
                update_hf: true,
                ..SpreadingOptions::default()
            }
        )?,
        Spread::None
    );
    assert_eq!(
        (state.tonal_average(), state.hf_average(), state.tapset()),
        (768, 64, 2)
    );
    let mut input = [0; 960];
    input[0] = 1 << 24;
    input[1] = 1 << 23;
    state.process(FrameSize::Ms20, &input, SpreadingOptions::default())?;
    let before = state;
    input[1] += 1;
    let original = input;
    assert_eq!(
        state.process(FrameSize::Ms20, &input, SpreadingOptions::default()),
        Err(AnalysisError::ArithmeticOverflow)
    );
    assert_eq!(state, before);
    assert_eq!(input, original);
    Ok(())
}
