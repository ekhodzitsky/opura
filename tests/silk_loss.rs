//! Public channel loss API bounds, burst losses, reset and failed recovery.
use opura::{
    range::RangeDecoder,
    silk::{
        channel::ChannelState,
        gains::{FrameActivity, FrameDuration},
        pitch::{PitchCoding, SilkRate},
    },
};

#[test]
fn cold_losses_are_silent_and_reset_repeats_concealment() -> Result<(), Box<dyn std::error::Error>>
{
    for rate in [SilkRate::Khz8, SilkRate::Khz12, SilkRate::Khz16] {
        for duration in [FrameDuration::Ms10, FrameDuration::Ms20] {
            let mut state = ChannelState::default();
            for _ in 0..64 {
                let pcm = state.conceal(rate, duration)?;
                assert!(pcm.samples().iter().all(|&sample| sample == 0));
                assert_eq!(pcm.rate(), rate);
            }
            state.reset();
            assert_eq!(state, ChannelState::default());
            let first = state.conceal(rate, duration)?;
            let mut clean = ChannelState::default();
            assert_eq!(first, clean.conceal(rate, duration)?);
            assert_eq!(state, clean);
        }
    }
    Ok(())
}

#[test]
fn long_bursts_and_failed_recovery_preserve_bounded_transactional_state()
-> Result<(), Box<dyn std::error::Error>> {
    let mut seed = 0x6c6f7373u32;
    let data: Vec<_> = (0..2048)
        .map(|_| {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed as u8
        })
        .collect();
    for rate in [SilkRate::Khz8, SilkRate::Khz12, SilkRate::Khz16] {
        let mut state = ChannelState::default();
        state.decode(
            &mut RangeDecoder::new(&data),
            rate,
            FrameDuration::Ms20,
            FrameActivity::Active,
            PitchCoding::Independent,
        )?;
        for index in 0..512 {
            let duration = if index % 2 == 0 {
                FrameDuration::Ms10
            } else {
                FrameDuration::Ms20
            };
            let pcm = state.conceal(rate, duration)?;
            assert!(matches!(pcm.samples().len(), 80 | 120 | 160 | 240 | 320));
        }
        let before = state.clone();
        let mut empty = RangeDecoder::new(&[]);
        let entropy_before = empty.clone();
        assert!(
            state
                .decode(
                    &mut empty,
                    rate,
                    FrameDuration::Ms20,
                    FrameActivity::Active,
                    PitchCoding::Independent
                )
                .is_err()
        );
        assert_eq!(state, before);
        assert_eq!(empty, entropy_before);
        let mut control = before;
        assert_eq!(
            state.decode(
                &mut RangeDecoder::new(&data),
                rate,
                FrameDuration::Ms20,
                FrameActivity::Active,
                PitchCoding::Independent
            )?,
            control.decode(
                &mut RangeDecoder::new(&data),
                rate,
                FrameDuration::Ms20,
                FrameActivity::Active,
                PitchCoding::Independent
            )?
        );
        assert_eq!(state, control);
    }
    Ok(())
}
