//! SILK gain-state and entropy failure contracts.
use opura::range::RangeDecoder;
use opura::silk::gains::{
    FrameActivity, FrameDuration, GainCoding, GainError, GainState, SignalType,
};

#[test]
fn history_and_entropy_commit_together() -> Result<(), GainError> {
    let mut gains = GainState::default();
    let mut decoder = RangeDecoder::new(&[0; 32]);
    let saved = decoder.clone();
    assert_eq!(
        gains.decode(
            &mut decoder,
            FrameDuration::Ms20,
            FrameActivity::Inactive,
            GainCoding::Conditional
        ),
        Err(GainError::MissingHistory)
    );
    assert_eq!(decoder, saved);
    assert_eq!(gains.previous_index(), None);
    let frame = gains.decode(
        &mut decoder,
        FrameDuration::Ms10,
        FrameActivity::Inactive,
        GainCoding::Independent,
    )?;
    assert_eq!(frame.signal_type(), SignalType::Inactive);
    assert_eq!(frame.offset_type(), 0);
    assert_eq!(frame.indices(), &[0, 0]);
    assert_eq!(frame.gains_q16(), &[81920, 81920]);
    assert_eq!(gains.previous_index(), Some(0));
    let before = gains.clone();
    let mut empty = RangeDecoder::new(&[]);
    let entropy_before = empty.clone();
    assert_eq!(
        gains.decode(
            &mut empty,
            FrameDuration::Ms20,
            FrameActivity::Active,
            GainCoding::Conditional
        ),
        Err(GainError::BitBudgetExceeded)
    );
    assert_eq!(empty, entropy_before);
    assert_eq!(gains, before);
    gains.reset();
    assert_eq!(gains, GainState::default());
    Ok(())
}

#[test]
fn redundancy_uses_active_context() -> Result<(), GainError> {
    for first in 0..=255 {
        let bytes = [first, 0x91, 0x28, 0x72, 0, 0, 0, 0];
        let mut a = RangeDecoder::new(&bytes);
        let mut b = a.clone();
        let mut ga = GainState::default();
        let mut gb = GainState::default();
        let left = ga.decode(
            &mut a,
            FrameDuration::Ms20,
            FrameActivity::Active,
            GainCoding::Independent,
        )?;
        let right = gb.decode(
            &mut b,
            FrameDuration::Ms20,
            FrameActivity::Redundant,
            GainCoding::Independent,
        )?;
        assert_eq!(left, right);
        assert_eq!(a, b);
        assert_eq!(ga, gb);
        assert_ne!(left.signal_type(), SignalType::Inactive);
    }
    Ok(())
}

#[test]
fn upstream_sequences_match_indices_linear_gains_and_entropy()
-> Result<(), Box<dyn std::error::Error>> {
    let mut input = Vec::new();
    let mut decoder = RangeDecoder::new(&[]);
    let mut gains = GainState::default();
    let mut duration = FrameDuration::Ms10;
    let mut activity = FrameActivity::Inactive;
    let mut count = 2;
    let mut cases = 0;
    let mut seen = [false; 3];
    for (line_number, line) in include_str!("silk-gain-fixtures.txt").lines().enumerate() {
        if line.starts_with('#') {
            continue;
        }
        let columns: Vec<_> = line.split_whitespace().collect();
        if columns[0] == "frame" {
            input.clear();
            for i in (0..columns[1].len()).step_by(2) {
                input.push(u8::from_str_radix(&columns[1][i..i + 2], 16)?);
            }
            decoder = RangeDecoder::new(&input);
            gains.reset();
            count = columns[2].parse()?;
            duration = if count == 2 {
                FrameDuration::Ms10
            } else {
                FrameDuration::Ms20
            };
            activity = match columns[3] {
                "0" => FrameActivity::Inactive,
                "1" => FrameActivity::Active,
                _ => FrameActivity::Redundant,
            };
            continue;
        }
        let expected: Vec<u32> = columns
            .iter()
            .map(|s| s.parse())
            .collect::<Result<_, _>>()?;
        let coding = if expected[0] == 0 {
            GainCoding::Independent
        } else {
            GainCoding::Conditional
        };
        let frame = gains.decode(&mut decoder, duration, activity, coding)?;
        let signal = match frame.signal_type() {
            SignalType::Inactive => 0,
            SignalType::Unvoiced => 1,
            SignalType::Voiced => 2,
        };
        seen[signal as usize] = true;
        assert_eq!(
            (signal, u32::from(frame.offset_type())),
            (expected[1], expected[2]),
            "line {}",
            line_number + 1
        );
        assert_eq!(
            frame
                .indices()
                .iter()
                .map(|&v| u32::from(v))
                .collect::<Vec<_>>(),
            expected[3..3 + count]
        );
        assert_eq!(
            frame
                .gains_q16()
                .iter()
                .map(|&v| v as u32)
                .collect::<Vec<_>>(),
            expected[7..7 + count]
        );
        assert_eq!(gains.previous_index().map(u32::from), Some(expected[11]));
        assert_eq!(
            [decoder.range(), decoder.tell(), decoder.tell_frac()],
            expected[12..15],
            "line {}",
            line_number + 1
        );
        cases += 1;
    }
    assert_eq!(cases, 264);
    assert!(seen.iter().all(|&v| v));
    Ok(())
}

