use super::*;
use crate::silk::{gains::SignalType, nlsf::NlsfOrder, pitch::SilkRate};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
fn order(count: usize) -> NlsfOrder {
    if count == 10 {
        NlsfOrder::Order10
    } else {
        NlsfOrder::Order16
    }
}
#[test]
fn original_public_encoder_gain_spectrum_burg_and_root_results_match() -> Result<()> {
    let mut counts = [0; 6];
    let mut recovered_roots = 0;
    let mut initial_vad = 0;
    for (line_number, line) in include_str!("../../../tests/silk-analysis-fixtures.txt")
        .lines()
        .enumerate()
    {
        if line.starts_with('#') {
            continue;
        }
        let fields: Vec<_> = line.split_whitespace().collect();
        let values: Vec<i32> = fields[1..]
            .iter()
            .map(|s| s.parse())
            .collect::<std::result::Result<_, _>>()?;
        match fields[0] {
            "vad" => {
                let khz = values[0] as usize;
                let subframes = values[1] as usize;
                let rate = match khz {
                    8 => SilkRate::Khz8,
                    12 => SilkRate::Khz12,
                    _ => SilkRate::Khz16,
                };
                let mut state = import_vad(&values[8..36]);
                if state.counter == 15 {
                    assert_eq!(state, VoiceActivityDetector::default());
                    initial_vad += 1;
                }
                let length = khz * subframes * 5;
                let input: Vec<i16> = values[36..36 + length].iter().map(|&v| v as i16).collect();
                let result = state.analyze(
                    rate,
                    if subframes == 2 {
                        FrameDuration::Ms10
                    } else {
                        FrameDuration::Ms20
                    },
                    &input,
                )?;
                assert_eq!(
                    i32::from(result.probability_q8),
                    values[2],
                    "VAD probability line {}",
                    line_number + 1
                );
                assert_eq!(i32::from(result.tilt_q15), values[3]);
                assert_eq!(
                    result.quality_bands_q15,
                    values[4..8]
                        .iter()
                        .map(|&v| v as i16)
                        .collect::<Vec<_>>()
                        .as_slice()
                );
                assert_eq!(
                    vad_values(&state),
                    values[36 + length..],
                    "VAD entire state line {}",
                    line_number + 1
                );
                counts[5] += 1;
            }
            "spectrum" => {
                let khz = values[0] as usize;
                let subframes = values[1] as usize;
                let rate = match khz {
                    8 => SilkRate::Khz8,
                    12 => SilkRate::Khz12,
                    _ => SilkRate::Khz16,
                };
                let count = if khz == 16 { 16 } else { 10 };
                let length = (5 * khz + count) * subframes;
                let input: Vec<i16> = values[10..10 + length].iter().map(|&v| v as i16).collect();
                let mut previous = [0i16; 16];
                for (j, &v) in values[10 + length..10 + length + count].iter().enumerate() {
                    previous[j] = v as i16;
                }
                let mut state = SpectralState {
                    rate: if values[3] != 0 { None } else { Some(rate) },
                    previous,
                };
                let options = SpectralOptions {
                    minimum_inverse_gain_q30: values[7] as u32,
                    speech_activity_q8: values[5] as u16,
                    survivors: values[6] as u8,
                    allow_interpolation: values[2] != 0,
                    signal: match values[4] {
                        0 => SignalType::Inactive,
                        1 => SignalType::Unvoiced,
                        _ => SignalType::Voiced,
                    },
                };
                let duration = if subframes == 2 {
                    FrameDuration::Ms10
                } else {
                    FrameDuration::Ms20
                };
                let result = state.analyze(rate, duration, &input, options)?;
                assert_eq!(
                    result.interpolation_q2(),
                    values[8] as u8,
                    "spectrum interpolation line {}",
                    line_number + 1
                );
                assert_eq!(
                    result.quantized().cost_q25(),
                    values[9],
                    "spectrum cost line {}",
                    line_number + 1
                );
                let start = 10 + length + count;
                assert_eq!(
                    result.quantized().spectrum_q15(),
                    values[start..start + count]
                        .iter()
                        .map(|&v| v as i16)
                        .collect::<Vec<_>>()
                );
                assert_eq!(
                    result.quantized().indices(),
                    values[start + count..start + 2 * count + 1]
                        .iter()
                        .map(|&v| v as i8)
                        .collect::<Vec<_>>()
                );
                for h in 0..2 {
                    let start = start + 2 * count + 1 + h * count;
                    assert_eq!(
                        &result.predictors_q12()[h][..count],
                        values[start..start + count]
                            .iter()
                            .map(|&v| v as i16)
                            .collect::<Vec<_>>(),
                        "spectrum predictors line {}",
                        line_number + 1
                    );
                }
                assert_eq!(state.rate, Some(rate));
                assert_eq!(&state.previous[..count], result.quantized().spectrum_q15());
                counts[4] += 1;
            }
            "gain" => {
                let count = values[1] as usize;
                let mut state = GainQuantizer {
                    previous: values[3] as u8,
                };
                if values[0] != 0 {
                    assert_eq!(state, GainQuantizer::default());
                }
                let duration = if count == 2 {
                    FrameDuration::Ms10
                } else {
                    FrameDuration::Ms20
                };
                let result = state.quantize(duration, &values[5..5 + count], values[2] != 0)?;
                assert_eq!(
                    result.indices(),
                    values[5 + count..5 + 2 * count]
                        .iter()
                        .map(|&v| v as u8)
                        .collect::<Vec<_>>(),
                    "gain indices line {}",
                    line_number + 1
                );
                assert_eq!(
                    result.gains_q16(),
                    &values[5 + 2 * count..],
                    "gain output line {}",
                    line_number + 1
                );
                assert_eq!(state.previous_index(), values[4] as u8);
                counts[0] += 1;
            }
            "nlsf" => {
                let count = values[0] as usize;
                let spectrum: Vec<i16> = values[5..5 + count].iter().map(|&v| v as i16).collect();
                let weights: Vec<i16> = values[5 + count..5 + 2 * count]
                    .iter()
                    .map(|&v| v as i16)
                    .collect();
                let signal = match values[1] {
                    0 => SignalType::Inactive,
                    1 => SignalType::Unvoiced,
                    _ => SignalType::Voiced,
                };
                let result = quantize_nlsf(
                    order(count),
                    &spectrum,
                    &weights,
                    values[2] as u16,
                    values[3] as usize,
                    signal,
                )?;
                assert_eq!(
                    result.cost_q25(),
                    values[4],
                    "spectral cost line {}",
                    line_number + 1
                );
                assert_eq!(
                    result.indices(),
                    values[5 + 2 * count..6 + 3 * count]
                        .iter()
                        .map(|&v| v as i8)
                        .collect::<Vec<_>>(),
                    "spectral path line {}",
                    line_number + 1
                );
                assert_eq!(
                    result.spectrum_q15(),
                    values[6 + 3 * count..6 + 4 * count]
                        .iter()
                        .map(|&v| v as i16)
                        .collect::<Vec<_>>(),
                    "spectrum line {}",
                    line_number + 1
                );
                assert_eq!(
                    &laroia_weights(order(count), &spectrum)?[..count],
                    values[6 + 4 * count..]
                        .iter()
                        .map(|&v| v as i16)
                        .collect::<Vec<_>>()
                );
                counts[1] += 1;
            }
            "burg" => {
                let count = values[0] as usize;
                let length = values[1] as usize;
                let subframes = values[2] as usize;
                let khz = (length - count) / 5;
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
                let n = length * subframes;
                let pcm: Vec<i16> = values[6..6 + n].iter().map(|&v| v as i16).collect();
                let result = analyze_lpc(rate, duration, &pcm, values[3] as u32)?;
                assert_eq!(
                    result.residual_energy(),
                    (values[4], values[5]),
                    "Burg energy line {}",
                    line_number + 1
                );
                assert_eq!(
                    result.coefficients_q16(),
                    &values[6 + n..],
                    "Burg coefficients line {}",
                    line_number + 1
                );
                result.frequencies_q15()?;
                counts[2] += 1;
            }
            "root" => {
                let count = values[0] as usize;
                let result = lpc_to_nlsf(order(count), &values[1..1 + count])?;
                recovered_roots +=
                    usize::from(values[1..1 + count] != values[1 + count..1 + 2 * count]);
                assert_eq!(
                    &result[..count],
                    values[1 + 2 * count..]
                        .iter()
                        .map(|&v| v as i16)
                        .collect::<Vec<_>>(),
                    "LPC roots line {}",
                    line_number + 1
                );
                counts[3] += 1;
            }
            _ => return Err("unknown analysis fixture".into()),
        }
    }
    assert_eq!(counts, [72, 72, 72, 80, 72, 72]);
    // Negative-pole probes require bandwidth expansion. The reference maps
    // the positive unstable poles directly to a zero first frequency.
    assert_eq!(recovered_roots, 2);
    assert_eq!(initial_vad, 6);
    Ok(())
}
#[test]
fn analysis_validation_gain_reset_and_quantized_spectral_lpc_are_bounded() -> Result<()> {
    let mut gain = GainQuantizer::default();
    gain.quantize(FrameDuration::Ms10, &[1000000, 2000000], false)?;
    let saved = gain.clone();
    assert_eq!(
        gain.quantize(FrameDuration::Ms10, &[1], false),
        Err(AnalysisError::InvalidLength)
    );
    assert_eq!(
        gain.quantize(FrameDuration::Ms10, &[1, 0], true),
        Err(AnalysisError::InvalidParameters)
    );
    assert_eq!(gain, saved);
    gain.reset();
    assert_eq!(gain, GainQuantizer::default());
    let spectrum: Vec<i16> = (1..=16).map(|i| (i * 32768 / 17) as i16).collect();
    let weights = laroia_weights(NlsfOrder::Order16, &spectrum)?;
    let result = quantize_nlsf(
        NlsfOrder::Order16,
        &spectrum,
        &weights,
        3000,
        8,
        SignalType::Unvoiced,
    )?;
    assert_eq!(result.spectrum_q15().len(), 16);
    assert!(result.spectrum_q15().windows(2).all(|w| w[0] < w[1]));
    assert!(result.predictor_q12().iter().any(|&v| v != 0));
    for (mu, survivors) in [(32768, 8), (1000, 0), (1000, 33)] {
        assert_eq!(
            quantize_nlsf(
                NlsfOrder::Order16,
                &spectrum,
                &weights,
                mu,
                survivors,
                SignalType::Voiced
            ),
            Err(AnalysisError::InvalidParameters)
        );
    }
    assert_eq!(
        analyze_lpc(SilkRate::Khz16, FrameDuration::Ms20, &[0; 383], 1),
        Err(AnalysisError::InvalidLength)
    );
    assert_eq!(
        analyze_lpc(SilkRate::Khz16, FrameDuration::Ms20, &[0; 384], 0),
        Err(AnalysisError::InvalidParameters)
    );
    assert_eq!(
        analyze_lpc(SilkRate::Khz16, FrameDuration::Ms20, &[i16::MIN; 384], 1),
        Err(AnalysisError::ArithmeticRange)
    );
    Ok(())
}

