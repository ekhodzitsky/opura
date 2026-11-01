use super::*;

fn take<'a>(row: &'a [i64], index: &mut usize, count: usize) -> &'a [i64] {
    let part = &row[*index..*index + count];
    *index += count;
    part
}

#[test]
fn reference_pcm_and_complete_cng_history_match() -> Result<(), Box<dyn std::error::Error>> {
    let mut state = CngState::default();
    let mut cases = 0;
    let mut clipped = 0;
    let mut mixed = 0;
    for line in include_str!("../../tests/silk-cng-fixtures.txt")
        .lines()
        .chain(include_str!("../../tests/silk-cng-wide-fixtures.txt").lines())
        .filter(|l| !l.starts_with('#'))
    {
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
        let length = row[1] as usize * 5 * count;
        let order = order(rate);
        let mut cursor = 7;
        let gains: Vec<_> = take(&row, &mut cursor, count)
            .iter()
            .map(|&v| v as i32)
            .collect();
        let spectrum: Vec<_> = take(&row, &mut cursor, order)
            .iter()
            .map(|&v| v as i16)
            .collect();
        let excitation: Vec<_> = take(&row, &mut cursor, length)
            .iter()
            .map(|&v| v as i32)
            .collect();
        let mut pcm: Vec<_> = take(&row, &mut cursor, length)
            .iter()
            .map(|&v| v as i16)
            .collect();
        let initial = pcm.clone();
        let expected_pcm: Vec<_> = take(&row, &mut cursor, length)
            .iter()
            .map(|&v| v as i16)
            .collect();
        if row[4] != 0 {
            state.mix(
                rate,
                if count == 2 {
                    FrameDuration::Ms10
                } else {
                    FrameDuration::Ms20
                },
                row[5] as u16,
                row[6] as i32,
                &mut pcm,
            )?;
        } else {
            state.observe(rate, &gains, &spectrum, &excitation, row[3] == 0);
        }
        assert_eq!(pcm, expected_pcm, "PCM case {cases}");
        let bank: Vec<_> = take(&row, &mut cursor, 320)
            .iter()
            .map(|&v| v as i32)
            .collect();
        let spectral: Vec<_> = take(&row, &mut cursor, 16)
            .iter()
            .map(|&v| v as i16)
            .collect();
        let synthesis: Vec<_> = take(&row, &mut cursor, 16)
            .iter()
            .map(|&v| v as i32)
            .collect();
        assert_eq!(state.excitation.as_slice(), bank, "excitation case {cases}");
        assert_eq!(state.spectrum.as_slice(), spectral, "spectrum case {cases}");
        assert_eq!(
            state.synthesis.as_slice(),
            synthesis,
            "filter history case {cases}"
        );
        assert_eq!(state.gain, row[cursor] as i32, "gain case {cases}");
        assert_eq!(state.seed, row[cursor + 1] as i32, "seed case {cases}");
        assert_eq!(state.rate.map(SilkRate::khz), Some(row[cursor + 2] as i32));
        assert_eq!(row.len(), cursor + 3);
        clipped += pcm
            .iter()
            .filter(|&&s| s == i16::MAX || s == i16::MIN)
            .count();
        mixed += pcm.iter().zip(initial).filter(|(a, b)| **a != *b).count();
        cases += 1;
    }
    assert_eq!(cases, 384);
    assert!(clipped > 0);
    assert!(mixed > 1000);
    Ok(())
}

#[test]
fn invalid_loss_inputs_preserve_history_and_samples() -> Result<(), CngError> {
    let mut state = CngState::default();
    let spectrum: Vec<_> = (1..=10).map(|i| i * 2900).collect();
    state.observe(
        SilkRate::Khz8,
        &[2097152; 4],
        &spectrum,
        &[16000; 160],
        true,
    );
    state.mix(SilkRate::Khz8, FrameDuration::Ms20, 0, 65536, &mut [0; 160])?;
    let before = state.clone();
    for (rate, duration, scale, gain, length, error) in [
        (
            SilkRate::Khz12,
            FrameDuration::Ms20,
            0,
            65536,
            160,
            CngError::IncompatibleFrame,
        ),
        (
            SilkRate::Khz8,
            FrameDuration::Ms10,
            0,
            65536,
            160,
            CngError::IncompatibleFrame,
        ),
        (
            SilkRate::Khz8,
            FrameDuration::Ms20,
            32768,
            65536,
            160,
            CngError::InvalidNoiseContext,
        ),
        (
            SilkRate::Khz8,
            FrameDuration::Ms20,
            0,
            0,
            160,
            CngError::InvalidNoiseContext,
        ),
        (
            SilkRate::Khz8,
            FrameDuration::Ms20,
            0,
            -1,
            160,
            CngError::InvalidNoiseContext,
        ),
    ] {
        let mut pcm = vec![42; length];
        assert_eq!(state.mix(rate, duration, scale, gain, &mut pcm), Err(error));
        assert_eq!(state, before);
        assert_eq!(pcm, vec![42; length]);
    }
    state.reset();
    assert_eq!(state, CngState::default());
    let mut silence = [0; 160];
    state.mix(SilkRate::Khz8, FrameDuration::Ms20, 0, 65536, &mut silence)?;
    assert_eq!(silence, [0; 160]);
    Ok(())
}
