//! Original bounded trim state and allocation policy analysis.
use opura::celt::{
    FrameSize,
    analysis::{AnalysisError, TrimOptions, TrimState},
};
const SIZES: [FrameSize; 4] = [
    FrameSize::Ms2_5,
    FrameSize::Ms5,
    FrameSize::Ms10,
    FrameSize::Ms20,
];
#[test]
fn original_trim_rate_correlation_saving_tilt_and_temporal_controls_match()
-> Result<(), Box<dyn std::error::Error>> {
    let mut lines = include_str!("celt-trim-analysis-fixtures.txt")
        .lines()
        .filter(|l| !l.starts_with('#'));
    let mut frames = 0;
    let mut analyses = 0;
    let mut overflow = 0;
    let mut indices = [0; 11];
    while let Some(line) = lines.next() {
        let f: Vec<_> = line.split_whitespace().collect();
        assert!(matches!(f[0], "frame" | "sequence"));
        let sequential = f[0] == "sequence";
        let mut chain = TrimState::default();
        let size = SIZES[f[1].parse::<usize>()?];
        let channels = f[2].parse::<usize>()?;
        let end = f[3].parse::<usize>()?;
        let count = f[4].parse::<usize>()?;
        let values: Vec<i32> = f[5..].iter().map(|s| s.parse()).collect::<Result<_, _>>()?;
        let n = channels * size.samples();
        assert_eq!(values.len(), n + 42);
        let input = &values[..n];
        let mut logs = [[0; 21]; 2];
        logs[0].copy_from_slice(&values[n..n + 21]);
        logs[1].copy_from_slice(&values[n + 21..]);
        let original = values.clone();
        for case in 0..count {
            let f: Vec<_> = lines.next().ok_or("trim row")?.split_whitespace().collect();
            assert_eq!(f.len(), 10);
            let d: Vec<i32> = f[1..].iter().map(|s| s.parse()).collect::<Result<_, _>>()?;
            let mut state = if sequential {
                assert_eq!(i32::from(chain.stereo_saving()), d[0]);
                chain
            } else {
                TrimState::from_saving(u16::try_from(d[0])?)?
            };
            let before = state;
            let options = TrimOptions {
                channels,
                end,
                equivalent_rate: d[1],
                tf_estimate: i16::try_from(d[2])?,
                intensity: usize::try_from(d[3])?,
                surround_trim: d[4],
            };
            let result = state.process(size, input, &logs, options);
            if f[0] == "overflow" {
                assert_eq!(
                    result,
                    Err(AnalysisError::ArithmeticOverflow),
                    "overflow frame{frames} case{case}"
                );
                assert_eq!(state, before);
                overflow += 1;
            } else {
                assert_eq!(f[0], "trim");
                let result = result?;
                assert_eq!(
                    [
                        i32::from(result.index()),
                        i32::from(result.trim_q8()),
                        result.spectral_tilt_q19(),
                        i32::from(state.stereo_saving())
                    ],
                    d[5..9],
                    "trim frame{frames} case{case}"
                );
                indices[usize::from(result.index())] += 1;
                analyses += 1;
            }
            if sequential {
                chain = state;
            }
            assert_eq!(values, original);
        }
        frames += 1;
    }
    eprintln!("trim frames{frames} analyses{analyses} overflow{overflow} indices{indices:?}");
    assert_eq!(frames, 1090);
    assert_eq!((analyses, overflow), (16233, 1216));
    assert!(overflow > 100);
    assert!(indices.iter().all(|&n| n > 0));
    Ok(())
}

