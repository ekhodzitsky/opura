//! Unchanged original run_prefilter, full filtered samples and both histories.
use opura::celt::{
    FrameSize,
    postfilter::Tapset,
    prefilter::{PrefilterOptions, PrefilterState},
};
fn numbers(line: &str, label: &str) -> Result<Vec<i32>, Box<dyn std::error::Error>> {
    let mut f = line.split_whitespace();
    assert_eq!(f.next(), Some(label));
    Ok(f.map(|s| s.parse()).collect::<Result<_, _>>()?)
}
#[test]
fn original_prefilter_samples_parameters_and_histories_match()
-> Result<(), Box<dyn std::error::Error>> {
    let sizes = [
        FrameSize::Ms2_5,
        FrameSize::Ms5,
        FrameSize::Ms10,
        FrameSize::Ms20,
    ];
    let taps = [Tapset::Set0, Tapset::Set1, Tapset::Set2];
    let mut lines = include_str!("celt-prefilter-fixtures.txt")
        .lines()
        .filter(|l| !l.starts_with('#'));
    let mut state = PrefilterState::new(1)?;
    let mut frames = 0;
    let mut active = 0;
    while let Some(line) = lines.next() {
        let f = numbers(line, "frame")?;
        assert_eq!(f.len(), 11);
        let size = sizes[usize::try_from(f[1])?];
        let channels = usize::try_from(f[2])?;
        if state.channels() != channels {
            state = PrefilterState::new(channels)?;
        } else if f[0] != 0 {
            state.reset();
        }
        let options = PrefilterOptions {
            enabled: f[3] != 0,
            complexity: u8::try_from(f[4])?,
            available_bytes: u16::try_from(f[5])?,
            tf_estimate: i16::try_from(f[6])?,
            tone_frequency: i16::try_from(f[7])?,
            toneishness: f[8],
            loss_rate: u8::try_from(f[9])?,
            tapset: taps[usize::try_from(f[10])?],
        };
        let input = numbers(lines.next().ok_or("input")?, "input")?;
        let expected = numbers(lines.next().ok_or("result")?, "result")?;
        let filtered = numbers(lines.next().ok_or("filtered")?, "filtered")?;
        let overlap = numbers(lines.next().ok_or("overlap")?, "overlap")?;
        let raw = numbers(lines.next().ok_or("raw")?, "raw")?;
        let original = input.clone();
        let options_before = options;
        let result = state
            .process(size, &input, options)
            .map_err(|e| format!("frame{frames} {line}: {e}"))?;
        assert_eq!(result.size(), size);
        assert_eq!(result.channels(), channels);
        let p = result.parameters();
        assert_eq!(
            [
                i32::from(result.active()),
                i32::from(p.period()),
                i32::from(p.gain()),
                i32::from(result.quantized_gain()),
                p.tapset() as i32
            ],
            expected.as_slice(),
            "parameters frame{frames} {line}"
        );
        assert_eq!(result.values(), filtered, "filtered frame{frames} {line}");
        assert_eq!(state.parameters(), p);
        for ch in 0..channels {
            assert_eq!(
                state.overlap(ch).ok_or("overlap")?.as_slice(),
                &overlap[ch * 120..(ch + 1) * 120],
                "overlap frame{frames} {line}"
            );
            assert_eq!(
                state.raw_history(ch).ok_or("raw")?.as_slice(),
                &raw[ch * 1024..(ch + 1) * 1024],
                "raw frame{frames} {line}"
            );
        }
        assert_eq!(state.overlap(channels), None);
        assert_eq!(state.raw_history(channels), None);
        assert_eq!(input, original);
        assert_eq!(options, options_before);
        frames += 1;
        active += usize::from(result.active());
    }
    eprintln!("prefilter frames{frames} active{active}");
    assert_eq!(frames, 2048);
    assert!(active > 100);
    Ok(())
}

