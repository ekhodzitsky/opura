//! Independent pulse/excitation oracle and untrusted-input checks.
use opura::range::RangeDecoder;
use opura::silk::excitation::{ExcitationError, decode_excitation};
use opura::silk::gains::{FrameDuration, SignalType};
use opura::silk::pitch::SilkRate;

fn fingerprint(values: impl Iterator<Item = i32>) -> u64 {
    let mut hash = 14695981039346656037_u64;
    for value in values {
        for byte in value.to_le_bytes() {
            hash = (hash ^ u64::from(byte)).wrapping_mul(1099511628211);
        }
    }
    hash
}

#[test]
fn upstream_pulses_excitation_and_entropy_match() -> Result<(), Box<dyn std::error::Error>> {
    let mut input = Vec::new();
    let mut cases = 0;
    let mut extreme = false;
    for (line_number, line) in include_str!("silk-excitation-fixtures.txt")
        .lines()
        .enumerate()
    {
        if line.starts_with('#') {
            continue;
        }
        let columns: Vec<_> = line.split_whitespace().collect();
        if columns[0] == "input" {
            input.clear();
            for i in (0..columns[1].len()).step_by(2) {
                input.push(u8::from_str_radix(&columns[1][i..i + 2], 16)?);
            }
            continue;
        }
        let numbers: Vec<u64> = columns
            .iter()
            .map(|s| s.parse())
            .collect::<Result<_, _>>()?;
        let length = numbers[0] as usize;
        let signal = match numbers[1] {
            0 => SignalType::Inactive,
            1 => SignalType::Unvoiced,
            _ => SignalType::Voiced,
        };
        let configs = match length {
            80 => vec![(SilkRate::Khz8, FrameDuration::Ms10)],
            120 => vec![(SilkRate::Khz12, FrameDuration::Ms10)],
            160 => vec![
                (SilkRate::Khz8, FrameDuration::Ms20),
                (SilkRate::Khz16, FrameDuration::Ms10),
            ],
            240 => vec![(SilkRate::Khz12, FrameDuration::Ms20)],
            _ => vec![(SilkRate::Khz16, FrameDuration::Ms20)],
        };
        for (rate, duration) in configs {
            let mut decoder = RangeDecoder::new(&input);
            let frame = decode_excitation(
                &mut decoder,
                rate,
                duration,
                signal,
                numbers[2] as u8,
                numbers[3] as u8,
            )?;
            assert_eq!(frame.pulses().len(), length);
            assert_eq!(frame.samples_q14().len(), length);
            let actual = [
                fingerprint(frame.pulses().iter().copied().map(i32::from)),
                fingerprint(frame.samples_q14().iter().copied()),
                u64::from(decoder.range()),
                u64::from(decoder.tell()),
                u64::from(decoder.tell_frac()),
            ];
            assert_eq!(actual, numbers[4..], "fixture line {}", line_number + 1);
            extreme |= frame.pulses().iter().any(|p| p.unsigned_abs() >= 16384);
            cases += 1;
        }
    }
    assert_eq!(cases, 1584);
    assert!(extreme, "ten-bit LSB escape path must be exercised");
    Ok(())
}

#[test]
fn invalid_context_and_exhaustion_leave_entropy_unchanged() {
    let mut decoder = RangeDecoder::new(&[0; 64]);
    let saved = decoder.clone();
    assert_eq!(
        decode_excitation(
            &mut decoder,
            SilkRate::Khz8,
            FrameDuration::Ms10,
            SignalType::Voiced,
            2,
            0
        ),
        Err(ExcitationError::InvalidOffset)
    );
    assert_eq!(decoder, saved);
    assert_eq!(
        decode_excitation(
            &mut decoder,
            SilkRate::Khz8,
            FrameDuration::Ms10,
            SignalType::Voiced,
            0,
            4
        ),
        Err(ExcitationError::InvalidSeed)
    );
    assert_eq!(decoder, saved);
    for input in [&[][..], &[255][..], &[255, 255][..]] {
        let mut decoder = RangeDecoder::new(input);
        let saved = decoder.clone();
        assert_eq!(
            decode_excitation(
                &mut decoder,
                SilkRate::Khz16,
                FrameDuration::Ms20,
                SignalType::Voiced,
                1,
                3
            ),
            Err(ExcitationError::BitBudgetExceeded)
        );
        assert_eq!(decoder, saved);
    }
}

#[test]
fn short_and_seeded_inputs_are_bounded() {
    let mut random = 0x71a10c39_u32;
    for rate in [SilkRate::Khz8, SilkRate::Khz12, SilkRate::Khz16] {
        for duration in [FrameDuration::Ms10, FrameDuration::Ms20] {
            for signal in [
                SignalType::Inactive,
                SignalType::Unvoiced,
                SignalType::Voiced,
            ] {
                for offset in 0..=1 {
                    for case in 0..1280 {
                        let mut bytes = [0_u8; 64];
                        let len = if case < 256 {
                            bytes[0] = case as u8;
                            1
                        } else {
                            for byte in &mut bytes {
                                random ^= random << 13;
                                random ^= random >> 17;
                                random ^= random << 5;
                                *byte = random as u8;
                            }
                            (random as usize % 64) + 1
                        };
                        let mut decoder = RangeDecoder::new(&bytes[..len]);
                        let saved = decoder.clone();
                        match decode_excitation(
                            &mut decoder,
                            rate,
                            duration,
                            signal,
                            offset,
                            (case % 4) as u8,
                        ) {
                            Ok(frame) => {
                                assert!(decoder.tell() as usize <= len * 8);
                                assert!(frame.pulses().iter().all(|p| p.unsigned_abs() <= 17407));
                                assert!(
                                    frame
                                        .samples_q14()
                                        .iter()
                                        .all(|p| p.unsigned_abs() < 300_000_000)
                                );
                            }
                            Err(error) => {
                                assert_eq!(error, ExcitationError::BitBudgetExceeded);
                                assert_eq!(decoder, saved);
                            }
                        }
                    }
                }
            }
        }
    }
}
