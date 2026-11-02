//! Original full-band bytes, stereo/TF resynthesis and transactional errors.
use opura::{
    celt::{
        partition::{
            self, BandControls, BandEncoding, PartitionControls, PartitionError, PartitionState,
            StereoControls,
        },
        pvq::Spread,
    },
    range::{RangeDecoder, RangeEncoder},
};
const EDGES: [usize; 22] = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 10, 12, 14, 16, 20, 24, 28, 34, 40, 48, 60, 78, 100,
];
#[test]
fn original_band_bitstreams_channels_folding_and_shared_state_match()
-> Result<(), Box<dyn std::error::Error>> {
    let mut rows = 0;
    let mut geometries = std::collections::BTreeSet::new();
    for (line_index, line) in include_str!("celt-band-encoder-fixtures.txt")
        .lines()
        .enumerate()
    {
        if line.starts_with('#') {
            continue;
        }
        let c: Vec<_> = line.split_whitespace().collect();
        assert_eq!(c[0], "band");
        let band = c[1].parse::<usize>()?;
        let lm = c[2].parse::<i8>()?;
        let blocks = c[3].parse::<usize>()?;
        let tf = c[4].parse::<i8>()?;
        let stereo = c[5] == "1";
        let controls = BandControls {
            partition: PartitionControls {
                band,
                lm,
                blocks,
                bits: c[8].parse()?,
                gain: c[9].parse()?,
                spread: [
                    Spread::None,
                    Spread::Light,
                    Spread::Normal,
                    Spread::Aggressive,
                ][c[10].parse::<usize>()?],
                fill: c[11].parse()?,
            },
            tf_change: tf,
            stereo: if stereo {
                Some(StereoControls {
                    intensity: c[6].parse()?,
                    disable_inversion: c[7] == "1",
                })
            } else {
                None
            },
        };
        let remaining = c[12].parse::<i32>()?;
        let seed = c[13].parse::<u32>()?;
        let folded = c[14] == "1";
        let options = BandEncoding {
            avoid_split_noise: c[15] == "1",
            theta_round: c[16].parse()?,
            amplitudes: [c[17].parse()?, c[18].parse()?],
        };
        let prefix_bits = c[19].parse::<u32>()?;
        let prefix_value = c[20].parse::<u32>()?;
        let data: Vec<u8> = (0..c[21].len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&c[21][i..i + 2], 16))
            .collect::<Result<_, _>>()?;
        let n = (EDGES[band + 1] - EDGES[band]) << lm;
        let values: Vec<i32> = c[22..22 + (1 + usize::from(stereo)) * n]
            .iter()
            .map(|s| s.parse())
            .collect::<Result<_, _>>()?;
        let offset = 22 + values.len();
        let fold: Option<Vec<i32>> = if folded {
            Some(
                c[offset..offset + n]
                    .iter()
                    .map(|s| s.parse())
                    .collect::<Result<_, _>>()?,
            )
        } else {
            None
        };
        let expected: Vec<i64> = c[offset + if folded { n } else { 0 }..]
            .iter()
            .map(|s| s.parse())
            .collect::<Result<_, _>>()?;
        assert_eq!(expected.len(), 7 + 3 * n);
        assert_eq!(expected[6], 0);
        let before_input = values.clone();
        let before_fold = fold.clone();
        let mut writer = RangeEncoder::new(1275)?;
        if prefix_bits > 0 {
            writer.encode_uint(prefix_value, 1 << prefix_bits)?;
        }
        let mut state = PartitionState::new(remaining, seed)?;
        let result = partition::encode_band(
            &mut writer,
            &mut state,
            controls,
            &values[..n],
            if stereo { Some(&values[n..]) } else { None },
            fold.as_deref(),
            options,
        )
        .map_err(|e| format!("line{line_index}: {e}"))?;
        assert_eq!(values, before_input);
        assert_eq!(fold, before_fold);
        assert_eq!(
            [
                i64::from(result.collapse_mask()),
                i64::from(state.remaining_bits()),
                i64::from(state.seed()),
                i64::from(writer.range()),
                i64::from(writer.tell()),
                i64::from(writer.tell_frac())
            ],
            expected[..6],
            "metadata line{line_index}"
        );
        let expected_left: Vec<i32> = expected[7..7 + n].iter().map(|&x| x as i32).collect();
        let expected_right: Vec<i32> = expected[7 + n..7 + 2 * n]
            .iter()
            .map(|&x| x as i32)
            .collect();
        let expected_fold: Vec<i32> = expected[7 + 2 * n..].iter().map(|&x| x as i32).collect();
        assert_eq!(result.left(), expected_left, "left line{line_index}");
        assert_eq!(
            result.right(),
            if stereo {
                Some(expected_right.as_slice())
            } else {
                None
            },
            "right line{line_index}"
        );
        assert_eq!(result.folding(), expected_fold, "fold line{line_index}");
        writer.shrink(data.len())?;
        assert_eq!(writer.finish()?, data, "bytes line{line_index}");
        let mut decoder = RangeDecoder::new(&data);
        if prefix_bits > 0 {
            assert_eq!(decoder.decode_uint(1 << prefix_bits)?, prefix_value);
        }
        let mut decoded_state = PartitionState::new(remaining, seed)?;
        let decoded =
            partition::decode_band(&mut decoder, &mut decoded_state, controls, fold.as_deref())?;
        assert_eq!(decoded, result, "roundtrip line{line_index}");
        assert_eq!(decoded_state, state);
        assert_eq!(
            [
                i64::from(decoder.range()),
                i64::from(decoder.tell()),
                i64::from(decoder.tell_frac())
            ],
            expected[3..6]
        );
        geometries.insert((band, lm, blocks, tf, stereo));
        rows += 1;
    }
    assert_eq!(rows, 22176);
    assert_eq!(geometries.len(), 924);
    eprintln!(
        "matched {rows} native band encodings across {} geometries",
        geometries.len()
    );
    Ok(())
}

