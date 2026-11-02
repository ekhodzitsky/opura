use super::*;
use crate::silk::{gains::SignalType, pitch::SilkRate};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
fn signal(v: i32) -> SignalType {
    match v {
        0 => SignalType::Inactive,
        1 => SignalType::Unvoiced,
        _ => SignalType::Voiced,
    }
}
fn rate(khz: i32) -> SilkRate {
    match khz {
        8 => SilkRate::Khz8,
        12 => SilkRate::Khz12,
        _ => SilkRate::Khz16,
    }
}
#[test]
fn public_native_connected_shaping_spectrum_energy_and_gains_match() -> Result<()> {
    let mut pitch = None;
    let mut shape = None;
    let mut ltp = None;
    let mut energies = None;
    let mut raw = Vec::new();
    let mut counts = [0; 3];
    let mut warped = 0;
    for (line_number, line) in include_str!("../../../tests/silk-shaping-fixtures.txt")
        .lines()
        .enumerate()
    {
        if line.starts_with('#') {
            continue;
        }
        let fields: Vec<_> = line.split_whitespace().collect();
        let v: Vec<i32> = fields[1..]
            .iter()
            .map(|s| s.parse())
            .collect::<std::result::Result<_, _>>()?;
        match fields[0] {
            "pitch" => {
                let r = rate(v[0]);
                let count = v[1] as usize;
                let length = v[15] as usize;
                let mut state = PitchState {
                    rate: if v[5] != 0 { None } else { Some(r) },
                    lag: v[8] as u16,
                    signal: signal(v[7]),
                    correlation: v[9],
                };
                let input: Vec<i16> = v[16 + count..16 + count + length]
                    .iter()
                    .map(|&v| v as i16)
                    .collect();
                let result = state.analyze(
                    r,
                    if count == 2 {
                        FrameDuration::Ms10
                    } else {
                        FrameDuration::Ms20
                    },
                    &input,
                    PitchOptions {
                        complexity: v[2] as u8,
                        active: v[6] != 0,
                        speech_activity_q8: v[3] as u16,
                        input_tilt_q15: v[4] as i16,
                    },
                )?;
                ltp = Some(LtpState::default().analyze(&result, true));
                pitch = Some(result);
            }
            "ltp" => {
                let pitch = pitch.as_ref().ok_or("missing pitch")?;
                let r = rate((pitch.length / (22 + pitch.count * 5)) as i32);
                let mut state = LtpState {
                    rate: Some(r),
                    sum_log_gain: v[0],
                };
                ltp = Some(state.analyze(pitch, false));
            }
            "shape" => {
                let r = rate(v[0]);
                let count = v[1] as usize;
                let length = v[19] as usize;
                let order = v[18] as usize;
                let activity = SpeechActivity {
                    probability_q8: v[5] as u8,
                    tilt_q15: v[6] as i16,
                    quality_bands_q15: [v[7] as i16, v[8] as i16, v[9] as i16, v[10] as i16],
                };
                let mut state = ShapeState {
                    rate: Some(r),
                    harmonic: v[11],
                    tilt: v[12],
                };
                raw = v[20..20 + length].iter().map(|&v| v as i16).collect();
                let result = state.analyze(
                    r,
                    if count == 2 {
                        FrameDuration::Ms10
                    } else {
                        FrameDuration::Ms20
                    },
                    &raw,
                    pitch.as_ref().ok_or("missing pitch")?,
                    ShapeOptions {
                        complexity: v[2] as u8,
                        snr_db_q7: v[3] as u16,
                        activity,
                        constant_bitrate: v[4] != 0,
                    },
                )?;
                assert_eq!(
                    [state.harmonic, state.tilt],
                    v[13..15],
                    "smoothing line {}",
                    line_number + 1
                );
                assert_eq!(result.input_quality_q14(), v[15]);
                assert_eq!(result.coding_quality_q14(), v[16]);
                assert_eq!(i32::from(result.offset()), v[17]);
                assert_eq!(result.order(), order);
                let at = 20 + length;
                let actual: Vec<_> = result
                    .coefficients_q13()
                    .iter()
                    .flat_map(|a| a[..order].iter().copied())
                    .collect();
                assert_eq!(
                    actual,
                    v[at..at + count * order]
                        .iter()
                        .map(|&v| v as i16)
                        .collect::<Vec<_>>(),
                    "AR line {}",
                    line_number + 1
                );
                let at = at + count * order;
                assert_eq!(
                    result.gains_q16(),
                    &v[at..at + count],
                    "initial gains line {}",
                    line_number + 1
                );
                assert_eq!(
                    result.low_frequency_q14(),
                    &v[at + count..at + 2 * count],
                    "LF line {}",
                    line_number + 1
                );
                assert_eq!(
                    result.harmonic,
                    v[at + 2 * count..at + 2 * count + 4]
                        .iter()
                        .map(|&v| v as i16)
                        .collect::<Vec<_>>()
                        .as_slice()
                );
                assert_eq!(
                    result.tilt,
                    v[at + 2 * count + 4..]
                        .iter()
                        .map(|&v| v as i16)
                        .collect::<Vec<_>>()
                        .as_slice()
                );
                warped += usize::from(result.warping_q16() > 0);
                shape = Some(result);
                counts[0] += 1;
            }
            "spectral" => {
                let shape = shape.as_ref().ok_or("missing shape")?;
                let ltp = ltp.as_ref().ok_or("missing LTP")?;
                let count = shape.count;
                let order = if shape.rate == SilkRate::Khz16 {
                    16
                } else {
                    10
                };
                let options = shape.spectral_options(ltp, v[2] != 0)?;
                assert_eq!(
                    options.minimum_inverse_gain_q30,
                    v[0] as u32,
                    "gain floor line {}",
                    line_number + 1
                );
                assert_eq!(options.allow_interpolation, v[1] != 0);
                assert_eq!(i32::from(options.survivors), v[3]);
                let inverse = shape.inverse_gains_q16();
                let pitch_length = (22 + count * 5) * shape.rate.khz() as usize;
                let normalized =
                    ltp.normalized_residual(&raw[..pitch_length], &inverse[..count])?;
                let mut previous = [0i16; 16];
                for i in 0..order {
                    previous[i] = v[4 + i] as i16;
                }
                let mut state = SpectralState {
                    rate: if v[2] != 0 { None } else { Some(shape.rate) },
                    previous,
                };
                let duration = if count == 2 {
                    FrameDuration::Ms10
                } else {
                    FrameDuration::Ms20
                };
                let spectral =
                    state.analyze(shape.rate, duration, normalized.samples(), options)?;
                energies = Some(ResidualEnergies::analyze(
                    shape.rate,
                    duration,
                    &normalized,
                    &spectral,
                    &inverse[..count],
                )?);
                counts[1] += 1;
            }
            "process" => {
                let shape = shape.as_ref().ok_or("missing shape")?;
                let energies = energies.as_ref().ok_or("missing energy")?;
                let ltp = ltp.as_ref().ok_or("missing LTP")?;
                let count = shape.count;
                assert_eq!(ltp.prediction_gain_db_q7(), v[5]);
                assert_eq!(shape.gains_q16(), &v[6..6 + count]);
                assert_eq!(
                    energies.values(),
                    &v[6 + count..6 + 2 * count],
                    "residual energy line {}",
                    line_number + 1
                );
                assert_eq!(
                    energies.precision(),
                    &v[6 + 2 * count..6 + 3 * count],
                    "energy Q line {}",
                    line_number + 1
                );
                let mut state = GainProcessState {
                    rate: Some(shape.rate),
                    quantizer: GainQuantizer {
                        previous: v[1] as u8,
                    },
                };
                let result = state.process(shape, energies, ltp, v[0] == 2)?;
                assert_eq!(i32::from(result.previous_index()), v[1]);
                assert_eq!(i32::from(state.quantizer.previous_index()), v[2]);
                assert_eq!(i32::from(result.offset()), v[3]);
                assert_eq!(i32::from(result.lambda_q10()), v[4]);
                assert_eq!(
                    result.unquantized_q16(),
                    &v[6 + 3 * count..6 + 4 * count],
                    "soft gains line {}",
                    line_number + 1
                );
                assert_eq!(
                    result.quantized().gains_q16(),
                    &v[6 + 4 * count..6 + 5 * count],
                    "quantized gains line {}",
                    line_number + 1
                );
                assert_eq!(
                    result.quantized().indices(),
                    v[6 + 5 * count..]
                        .iter()
                        .map(|&v| v as u8)
                        .collect::<Vec<_>>()
                );
                counts[2] += 1;
            }
            other => return Err(format!("unknown trace {other}").into()),
        }
    }
    assert_eq!(counts, [144; 3]);
    assert_eq!(warped, 96);
    Ok(())
}

