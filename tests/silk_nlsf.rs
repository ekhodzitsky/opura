//! Differential tests against the pinned upstream SILK NLSF decoder.
use opura::range::RangeDecoder;
use opura::silk::gains::{FrameDuration, SignalType};
use opura::silk::nlsf::{NlsfError, NlsfOrder, NlsfState};

#[test]
fn upstream_sequences_match_spectrum_interpolation_and_entropy()
-> Result<(), Box<dyn std::error::Error>> {
    let mut input = Vec::new();
    let mut decoder = RangeDecoder::new(&[]);
    let mut state = NlsfState::default();
    let mut order = NlsfOrder::Order10;
    let mut count = 10;
    let mut duration = FrameDuration::Ms10;
    let mut signal = SignalType::Inactive;
    let mut cases = 0;
    let mut rejected = 0;
    let mut factors = [false; 5];
    let mut codebooks = [[false; 32]; 2];
    let mut residuals = [false; 21];
    for (line_number, line) in include_str!("silk-nlsf-fixtures.txt").lines().enumerate() {
        if line.starts_with('#') {
            continue;
        }
        let columns: Vec<_> = line.split_whitespace().collect();
        if columns[0] == "frame" {
            input.clear();
            for i in (0..columns[1].len()).step_by(2) {
                input.push(u8::from_str_radix(&columns[1][i..i + 2], 16)?);
            }
            count = columns[2].parse()?;
            order = if count == 10 {
                NlsfOrder::Order10
            } else {
                NlsfOrder::Order16
            };
            duration = if columns[3] == "2" {
                FrameDuration::Ms10
            } else {
                FrameDuration::Ms20
            };
            signal = match columns[4] {
                "0" => SignalType::Inactive,
                "1" => SignalType::Unvoiced,
                _ => SignalType::Voiced,
            };
            decoder = RangeDecoder::new(&input);
            state.reset();
            continue;
        }
        let expected: Vec<i64> = columns
            .iter()
            .map(|s| s.parse())
            .collect::<Result<_, _>>()?;
        let before = state.clone();
        let saved = decoder.clone();
        let result = state.decode(&mut decoder, order, duration, signal);
        cases += 1;
        // The reference zero-extends its range reads. These six final steps
        // exceed their input budget and must fail without committing either state.
        if expected[expected.len() - 2] > (input.len() * 8) as i64 {
            assert_eq!(result, Err(NlsfError::BitBudgetExceeded));
            assert_eq!(state, before);
            assert_eq!(decoder, saved);
            rejected += 1;
            continue;
        }
        let actual = result?;
        let mut values = vec![
            i64::from(actual.coded_interpolation_q2()),
            i64::from(actual.interpolation_q2()),
        ];
        values.extend(actual.indices().iter().copied().map(i64::from));
        values.extend(actual.current_q15().iter().copied().map(i64::from));
        values.extend(actual.first_half_q15().iter().copied().map(i64::from));
        values.extend([
            i64::from(decoder.range()),
            i64::from(decoder.tell()),
            i64::from(decoder.tell_frac()),
        ]);
        assert_eq!(values, expected, "fixture line {}", line_number + 1);
        assert_eq!(state.previous_q15(), Some(actual.current_q15()));
        assert_eq!(actual.current_q15().len(), count);
        assert!(actual.current_q15().windows(2).all(|w| w[0] < w[1]));
        factors[actual.coded_interpolation_q2() as usize] = true;
        codebooks[usize::from(count == 16)][actual.indices()[0] as usize] = true;
        for &index in &actual.indices()[1..] {
            residuals[(index + 10) as usize] = true;
        }
    }
    assert_eq!(cases, 3408);
    assert_eq!(rejected, 6);
    assert!(factors.into_iter().all(|v| v));
    assert!(codebooks.into_iter().flatten().all(|v| v));
    assert!(residuals.into_iter().all(|v| v));
    Ok(())
}

#[test]
fn history_reset_order_switch_and_errors_are_transactional() -> Result<(), NlsfError> {
    let mut state = NlsfState::default();
    let mut decoder = RangeDecoder::new(&[0; 128]);
    let first = state.decode(
        &mut decoder,
        NlsfOrder::Order10,
        FrameDuration::Ms20,
        SignalType::Voiced,
    )?;
    assert_eq!(first.coded_interpolation_q2(), 0);
    assert_eq!(first.interpolation_q2(), 4);
    assert_eq!(first.current_q15(), first.first_half_q15());
    let second = state.decode(
        &mut decoder,
        NlsfOrder::Order10,
        FrameDuration::Ms20,
        SignalType::Voiced,
    )?;
    assert_eq!(second.interpolation_q2(), 0);
    assert_eq!(second.first_half_q15(), first.current_q15());
    let before = state.clone();
    let mut empty = RangeDecoder::new(&[]);
    let saved = empty.clone();
    assert_eq!(
        state.decode(
            &mut empty,
            NlsfOrder::Order16,
            FrameDuration::Ms20,
            SignalType::Voiced
        ),
        Err(NlsfError::BitBudgetExceeded)
    );
    assert_eq!(state, before);
    assert_eq!(empty, saved);
    let switched = state.decode(
        &mut decoder,
        NlsfOrder::Order16,
        FrameDuration::Ms20,
        SignalType::Voiced,
    )?;
    assert_eq!(switched.interpolation_q2(), 4);
    assert_eq!(switched.first_half_q15(), switched.current_q15());
    state.reset();
    assert_eq!(state, NlsfState::default());
    assert_eq!(state.previous_q15(), None);
    Ok(())
}

#[test]
fn every_two_byte_input_is_bounded_and_preserves_state_on_failure() {
    for order in [NlsfOrder::Order10, NlsfOrder::Order16] {
        for duration in [FrameDuration::Ms10, FrameDuration::Ms20] {
            for signal in [
                SignalType::Inactive,
                SignalType::Unvoiced,
                SignalType::Voiced,
            ] {
                let mut state = NlsfState::default();
                for value in 0..=u16::MAX {
                    let bytes = value.to_le_bytes();
                    let mut decoder = RangeDecoder::new(&bytes);
                    let saved = decoder.clone();
                    let before = state.clone();
                    match state.decode(&mut decoder, order, duration, signal) {
                        Ok(frame) => {
                            assert!(decoder.tell() <= 16);
                            assert!(frame.current_q15().windows(2).all(|w| w[0] < w[1]));
                        }
                        Err(error) => {
                            assert_eq!(error, NlsfError::BitBudgetExceeded);
                            assert_eq!(state, before);
                            assert_eq!(decoder, saved);
                        }
                    }
                }
            }
        }
    }
}
