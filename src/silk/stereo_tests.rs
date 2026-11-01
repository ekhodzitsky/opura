use super::*;

#[test]
fn upstream_mid_side_reconstruction_and_raw_histories_match()
-> Result<(), Box<dyn std::error::Error>> {
    let mut state = StereoState::default();
    let mut lines = include_str!("../../tests/silk-stereo-fixtures.txt")
        .lines()
        .filter(|l| !l.starts_with('#'));
    let mut cases = 0;
    let mut clipped = 0;
    let mut wrapped_interpolation = false;
    while let Some(line) = lines.next() {
        let input: Vec<i64> = line
            .split_whitespace()
            .map(str::parse)
            .collect::<Result<_, _>>()?;
        if input[0] != 0 {
            state.reset();
        }
        let rate = match input[1] {
            8 => SilkRate::Khz8,
            12 => SilkRate::Khz12,
            _ => SilkRate::Khz16,
        };
        let length = input[2] as usize;
        let prediction = StereoPrediction {
            coefficients: [input[3] as i32, input[4] as i32],
        };
        wrapped_interpolation |=
            (prediction.coefficients[0] - state.prediction.coefficients[0]).abs() > 32767;
        let mid: Vec<_> = input[5..5 + length].iter().map(|&v| v as i16).collect();
        let side: Vec<_> = input[5 + length..].iter().map(|&v| v as i16).collect();
        let expected: Vec<i64> = lines
            .next()
            .ok_or("missing stereo result")?
            .split_whitespace()
            .map(str::parse)
            .collect::<Result<_, _>>()?;
        let result = state.reconstruct(rate, &mid, &side, prediction)?;
        assert_eq!(result.rate(), rate);
        let left: Vec<_> = expected[..length].iter().map(|&v| v as i16).collect();
        let right: Vec<_> = expected[length..2 * length]
            .iter()
            .map(|&v| v as i16)
            .collect();
        assert_eq!(result.left(), left, "left case {cases}");
        assert_eq!(result.right(), right, "right case {cases}");
        let history: Vec<_> = state
            .prediction
            .coefficients
            .iter()
            .copied()
            .map(i64::from)
            .chain(state.mid.iter().copied().map(i64::from))
            .chain(state.side.iter().copied().map(i64::from))
            .collect();
        assert_eq!(history, expected[2 * length..], "history case {cases}");
        clipped += left
            .iter()
            .chain(&right)
            .filter(|&&s| s == i16::MIN || s == i16::MAX)
            .count();
        cases += 1;
    }
    assert_eq!(cases, 192);
    assert!(clipped > 0);
    assert!(wrapped_interpolation);
    Ok(())
}
