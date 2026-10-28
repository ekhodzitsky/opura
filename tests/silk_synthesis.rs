//! Typed synthesis integration and unchanged-state checks for invalid contexts.
use opura::{
    range::RangeDecoder,
    silk::{
        excitation::{FrameExcitation, decode_excitation},
        gains::{FrameActivity, FrameDuration, FrameGains, GainCoding, GainState, SignalType},
        nlsf::{FrameNlsf, NlsfOrder, NlsfState},
        pitch::{FramePitch, PitchCoding, PitchState, SilkRate},
        synthesis::{SynthesisError, SynthesisState},
    },
};

type Parts = (FrameGains, FrameNlsf, FramePitch, FrameExcitation);
fn parts(
    mut seed: u32,
    rate: SilkRate,
    duration: FrameDuration,
) -> Result<Parts, Box<dyn std::error::Error>> {
    let input: Vec<_> = (0..2048)
        .map(|_| {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed as u8
        })
        .collect();
    let mut decoder = RangeDecoder::new(&input);
    let gains = GainState::default().decode(
        &mut decoder,
        duration,
        FrameActivity::Active,
        GainCoding::Independent,
    )?;
    let order = if rate == SilkRate::Khz16 {
        NlsfOrder::Order16
    } else {
        NlsfOrder::Order10
    };
    let nlsf = NlsfState::default().decode(&mut decoder, order, duration, gains.signal_type())?;
    let pitch = PitchState::default().decode(
        &mut decoder,
        rate,
        duration,
        gains.signal_type(),
        PitchCoding::Independent,
    )?;
    let excitation = decode_excitation(
        &mut decoder,
        rate,
        duration,
        gains.signal_type(),
        gains.offset_type(),
        pitch.seed(),
    )?;
    Ok((gains, nlsf, pitch, excitation))
}

#[test]
fn decoded_parameters_produce_bounded_pcm_and_reset_repeats_output()
-> Result<(), Box<dyn std::error::Error>> {
    for rate in [SilkRate::Khz8, SilkRate::Khz12, SilkRate::Khz16] {
        for duration in [FrameDuration::Ms10, FrameDuration::Ms20] {
            let mut state = SynthesisState::default();
            for seed in 1..=32 {
                let (g, n, p, e) = parts(seed, rate, duration)?;
                let output = state.synthesize(rate, &g, &n, &p, &e)?;
                assert_eq!(output.samples().len(), e.samples_q14().len());
                assert_eq!(output.rate(), rate);
                if seed == 1 {
                    let after = state.clone();
                    state.reset();
                    assert_eq!(state, SynthesisState::default());
                    assert_eq!(state.synthesize(rate, &g, &n, &p, &e)?, output);
                    assert_eq!(state, after);
                }
            }
        }
    }
    Ok(())
}

#[test]
fn incompatible_geometry_does_not_change_history() -> Result<(), Box<dyn std::error::Error>> {
    let rate = SilkRate::Khz8;
    let (g, n, p, e) = parts(7, rate, FrameDuration::Ms10)?;
    let (g20, _, p20, e20) = parts(8, rate, FrameDuration::Ms20)?;
    let (_, wide, _, _) = parts(9, SilkRate::Khz16, FrameDuration::Ms10)?;
    let opposite = if g.signal_type() == SignalType::Voiced {
        SignalType::Unvoiced
    } else {
        SignalType::Voiced
    };
    let data = [0u8; 1024];
    let wrong_signal = PitchState::default().decode(
        &mut RangeDecoder::new(&data),
        rate,
        FrameDuration::Ms10,
        opposite,
        PitchCoding::Independent,
    )?;
    let mut state = SynthesisState::default();
    state.synthesize(rate, &g, &n, &p, &e)?;
    let before = state.clone();
    for (r, gain, nlsf, pitch, excitation) in [
        (SilkRate::Khz12, &g, &n, &p, &e),
        (rate, &g20, &n, &p, &e),
        (rate, &g, &wide, &p, &e),
        (rate, &g, &n, &p20, &e),
        (rate, &g, &n, &p, &e20),
        (rate, &g, &n, &wrong_signal, &e),
    ] {
        assert_eq!(
            state.synthesize(r, gain, nlsf, pitch, excitation),
            Err(SynthesisError::IncompatibleFrame)
        );
        assert_eq!(state, before);
    }
    Ok(())
}
