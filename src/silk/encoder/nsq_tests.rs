use super::*;
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
fn state_values(s: &NoiseShapingQuantizer) -> Vec<i32> {
    s.xq.iter()
        .map(|&v| i32::from(v))
        .chain(s.shaping_ltp.iter().copied())
        .chain(s.lpc.iter().copied())
        .chain(s.ar.iter().copied())
        .chain([
            s.low_frequency,
            s.difference,
            s.previous_lag as i32,
            s.ltp_index as i32,
            s.shaping_index as i32,
            s.random,
            s.previous_gain,
            i32::from(s.rewhite),
        ])
        .collect()
}
fn fingerprint(s: &NoiseShapingQuantizer) -> u64 {
    state_values(s)
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .fold(14695981039346656037, |h, b| {
            (h ^ u64::from(b)).wrapping_mul(1099511628211)
        })
}
fn parameters(meta: &[i32], coefficients: &[i32]) -> NsqParameters {
    let mut p = NsqParameters {
        signal: match meta[4] {
            0 => SignalType::Inactive,
            1 => SignalType::Unvoiced,
            _ => SignalType::Voiced,
        },
        offset: meta[5] as u8,
        seed: meta[6] as u8,
        interpolation_q2: meta[7] as u8,
        lambda_q10: meta[8] as u16,
        ltp_scale_q14: meta[9] as u16,
        shaping_order: meta[3] as u8,
        predictor_q12: [[0; 16]; 2],
        ltp_q14: [[0; 5]; 4],
        shaping_q13: [[0; 24]; 4],
        harmonic_q14: [0; 4],
        tilt_q14: [0; 4],
        low_frequency_q14: [0; 4],
        gains_q16: [0; 4],
        pitch: [0; 4],
    };
    for h in 0..2 {
        for j in 0..16 {
            p.predictor_q12[h][j] = coefficients[h * 16 + j] as i16;
        }
    }
    for k in 0..4 {
        for j in 0..5 {
            p.ltp_q14[k][j] = coefficients[32 + k * 5 + j] as i16;
        }
        for j in 0..24 {
            p.shaping_q13[k][j] = coefficients[52 + k * 24 + j] as i16;
        }
        p.harmonic_q14[k] = coefficients[148 + k] as i16;
        p.tilt_q14[k] = coefficients[152 + k] as i16;
        p.low_frequency_q14[k] = coefficients[156 + k];
        p.gains_q16[k] = coefficients[160 + k];
        p.pitch[k] = coefficients[164 + k] as u16;
    }
    p
}
fn import_state(rate: SilkRate, values: &[i32]) -> NoiseShapingQuantizer {
    let mut s = NoiseShapingQuantizer {
        rate: Some(rate),
        ..NoiseShapingQuantizer::default()
    };
    for (v, &n) in s.xq.iter_mut().zip(&values[..640]) {
        *v = n as i16;
    }
    s.shaping_ltp.copy_from_slice(&values[640..1280]);
    s.lpc.copy_from_slice(&values[1280..1376]);
    s.ar.copy_from_slice(&values[1376..1400]);
    s.low_frequency = values[1400];
    s.difference = values[1401];
    s.previous_lag = values[1402] as usize;
    s.ltp_index = values[1403] as usize;
    s.shaping_index = values[1404] as usize;
    s.random = values[1405];
    s.previous_gain = values[1406];
    s.rewhite = values[1407] != 0;
    s
}
fn verify_trace(fixture: &str, delayed: bool) -> Result<()> {
    let mut cases = 0;
    let mut signals = [false; 3];
    let mut geometry = [[false; 2]; 3];
    let mut aggressive = false;
    let mut warped = false;
    let mut interpolated = false;
    for line in fixture.lines().filter(|s| !s.starts_with('#')) {
        let columns: Vec<_> = line.split_whitespace().collect();
        let meta: Vec<i32> = columns[..13]
            .iter()
            .map(|s| s.parse())
            .collect::<std::result::Result<_, _>>()?;
        let expected_hash = columns[13].parse::<u64>()?;
        let values: Vec<i32> = columns[14..]
            .iter()
            .map(|s| s.parse())
            .collect::<std::result::Result<_, _>>()?;
        let khz = meta[1] as usize;
        let subframes = meta[2] as usize;
        let length = khz * subframes * 5;
        let rate = match khz {
            8 => SilkRate::Khz8,
            12 => SilkRate::Khz12,
            _ => SilkRate::Khz16,
        };
        let duration = if subframes == 2 {
            FrameDuration::Ms10
        } else {
            FrameDuration::Ms20
        };
        let p = parameters(&meta, &values[..168]);
        let mut state = import_state(rate, &values[168..168 + 1408]);
        if meta[0] != 0 {
            assert_eq!(
                state,
                NoiseShapingQuantizer {
                    rate: Some(rate),
                    ..NoiseShapingQuantizer::default()
                }
            );
        }
        let start = 168 + 1408;
        let input: Vec<i16> = values[start..start + length]
            .iter()
            .map(|&v| v as i16)
            .collect();
        let expected_pulses: Vec<i8> = values[start + length..start + 2 * length]
            .iter()
            .map(|&v| v as i8)
            .collect();
        let expected_pcm: Vec<i16> = values[start + 2 * length..]
            .iter()
            .map(|&v| v as i16)
            .collect();
        let result = if delayed {
            state.quantize_delayed(rate, duration, &input, &p, meta[10] as u8, meta[11] as i16)?
        } else {
            state.quantize(rate, duration, &input, &p)?
        };
        assert_eq!(result.seed(), meta[12] as u8);
        assert_eq!(
            result.pulses(),
            expected_pulses,
            "native pulses case {cases}"
        );
        assert_eq!(
            result.reconstructed_pcm(),
            expected_pcm,
            "native PCM case {cases}"
        );
        assert_eq!(
            fingerprint(&state),
            expected_hash,
            "entire native NSQ state case {cases}"
        );
        signals[meta[4] as usize] = true;
        geometry[match khz {
            8 => 0,
            12 => 1,
            _ => 2,
        }][usize::from(subframes == 4)] = true;
        aggressive |= p.lambda_q10 > 2048;
        warped |= meta[11] != 0;
        interpolated |= p.interpolation_q2 != 4;
        cases += 1;
    }
    assert_eq!(cases, 72);
    assert_eq!(signals, [true; 3]);
    assert_eq!(geometry, [[true; 2]; 3]);
    if delayed {
        assert!(warped);
        assert!(interpolated);
    } else {
        assert!(aggressive);
    }
    Ok(())
}
#[test]
fn actual_public_encoder_nsq_pulses_pcm_and_entire_state_match() -> Result<()> {
    verify_trace(include_str!("../../../tests/silk-nsq-fixtures.txt"), false)
}
#[test]
fn actual_public_delayed_decision_pulses_pcm_seed_and_entire_state_match() -> Result<()> {
    verify_trace(
        include_str!("../../../tests/silk-nsq-delayed-fixtures.txt"),
        true,
    )
}
fn simple() -> NsqParameters {
    NsqParameters {
        signal: SignalType::Unvoiced,
        offset: 0,
        seed: 0,
        interpolation_q2: 4,
        predictor_q12: [[0; 16]; 2],
        ltp_q14: [[0; 5]; 4],
        shaping_order: 12,
        shaping_q13: [[0; 24]; 4],
        harmonic_q14: [0; 4],
        tilt_q14: [0; 4],
        low_frequency_q14: [0; 4],
        gains_q16: [65536; 4],
        pitch: [0; 4],
        lambda_q10: 1024,
        ltp_scale_q14: 15565,
    }
}
#[test]
fn validation_rate_reset_and_pulse_serialization_preserve_contracts() -> Result<()> {
    use crate::silk::excitation::decode_excitation;
    use crate::{
        range::{RangeDecoder, RangeEncoder},
        silk::encoder::pulses,
    };
    let mut state = NoiseShapingQuantizer::default();
    let p = simple();
    let input: Vec<i16> = (0..320).map(|i| (i as i16).wrapping_mul(173)).collect();
    let frame = state.quantize(SilkRate::Khz16, FrameDuration::Ms20, &input, &p)?;
    let saved = state.clone();
    for case in 0..11 {
        let mut invalid = p.clone();
        match case {
            0 => invalid.offset = 2,
            1 => invalid.seed = 4,
            2 => invalid.interpolation_q2 = 5,
            3 => invalid.shaping_order = 0,
            4 => invalid.shaping_order = 25,
            5 => invalid.shaping_order = 3,
            6 => invalid.lambda_q10 = 32768,
            7 => invalid.ltp_scale_q14 = 16385,
            8 => invalid.gains_q16[2] = 0,
            9 => invalid.harmonic_q14[0] = -1,
            _ => invalid.pitch[0] = 289,
        }
        assert_eq!(
            state.quantize(SilkRate::Khz16, FrameDuration::Ms20, &input, &invalid),
            Err(NsqError::InvalidParameters)
        );
        assert_eq!(state, saved);
    }
    assert_eq!(
        state.quantize(SilkRate::Khz16, FrameDuration::Ms20, &input[..319], &p),
        Err(NsqError::InvalidInputLength)
    );
    assert_eq!(state, saved);
    let mut encoder = RangeEncoder::new(1275)?;
    pulses::encode(&mut encoder, p.signal, p.offset, frame.pulses())?;
    encoder.shrink(encoder.tell().div_ceil(8) as usize)?;
    let bytes = encoder.finish()?;
    let decoded = decode_excitation(
        &mut RangeDecoder::new(bytes),
        SilkRate::Khz16,
        FrameDuration::Ms20,
        p.signal,
        p.offset,
        p.seed,
    )?;
    assert_eq!(
        decoded.pulses(),
        frame
            .pulses()
            .iter()
            .map(|&v| i16::from(v))
            .collect::<Vec<_>>()
    );
    let mut fresh = NoiseShapingQuantizer::default();
    let small = &input[..80];
    assert_eq!(
        state.quantize(SilkRate::Khz8, FrameDuration::Ms10, small, &p)?,
        fresh.quantize(SilkRate::Khz8, FrameDuration::Ms10, small, &p)?
    );
    assert_eq!(state, fresh);
    state.reset();
    assert_eq!(state, NoiseShapingQuantizer::default());
    Ok(())
}