fn controls() -> BandControls {
    BandControls {
        partition: PartitionControls {
            band: 20,
            lm: 3,
            blocks: 8,
            bits: 4096,
            gain: i32::MAX,
            spread: Spread::Normal,
            fill: 255,
        },
        tf_change: 0,
        stereo: Some(StereoControls {
            intensity: 21,
            disable_inversion: false,
        }),
    }
}
fn options() -> BandEncoding {
    BandEncoding {
        avoid_split_noise: true,
        theta_round: 0,
        amplitudes: [1 << 20; 2],
    }
}
#[test]
fn invalid_domains_and_late_capacity_failures_are_atomic() -> Result<(), Box<dyn std::error::Error>>
{
    let input = [1 << 20; 176];
    let mut writer = RangeEncoder::new(128)?;
    writer.encode_uint(173, 257)?;
    let mut state = PartitionState::new(8191, 123)?;
    let original = writer.clone();
    let before = state.clone();
    let mut invalid = Vec::new();
    for band in [21, usize::MAX] {
        let mut c = controls();
        c.partition.band = band;
        invalid.push((c, PartitionError::InvalidGeometry));
    }
    for lm in [-1, 4, i8::MAX] {
        let mut c = controls();
        c.partition.lm = lm;
        invalid.push((c, PartitionError::InvalidGeometry));
    }
    for blocks in [0, 3, 4, 16, usize::MAX] {
        let mut c = controls();
        c.partition.blocks = blocks;
        invalid.push((c, PartitionError::InvalidGeometry));
    }
    for tf in [-4, 4, i8::MIN, i8::MAX] {
        let mut c = controls();
        c.tf_change = tf;
        invalid.push((c, PartitionError::InvalidGeometry));
    }
    let mut c = controls();
    c.partition.fill = 256;
    invalid.push((c, PartitionError::InvalidGeometry));
    let mut c = controls();
    c.stereo = Some(StereoControls {
        intensity: 22,
        disable_inversion: false,
    });
    invalid.push((c, PartitionError::InvalidGeometry));
    for bits in [-1, 81601] {
        let mut c = controls();
        c.partition.bits = bits;
        invalid.push((c, PartitionError::InvalidBudget));
    }
    for gain in [-1, 0, 1, i32::MAX - 1] {
        let mut c = controls();
        c.partition.gain = gain;
        invalid.push((c, PartitionError::InvalidGain));
    }
    for (c, error) in invalid {
        assert_eq!(
            partition::encode_band(
                &mut writer,
                &mut state,
                c,
                &input,
                Some(&input),
                None,
                options()
            ),
            Err(error)
        );
        assert_eq!(writer, original);
        assert_eq!(state, before);
    }
    for bad in [
        BandEncoding {
            theta_round: -2,
            ..options()
        },
        BandEncoding {
            theta_round: 2,
            ..options()
        },
        BandEncoding {
            amplitudes: [-1, 0],
            ..options()
        },
        BandEncoding {
            amplitudes: [0, i32::MIN],
            ..options()
        },
    ] {
        assert_eq!(
            partition::encode_band(
                &mut writer,
                &mut state,
                controls(),
                &input,
                Some(&input),
                None,
                bad
            ),
            Err(PartitionError::InvalidInput)
        );
    }
    for vector in [&[][..], &input[..175], &[i32::MIN; 176], &[i32::MAX; 176]] {
        assert_eq!(
            partition::encode_band(
                &mut writer,
                &mut state,
                controls(),
                vector,
                Some(&input),
                None,
                options()
            ),
            Err(PartitionError::InvalidInput)
        );
        assert_eq!(
            partition::encode_band(
                &mut writer,
                &mut state,
                controls(),
                &input,
                Some(vector),
                None,
                options()
            ),
            Err(PartitionError::InvalidInput)
        );
    }
    assert_eq!(
        partition::encode_band(
            &mut writer,
            &mut state,
            controls(),
            &input,
            None,
            None,
            options()
        ),
        Err(PartitionError::InvalidInput)
    );
    let mono = BandControls {
        stereo: None,
        ..controls()
    };
    assert_eq!(
        partition::encode_band(
            &mut writer,
            &mut state,
            mono,
            &input,
            Some(&input),
            None,
            options()
        ),
        Err(PartitionError::InvalidInput)
    );
    for fold in [&[0; 175][..], &[i32::MIN; 176], &[i32::MAX; 176]] {
        assert_eq!(
            partition::encode_band(
                &mut writer,
                &mut state,
                controls(),
                &input,
                Some(&input),
                Some(fold),
                options()
            ),
            Err(PartitionError::InvalidFold)
        );
    }
    assert_eq!(writer, original);
    assert_eq!(state, before);
    let mut boundary = [0; 176];
    boundary[0] = 1 << 24;
    boundary[1] = 1 << 23;
    let silent = BandControls {
        partition: PartitionControls {
            bits: 0,
            fill: 0,
            ..mono.partition
        },
        ..mono
    };
    partition::encode_band(
        &mut writer,
        &mut state,
        silent,
        &boundary,
        None,
        None,
        options(),
    )?;
    assert_eq!(writer, original);
    assert_eq!(state, before);
    boundary[1] += 1;
    assert_eq!(
        partition::encode_band(
            &mut writer,
            &mut state,
            silent,
            &boundary,
            None,
            None,
            options()
        ),
        Err(PartitionError::InvalidInput)
    );
    // Failure occurs after theta coding and shared budget deductions, not validation.
    let mut tight = RangeEncoder::new(2)?;
    let tight_before = tight.clone();
    let failure = partition::encode_band(
        &mut tight,
        &mut state,
        controls(),
        &input,
        Some(&input),
        None,
        options(),
    );
    assert!(
        matches!(
            failure,
            Err(PartitionError::Encoding(
                opura::range::EncoderError::CapacityExhausted
            )) | Err(PartitionError::Pvq(opura::celt::pvq::PvqError::Pulses(
                opura::celt::pulses::PulseError::Encoding(
                    opura::range::EncoderError::CapacityExhausted
                )
            )))
        ),
        "{failure:?}"
    );
    assert_eq!(tight, tight_before);
    assert_eq!(state, before);
    writer.finish()?;
    let finished = writer.clone();
    assert_eq!(
        partition::encode_band(
            &mut writer,
            &mut state,
            controls(),
            &input,
            Some(&input),
            None,
            options()
        ),
        Err(PartitionError::Encoding(
            opura::range::EncoderError::Finished
        ))
    );
    assert_eq!(writer, finished);
    assert_eq!(state, before);
    Ok(())
}

