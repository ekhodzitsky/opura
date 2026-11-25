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

#[test]
fn transposed_lpc_matches_direct_sum_at_arithmetic_limits() {
    fn compare<const ORDER: usize>(length: usize) {
        let mut seed = 0x5a17_0e31u32;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed as i32
        };
        for case in 0..16 {
            let filter: [i16; ORDER] = std::array::from_fn(|i| match case {
                0 => i16::MIN,
                1 if i % 2 == 0 => i16::MIN,
                1 => i16::MAX,
                _ => next() as i16,
            });
            let mut expected = std::array::from_fn(|_| if case == 0 { i32::MIN } else { next() });
            let mut actual = expected;
            let residual: [i32; 80] = std::array::from_fn(|_| next());
            for (i, &sample) in residual[..length].iter().enumerate() {
                let mut sum = (ORDER / 2) as i64;
                for (j, &coefficient) in filter.iter().enumerate() {
                    sum += (i64::from(expected[15 + i - j]) * i64::from(coefficient)) >> 16;
                }
                let prediction = (sum as i32).clamp(i32::MIN >> 4, i32::MAX >> 4) << 4;
                expected[16 + i] = sample.saturating_add(prediction);
            }
            super::synthesize_lpc::<ORDER>(&mut actual, &residual[..length], &filter);
            assert_eq!(
                actual, expected,
                "LPC order {ORDER}, length {length}, case {case}"
            );
        }
    }
    compare::<10>(40);
    compare::<10>(60);
    compare::<16>(80);
}