#[test]
fn short_inputs_preserve_both_states_on_failure() -> Result<(), GainError> {
    let mut warm = GainState::default();
    warm.decode(
        &mut RangeDecoder::new(&[0xff; 64]),
        FrameDuration::Ms20,
        FrameActivity::Active,
        GainCoding::Independent,
    )?;
    let mut accepted = 0;
    let mut rejected = 0;
    for word in 0..=u16::MAX {
        let bytes = word.to_be_bytes();
        for duration in [FrameDuration::Ms10, FrameDuration::Ms20] {
            for activity in [
                FrameActivity::Inactive,
                FrameActivity::Active,
                FrameActivity::Redundant,
            ] {
                for coding in [GainCoding::Independent, GainCoding::Conditional] {
                    let mut state = warm.clone();
                    let mut decoder = RangeDecoder::new(&bytes);
                    let before = decoder.clone();
                    match state.decode(&mut decoder, duration, activity, coding) {
                        Ok(gains) => {
                            accepted += 1;
                            assert!(decoder.tell() <= 16);
                            let count = if duration == FrameDuration::Ms10 {
                                2
                            } else {
                                4
                            };
                            assert_eq!(gains.indices().len(), count);
                            assert_eq!(gains.gains_q16().len(), count);
                            assert!(
                                gains
                                    .gains_q16()
                                    .iter()
                                    .all(|v| (81920..=1686110208).contains(v))
                            );
                            assert!(state.previous_index().is_some_and(|v| v <= 63));
                        }
                        Err(error) => {
                            rejected += 1;
                            assert_eq!(error, GainError::BitBudgetExceeded);
                            assert_eq!(state, warm);
                            assert_eq!(decoder, before);
                        }
                    }
                }
            }
        }
    }
    assert!(accepted > 0 && rejected > 0);
    Ok(())
}

#[test]
fn mono_header_cursor_feeds_gain_prefix() -> Result<(), Box<dyn std::error::Error>> {
    let packet = opura::Packet::parse(&[0x08, 0, 0, 0, 0, 0, 0, 0, 0])?;
    let (header, mut cursor) = opura::silk::decode_header(&packet, 0)?;
    assert_eq!(header.redundancy(0), Some(&[false][..]));
    let mut state = GainState::default();
    let gains = state.decode(
        &mut cursor,
        FrameDuration::Ms20,
        FrameActivity::Inactive,
        GainCoding::Independent,
    )?;
    assert_eq!(gains.indices(), &[0, 0, 0, 0]);
    assert_eq!(gains.gains_q16(), &[81920; 4]);
    assert!(cursor.tell() > 3);
    Ok(())
}
