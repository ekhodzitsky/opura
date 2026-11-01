//! Typed comfort-noise integration and invalid-parameter transactions.
use opura::{
    range::RangeDecoder,
    silk::{
        cng::{CngError, CngState},
        excitation::decode_excitation,
        gains::{FrameActivity, FrameDuration, GainCoding, GainState},
        nlsf::{NlsfOrder, NlsfState},
        pitch::{PitchCoding, PitchState, SilkRate},
    },
};

#[test]
fn typed_frames_update_noise_and_mismatches_preserve_state()
-> Result<(), Box<dyn std::error::Error>> {
    for rate in [SilkRate::Khz8, SilkRate::Khz12, SilkRate::Khz16] {
        for duration in [FrameDuration::Ms10, FrameDuration::Ms20] {
            let input = [255; 1024];
            let mut entropy = RangeDecoder::new(&input);
            let gains = GainState::default().decode(
                &mut entropy,
                duration,
                FrameActivity::Inactive,
                GainCoding::Independent,
            )?;
            let order = if rate == SilkRate::Khz16 {
                NlsfOrder::Order16
            } else {
                NlsfOrder::Order10
            };
            let nlsf =
                NlsfState::default().decode(&mut entropy, order, duration, gains.signal_type())?;
            let pitch = PitchState::default().decode(
                &mut entropy,
                rate,
                duration,
                gains.signal_type(),
                PitchCoding::Independent,
            )?;
            let excitation = decode_excitation(
                &mut entropy,
                rate,
                duration,
                gains.signal_type(),
                gains.offset_type(),
                pitch.seed(),
            )?;
            let mut state = CngState::default();
            state.update(rate, &gains, &nlsf, &excitation)?;
            let before = state.clone();
            let wrong_rate = if rate == SilkRate::Khz16 {
                SilkRate::Khz8
            } else {
                SilkRate::Khz16
            };
            assert_eq!(
                state.update(wrong_rate, &gains, &nlsf, &excitation),
                Err(CngError::IncompatibleFrame)
            );
            assert_eq!(state, before);
            let mut pcm = vec![0; excitation.samples_q14().len()];
            state.mix(rate, duration, 0, 65536, &mut pcm)?;
            assert!(pcm.iter().any(|&sample| sample != 0));
            state.reset();
            assert_eq!(state, CngState::default());
            state.update(rate, &gains, &nlsf, &excitation)?;
            assert_eq!(state, before);
        }
    }
    Ok(())
}