#[test]
fn delayed_bounds_warped_extremes_and_selected_seed_serialize() -> Result<()> {
    use crate::range::{RangeDecoder, RangeEncoder};
    use crate::silk::{encoder::pulses, excitation::decode_excitation};
    let input: Vec<i16> = (0..320)
        .map(|i| if i % 2 == 0 { i16::MIN } else { i16::MAX })
        .collect();
    let mut state = NoiseShapingQuantizer::default();
    let p = simple();
    state.quantize(SilkRate::Khz16, FrameDuration::Ms20, &input, &p)?;
    let saved = state.clone();
    for count in [0, 5] {
        assert_eq!(
            state.quantize_delayed(
                SilkRate::Khz8,
                FrameDuration::Ms10,
                &input[..80],
                &p,
                count,
                0
            ),
            Err(NsqError::InvalidParameters)
        );
        assert_eq!(state, saved);
    }
    for (count, warp) in [(1, 0), (4, i16::MAX)] {
        let mut parameters = p.clone();
        if count == 4 {
            parameters.signal = SignalType::Voiced;
            parameters.pitch = [32; 4];
            parameters.interpolation_q2 = 0;
            parameters.predictor_q12 = [[i16::MIN; 16]; 2];
            parameters.ltp_q14 = [[i16::MAX; 5]; 4];
            parameters.shaping_order = 24;
            parameters.shaping_q13 = [[i16::MIN; 24]; 4];
            parameters.harmonic_q14 = [i16::MAX; 4];
            parameters.tilt_q14 = [i16::MIN; 4];
            parameters.low_frequency_q14 = [i32::MIN; 4];
            parameters.gains_q16 = [1, i32::MAX, 65536, 1];
            parameters.lambda_q10 = 32767;
            parameters.ltp_scale_q14 = 16384;
        }
        let frame = state.quantize_delayed(
            SilkRate::Khz16,
            FrameDuration::Ms20,
            &input,
            &parameters,
            count,
            warp,
        )?;
        assert!(frame.seed() <= 3);
        assert_eq!(frame.reconstructed_pcm().len(), 320);
        let mut encoder = RangeEncoder::new(1275)?;
        pulses::encode(
            &mut encoder,
            parameters.signal,
            parameters.offset,
            frame.pulses(),
        )?;
        encoder.shrink(encoder.tell().div_ceil(8) as usize)?;
        let decoded = decode_excitation(
            &mut RangeDecoder::new(encoder.finish()?),
            SilkRate::Khz16,
            FrameDuration::Ms20,
            parameters.signal,
            parameters.offset,
            frame.seed(),
        )?;
        assert_eq!(
            decoded.pulses(),
            frame
                .pulses()
                .iter()
                .map(|&v| i16::from(v))
                .collect::<Vec<_>>()
        );
    }
    Ok(())
}
