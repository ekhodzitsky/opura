use super::*;

fn take<'a>(row: &'a [i64], index: &mut usize, count: usize) -> &'a [i64] {
    let values = &row[*index..*index + count];
    *index += count;
    values
}

#[test]
fn reference_pcm_and_histories_match() -> Result<(), Box<dyn std::error::Error>> {
    let mut state = SynthesisState::default();
    let mut cases = 0;
    let mut saturated = 0;
    for line in include_str!("../../tests/silk-synthesis-fixtures.txt").lines() {
        if line.starts_with('#') {
            continue;
        }
        let row: Vec<i64> = line
            .split_whitespace()
            .map(str::parse)
            .collect::<Result<_, _>>()?;
        if row[0] != 0 {
            state.reset();
        }
        let rate = match row[1] {
            8 => SilkRate::Khz8,
            12 => SilkRate::Khz12,
            _ => SilkRate::Khz16,
        };
        let count = row[2] as usize;
        let order = if rate == SilkRate::Khz16 { 16 } else { 10 };
        let length = row[1] as usize * 5 * count;
        let mut index = 5;
        let gains: Vec<_> = take(&row, &mut index, count)
            .iter()
            .map(|&v| v as i32)
            .collect();
        let coefficients: Vec<_> = take(&row, &mut index, order * 2)
            .iter()
            .map(|&v| v as i16)
            .collect();
        let lags: Vec<_> = take(&row, &mut index, count)
            .iter()
            .map(|&v| v as u16)
            .collect();
        let ltp: Vec<[i16; 5]> = take(&row, &mut index, count * 5)
            .chunks_exact(5)
            .map(|c| std::array::from_fn(|i| c[i] as i16))
            .collect();
        let scale = take(&row, &mut index, 1)[0] as i16;
        let excitation: Vec<_> = take(&row, &mut index, length)
            .iter()
            .map(|&v| v as i32)
            .collect();
        let expected: Vec<_> = take(&row, &mut index, length)
            .iter()
            .map(|&v| v as i16)
            .collect();
        let expected_lpc: Vec<_> = take(&row, &mut index, 16)
            .iter()
            .map(|&v| v as i32)
            .collect();
        let previous_gain = take(&row, &mut index, 1)[0] as i32;
        let history: Vec<_> = take(&row, &mut index, row[1] as usize * 20)
            .iter()
            .map(|&v| v as i16)
            .collect();
        assert_eq!(index, row.len());
        let output = state.apply(Parameters {
            rate,
            gains: &gains,
            filters: [&coefficients[..order], &coefficients[order..]],
            lags: &lags,
            ltp: &ltp,
            scale,
            voiced: row[3] == 2,
            interpolate: row[4] != 0,
            excitation: &excitation,
        });
        assert_eq!(output.samples(), expected, "PCM case {cases}");
        assert_eq!(output.rate(), rate);
        assert_eq!(state.lpc.as_slice(), expected_lpc, "LPC case {cases}");
        assert_eq!(state.previous_gain, previous_gain);
        assert_eq!(
            &state.history[..history.len()],
            history,
            "history case {cases}"
        );
        saturated += expected
            .iter()
            .filter(|&&v| v == i16::MIN || v == i16::MAX)
            .count();
        cases += 1;
    }
    assert_eq!(cases, 192);
    assert!(saturated > 0);
    Ok(())
}
