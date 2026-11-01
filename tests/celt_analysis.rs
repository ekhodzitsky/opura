//! Original complete CELT channel/subframe spectra and analysis composition.
use opura::celt::{
    FrameSize,
    analysis::{self, AnalysisError},
    input::{InputRate, PreemphasisState},
    mdct,
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
fn every_original_complete_spectrum_matches_across_all_configurations()
-> Result<(), Box<dyn std::error::Error>> {
    let mut geometry = [[[[false; 2]; 3]; 5]; 4];
    let mut rows = 0;
    let mut separate_rounding = false;
    for (index, line) in include_str!("celt-analysis-fixtures.txt")
        .lines()
        .enumerate()
    {
        if line.starts_with('#') {
            continue;
        }
        let cols: Vec<_> = line.split_whitespace().collect();
        assert_eq!(cols[0], "frame");
        let lm = cols[1].parse::<usize>()?;
        let factor = cols[2].parse::<usize>()?;
        let cc = cols[3].parse::<usize>()?;
        let channels = cols[4].parse::<usize>()?;
        let short = cols[5].parse::<usize>()?;
        let rate_index = RATES
            .iter()
            .position(|r| r.factor() == factor)
            .ok_or("invalid rate")?;
        let n = SIZES[lm].samples();
        let count = cc * (n + 120);
        let input: Vec<i32> = cols[6..6 + count]
            .iter()
            .map(|s| s.parse())
            .collect::<Result<_, _>>()?;
        let expected: Vec<i32> = cols[6 + count..]
            .iter()
            .map(|s| s.parse())
            .collect::<Result<_, _>>()?;
        assert_eq!(expected.len(), channels * n);
        let before = input.clone();
        let result = analysis::analyze_frame(
            SIZES[lm],
            &input,
            cc,
            channels,
            RATES[rate_index],
            short != 0,
        )?;
        assert_eq!(result.values(), expected, "row{index}");
        assert_eq!(input, before);
        assert_eq!(result.size(), SIZES[lm]);
        assert_eq!(result.channels(), channels);
        for c in 0..channels {
            assert_eq!(result.channel(c), Some(&expected[c * n..(c + 1) * n]));
        }
        assert_eq!(result.channel(channels), None);
        assert_eq!(result.channel(usize::MAX), None);
        if cc == 2 && channels == 1 && factor == 1 {
            let stereo =
                analysis::analyze_frame(SIZES[lm], &input, 2, 2, RATES[rate_index], short != 0)?;
            separate_rounding |= (0..n).any(|i| {
                result.values()[i] != (stereo.values()[i].wrapping_add(stereo.values()[n + i]) >> 1)
            });
        }
        let layout = match (cc, channels) {
            (1, 1) => 0,
            (2, 1) => 1,
            (2, 2) => 2,
            _ => panic!("invalid fixture layout"),
        };
        geometry[lm][rate_index][layout][short] = true;
        rows += 1;
    }
    assert_eq!(rows, 1080);
    assert!(geometry.iter().flatten().flatten().flatten().all(|&v| v));
    assert!(separate_rounding);
    Ok(())
}

#[test]
fn malformed_context_and_channel_domains_are_rejected_without_input_changes() {
    for size in SIZES {
        for (input_channels, coded_channels, length) in [
            (0, 0, 0),
            (0, 1, 0),
            (3, 1, 0),
            (usize::MAX, 1, 0),
            (1, 0, 0),
            (1, 2, size.samples() + 120),
            (2, 3, 0),
            (2, usize::MAX, 0),
            (1, 1, size.samples() + 119),
            (1, 1, size.samples() + 121),
            (2, 1, 2 * (size.samples() + 120) - 1),
            (2, 2, 2 * (size.samples() + 120) + 1),
        ] {
            let input = vec![123; length];
            let before = input.clone();
            assert_eq!(
                analysis::analyze_frame(
                    size,
                    &input,
                    input_channels,
                    coded_channels,
                    InputRate::Khz48,
                    false
                ),
                Err(AnalysisError::InvalidGeometry)
            );
            assert_eq!(input, before);
        }
    }
}

#[test]
fn pcm_preprocessing_context_and_spectral_composition_agree()
-> Result<(), Box<dyn std::error::Error>> {
    for rate in RATES {
        for size in SIZES {
            let n = size.samples();
            let count = n / rate.factor();
            let mut states = [PreemphasisState::default(), PreemphasisState::default()];
            let mut overlap = [[0; 120]; 2];
            for frame in 0..3 {
                let pcm: Vec<i16> = (0..count * 2)
                    .map(|i| (i as i16).wrapping_mul(9753).wrapping_add(frame * 12345))
                    .collect();
                let mut context = vec![0; 2 * (n + 120)];
                for channel in 0..2 {
                    let start = channel * (n + 120);
                    context[start..start + 120].copy_from_slice(&overlap[channel]);
                    states[channel].process(
                        size,
                        rate,
                        &pcm,
                        2,
                        channel,
                        &mut context[start + 120..start + 120 + n],
                    )?;
                    overlap[channel].copy_from_slice(&context[start + n..start + n + 120]);
                }
                for short in [false, true] {
                    let result = analysis::analyze_frame(size, &context, 2, 2, rate, short)?;
                    let mut expected = vec![0; 2 * n];
                    let blocks = if short { n / 120 } else { 1 };
                    let part = if short { FrameSize::Ms2_5 } else { size };
                    for channel in 0..2 {
                        for b in 0..blocks {
                            let start = channel * (n + 120) + b * part.samples();
                            let mut bins = vec![0; part.samples()];
                            mdct::analyze(
                                part,
                                &context[start..start + part.samples() + 120],
                                &mut bins,
                                1,
                            )?;
                            for (i, &value) in bins.iter().enumerate() {
                                expected[channel * n + i * blocks + b] = value;
                            }
                        }
                        for i in 0..n {
                            expected[channel * n + i] = if i < n / rate.factor() {
                                expected[channel * n + i].wrapping_mul(rate.factor() as i32)
                            } else {
                                0
                            };
                        }
                    }
                    assert_eq!(result.values(), expected);
                    let mono = analysis::analyze_frame(size, &context, 2, 1, rate, short)?;
                    assert_eq!(mono.values().len(), n);
                    assert!(mono.values()[n / rate.factor()..].iter().all(|&v| v == 0));
                }
            }
        }
    }
    Ok(())
}
