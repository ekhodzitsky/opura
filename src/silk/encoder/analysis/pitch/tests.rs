use super::*;
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
fn signal(value: i32) -> SignalType {
    match value {
        0 => SignalType::Inactive,
        1 => SignalType::Unvoiced,
        _ => SignalType::Voiced,
    }
}
#[test]
fn public_native_pitch_whitening_and_contours_match() -> Result<()> {
    let mut calls = 0;
    let mut voiced = 0;
    let mut rates = [0; 3];
    let mut first = 0;
    for (line_number, line) in include_str!("../../../../../tests/silk-pitch-analysis-fixtures.txt")
        .lines()
        .enumerate()
    {
        let Some(line) = line.strip_prefix("pitch ") else {
            continue;
        };
        let v: Vec<i32> = line
            .split_whitespace()
            .map(str::parse)
            .collect::<std::result::Result<_, _>>()?;
        let rate = match v[0] {
            8 => SilkRate::Khz8,
            12 => SilkRate::Khz12,
            _ => SilkRate::Khz16,
        };
        let count = v[1] as usize;
        let length = v[15] as usize;
        let duration = if count == 2 {
            FrameDuration::Ms10
        } else {
            FrameDuration::Ms20
        };
        let mut state = PitchState {
            rate: if v[5] != 0 { None } else { Some(rate) },
            lag: v[8] as u16,
            signal: signal(v[7]),
            correlation: v[9],
        };
        let input: Vec<i16> = v[16 + count..16 + count + length]
            .iter()
            .map(|&v| v as i16)
            .collect();
        let result = state.analyze(
            rate,
            duration,
            &input,
            PitchOptions {
                complexity: v[2] as u8,
                active: v[6] != 0,
                speech_activity_q8: v[3] as u16,
                input_tilt_q15: v[4] as i16,
            },
        )?;
        assert_eq!(
            result.prediction_gain_q16(),
            v[12],
            "gain line {}",
            line_number + 1
        );
        assert_eq!(
            result.residual(),
            v[16 + count + length..]
                .iter()
                .map(|&v| v as i16)
                .collect::<Vec<_>>(),
            "residual line {}",
            line_number + 1
        );
        assert_eq!(
            result.signal(),
            signal(v[10]),
            "signal line {}",
            line_number + 1
        );
        assert_eq!(
            result.correlation_q15(),
            v[11],
            "correlation line {}",
            line_number + 1
        );
        assert_eq!(
            i32::from(result.lag_index()),
            v[13],
            "index line {}",
            line_number + 1
        );
        assert_eq!(
            i32::from(result.contour_index()),
            v[14],
            "contour line {}",
            line_number + 1
        );
        assert_eq!(
            result.lags(),
            v[16..16 + count]
                .iter()
                .map(|&v| v as u16)
                .collect::<Vec<_>>(),
            "lags line {}",
            line_number + 1
        );
        assert_eq!(state.lag, result.lags()[count - 1]);
        assert_eq!(state.correlation, v[11]);
        assert_eq!(state.signal, result.signal());
        voiced += usize::from(v[10] == 2);
        first += usize::from(v[5] != 0);
        rates[match rate {
            SilkRate::Khz8 => 0,
            SilkRate::Khz12 => 1,
            SilkRate::Khz16 => 2,
        }] += 1;
        calls += 1;
    }
    assert_eq!(calls, 72);
    assert!(voiced > 24);
    // Six stream starts and the native 12-to-8 kHz transition at call 34.
    assert_eq!(first, 7);
    assert!(rates.iter().all(|&n| n >= 20));
    Ok(())
}
#[test]
fn pitch_reset_errors_and_full_scale_inputs() -> Result<()> {
    let options = PitchOptions {
        complexity: 10,
        active: true,
        speech_activity_q8: 256,
        input_tilt_q15: 0,
    };
    let mut state = PitchState::default();
    let mut ltp = crate::silk::encoder::analysis::LtpState::default();
    for rate in [SilkRate::Khz8, SilkRate::Khz12, SilkRate::Khz16] {
        for duration in [FrameDuration::Ms10, FrameDuration::Ms20] {
            let length = rate.khz() as usize
                * if duration == FrameDuration::Ms10 {
                    32
                } else {
                    42
                };
            let mut input = vec![0i16; length];
            for complexity in 0..=10 {
                for kind in 0..6 {
                    for (i, x) in input.iter_mut().enumerate() {
                        *x = match kind {
                            0 => 0,
                            1 => i16::MIN,
                            2 => i16::MAX,
                            3 => {
                                if i % 2 == 0 {
                                    i16::MIN
                                } else {
                                    i16::MAX
                                }
                            }
                            4 => {
                                (((i * 197 + complexity as usize * 713) % 65536) as i32 - 32768)
                                    as i16
                            }
                            _ => ((i % 64) as i32 * 1024 - 32768) as i16,
                        };
                    }
                    let frame = state.analyze(
                        rate,
                        duration,
                        &input,
                        PitchOptions {
                            complexity,
                            ..options
                        },
                    )?;
                    let fitted = ltp.analyze(&frame, false);
                    fitted.normalized_residual(&input, &vec![16384; frame.lags().len()])?;
                }
            }
            let before = state.clone();
            assert_eq!(
                state.analyze(rate, duration, &input[..length - 1], options),
                Err(AnalysisError::InvalidLength)
            );
            for invalid in [
                PitchOptions {
                    complexity: 11,
                    ..options
                },
                PitchOptions {
                    speech_activity_q8: 257,
                    ..options
                },
            ] {
                assert_eq!(
                    state.analyze(rate, duration, &input, invalid),
                    Err(AnalysisError::InvalidParameters)
                );
            }
            assert_eq!(state, before);
            state.reset();
            assert_eq!(state, PitchState::default());
            let initial = state.analyze(rate, duration, &input, options)?;
            assert_eq!(initial.signal(), SignalType::Unvoiced);
            assert!(initial.lags().iter().all(|&v| v == 0));
            let inactive = state.analyze(
                rate,
                duration,
                &input,
                PitchOptions {
                    active: false,
                    ..options
                },
            )?;
            assert_eq!(inactive.signal(), SignalType::Inactive);
            assert_eq!(inactive.correlation_q15(), 0);
        }
    }
    let mut fresh = PitchState::default();
    let changed = state.analyze(SilkRate::Khz8, FrameDuration::Ms20, &[0; 336], options)?;
    assert_eq!(
        changed,
        fresh.analyze(SilkRate::Khz8, FrameDuration::Ms20, &[0; 336], options)?
    );
    assert_eq!(state, fresh);
    Ok(())
}
