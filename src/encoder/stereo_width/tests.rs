use super::*;
type TestResult = Result<(), Box<dyn std::error::Error>>;

fn rate(hz: i32) -> InputRate {
    match hz {
        8000 => InputRate::Khz8,
        12000 => InputRate::Khz12,
        16000 => InputRate::Khz16,
        24000 => InputRate::Khz24,
        48000 => InputRate::Khz48,
        _ => panic!("invalid fixture rate"),
    }
}
fn input(n: usize, frame: usize, pattern: usize) -> Vec<i16> {
    let mut pcm = Vec::with_capacity(2 * n);
    for i in 0..n {
        let phase = ((frame * n + i) % 80) as i32;
        let base = (phase.min(80 - phase) - 20) * 1200;
        let pair = match pattern {
            0 | 11 => (0, 0),
            1 => (base, base),
            2 => (base, -base),
            3 => (base, base / 4),
            4 => (base, 0),
            5 => (0, base),
            6 => (32767, -32768),
            7 => (32767, 32767),
            8 => (
                ((i * 719 + frame * 113) % 65536) as i32 - 32768,
                ((i * 317 + frame * 991) % 65536) as i32 - 32768,
            ),
            9 => (1, 0),
            10 => (base, (((phase + 17) % 80) - 40) * 400),
            _ => panic!("invalid fixture pattern"),
        };
        pcm.extend([pair.0 as i16, pair.1 as i16]);
    }
    pcm
}

#[test]
fn unchanged_native_width_histories() -> TestResult {
    let mut state = StereoWidthState::default();
    let mut count = 0;
    for line in include_str!("../../../tests/opus-stereo-width-fixtures.txt")
        .lines()
        .filter(|l| !l.starts_with('#'))
    {
        let row: Vec<i32> = line
            .split_whitespace()
            .map(str::parse)
            .collect::<Result<_, _>>()?;
        assert_eq!(row.len(), 10);
        let rate = rate(row[0]);
        let duration = row[1] as usize;
        let frame = row[2] as usize;
        if frame == 0 {
            state.reset();
        }
        let pcm = input(duration / rate.factor(), frame, row[3] as usize);
        assert_eq!(
            i32::from(state.analyze(rate, duration, &pcm)?),
            row[4],
            "row{count}"
        );
        assert_eq!(state.snapshot(), &row[5..], "row{count}");
        count += 1;
    }
    assert_eq!(count, 4320);
    Ok(())
}

#[test]
fn invalid_geometry_preserves_history_and_rare_remainder_is_ignored() -> TestResult {
    let mut state = StereoWidthState::default();
    state.analyze(InputRate::Khz48, 960, &input(960, 40, 4))?;
    let before = state.clone();
    for (duration, len) in [
        (0, 0),
        (121, 242),
        (usize::MAX, 0),
        (960, 1919),
        (960, 1921),
    ] {
        assert_eq!(
            state.analyze(InputRate::Khz48, duration, &vec![0; len]),
            Err(FrontendError::InvalidGeometry)
        );
        assert_eq!(state, before);
    }
    let mut a = StereoWidthState::default();
    let mut b = a.clone();
    let mut pcm = input(30, 42, 8);
    a.analyze(InputRate::Khz12, 120, &pcm)?;
    pcm[56..].fill(i16::MIN);
    b.analyze(InputRate::Khz12, 120, &pcm)?;
    assert_eq!(a, b);
    let mut silence = StereoWidthState::default();
    assert_eq!(silence.analyze(InputRate::Khz48, 960, &[0; 1920])?, 0);
    assert_eq!(silence, StereoWidthState::default());
    Ok(())
}
