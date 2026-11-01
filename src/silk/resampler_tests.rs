use super::*;

fn output_rate(khz: i64) -> OutputRate {
    match khz {
        8 => OutputRate::Khz8,
        12 => OutputRate::Khz12,
        16 => OutputRate::Khz16,
        24 => OutputRate::Khz24,
        _ => OutputRate::Khz48,
    }
}

#[test]
fn reference_pcm_and_all_persistent_filter_delay_states_match()
-> Result<(), Box<dyn std::error::Error>> {
    let mut state = ResamplerState::new(OutputRate::Khz8);
    let mut lines = include_str!("../../tests/silk-resampler-fixtures.txt")
        .lines()
        .filter(|l| !l.starts_with('#'));
    let mut cases = 0;
    let mut pairs = [[false; 5]; 3];
    let mut clipped = 0;
    while let Some(line) = lines.next() {
        let header: Vec<i64> = line
            .split_whitespace()
            .map(str::parse)
            .collect::<Result<_, _>>()?;
        let rate = match header[1] {
            8 => SilkRate::Khz8,
            12 => SilkRate::Khz12,
            _ => SilkRate::Khz16,
        };
        let target = output_rate(header[2]);
        if target != state.output_rate() {
            state = ResamplerState::new(target);
        }
        if header[0] != 0 {
            state.reset();
        }
        let input: Vec<_> = header[4..].iter().map(|&v| v as i16).collect();
        let expected: Vec<i64> = lines
            .next()
            .ok_or("missing reference result")?
            .split_whitespace()
            .map(str::parse)
            .collect::<Result<_, _>>()?;
        let length = header[2] as usize * header[3] as usize;
        let mut output = vec![1234; length + 5];
        assert_eq!(state.process(rate, &input, &mut output)?, length);
        let pcm: Vec<_> = expected[..length].iter().map(|&v| v as i16).collect();
        assert_eq!(
            &output[..length],
            pcm,
            "PCM {cases} {}->{}",
            header[1],
            header[2]
        );
        assert_eq!(&output[length..], &[1234; 5]);
        let serialized = state
            .iir
            .iter()
            .map(|&v| i64::from(v))
            .chain(state.fir16.iter().map(|&v| i64::from(v)))
            .chain(state.fir32.iter().map(|&v| i64::from(v)))
            .chain(state.delay.iter().map(|&v| i64::from(v)))
            .collect::<Vec<_>>();
        assert_eq!(
            serialized,
            expected[length..length + 54],
            "filter/delay history {cases}"
        );
        let (method, order, fractions) = match state.method {
            Method::Copy => (0, 0, 0),
            Method::Up2 => (1, 0, 0),
            Method::UpFir => (2, 0, 0),
            Method::Down {
                order, fractions, ..
            } => (3, order as i64, fractions as i64),
        };
        assert_eq!(
            [
                method,
                i64::from(state.ratio),
                state.input_delay as i64,
                order,
                fractions
            ],
            expected[length + 54..],
            "configuration {cases}"
        );
        pairs[match rate {
            SilkRate::Khz8 => 0,
            SilkRate::Khz12 => 1,
            SilkRate::Khz16 => 2,
        }][target.index()] = true;
        clipped += pcm
            .iter()
            .filter(|&&s| s == i16::MIN || s == i16::MAX)
            .count();
        cases += 1;
    }
    assert_eq!(cases, 360);
    assert_eq!(pairs, [[true; 5]; 3]);
    assert!(clipped > 0);
    Ok(())
}
