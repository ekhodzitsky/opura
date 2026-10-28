//! Independent reference and transactional-state tests for SILK pitch/LTP.
use opura::range::RangeDecoder;
use opura::silk::gains::{FrameDuration, SignalType};
use opura::silk::pitch::{PitchCoding, PitchError, PitchState, SilkRate};

#[test]
fn upstream_sequences_match_parameters_and_entropy() -> Result<(), Box<dyn std::error::Error>> {
    let mut input = Vec::new();
    let mut decoder = RangeDecoder::new(&[]);
    let mut state = PitchState::default();
    let mut rate = SilkRate::Khz8;
    let mut duration = FrameDuration::Ms10;
    let mut count = 2;
    let mut cases = 0;
    let mut codebooks = [[false; 32]; 3];
    let mut contours = [[false; 34]; 4];
    let mut scales = [false; 3];
    let mut seeds = [false; 4];
    for (line_number, line) in include_str!("silk-pitch-fixtures.txt").lines().enumerate() {
        if line.starts_with('#') {
            continue;
        }
        let columns: Vec<_> = line.split_whitespace().collect();
        if columns[0] == "frame" {
            input.clear();
            for i in (0..columns[1].len()).step_by(2) {
                input.push(u8::from_str_radix(&columns[1][i..i + 2], 16)?);
            }
            rate = match columns[2] {
                "8" => SilkRate::Khz8,
                "12" => SilkRate::Khz12,
                _ => SilkRate::Khz16,
            };
            count = columns[3].parse()?;
            duration = if count == 2 {
                FrameDuration::Ms10
            } else {
                FrameDuration::Ms20
            };
            decoder = RangeDecoder::new(&input);
            state.reset();
            continue;
        }
        let expected: Vec<i64> = columns
            .iter()
            .map(|s| s.parse())
            .collect::<Result<_, _>>()?;
        let signal = match expected[0] {
            0 => SignalType::Inactive,
            1 => SignalType::Unvoiced,
            _ => SignalType::Voiced,
        };
        let coding = match expected[1] {
            0 => PitchCoding::Independent,
            1 => PitchCoding::IndependentWithoutScaling,
            _ => PitchCoding::Conditional,
        };
        let frame = state.decode(&mut decoder, rate, duration, signal, coding)?;
        let mut actual = vec![
            expected[0],
            expected[1],
            i64::from(frame.lag_index().unwrap_or(0)),
            i64::from(frame.contour_index()),
            i64::from(frame.periodicity_index()),
            i64::from(frame.scale_index()),
            i64::from(frame.scale_q14()),
            i64::from(frame.seed()),
        ];
        actual.extend(frame.ltp_indices().iter().copied().map(i64::from));
        actual.extend(frame.lags().iter().copied().map(i64::from));
        actual.extend(
            frame
                .coefficients_q14()
                .iter()
                .flatten()
                .copied()
                .map(i64::from),
        );
        actual.extend([
            i64::from(decoder.range()),
            i64::from(decoder.tell()),
            i64::from(decoder.tell_frac()),
        ]);
        assert_eq!(actual, expected, "fixture line {}", line_number + 1);
        assert_eq!(frame.lags().len(), count);
        assert_eq!(state.previous_lag_index(), frame.lag_index());
        if signal == SignalType::Voiced {
            let model = usize::from(rate != SilkRate::Khz8) * 2 + usize::from(count == 4);
            contours[model][frame.contour_index() as usize] = true;
            for &index in frame.ltp_indices() {
                codebooks[frame.periodicity_index() as usize][index as usize] = true;
            }
            scales[frame.scale_index() as usize] = true;
        }
        seeds[frame.seed() as usize] = true;
        cases += 1;
    }
    assert_eq!(cases, 3618);
    for (i, size) in [8, 16, 32].into_iter().enumerate() {
        assert!(codebooks[i][..size].iter().all(|v| *v));
    }
    for (model, size) in [3, 11, 12, 34].into_iter().enumerate() {
        assert!(contours[model][..size].iter().all(|v| *v));
    }
    assert!(scales.into_iter().all(|v| v));
    assert!(seeds.into_iter().all(|v| v));
    Ok(())
}