fn import_vad(v: &[i32]) -> VoiceActivityDetector {
    let mut s = VoiceActivityDetector::default();
    for j in 0..3 {
        s.filters[j].copy_from_slice(&v[j * 2..j * 2 + 2]);
    }
    s.previous_energy.copy_from_slice(&v[6..10]);
    s.ratio.copy_from_slice(&v[10..14]);
    s.highpass = v[14] as i16;
    s.noise.copy_from_slice(&v[15..19]);
    s.inverse_noise.copy_from_slice(&v[19..23]);
    s.bias.copy_from_slice(&v[23..27]);
    s.counter = v[27];
    s
}
fn vad_values(s: &VoiceActivityDetector) -> Vec<i32> {
    s.filters
        .iter()
        .flatten()
        .copied()
        .chain(s.previous_energy)
        .chain(s.ratio)
        .chain([i32::from(s.highpass)])
        .chain(s.noise)
        .chain(s.inverse_noise)
        .chain(s.bias)
        .chain([s.counter])
        .collect()
}

#[test]
fn spectral_and_vad_lifecycle_validation_and_noise_warmup() -> Result<()> {
    let options = SpectralOptions {
        minimum_inverse_gain_q30: 1 << 20,
        speech_activity_q8: 128,
        survivors: 8,
        allow_interpolation: true,
        signal: SignalType::Unvoiced,
    };
    let mut spectral = SpectralState::default();
    spectral.analyze(SilkRate::Khz16, FrameDuration::Ms20, &[0; 384], options)?;
    let before = spectral.clone();
    assert_eq!(
        spectral.analyze(
            SilkRate::Khz16,
            FrameDuration::Ms20,
            &[0; 384],
            SpectralOptions {
                speech_activity_q8: 257,
                ..options
            }
        ),
        Err(AnalysisError::InvalidParameters)
    );
    assert_eq!(
        spectral.analyze(
            SilkRate::Khz16,
            FrameDuration::Ms20,
            &[i16::MIN; 384],
            options
        ),
        Err(AnalysisError::ArithmeticRange)
    );
    assert_eq!(spectral, before);
    spectral.reset();
    assert_eq!(spectral, SpectralState::default());
    let first = spectral.analyze(SilkRate::Khz8, FrameDuration::Ms20, &[0; 200], options)?;
    assert_eq!(first.interpolation_q2(), 4);
    let frame = spectral.analyze(SilkRate::Khz12, FrameDuration::Ms20, &[0; 280], options)?;
    let mut fresh = SpectralState::default();
    assert_eq!(
        frame,
        fresh.analyze(SilkRate::Khz12, FrameDuration::Ms20, &[0; 280], options)?
    );
    assert_eq!(frame.interpolation_q2(), 4);
    assert_eq!(spectral, fresh);
    let mut vad = VoiceActivityDetector::default();
    for _ in 0..1020 {
        let result = vad.analyze(SilkRate::Khz16, FrameDuration::Ms20, &[0; 320])?;
        assert!(result.probability_q8 < 8);
    }
    assert_eq!(vad.counter, 1000);
    assert!(vad.noise.iter().all(|&v| v > 0));
    assert!(vad.inverse_noise.iter().all(|&v| v > 0));
    for input in [[i16::MIN; 320], [i16::MAX; 320]] {
        vad.analyze(SilkRate::Khz16, FrameDuration::Ms20, &input)?;
    }
    let saved = vad.clone();
    assert_eq!(
        vad.analyze(SilkRate::Khz16, FrameDuration::Ms20, &[0; 319]),
        Err(AnalysisError::InvalidLength)
    );
    assert_eq!(vad, saved);
    vad.reset();
    assert_eq!(vad, VoiceActivityDetector::default());
    Ok(())
}