#[test]
fn allocation_analysis_domains_unused_inputs_and_late_tilt_errors_are_atomic()
-> Result<(), Box<dyn std::error::Error>> {
    use opura::celt::{
        analysis::{IntensityState, analyze_stereo, equivalent_rate},
        energy::BandLayout,
    };
    for band in [22, u8::MAX] {
        assert_eq!(
            IntensityState::from_band(band),
            Err(AnalysisError::InvalidAllocationParameters)
        );
    }
    let mut intensity = IntensityState::from_band(21)?;
    assert_eq!(intensity.select(i16::MIN, BandLayout::new(2, 17, 21)?), 17);
    assert_eq!(intensity.select(i16::MAX, BandLayout::new(2, 17, 21)?), 21);
    assert_eq!(intensity.select(i16::MIN, BandLayout::new(1, 18, 18)?), 18);
    assert_eq!(intensity.select(i16::MAX, BandLayout::new(2, 17, 19)?), 19);
    intensity.reset();
    assert_eq!(intensity.band(), 0);
    for saving in [1281, u16::MAX] {
        assert_eq!(
            TrimState::from_saving(saving),
            Err(AnalysisError::InvalidAllocationParameters)
        );
    }
    let options = TrimOptions::default();
    let logs = [[0; 21]; 2];
    for size in SIZES {
        let n = size.samples();
        let stereo = vec![0; 2 * n];
        for channels in [0, 3, usize::MAX] {
            assert_eq!(
                equivalent_rate(size, channels, 128),
                Err(AnalysisError::InvalidGeometry)
            );
        }
        for payload in [0, 1, 1276, usize::MAX] {
            assert_eq!(
                equivalent_rate(size, 1, payload),
                Err(AnalysisError::InvalidAllocationParameters)
            );
        }
        for count in [0, n, 2 * n - 1, 2 * n + 1] {
            assert_eq!(
                analyze_stereo(size, &vec![0; count]),
                Err(AnalysisError::InvalidGeometry)
            );
        }
        let mut invalid_stereo = stereo.clone();
        invalid_stereo[n] = i32::MIN;
        assert_eq!(
            analyze_stereo(size, &invalid_stereo),
            Err(AnalysisError::ArithmeticOverflow)
        );
        let mut state = TrimState::from_saving(1280)?;
        let before = state;
        for channels in [0, 3, usize::MAX] {
            assert_eq!(
                state.process(
                    size,
                    &[],
                    &logs,
                    TrimOptions {
                        channels,
                        ..options
                    }
                ),
                Err(AnalysisError::InvalidGeometry)
            );
            assert_eq!(state, before);
        }
        for count in [0, n - 1, n + 1] {
            assert_eq!(
                state.process(size, &vec![0; count], &logs, options),
                Err(AnalysisError::InvalidGeometry)
            );
            assert_eq!(state, before);
        }
        for end in [0, 1, 22, usize::MAX] {
            assert_eq!(
                state.process(size, &vec![0; n], &logs, TrimOptions { end, ..options }),
                Err(AnalysisError::InvalidBands)
            );
            assert_eq!(state, before);
        }
        for end in [2, 7] {
            assert_eq!(
                state.process(
                    size,
                    &stereo,
                    &logs,
                    TrimOptions {
                        channels: 2,
                        end,
                        intensity: end,
                        ..options
                    }
                ),
                Err(AnalysisError::InvalidBands)
            );
            assert_eq!(state, before);
        }
        let mut invalid = vec![TrimOptions {
            intensity: 22,
            ..options
        }];
        for tf_estimate in [i16::MIN, -1, 16385, i16::MAX] {
            invalid.push(TrimOptions {
                tf_estimate,
                ..options
            });
        }
        for surround_trim in [i32::MIN, -(4 << 24) - 1, (4 << 24) + 1, i32::MAX] {
            invalid.push(TrimOptions {
                surround_trim,
                ..options
            });
        }
        for control in invalid {
            assert_eq!(
                state.process(size, &vec![0; n], &logs, control),
                Err(AnalysisError::InvalidAllocationParameters)
            );
            assert_eq!(state, before);
        }
        for bad in [i32::MIN, -(32 << 24) - 1, (32 << 24) + 1, i32::MAX] {
            let mut invalid = logs;
            invalid[0][19] = bad;
            let original = invalid;
            assert_eq!(
                state.process(size, &vec![0; n], &invalid, options),
                Err(AnalysisError::ArithmeticOverflow)
            );
            assert_eq!(state, before);
            assert_eq!(invalid, original);
            let mut vector = stereo.clone();
            vector[n + 7 * (n / 120)] = bad;
            if bad.abs_diff(0) > 1 << 25 {
                assert_eq!(
                    state.process(
                        size,
                        &vector,
                        &logs,
                        TrimOptions {
                            channels: 2,
                            ..options
                        }
                    ),
                    Err(AnalysisError::ArithmeticOverflow)
                );
                assert_eq!(state, before);
            }
        }
        // Mono never reads normalized coordinates, and its saving history persists.
        state.process(
            size,
            &vec![i32::MAX; n],
            &logs,
            TrimOptions {
                end: 2,
                intensity: 0,
                ..options
            },
        )?;
        assert_eq!(state, before);
        // Stereo with intensity below eight only reads the first eight bands.
        let mut partial = vec![i32::MAX; 2 * n];
        let active = 8 * (n / 120);
        partial[..active].fill(0);
        partial[n..n + active].fill(0);
        state.process(
            size,
            &partial,
            &logs,
            TrimOptions {
                channels: 2,
                intensity: 0,
                ..options
            },
        )?;
        state.reset();
        assert_eq!(state.stereo_saving(), 0);
    }
    let size = FrameSize::Ms20;
    let mut normalized = vec![0; 1920];
    let bands = [
        0, 1, 2, 3, 4, 5, 6, 7, 8, 10, 12, 14, 16, 20, 24, 28, 34, 40, 48, 60, 78, 100,
    ];
    for c in 0..2 {
        for band in 0..21 {
            normalized[c * 960 + bands[band] * 8] = 1 << 24;
        }
    }
    let mut overflowing = [[0; 21]; 2];
    for channel in &mut overflowing {
        for (i, value) in channel[..20].iter_mut().enumerate() {
            *value = if 2 + 2 * i >= 21 {
                32 << 24
            } else {
                -(32 << 24)
            };
        }
    }
    let mut state = TrimState::default();
    let before = state;
    assert_eq!(
        state.process(
            size,
            &normalized,
            &overflowing,
            TrimOptions {
                channels: 2,
                ..options
            }
        ),
        Err(AnalysisError::ArithmeticOverflow)
    );
    assert_eq!(state, before);
    let mut boundary = vec![0; 480];
    boundary[0] = 1 << 24;
    boundary[1] = 1 << 23;
    let mut state = TrimState::default();
    state.process(
        FrameSize::Ms5,
        &boundary,
        &logs,
        TrimOptions {
            channels: 2,
            ..options
        },
    )?;
    boundary[1] += 1;
    let before = state;
    assert_eq!(
        state.process(
            FrameSize::Ms5,
            &boundary,
            &logs,
            TrimOptions {
                channels: 2,
                ..options
            }
        ),
        Err(AnalysisError::ArithmeticOverflow)
    );
    assert_eq!(state, before);
    Ok(())
}

