//! Original band denormalization and canonical de-emphasis output/state.
use opura::celt::{
    FrameSize,
    output::{self, Decimation, DeemphasisState, OutputError},
};
const SIZES: [FrameSize; 4] = [
    FrameSize::Ms2_5,
    FrameSize::Ms5,
    FrameSize::Ms10,
    FrameSize::Ms20,
];
const RATES: [Decimation; 5] = [
    Decimation::By1,
    Decimation::By2,
    Decimation::By3,
    Decimation::By4,
    Decimation::By6,
];
fn rate(factor: usize) -> Decimation {
    match factor {
        1 => Decimation::By1,
        2 => Decimation::By2,
        3 => Decimation::By3,
        4 => Decimation::By4,
        6 => Decimation::By6,
        _ => panic!("fixture rate"),
    }
}

#[test]
fn spectra_pcm_and_feedback_match_original_across_rates_and_channels()
-> Result<(), Box<dyn std::error::Error>> {
    let mut spectra = 0;
    let mut frames = 0;
    let mut state = DeemphasisState::default();
    let mut geometry = [[[false; 2]; 5]; 4];
    for line in include_str!("celt-output-fixtures.txt")
        .lines()
        .filter(|l| !l.starts_with('#'))
    {
        let row: Vec<_> = line.split_whitespace().collect();
        if row[0] == "spectrum" {
            let lm = row[1].parse::<usize>()?;
            let target = rate(row[2].parse()?);
            let start = row[3].parse()?;
            let end = row[4].parse()?;
            let silence = row[5] == "1";
            let values: Vec<_> = row[6..]
                .iter()
                .map(|s| s.parse::<i32>())
                .collect::<Result<_, _>>()?;
            let energies: [i32; 21] = values[..21].try_into()?;
            let n = SIZES[lm].samples();
            let result = output::denormalize(
                SIZES[lm],
                &values[21..21 + n],
                &energies,
                start..end,
                target,
                silence,
            )?;
            assert_eq!(
                result.values(),
                &values[21 + n..],
                "spectrum {spectra} LM{lm} rate{} bands{start}..{end}",
                target.factor()
            );
            assert!(
                result.values()[n / target.factor()..]
                    .iter()
                    .all(|&x| x == 0)
            );
            if silence {
                assert!(result.values().iter().all(|&x| x == 0));
            }
            spectra += 1;
        } else {
            assert_eq!(row[0], "frame");
            if row[1] == "1" {
                state.reset();
            }
            let target = rate(row[2].parse()?);
            let channels = row[3].parse::<usize>()?;
            let n = row[4].parse::<usize>()?;
            let values: Vec<_> = row[5..]
                .iter()
                .map(|s| s.parse::<i32>())
                .collect::<Result<_, _>>()?;
            let left = &values[..n];
            let right = if channels == 2 {
                Some(&values[n..2 * n])
            } else {
                None
            };
            let pcm = state.process(left, right, target)?;
            let count = n / target.factor() * channels;
            let expected: Vec<_> = values[n * channels..n * channels + count]
                .iter()
                .map(|&v| i16::try_from(v))
                .collect::<Result<_, _>>()?;
            assert_eq!(pcm.samples(), expected, "PCM frame {frames}");
            assert_eq!(state.memory(), &values[n * channels + count..]);
            assert_eq!(pcm.channels(), channels);
            assert_eq!(pcm.samples_per_channel(), n / target.factor());
            assert_eq!(pcm.sample_rate(), 48000 / target.factor() as u32);
            let lm = SIZES.iter().position(|s| s.samples() == n).ok_or("size")?;
            let r = RATES.iter().position(|r| *r == target).ok_or("rate")?;
            geometry[lm][r][channels - 1] = true;
            frames += 1;
        }
    }
    assert_eq!(spectra, 640);
    assert_eq!(frames, 384);
    assert_eq!(geometry, [[[true; 2]; 5]; 4]);
    Ok(())
}
#[test]
fn invalid_geometry_and_reset_preserve_both_histories() -> Result<(), Box<dyn std::error::Error>> {
    let mut state = DeemphasisState::default();
    state.process(&[123456; 120], Some(&[-654321; 120]), Decimation::By1)?;
    let before = state.clone();
    for length in [0, 1, 119, 121, 961] {
        assert_eq!(
            state.process(&vec![0; length], None, Decimation::By6),
            Err(OutputError::InvalidSignal)
        );
        assert_eq!(state, before);
    }
    assert_eq!(
        state.process(&[0; 120], Some(&[0; 240]), Decimation::By2),
        Err(OutputError::InvalidSignal)
    );
    assert_eq!(state, before);
    for size in SIZES {
        let n = size.samples();
        let input = vec![0; n];
        for (start, end) in [(10, 9), (21, 22), (usize::MAX, usize::MAX)] {
            assert_eq!(
                output::denormalize(size, &input, &[0; 21], start..end, Decimation::By1, false),
                Err(OutputError::InvalidSpectrum)
            );
        }
        assert_eq!(
            output::denormalize(
                size,
                &input[..n - 1],
                &[0; 21],
                0..21,
                Decimation::By1,
                true
            ),
            Err(OutputError::InvalidSpectrum)
        );
    }
    let right_memory = state.memory()[1];
    state.process(&[0; 120], None, Decimation::By3)?;
    assert_eq!(state.memory()[1], right_memory);
    state.reset();
    assert_eq!(state, DeemphasisState::default());
    for target in RATES {
        assert_eq!(
            state.process(&[0; 960], Some(&[0; 960]), target)?.samples(),
            vec![0; 960 / target.factor() * 2]
        );
    }
    Ok(())
}
