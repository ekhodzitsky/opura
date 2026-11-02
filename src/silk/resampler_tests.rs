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
fn verify_native<const DELAY: usize, const FIR: usize, const WORK: usize>(
    fixture: &str,
    encoder: bool,
) -> Result<[[bool; 5]; 5], Box<dyn std::error::Error>> {
    let mut state = ResamplerCore::<DELAY, FIR, WORK>::new(OutputRate::Khz8, encoder);
    let mut lines = fixture.lines().filter(|l| !l.starts_with('#'));
    let mut cases = 0;
    let mut pairs = [[false; 5]; 5];
    let mut clipped = 0;
    while let Some(line) = lines.next() {
        let header: Vec<i64> = line
            .split_whitespace()
            .map(str::parse)
            .collect::<Result<_, _>>()?;
        let rate = output_rate(header[1]);
        let target = output_rate(header[2]);
        if target != state.output {
            state = ResamplerCore::new(target, encoder);
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
        let history = 6 + 8 + FIR + DELAY;
        assert_eq!(
            serialized,
            expected[length..length + history],
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
            expected[length + history..],
            "configuration {cases}"
        );
        pairs[rate.index()][target.index()] = true;
        clipped += pcm
            .iter()
            .filter(|&&v| v == i16::MIN || v == i16::MAX)
            .count();
        cases += 1;
    }
    assert_eq!(cases, 360);
    assert!(clipped > 0);
    Ok(pairs)
}
#[test]
fn reference_pcm_and_all_persistent_filter_delay_states_match()
-> Result<(), Box<dyn std::error::Error>> {
    let pairs = verify_native::<16, 24, 184>(
        include_str!("../../tests/silk-resampler-fixtures.txt"),
        false,
    )?;
    assert_eq!(&pairs[..3], &[[true; 5]; 3]);
    Ok(())
}
#[test]
fn encoder_native_pcm_and_all_filter_delay_states_match() -> Result<(), Box<dyn std::error::Error>>
{
    let pairs = verify_native::<48, 36, 516>(
        include_str!("../../tests/silk-encoder-resampler-fixtures.txt"),
        true,
    )?;
    for row in pairs {
        assert_eq!(&row[..3], &[true; 3]);
    }
    Ok(())
}
#[test]
fn encoder_public_chunking_reset_and_geometry_failure() -> Result<(), Box<dyn std::error::Error>> {
    let input: Vec<i16> = (0..2400).map(|i| (i as i16).wrapping_mul(239)).collect();
    let mut whole = EncoderResamplerState::new(OutputRate::Khz48, SilkRate::Khz16);
    let mut split = whole.clone();
    let mut expected = [1234; 805];
    let mut actual = expected;
    assert_eq!(whole.process(&input, &mut expected)?, 800);
    assert_eq!(split.process(&input[..48], &mut actual[..16])?, 16);
    assert_eq!(split.process(&input[48..], &mut actual[16..])?, 784);
    assert_eq!(actual, expected);
    // The unused delay-buffer scratch tail depends on call boundaries. Check
    // future PCM and full state after an identical continuation instead.
    let mut next_whole = [0; 160];
    let mut next_split = [0; 160];
    whole.process(&input[..480], &mut next_whole)?;
    split.process(&input[..480], &mut next_split)?;
    assert_eq!(next_whole, next_split);
    assert_eq!(whole, split);
    let before = whole.clone();
    let mut output = [1234; 799];
    assert_eq!(
        whole.process(&input, &mut output),
        Err(ResampleError::OutputTooSmall {
            required: 800,
            available: 799
        })
    );
    assert_eq!(output, [1234; 799]);
    assert_eq!(whole, before);
    assert_eq!(
        whole.process(&input[..47], &mut actual),
        Err(ResampleError::InvalidInputLength)
    );
    assert_eq!(whole, before);
    assert_eq!(actual, expected);
    whole.reset();
    assert_eq!(
        whole,
        EncoderResamplerState::new(OutputRate::Khz48, SilkRate::Khz16)
    );
    Ok(())
}