#[test]
fn bounded_allocation_policy_contexts_keep_histories_and_input_contracts()
-> Result<(), Box<dyn std::error::Error>> {
    let mut state = TrimState::from_saving(1022)?;
    state.reset();
    assert_eq!(state.stereo_saving(), 0);
    let bands = [
        0, 1, 2, 3, 4, 5, 6, 7, 8, 10, 12, 14, 16, 20, 24, 28, 34, 40, 48, 60, 78, 100,
    ];
    let mut seed = 0x5452494du32;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed
    };
    let mut successes = 0;
    let mut rejections = 0;
    for size in SIZES {
        for channels in [1, 2] {
            for _ in 0..128 {
                let end = if channels == 1 {
                    2 + next() as usize % 20
                } else {
                    8 + next() as usize % 14
                };
                let intensity = next() as usize % (end + 1);
                let n = size.samples();
                let factor = n / 120;
                let mut normalized = vec![i32::MAX; channels * n];
                if channels == 2 {
                    for c in 0..2 {
                        for band in 0..8.max(intensity) {
                            let start = c * n + bands[band] * factor;
                            let stop = c * n + bands[band + 1] * factor;
                            normalized[start..stop].fill(0);
                            let at = start + next() as usize % (stop - start);
                            normalized[at] =
                                [0, 1 << 23, -(1 << 23), 1 << 24, -(1 << 24)][next() as usize % 5];
                        }
                    }
                }
                let mut logs = [[i32::MAX; 21]; 2];
                for channel in &mut logs[..channels] {
                    for value in &mut channel[..end - 1] {
                        *value = (next() % ((64 << 24) + 1)) as i32 - (32 << 24);
                    }
                }
                let options = TrimOptions {
                    channels,
                    end,
                    intensity,
                    equivalent_rate: next() as i32,
                    tf_estimate: (next() % 16385) as i16,
                    surround_trim: (next() % ((8 << 24) + 1)) as i32 - (4 << 24),
                };
                let mut state = TrimState::from_saving((next() % 1281) as u16)?;
                let before = state;
                let original = normalized.clone();
                let original_logs = logs;
                match state.process(size, &normalized, &logs, options) {
                    Ok(result) => {
                        assert!(result.index() <= 10);
                        assert!(state.stereo_saving() <= 1280);
                        if channels == 1 {
                            assert_eq!(state, before);
                        }
                        successes += 1;
                    }
                    Err(AnalysisError::ArithmeticOverflow) => {
                        assert_eq!(state, before);
                        rejections += 1;
                    }
                    Err(error) => {
                        return Err(format!("unexpected bounded trim error: {error}").into());
                    }
                }
                assert_eq!(normalized, original);
                assert_eq!(logs, original_logs);
            }
        }
    }
    assert_eq!(successes + rejections, 1024);
    assert!(successes > 900);
    Ok(())
}