#[test]
fn pcm_analysis_bands_encode_and_decode_at_all_native_rates_and_durations()
-> Result<(), Box<dyn std::error::Error>> {
    use opura::celt::{
        FrameSize, analysis,
        input::{InputRate, PreemphasisState},
    };
    let sizes = [
        FrameSize::Ms2_5,
        FrameSize::Ms5,
        FrameSize::Ms10,
        FrameSize::Ms20,
    ];
    let mut rows = 0;
    for rate in [
        InputRate::Khz8,
        InputRate::Khz12,
        InputRate::Khz16,
        InputRate::Khz24,
        InputRate::Khz48,
    ] {
        for (lm, size) in sizes.into_iter().enumerate() {
            let n = size.samples();
            let count = n / rate.factor();
            for pattern in 0..4 {
                let pcm: Vec<i16> = (0..2 * count)
                    .map(|i| match pattern {
                        0 => 0,
                        1 => {
                            if i % 2 == 0 {
                                32767
                            } else {
                                -32768
                            }
                        }
                        2 => (i as i16).wrapping_mul(7659).wrapping_add(12345),
                        _ => {
                            if i % 17 == 0 {
                                1
                            } else {
                                0
                            }
                        }
                    })
                    .collect();
                let mut context = vec![0; 2 * (n + 120)];
                for channel in 0..2 {
                    PreemphasisState::default().process(
                        size,
                        rate,
                        &pcm,
                        2,
                        channel,
                        &mut context[channel * (n + 120) + 120..(channel + 1) * (n + 120)],
                    )?;
                }
                for short in [false, true] {
                    for channels in [1, 2] {
                        let spectrum =
                            analysis::analyze_frame(size, &context, 2, channels, rate, short)?;
                        let bands =
                            analysis::analyze_bands(size, spectrum.values(), channels, 21, 21)?;
                        let left = bands.normalized(0).ok_or("missing left")?;
                        let right = bands.normalized(1);
                        for band in 0..21 {
                            let start = EDGES[band] << lm;
                            let end = EDGES[band + 1] << lm;
                            let blocks = if short { 1 << lm } else { 1 };
                            let c = BandControls {
                                partition: PartitionControls {
                                    band,
                                    lm: lm as i8,
                                    blocks,
                                    bits: 32 + band as i32 * 16,
                                    gain: i32::MAX,
                                    spread: Spread::Normal,
                                    fill: ((1 << blocks) - 1) as u16,
                                },
                                tf_change: if short && lm > 0 {
                                    1
                                } else if lm >= 2 {
                                    -2
                                } else {
                                    -1
                                },
                                stereo: if channels == 2 {
                                    Some(StereoControls {
                                        intensity: 18,
                                        disable_inversion: false,
                                    })
                                } else {
                                    None
                                },
                            };
                            let options = BandEncoding {
                                amplitudes: [
                                    bands.amplitudes(0).ok_or("missing amplitudes")?[band],
                                    bands.amplitudes(1).map_or(0, |a| a[band]),
                                ],
                                ..options()
                            };
                            let mut writer = RangeEncoder::new(128)?;
                            let mut state = PartitionState::new(8191, 12345)?;
                            let result = partition::encode_band(
                                &mut writer,
                                &mut state,
                                c,
                                &left[start..end],
                                right.map(|r| &r[start..end]),
                                None,
                                options,
                            )?;
                            let len = writer.tell().div_ceil(8).max(2) as usize;
                            writer.shrink(len)?;
                            let bytes = writer.finish()?;
                            let mut decoder = RangeDecoder::new(bytes);
                            let mut shared = PartitionState::new(8191, 12345)?;
                            assert_eq!(
                                partition::decode_band(&mut decoder, &mut shared, c, None)?,
                                result
                            );
                            assert_eq!(shared, state);
                            assert_eq!(decoder.range(), writer.range());
                            rows += 1;
                        }
                    }
                }
            }
        }
    }
    assert_eq!(rows, 6720);
    Ok(())
}
