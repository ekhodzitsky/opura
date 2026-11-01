//! Public resampler chunking, reset and transactional buffer validation.
use opura::silk::{
    pitch::SilkRate,
    resampler::{OutputRate, ResampleError, ResamplerState},
};

#[test]
fn whole_ms_chunking_preserves_pcm_and_following_filter_behavior()
-> Result<(), Box<dyn std::error::Error>> {
    let mut seed = 0x6d73504du32;
    for (rate, khz) in [
        (SilkRate::Khz8, 8),
        (SilkRate::Khz12, 12),
        (SilkRate::Khz16, 16),
    ] {
        for target in [
            OutputRate::Khz8,
            OutputRate::Khz12,
            OutputRate::Khz16,
            OutputRate::Khz24,
            OutputRate::Khz48,
        ] {
            let data: Vec<_> = (0..khz * 120)
                .map(|_| {
                    seed ^= seed << 13;
                    seed ^= seed >> 17;
                    seed ^= seed << 5;
                    seed as i16
                })
                .collect();
            let mut full = ResamplerState::new(target);
            let mut split = full.clone();
            let output_khz = target.hz() as usize / 1000;
            let mut expected = vec![0; output_khz * 120];
            full.process(rate, &data, &mut expected)?;
            let mut actual = Vec::new();
            let mut at = 0;
            for ms in [1, 2, 5, 10, 20, 40, 42] {
                let mut output = vec![0; ms * output_khz];
                split.process(rate, &data[at..at + ms * khz], &mut output)?;
                at += ms * khz;
                actual.extend_from_slice(&output);
            }
            assert_eq!(actual, expected, "{rate:?} -> {target:?}");
            // Unused delay-buffer slots depend on the last call's boundary;
            // a common follow-up overwrites them and compares future behavior.
            let mut a = vec![0; output_khz * 10];
            let mut b = a.clone();
            full.process(rate, &data[..khz * 10], &mut a)?;
            split.process(rate, &data[..khz * 10], &mut b)?;
            assert_eq!(a, b);
            assert_eq!(full, split);
            full.reset();
            let mut clean = ResamplerState::new(target);
            assert_eq!(full, clean);
            full.process(rate, &data, &mut actual)?;
            clean.process(rate, &data, &mut expected)?;
            assert_eq!(actual, expected);
            assert_eq!(full, clean);
        }
    }
    Ok(())
}

#[test]
fn invalid_geometry_and_capacity_do_not_touch_state_or_output()
-> Result<(), Box<dyn std::error::Error>> {
    let mut state = ResamplerState::new(OutputRate::Khz48);
    state.process(SilkRate::Khz16, &[1000; 160], &mut [0; 480])?;
    let before = state.clone();
    for length in [0, 1, 7, 9, 8 * 121] {
        let mut output = vec![42; 5808];
        assert_eq!(
            state.process(SilkRate::Khz8, &vec![0; length], &mut output),
            Err(ResampleError::InvalidInputLength)
        );
        assert_eq!(state, before);
        assert!(output.iter().all(|&v| v == 42));
    }
    let mut output = [42; 47];
    assert_eq!(
        state.process(SilkRate::Khz8, &[1000; 8], &mut output),
        Err(ResampleError::OutputTooSmall {
            required: 48,
            available: 47
        })
    );
    assert_eq!(output, [42; 47]);
    assert_eq!(state, before);
    Ok(())
}
