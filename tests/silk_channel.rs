//! Reference PCM/entropy sequences and complete channel transaction checks.
use opura::{
    range::RangeDecoder,
    silk::{
        channel::{ChannelError, ChannelState},
        gains::{FrameActivity, FrameDuration, GainError},
        pitch::{PitchCoding, SilkRate},
    },
};

fn bytes(hex: &str) -> Result<Vec<u8>, std::num::ParseIntError> {
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16))
        .collect()
}
fn signature(decoder: &RangeDecoder<'_>) -> (u32, u32, u32) {
    (decoder.range(), decoder.tell(), decoder.tell_frac())
}

#[test]
fn consecutive_compressed_frames_match_upstream_pcm_and_entropy()
-> Result<(), Box<dyn std::error::Error>> {
    let mut state = ChannelState::default();
    let mut lines = include_str!("silk-channel-fixtures.txt")
        .lines()
        .filter(|l| !l.starts_with('#'));
    let mut frames = 0;
    let mut signals = [false; 3];
    let mut interpolated = false;
    while let Some(line) = lines.next() {
        let header: Vec<_> = line.split_whitespace().collect();
        assert_eq!(header[0], "stream");
        if header[1] == "1" {
            state.reset();
        }
        let rate = match header[2] {
            "8" => SilkRate::Khz8,
            "12" => SilkRate::Khz12,
            "16" => SilkRate::Khz16,
            _ => panic!("bad rate"),
        };
        let data = bytes(header[3])?;
        let mut entropy = RangeDecoder::new(&data);
        for _ in 0..3 {
            let row: Vec<i64> = lines
                .next()
                .ok_or("missing frame")?
                .split_whitespace()
                .map(str::parse)
                .collect::<Result<_, _>>()?;
            let duration = if row[0] == 2 {
                FrameDuration::Ms10
            } else {
                FrameDuration::Ms20
            };
            let activity = match row[1] {
                0 => FrameActivity::Inactive,
                1 => FrameActivity::Active,
                _ => FrameActivity::Redundant,
            };
            let coding = match row[2] {
                0 => PitchCoding::Independent,
                1 => PitchCoding::IndependentWithoutScaling,
                _ => PitchCoding::Conditional,
            };
            let pcm = state.decode(&mut entropy, rate, duration, activity, coding)?;
            assert_eq!(pcm.rate(), rate);
            assert_eq!(
                signature(&entropy),
                (row[3] as u32, row[4] as u32, row[5] as u32),
                "frame {frames}"
            );
            let expected: Vec<_> = row[8..].iter().map(|&v| v as i16).collect();
            assert_eq!(pcm.samples(), expected, "frame {frames}");
            signals[row[6] as usize] = true;
            interpolated |= row[7] < 4;
            frames += 1;
        }
    }
    assert_eq!(frames, 144);
    assert_eq!(signals, [true; 3]);
    assert!(interpolated);
    Ok(())
}

#[test]
fn failures_at_each_entropy_stage_roll_back_all_history_and_allow_retry()
-> Result<(), Box<dyn std::error::Error>> {
    let input: Vec<_> = (0..1024).map(|i| (i * 97 + 23) as u8).collect();
    let mut state = ChannelState::default();
    state.decode(
        &mut RangeDecoder::new(&input),
        SilkRate::Khz8,
        FrameDuration::Ms20,
        FrameActivity::Active,
        PitchCoding::Independent,
    )?;
    let before = state.clone();
    let mut stages = [false; 4];
    for rate in [SilkRate::Khz8, SilkRate::Khz12, SilkRate::Khz16] {
        for length in 0..=input.len() {
            let mut entropy = RangeDecoder::new(&input[..length]);
            let saved = entropy.clone();
            let result = state.decode(
                &mut entropy,
                rate,
                FrameDuration::Ms20,
                FrameActivity::Active,
                PitchCoding::Independent,
            );
            if let Err(error) = result {
                stages[match error {
                    ChannelError::Gains(_) => 0,
                    ChannelError::Nlsf(_) => 1,
                    ChannelError::Pitch(_) => 2,
                    ChannelError::Excitation(_) => 3,
                    _ => panic!("unexpected synthesis error: {error}"),
                }] = true;
                assert_eq!(state, before, "rate={rate:?} length={length}");
                assert_eq!(signature(&entropy), signature(&saved));
                // Probe continued decoding too: equal counters alone do not
                // establish that buffered arithmetic state was restored.
                let mut expected = before.clone();
                let mut a = RangeDecoder::new(&input);
                let mut b = a.clone();
                assert_eq!(
                    state.decode(
                        &mut a,
                        rate,
                        FrameDuration::Ms20,
                        FrameActivity::Active,
                        PitchCoding::Independent
                    )?,
                    expected.decode(
                        &mut b,
                        rate,
                        FrameDuration::Ms20,
                        FrameActivity::Active,
                        PitchCoding::Independent
                    )?
                );
                assert_eq!(state, expected);
                let mut resumed = entropy;
                let mut original = saved;
                for _ in 0..8 {
                    assert_eq!(resumed.decode_bit_logp(1)?, original.decode_bit_logp(1)?);
                }
                assert_eq!(signature(&resumed), signature(&original));
            }
            state = before.clone();
        }
    }
    assert_eq!(stages, [true; 4]);
    Ok(())
}

#[test]
fn reset_and_rate_change_reject_conditional_start_without_mutation()
-> Result<(), Box<dyn std::error::Error>> {
    let data = [0u8; 1024];
    let mut state = ChannelState::default();
    for rate in [SilkRate::Khz8, SilkRate::Khz12, SilkRate::Khz16] {
        let before = state.clone();
        let mut entropy = RangeDecoder::new(&data);
        let signature_before = signature(&entropy);
        assert_eq!(
            state.decode(
                &mut entropy,
                rate,
                FrameDuration::Ms10,
                FrameActivity::Active,
                PitchCoding::Conditional
            ),
            Err(ChannelError::Gains(GainError::MissingHistory))
        );
        assert_eq!(state, before);
        assert_eq!(signature(&entropy), signature_before);
        state.decode(
            &mut entropy,
            rate,
            FrameDuration::Ms10,
            FrameActivity::Active,
            PitchCoding::Independent,
        )?;
    }
    state.reset();
    assert_eq!(state, ChannelState::default());
    let mut clean = ChannelState::default();
    assert_eq!(
        state.decode(
            &mut RangeDecoder::new(&data),
            SilkRate::Khz8,
            FrameDuration::Ms10,
            FrameActivity::Inactive,
            PitchCoding::Independent
        )?,
        clean.decode(
            &mut RangeDecoder::new(&data),
            SilkRate::Khz8,
            FrameDuration::Ms10,
            FrameActivity::Inactive,
            PitchCoding::Independent
        )?
    );
    assert_eq!(state, clean);
    Ok(())
}