#[test]
fn invalid_geometry_signal_options_and_reset_preserve_contracts()
-> Result<(), Box<dyn std::error::Error>> {
    use opura::celt::{math, postfilter::PostfilterParams, prefilter::PrefilterError};
    for c in [0, 3, usize::MAX] {
        assert_eq!(PrefilterState::new(c), Err(PrefilterError::InvalidGeometry));
    }
    let defaults = PrefilterOptions::default();
    for channels in [1, 2] {
        let mut state = PrefilterState::new(channels)?;
        assert_eq!(state.channels(), channels);
        assert_eq!(state.parameters(), PostfilterParams::default());
        assert_eq!(state.raw_history(usize::MAX), None);
        assert_eq!(state.overlap(usize::MAX), None);
        let input: Vec<_> = (0..channels * 960)
            .map(|i| i32::from(math::cos_norm((i % 960) as i32 * 131072 / 60)) * 2048)
            .collect();
        let tone = PrefilterOptions {
            tone_frequency: 858,
            toneishness: 1 << 29,
            ..defaults
        };
        state.process(FrameSize::Ms20, &input, tone)?;
        let before = state.clone();
        assert!(before.parameters().gain() > 0);
        for size in [
            FrameSize::Ms2_5,
            FrameSize::Ms5,
            FrameSize::Ms10,
            FrameSize::Ms20,
        ] {
            let n = size.samples() * channels;
            for length in [0, 1, n - 1, n + 1, 2161] {
                assert_eq!(
                    state.process(size, &vec![0; length], defaults),
                    Err(PrefilterError::InvalidGeometry)
                );
                assert_eq!(state, before);
            }
            for value in [i32::MIN, -536870912, 536870912, i32::MAX] {
                for at in [0, n - 1] {
                    let mut bad = vec![0; n];
                    bad[at] = value;
                    let original = bad.clone();
                    assert_eq!(
                        state.process(size, &bad, defaults),
                        Err(PrefilterError::InvalidSignal)
                    );
                    assert_eq!(state, before);
                    assert_eq!(bad, original);
                }
            }
            let invalid = [
                PrefilterOptions {
                    complexity: 11,
                    ..defaults
                },
                PrefilterOptions {
                    complexity: u8::MAX,
                    ..defaults
                },
                PrefilterOptions {
                    available_bytes: 1276,
                    ..defaults
                },
                PrefilterOptions {
                    tf_estimate: -1,
                    ..defaults
                },
                PrefilterOptions {
                    tf_estimate: 16385,
                    ..defaults
                },
                PrefilterOptions {
                    tone_frequency: -2,
                    ..defaults
                },
                PrefilterOptions {
                    tone_frequency: 25737,
                    ..defaults
                },
                PrefilterOptions {
                    toneishness: -1,
                    ..defaults
                },
                PrefilterOptions {
                    toneishness: (1 << 29) + 1,
                    ..defaults
                },
                PrefilterOptions {
                    loss_rate: 101,
                    ..defaults
                },
            ];
            for options in invalid {
                assert_eq!(
                    state.process(size, &vec![0; n], options),
                    Err(PrefilterError::InvalidOptions)
                );
                assert_eq!(state, before);
                assert_eq!(
                    state.process(
                        size,
                        &vec![0; n],
                        PrefilterOptions {
                            enabled: false,
                            ..options
                        }
                    ),
                    Err(PrefilterError::InvalidOptions)
                );
                assert_eq!(state, before);
            }
        }
        state.reset();
        let fresh = PrefilterState::new(channels)?;
        assert_eq!(state, fresh);
        for channel in 0..channels {
            assert_eq!(state.raw_history(channel), Some(&[0; 1024]));
            assert_eq!(state.overlap(channel), Some(&[0; 120]));
        }
        let mut reference = fresh;
        assert_eq!(
            state.process(FrameSize::Ms20, &input, tone)?,
            reference.process(FrameSize::Ms20, &input, tone)?
        );
        assert_eq!(state, reference);
    }
    Ok(())
}

#[test]
fn bounded_prefilter_sequences_are_immutable_and_atomic() -> Result<(), Box<dyn std::error::Error>>
{
    use opura::celt::{pitch::PitchError, prefilter::PrefilterError};
    let sizes = [
        FrameSize::Ms2_5,
        FrameSize::Ms5,
        FrameSize::Ms10,
        FrameSize::Ms20,
    ];
    let taps = [Tapset::Set0, Tapset::Set1, Tapset::Set2];
    let mut random = 0x50524546494c5445u64;
    for channels in [1, 2] {
        let mut state = PrefilterState::new(channels)?;
        for frame in 0..512 {
            let size = sizes[frame % 4];
            let mut next = || {
                random ^= random << 13;
                random ^= random >> 7;
                random ^= random << 17;
                random
            };
            let input: Vec<_> = (0..size.samples() * channels)
                .map(|_| ((next() >> 32) as i32) >> 3)
                .collect();
            let options = PrefilterOptions {
                enabled: next() % 2 != 0,
                complexity: (next() % 11) as u8,
                available_bytes: (next() % 1276) as u16,
                tf_estimate: (next() % 16385) as i16,
                tone_frequency: (next() % 25738) as i16 - 1,
                toneishness: (next() % ((1 << 29) + 1)) as i32,
                loss_rate: (next() % 101) as u8,
                tapset: taps[(next() % 3) as usize],
            };
            let before = state.clone();
            let original = input.clone();
            match state.process(size, &input, options) {
                Ok(result) => {
                    assert_eq!(result.values().len(), channels * (size.samples() + 120));
                    assert!((15..=1022).contains(&result.parameters().period()));
                    assert!(result.parameters().gain() <= 24576);
                    assert_eq!(result.active(), result.parameters().gain() != 0);
                    assert!(result.quantized_gain() <= 7);
                    assert!(
                        result
                            .values()
                            .iter()
                            .all(|v| (-536870911..=536870911).contains(v))
                    );
                }
                Err(PrefilterError::Pitch(PitchError::ArithmeticOverflow)) => {
                    assert_eq!(state, before)
                }
                Err(e) => return Err(format!("frame{frame} C{channels}: {e}").into()),
            }
            assert_eq!(input, original);
        }
    }
    Ok(())
}