#[test]
fn shaping_pipeline_full_scale_complexity_and_atomic_validation() -> Result<()> {
    let mut shape_state = ShapeState::default();
    let mut pitch_state = PitchState::default();
    let mut gains_state = GainProcessState::default();
    let mut spectral_state = SpectralState::default();
    let mut ltp_state = LtpState::default();
    for r in [SilkRate::Khz8, SilkRate::Khz12, SilkRate::Khz16] {
        let khz = r.khz() as usize;
        for duration in [FrameDuration::Ms10, FrameDuration::Ms20] {
            let count = if duration == FrameDuration::Ms10 {
                2
            } else {
                4
            };
            for complexity in 0..=10 {
                let lookahead = if matches!(complexity, 0 | 2) { 3 } else { 5 };
                for kind in 0..4 {
                    let length = (20 + 5 * count + lookahead) * khz;
                    let input: Vec<i16> = (0..length)
                        .map(|i| match kind {
                            0 => 0,
                            1 => i16::MIN,
                            2 => {
                                if i % 2 == 0 {
                                    i16::MIN
                                } else {
                                    i16::MAX
                                }
                            }
                            _ => ((i % 64) as i32 * 1024 - 32768) as i16,
                        })
                        .collect();
                    let pitch = pitch_state.analyze(
                        r,
                        duration,
                        &input[..(22 + 5 * count) * khz],
                        PitchOptions {
                            complexity,
                            active: kind != 0,
                            speech_activity_q8: 255,
                            input_tilt_q15: 0,
                        },
                    )?;
                    let activity = SpeechActivity {
                        probability_q8: if kind == 0 { 0 } else { 255 },
                        tilt_q15: if kind == 1 { i16::MIN } else { i16::MAX },
                        quality_bands_q15: if kind == 0 { [0; 4] } else { [32767; 4] },
                    };
                    let options = ShapeOptions {
                        complexity,
                        snr_db_q7: match kind {
                            0 => 0,
                            2 => 8192,
                            _ => 4096,
                        },
                        activity,
                        constant_bitrate: kind % 2 == 0,
                    };
                    let shape = shape_state.analyze(r, duration, &input, &pitch, options)?;
                    assert!(shape.gains_q16().iter().all(|&v| v > 0));
                    let inverse = shape.inverse_gains_q16();
                    assert!(inverse[..count].iter().all(|v| (100..=16384).contains(v)));
                    let ltp = ltp_state.analyze(&pitch, true);
                    let normalized = ltp
                        .normalized_residual(&input[..(22 + 5 * count) * khz], &inverse[..count])?;
                    let spectrum = spectral_state.analyze(
                        r,
                        duration,
                        normalized.samples(),
                        shape.spectral_options(&ltp, true)?,
                    )?;
                    let energy = ResidualEnergies::analyze(
                        r,
                        duration,
                        &normalized,
                        &spectrum,
                        &inverse[..count],
                    )?;
                    let gain = gains_state.process(&shape, &energy, &ltp, false)?;
                    assert!((1..2048).contains(&gain.lambda_q10()));
                    let before = shape_state.clone();
                    assert_eq!(
                        shape_state.analyze(
                            r,
                            duration,
                            &input[..input.len() - 1],
                            &pitch,
                            options
                        ),
                        Err(AnalysisError::InvalidLength)
                    );
                    for invalid in [
                        ShapeOptions {
                            complexity: 11,
                            ..options
                        },
                        ShapeOptions {
                            snr_db_q7: 8193,
                            ..options
                        },
                        ShapeOptions {
                            activity: SpeechActivity {
                                quality_bands_q15: [-1; 4],
                                ..activity
                            },
                            ..options
                        },
                    ] {
                        assert_eq!(
                            shape_state.analyze(r, duration, &input, &pitch, invalid),
                            Err(AnalysisError::InvalidParameters)
                        );
                    }
                    assert_eq!(shape_state, before);
                    let before = gains_state.clone();
                    let mut mismatched = energy.clone();
                    mismatched.count = if count == 2 { 4 } else { 2 };
                    assert_eq!(
                        gains_state.process(&shape, &mismatched, &ltp, false),
                        Err(AnalysisError::InvalidLength)
                    );
                    assert_eq!(gains_state, before);
                }
            }
        }
    }
    shape_state.reset();
    gains_state.reset();
    assert_eq!(shape_state, ShapeState::default());
    assert_eq!(gains_state, GainProcessState::default());
    Ok(())
}