#[test]
fn rate_changes_reset_history_and_errors_preserve_both_states() -> Result<(), PitchError> {
    let mut state = PitchState::default();
    let data = [0x8a; 64];
    let mut decoder = RangeDecoder::new(&data);
    state.decode(
        &mut decoder,
        SilkRate::Khz8,
        FrameDuration::Ms20,
        SignalType::Voiced,
        PitchCoding::Independent,
    )?;
    let before = state.clone();
    let mut empty = RangeDecoder::new(&[]);
    let saved = empty.clone();
    assert_eq!(
        state.decode(
            &mut empty,
            SilkRate::Khz16,
            FrameDuration::Ms20,
            SignalType::Voiced,
            PitchCoding::Conditional
        ),
        Err(PitchError::BitBudgetExceeded)
    );
    assert_eq!(state, before);
    assert_eq!(empty, saved);
    let mut a = RangeDecoder::new(&data);
    let mut b = a.clone();
    let actual = state.decode(
        &mut a,
        SilkRate::Khz12,
        FrameDuration::Ms20,
        SignalType::Voiced,
        PitchCoding::Conditional,
    )?;
    let expected = PitchState::default().decode(
        &mut b,
        SilkRate::Khz12,
        FrameDuration::Ms20,
        SignalType::Voiced,
        PitchCoding::Conditional,
    )?;
    assert_eq!(actual, expected);
    assert_eq!(a, b);
    state.decode(
        &mut decoder,
        SilkRate::Khz12,
        FrameDuration::Ms10,
        SignalType::Inactive,
        PitchCoding::Conditional,
    )?;
    assert_eq!(state.previous_lag_index(), None);
    state.reset();
    assert_eq!(state, PitchState::default());
    Ok(())
}

#[test]
fn all_short_inputs_are_bounded_and_transactional() -> Result<(), PitchError> {
    for rate in [SilkRate::Khz8, SilkRate::Khz12, SilkRate::Khz16] {
        for duration in [FrameDuration::Ms10, FrameDuration::Ms20] {
            for coding in [
                PitchCoding::Independent,
                PitchCoding::IndependentWithoutScaling,
                PitchCoding::Conditional,
            ] {
                let mut state = PitchState::default();
                state.decode(
                    &mut RangeDecoder::new(&[255; 64]),
                    rate,
                    duration,
                    SignalType::Voiced,
                    PitchCoding::Independent,
                )?;
                for value in 0..=u16::MAX {
                    let bytes = value.to_le_bytes();
                    let mut decoder = RangeDecoder::new(&bytes);
                    let saved = decoder.clone();
                    let before = state.clone();
                    match state.decode(&mut decoder, rate, duration, SignalType::Voiced, coding) {
                        Ok(frame) => {
                            assert!(decoder.tell() <= 16);
                            assert!(frame.lags().iter().all(|&lag| (16..=288).contains(&lag)));
                        }
                        Err(error) => {
                            assert_eq!(error, PitchError::BitBudgetExceeded);
                            assert_eq!(state, before);
                            assert_eq!(decoder, saved);
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

#[test]
fn unvoiced_frames_consume_only_seed_and_clear_history() -> Result<(), Box<dyn std::error::Error>> {
    for value in 0..=255 {
        for signal in [SignalType::Inactive, SignalType::Unvoiced] {
            let mut state = PitchState::default();
            state.decode(
                &mut RangeDecoder::new(&[255; 64]),
                SilkRate::Khz16,
                FrameDuration::Ms20,
                SignalType::Voiced,
                PitchCoding::Independent,
            )?;
            let input = [value];
            let mut decoder = RangeDecoder::new(&input);
            let mut expected = decoder.clone();
            let seed = expected.decode_icdf(&[192, 128, 64, 0], 8)?;
            let frame = state.decode(
                &mut decoder,
                SilkRate::Khz16,
                FrameDuration::Ms20,
                signal,
                PitchCoding::Conditional,
            )?;
            assert_eq!(frame.seed() as usize, seed);
            assert_eq!(decoder, expected);
            assert_eq!(frame.lags(), &[0; 4]);
            assert_eq!(frame.coefficients_q14(), &[[0; 5]; 4]);
            assert_eq!(frame.scale_q14(), 0);
            assert_eq!(state.previous_lag_index(), None);
        }
    }
    Ok(())
}

#[test]
fn malformed_delta_chains_wrap_raw_history_and_clamp_sample_lags() -> Result<(), PitchError> {
    let mut state = PitchState::default();
    let mut saw_negative = false;
    for _ in 0..3500 {
        let frame = state.decode(
            &mut RangeDecoder::new(&[255; 64]),
            SilkRate::Khz16,
            FrameDuration::Ms20,
            SignalType::Voiced,
            PitchCoding::Conditional,
        )?;
        saw_negative |= frame.lag_index().is_some_and(|lag| lag < 0);
        assert!(frame.lags().iter().all(|lag| (32..=288).contains(lag)));
    }
    assert!(saw_negative);
    Ok(())
}
