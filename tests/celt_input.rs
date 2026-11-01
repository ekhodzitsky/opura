//! Original CELT input signals, channel memory, chunking and rollback contracts.
use opura::celt::{
    FrameSize,
    input::{InputError, InputRate, PreemphasisState},
};
const SIZES: [FrameSize; 4] = [
    FrameSize::Ms2_5,
    FrameSize::Ms5,
    FrameSize::Ms10,
    FrameSize::Ms20,
];
const RATES: [InputRate; 5] = [
    InputRate::Khz48,
    InputRate::Khz24,
    InputRate::Khz16,
    InputRate::Khz12,
    InputRate::Khz8,
];

#[test]
fn original_samples_and_both_memories_match_across_context_changes()
-> Result<(), Box<dyn std::error::Error>> {
    let mut state = [PreemphasisState::default(), PreemphasisState::default()];
    let mut geometry = [[[false; 2]; 5]; 4];
    let mut rows = 0;
    for (index, line) in include_str!("celt-input-fixtures.txt").lines().enumerate() {
        if line.starts_with('#') {
            continue;
        }
        let cols: Vec<_> = line.split_whitespace().collect();
        assert_eq!(cols[0], "frame");
        if cols[1] == "1" {
            state[0].reset();
            state[1].reset();
        }
        let lm = cols[2].parse::<usize>()?;
        let factor = cols[3].parse::<usize>()?;
        let rate_index = RATES
            .iter()
            .position(|r| r.factor() == factor)
            .ok_or("invalid reference rate")?;
        let rate = RATES[rate_index];
        assert_eq!(rate.hz() as usize * factor, 48000);
        let channels = cols[4].parse::<usize>()?;
        let n = SIZES[lm].samples();
        let count = n / factor * channels;
        let input: Vec<i16> = cols[6..6 + count]
            .iter()
            .map(|s| s.parse())
            .collect::<Result<_, _>>()?;
        let expected: Vec<i32> = cols[6 + count..]
            .iter()
            .map(|s| s.parse())
            .collect::<Result<_, _>>()?;
        assert_eq!(expected.len(), n * channels + 2);
        let before = input.clone();
        for (channel, state) in state.iter_mut().enumerate().take(channels) {
            let mut output = [0x12345678; 960];
            state.process(SIZES[lm], rate, &input, channels, channel, &mut output)?;
            assert_eq!(
                output[..n],
                expected[channel * n..(channel + 1) * n],
                "row{index} channel{channel}"
            );
            assert!(output[n..].iter().all(|&s| s == 0x12345678));
        }
        assert_eq!(input, before);
        assert_eq!(
            [state[0].memory(), state[1].memory()],
            expected[n * channels..]
        );
        geometry[lm][rate_index][channels - 1] = true;
        rows += 1;
    }
    assert_eq!(rows, 512);
    assert!(geometry.iter().flatten().flatten().all(|&x| x));
    Ok(())
}

#[test]
fn duration_chunking_and_selected_channels_preserve_filter_continuation() -> Result<(), InputError>
{
    for rate in RATES {
        let count = 960 / rate.factor();
        let mono: Vec<i16> = (0..count).map(|i| (i as i16).wrapping_mul(12345)).collect();
        let stereo: Vec<i16> = mono.iter().flat_map(|&x| [x, x.wrapping_neg()]).collect();
        let mut whole = PreemphasisState::default();
        let mut output = [0; 960];
        whole.process(FrameSize::Ms20, rate, &mono, 1, 0, &mut output)?;
        let mut chunked = PreemphasisState::default();
        let mut channel = PreemphasisState::default();
        let mut right = PreemphasisState::default();
        let mut pieces = [0; 960];
        for block in 0..8 {
            let start = block * count / 8;
            let end = (block + 1) * count / 8;
            chunked.process(
                FrameSize::Ms2_5,
                rate,
                &mono[start..end],
                1,
                0,
                &mut pieces[block * 120..(block + 1) * 120],
            )?;
        }
        assert_eq!(pieces, output);
        assert_eq!(chunked, whole);
        let mut selected = [0; 960];
        channel.process(FrameSize::Ms20, rate, &stereo, 2, 0, &mut selected)?;
        assert_eq!(selected, output);
        assert_eq!(channel, whole);
        let right_mono: Vec<i16> = stereo.iter().skip(1).step_by(2).copied().collect();
        right.process(FrameSize::Ms20, rate, &stereo, 2, 1, &mut selected)?;
        let mut independent = PreemphasisState::default();
        let mut expected = [0; 960];
        independent.process(FrameSize::Ms20, rate, &right_mono, 1, 0, &mut expected)?;
        assert_eq!(selected, expected);
        assert_eq!(right, independent);
        right.reset();
        assert_eq!(right, PreemphasisState::default());
    }
    Ok(())
}

#[test]
fn invalid_domains_preserve_entire_output_and_nonzero_memory() -> Result<(), InputError> {
    let mut state = PreemphasisState::default();
    state.process(
        FrameSize::Ms2_5,
        InputRate::Khz48,
        &[32767; 120],
        1,
        0,
        &mut [0; 120],
    )?;
    assert_ne!(state.memory(), 0);
    let before = state.clone();
    for size in SIZES {
        let n = size.samples();
        for rate in RATES {
            let count = n / rate.factor();
            for (channels, channel, length) in [
                (0, 0, 0),
                (3, 0, count * 3),
                (usize::MAX, 0, 0),
                (1, 1, count),
                (2, 2, count * 2),
                (1, usize::MAX, count),
                (1, 0, count - 1),
                (1, 0, count + 1),
            ] {
                let mut output = [0x76543210; 960];
                assert_eq!(
                    state.process(size, rate, &vec![0; length], channels, channel, &mut output),
                    Err(InputError::InvalidPcm)
                );
                assert_eq!(output, [0x76543210; 960]);
                assert_eq!(state, before);
            }
            for length in [0, n - 1, 961] {
                let mut output = vec![0x76543210; length];
                let original = output.clone();
                assert_eq!(
                    state.process(size, rate, &vec![0; count], 1, 0, &mut output),
                    Err(InputError::InvalidOutput)
                );
                assert_eq!(output, original);
                assert_eq!(state, before);
            }
        }
    }
    Ok(())
}
