//! Stateful CELT packet API and independently retained whole-codec evidence.
use opura::{
    celt::{
        FrameSize,
        encoder::{CeltEncoder, EncoderError},
        input::InputRate,
    },
    decoder::OpusDecoder,
    silk::{decoder::PcmChannels, resampler::OutputRate},
};
const SIZES: [FrameSize; 4] = [
    FrameSize::Ms2_5,
    FrameSize::Ms5,
    FrameSize::Ms10,
    FrameSize::Ms20,
];
#[test]
fn public_pcm_packets_are_decodable_and_failures_preserve_whole_state()
-> Result<(), Box<dyn std::error::Error>> {
    let mut successes = 0;
    let mut rejected = 0;
    for rate in [
        InputRate::Khz8,
        InputRate::Khz12,
        InputRate::Khz16,
        InputRate::Khz24,
        InputRate::Khz48,
    ] {
        for channels in [1, 2] {
            for budget in [2, 3, 4, 5, 6, 7, 8, 12, 16, 24, 32, 64, 128, 256, 512, 1275] {
                let mut encoder = CeltEncoder::new(rate, channels)?;
                let mut decoder = OpusDecoder::new(
                    OutputRate::Khz48,
                    if channels == 1 {
                        PcmChannels::Mono
                    } else {
                        PcmChannels::Stereo
                    },
                )?;
                for frame in 0..16 {
                    let size = SIZES[frame % 4];
                    let n = size.samples();
                    let input: Vec<i16> = (0..n / rate.factor() * channels)
                        .map(|i| match frame % 4 {
                            0 => 0,
                            1 => {
                                if i % 2 == 0 {
                                    32767
                                } else {
                                    -32768
                                }
                            }
                            2 => (i as i16).wrapping_mul(7659).wrapping_add(frame as i16),
                            _ => {
                                if i % 17 == 0 {
                                    1
                                } else {
                                    0
                                }
                            }
                        })
                        .collect();
                    let original = input.clone();
                    let before = encoder.clone();
                    match encoder.encode(size, &input, budget) {
                        Ok(packet) => {
                            assert_eq!(packet.as_bytes().len(), budget + 1);
                            assert_eq!(packet.frame_size(), size);
                            assert_eq!(packet.channels(), channels);
                            assert_eq!(encoder.final_range(), packet.final_range());
                            let pcm = decoder.decode(packet.as_bytes()).map_err(|e| {
                                format!(
                                    "rate{} C{channels} budget{budget} frame{frame}: {e}",
                                    rate.hz()
                                )
                            })?;
                            assert_eq!(pcm.samples_per_channel(), n);
                            assert_eq!(pcm.channels(), channels);
                            assert_eq!(pcm.final_range(), packet.final_range());
                            successes += 1;
                        }
                        Err(error) => {
                            assert_eq!(encoder, before);
                            match error{EncoderError::BudgetExhausted|EncoderError::Entropy(opura::range::EncoderError::CapacityExhausted)|EncoderError::Energy(opura::celt::energy::EnergyEncodingError::Entropy(opura::range::EncoderError::CapacityExhausted))=>{},_=>return Err(format!("unexpected rate{} C{channels} budget{budget} frame{frame}: {error}",rate.hz()).into())}
                            rejected += 1;
                        }
                    }
                    assert_eq!(input, original);
                }
            }
        }
    }
    eprintln!("packets{successes}, capacity-rejected{rejected}");
    assert_eq!((successes, rejected), (2560, 0));
    Ok(())
}

#[test]
fn original_policy_packets_and_public_libopus_pcm_match_at_every_output_geometry()
-> Result<(), Box<dyn std::error::Error>> {
    verify_public_packets(
        include_str!("celt-encoder-fixtures.txt"),
        false,
        false,
        false,
        false,
    )
}
#[test]
fn original_transient_packets_and_public_libopus_pcm_match_at_every_output_geometry()
-> Result<(), Box<dyn std::error::Error>> {
    verify_public_packets(
        include_str!("celt-encoder-transient-fixtures.txt"),
        true,
        false,
        false,
        false,
    )
}
#[test]
fn original_tf_packets_and_public_libopus_pcm_match_at_every_output_geometry()
-> Result<(), Box<dyn std::error::Error>> {
    verify_public_packets(
        include_str!("celt-encoder-tf-fixtures.txt"),
        true,
        true,
        false,
        false,
    )
}
#[test]
fn original_dynamic_packets_and_public_libopus_pcm_match_at_every_output_geometry()
-> Result<(), Box<dyn std::error::Error>> {
    verify_public_packets(
        include_str!("celt-encoder-dynalloc-fixtures.txt"),
        true,
        true,
        true,
        false,
    )
}
#[test]
fn original_spreading_packets_and_public_libopus_pcm_match_at_every_output_geometry()
-> Result<(), Box<dyn std::error::Error>> {
    verify_public_packets(
        include_str!("celt-encoder-spreading-fixtures.txt"),
        true,
        true,
        true,
        true,
    )
}
fn verify_public_packets(
    fixtures: &str,
    detect: bool,
    tf_analysis: bool,
    dynamic_analysis: bool,
    spreading_corpus: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    verify_public_policy_packets(
        fixtures,
        detect,
        tf_analysis,
        dynamic_analysis,
        spreading_corpus,
        false,
    )
}
#[test]
fn original_allocation_packets_and_public_libopus_pcm_match_at_every_output_geometry()
-> Result<(), Box<dyn std::error::Error>> {
    verify_public_policy_packets(
        include_str!("celt-encoder-allocation-fixtures.txt"),
        true,
        true,
        true,
        true,
        true,
    )
}
fn verify_public_policy_packets(
    fixtures: &str,
    detect: bool,
    tf_analysis: bool,
    dynamic_analysis: bool,
    spreading_corpus: bool,
    allocation_corpus: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    verify_full_public_packets(
        fixtures,
        detect,
        tf_analysis,
        dynamic_analysis,
        spreading_corpus,
        allocation_corpus,
        false,
    )
}
#[test]
fn original_pitch_packets_and_public_libopus_pcm_match_at_every_output_geometry()
-> Result<(), Box<dyn std::error::Error>> {
    verify_full_public_packets(
        include_str!("celt-encoder-pitch-packets-fixtures.txt"),
        true,
        true,
        true,
        true,
        true,
        true,
    )
}
fn verify_full_public_packets(
    fixtures: &str,
    detect: bool,
    tf_analysis: bool,
    dynamic_analysis: bool,
    spreading_corpus: bool,
    allocation_corpus: bool,
    pitch_corpus: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    use opura::Packet;
    let rates = [
        OutputRate::Khz8,
        OutputRate::Khz12,
        OutputRate::Khz16,
        OutputRate::Khz24,
        OutputRate::Khz48,
    ];
    let mut encoder = CeltEncoder::new(InputRate::Khz48, 1)?;
    encoder.set_transient_detection(detect);
    encoder.set_tf_analysis(tf_analysis);
    encoder.set_dynamic_allocation(dynamic_analysis);
    encoder.set_spreading_analysis(spreading_corpus);
    let mut decoders: Vec<_> = rates
        .into_iter()
        .flat_map(|rate| {
            [PcmChannels::Mono, PcmChannels::Stereo]
                .into_iter()
                .map(move |channels| (rate, channels))
        })
        .map(|(rate, channels)| OpusDecoder::new(rate, channels))
        .collect::<Result<_, _>>()?;
    let mut lines = fixtures.lines().filter(|l| !l.starts_with('#'));
    let mut frames = 0;
    let mut decoded = 0;
    while let Some(line) = lines.next() {
        let f: Vec<_> = line.split_whitespace().collect();
        assert_eq!(f[0], "frame");
        let (
            detect,
            tf_analysis,
            dynamic_analysis,
            spreading_enabled,
            allocation_enabled,
            pitch_enabled,
        ) = if spreading_corpus || allocation_corpus || pitch_corpus {
            let flags = f[7].parse::<u8>()?;
            (
                flags & 1 != 0,
                flags & 2 != 0,
                flags & 4 != 0,
                flags & 8 != 0,
                flags & 16 != 0,
                flags & 32 != 0,
            )
        } else {
            (detect, tf_analysis, dynamic_analysis, false, false, false)
        };

        let reset = f[1] == "1";
        let rate = match f[2] {
            "8000" => InputRate::Khz8,
            "12000" => InputRate::Khz12,
            "16000" => InputRate::Khz16,
            "24000" => InputRate::Khz24,
            "48000" => InputRate::Khz48,
            _ => return Err("input rate".into()),
        };
        let channels = f[3].parse::<usize>()?;
        let size = SIZES[f[4].parse::<usize>()?];
        let budget = f[5].parse::<usize>()?;
        let expected: Vec<u8> = (0..f[6].len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&f[6][i..i + 2], 16))
            .collect::<Result<_, _>>()?;
        let input: Vec<i16> = lines
            .next()
            .ok_or("input")?
            .split_whitespace()
            .skip(1)
            .map(|s| s.parse())
            .collect::<Result<_, _>>()?;
        let state: Vec<i64> = lines
            .next()
            .ok_or("state")?
            .split_whitespace()
            .skip(1)
            .map(|s| s.parse())
            .collect::<Result<_, _>>()?;
        if encoder.input_rate() != rate || encoder.channels() != channels {
            encoder = CeltEncoder::new(rate, channels)?;
            encoder.set_transient_detection(detect);
            encoder.set_tf_analysis(tf_analysis);
            encoder.set_dynamic_allocation(dynamic_analysis);
            encoder.set_spreading_analysis(spreading_corpus);
        } else if reset {
            encoder.reset();
        }
        if reset {
            for decoder in &mut decoders {
                decoder.reset();
            }
        }
        encoder.set_transient_detection(detect);
        encoder.set_tf_analysis(tf_analysis);
        encoder.set_dynamic_allocation(dynamic_analysis);
        encoder.set_spreading_analysis(spreading_enabled);
        encoder.set_allocation_analysis(allocation_enabled);
        encoder.set_pitch_prefilter(pitch_enabled);
        let packet = encoder.encode(size, &input, budget)?;
        assert_eq!(packet.as_bytes(), expected, "frame{frames}");
        assert_eq!(i64::from(packet.final_range()), state[0]);
        let parsed = Packet::parse(packet.as_bytes())?;
        assert_eq!(parsed.frame_samples(), size.samples() as u16);
        assert_eq!(usize::from(parsed.channels()), channels);
        assert_eq!(parsed.frames().len(), 1);
        for decoder in &mut decoders {
            let c: Vec<_> = lines.next().ok_or("decoded")?.split_whitespace().collect();
            assert_eq!(c[0], "decoded");
            let output = c[1].parse::<u32>()?;
            let api = c[2].parse::<usize>()?;
            let count = c[3].parse::<usize>()?;
            let range = c[4].parse::<u32>()?;
            assert_eq!(decoder.output_rate().hz(), output);
            assert_eq!(decoder.output_channels().count(), api);
            let result = decoder.decode(packet.as_bytes())?;
            assert_eq!(result.samples_per_channel(), count);
            assert_eq!(result.final_range(), range);
            let pcm: Vec<i16> = c[47..]
                .iter()
                .map(|s| s.parse())
                .collect::<Result<_, _>>()?;
            assert_eq!(
                result.samples(),
                pcm,
                "native PCM frame{frames} rate{output} api{api}"
            );
            for ch in 0..2 {
                let energy = decoder
                    .celt()
                    .energy()
                    .channel(ch)
                    .ok_or("decoder energy")?;
                for i in 0..21 {
                    assert_eq!(
                        energy[i],
                        c[5 + ch * 21 + i].parse::<i32>()?,
                        "decoder history frame{frames}"
                    );
                }
            }
            decoded += 1;
        }
        frames += 1;
    }
    assert_eq!(
        (frames, decoded),
        if pitch_corpus {
            (1920, 19200)
        } else if allocation_corpus {
            (1440, 14400)
        } else if detect {
            (960, 9600)
        } else {
            (720, 7200)
        }
    );
    Ok(())
}

#[test]
fn invalid_geometry_and_reset_preserve_or_clear_the_complete_state()
-> Result<(), Box<dyn std::error::Error>> {
    for channels in [0, 3, usize::MAX] {
        assert_eq!(
            CeltEncoder::new(InputRate::Khz48, channels),
            Err(EncoderError::InvalidChannels)
        );
    }
    for rate in [
        InputRate::Khz8,
        InputRate::Khz12,
        InputRate::Khz16,
        InputRate::Khz24,
        InputRate::Khz48,
    ] {
        for channels in [1, 2] {
            let mut encoder = CeltEncoder::new(rate, channels)?;
            let fresh = encoder.clone();
            let size = FrameSize::Ms20;
            let length = size.samples() / rate.factor() * channels;
            let samples = vec![12345; length];
            encoder.encode(size, &samples, 128)?;
            assert_ne!(encoder, fresh);
            let before = encoder.clone();
            for bytes in [0, 1, 1276, usize::MAX] {
                assert_eq!(
                    encoder.encode(size, &samples, bytes),
                    Err(EncoderError::InvalidBudget)
                );
                assert_eq!(encoder, before);
            }
            for input in [&[][..], &samples[..length - 1], &vec![0; length + 1][..]] {
                assert_eq!(
                    encoder.encode(size, input, 128),
                    Err(EncoderError::InvalidInput)
                );
                assert_eq!(encoder, before);
            }
            encoder.reset();
            assert_eq!(encoder, fresh);
            assert_eq!(encoder.final_range(), 0);
            assert_eq!(encoder.input_rate(), rate);
            assert_eq!(encoder.channels(), channels);
            let mut second = fresh;
            assert_eq!(
                encoder.encode(size, &samples, 128)?,
                second.encode(size, &samples, 128)?
            );
            assert_eq!(encoder, second);
        }
    }
    Ok(())
}

#[test]
fn transient_setting_is_opt_in_continuous_and_retained_by_reset()
-> Result<(), Box<dyn std::error::Error>> {
    use opura::{celt::control, range::RangeDecoder};
    let mut encoder = CeltEncoder::new(InputRate::Khz48, 2)?;
    assert!(!encoder.transient_detection());
    let mut input = vec![0; 1920];
    input[960] = 32767;
    input[961] = -32768;
    let baseline = encoder.encode(FrameSize::Ms20, &input, 128)?;
    assert!(
        !control::decode_prefix(
            &mut RangeDecoder::new(&baseline.as_bytes()[1..]),
            FrameSize::Ms20,
            0
        )?
        .transient()
    );
    encoder.set_transient_detection(true);
    assert!(encoder.transient_detection());
    let before = encoder.clone();
    assert_eq!(
        encoder.encode(FrameSize::Ms20, &input[..1919], 128),
        Err(EncoderError::InvalidInput)
    );
    assert_eq!(encoder, before);
    for _ in 0..4 {
        let packet = encoder.encode(FrameSize::Ms20, &input, 128)?;
        assert!(
            control::decode_prefix(
                &mut RangeDecoder::new(&packet.as_bytes()[1..]),
                FrameSize::Ms20,
                0
            )?
            .transient()
        );
    }
    encoder.reset();
    let mut fresh = CeltEncoder::new(InputRate::Khz48, 2)?;
    fresh.set_transient_detection(true);
    assert_eq!(encoder, fresh);
    assert_eq!(
        encoder.encode(FrameSize::Ms20, &input, 128)?,
        fresh.encode(FrameSize::Ms20, &input, 128)?
    );
    encoder.set_transient_detection(false);
    let packet = encoder.encode(FrameSize::Ms20, &input, 128)?;
    assert!(
        !control::decode_prefix(
            &mut RangeDecoder::new(&packet.as_bytes()[1..]),
            FrameSize::Ms20,
            0
        )?
        .transient()
    );
    Ok(())
}

#[test]
fn tf_setting_is_independent_continuous_retained_and_budget_bounded()
-> Result<(), Box<dyn std::error::Error>> {
    let mut completed = 0;
    for detect in [false, true] {
        for channels in [1, 2] {
            let mut encoder = CeltEncoder::new(InputRate::Khz48, channels)?;
            assert!(!encoder.tf_analysis());
            encoder.set_transient_detection(detect);
            encoder.set_tf_analysis(true);
            let mut decoder = OpusDecoder::new(
                OutputRate::Khz48,
                if channels == 1 {
                    PcmChannels::Mono
                } else {
                    PcmChannels::Stereo
                },
            )?;
            for size in SIZES {
                let n = size.samples() * channels;
                let input: Vec<i16> = (0..n)
                    .map(|i| {
                        if i == n / 2 {
                            32767
                        } else {
                            (i as i16).wrapping_mul(7659)
                        }
                    })
                    .collect();
                for budget in [2, 14, 15, 16, 29, 30, 31, 64, 256] {
                    let packet = encoder.encode(size, &input, budget)?;
                    assert_eq!(
                        decoder.decode(packet.as_bytes())?.final_range(),
                        packet.final_range()
                    );
                    completed += 1;
                }
            }
            let before = encoder.clone();
            assert_eq!(
                encoder.encode(FrameSize::Ms20, &[], 128),
                Err(EncoderError::InvalidInput)
            );
            assert_eq!(encoder, before);
            encoder.reset();
            let mut fresh = CeltEncoder::new(InputRate::Khz48, channels)?;
            fresh.set_transient_detection(detect);
            fresh.set_tf_analysis(true);
            assert_eq!(encoder, fresh);
            let input = vec![0; 960 * channels];
            assert_eq!(
                encoder.encode(FrameSize::Ms20, &input, 128)?,
                fresh.encode(FrameSize::Ms20, &input, 128)?
            );
            encoder.set_tf_analysis(false);
            assert!(!encoder.tf_analysis());
            assert_eq!(encoder.transient_detection(), detect);
        }
    }
    assert_eq!(completed, 144);
    Ok(())
}

#[test]
fn dynamic_setting_is_independent_retained_and_safe_across_all_budget_thresholds()
-> Result<(), Box<dyn std::error::Error>> {
    let mut frames = 0;
    for detect in [false, true] {
        for tf in [false, true] {
            for channels in [1, 2] {
                let mut encoder = CeltEncoder::new(InputRate::Khz48, channels)?;
                assert!(!encoder.dynamic_allocation());
                encoder.set_transient_detection(detect);
                encoder.set_tf_analysis(tf);
                encoder.set_dynamic_allocation(true);
                let mut decoder = OpusDecoder::new(
                    OutputRate::Khz48,
                    if channels == 1 {
                        PcmChannels::Mono
                    } else {
                        PcmChannels::Stereo
                    },
                )?;
                for size in SIZES {
                    let n = size.samples() * channels;
                    let input: Vec<i16> = (0..n)
                        .map(|i| {
                            if i == n / 2 {
                                32767
                            } else {
                                (i as i16).wrapping_mul(7659)
                            }
                        })
                        .collect();
                    for budget in [
                        2, 29, 30, 31, 34, 35, 36, 39, 40, 41, 44, 45, 46, 64, 128, 256,
                    ] {
                        let packet = encoder.encode(size, &input, budget)?;
                        assert_eq!(
                            decoder.decode(packet.as_bytes())?.final_range(),
                            packet.final_range()
                        );
                        frames += 1;
                    }
                }
                let before = encoder.clone();
                assert_eq!(
                    encoder.encode(FrameSize::Ms20, &[], 128),
                    Err(EncoderError::InvalidInput)
                );
                assert_eq!(encoder, before);
                encoder.reset();
                let mut fresh = CeltEncoder::new(InputRate::Khz48, channels)?;
                fresh.set_transient_detection(detect);
                fresh.set_tf_analysis(tf);
                fresh.set_dynamic_allocation(true);
                assert_eq!(encoder, fresh);
                let input = vec![0; 960 * channels];
                assert_eq!(
                    encoder.encode(FrameSize::Ms20, &input, 128)?,
                    fresh.encode(FrameSize::Ms20, &input, 128)?
                );
                encoder.set_dynamic_allocation(false);
                assert!(!encoder.dynamic_allocation());
                assert_eq!(encoder.transient_detection(), detect);
                assert_eq!(encoder.tf_analysis(), tf);
            }
        }
    }
    assert_eq!(frames, 512);
    Ok(())
}

#[test]
fn spreading_setting_is_independent_retained_and_bounded_at_exact_byte_gates()
-> Result<(), Box<dyn std::error::Error>> {
    let mut frames = 0;
    for detect in [false, true] {
        for tf in [false, true] {
            for dynamic in [false, true] {
                for channels in [1, 2] {
                    let mut encoder = CeltEncoder::new(InputRate::Khz48, channels)?;
                    assert!(!encoder.spreading_analysis());
                    encoder.set_transient_detection(detect);
                    encoder.set_tf_analysis(tf);
                    encoder.set_dynamic_allocation(dynamic);
                    encoder.set_spreading_analysis(true);
                    let mut decoder = OpusDecoder::new(
                        OutputRate::Khz48,
                        if channels == 1 {
                            PcmChannels::Mono
                        } else {
                            PcmChannels::Stereo
                        },
                    )?;
                    for size in SIZES {
                        let n = size.samples() * channels;
                        let input: Vec<i16> = (0..n)
                            .map(|i| {
                                if i == n / 2 {
                                    32767
                                } else {
                                    (i as i16).wrapping_mul(7659)
                                }
                            })
                            .collect();
                        for budget in [2, 8, 9, 10, 11, 16, 19, 20, 21, 32, 64, 256] {
                            let packet = encoder.encode(size, &input, budget)?;
                            assert_eq!(
                                decoder.decode(packet.as_bytes())?.final_range(),
                                packet.final_range()
                            );
                            frames += 1;
                        }
                    }
                    let before = encoder.clone();
                    assert_eq!(
                        encoder.encode(FrameSize::Ms20, &[], 128),
                        Err(EncoderError::InvalidInput)
                    );
                    assert_eq!(encoder, before);
                    encoder.reset();
                    let mut fresh = CeltEncoder::new(InputRate::Khz48, channels)?;
                    fresh.set_transient_detection(detect);
                    fresh.set_tf_analysis(tf);
                    fresh.set_dynamic_allocation(dynamic);
                    fresh.set_spreading_analysis(true);
                    assert_eq!(encoder, fresh);
                    let input = vec![0; 960 * channels];
                    assert_eq!(
                        encoder.encode(FrameSize::Ms20, &input, 128)?,
                        fresh.encode(FrameSize::Ms20, &input, 128)?
                    );
                    encoder.set_spreading_analysis(false);
                    assert!(!encoder.spreading_analysis());
                    assert_eq!(encoder.transient_detection(), detect);
                    assert_eq!(encoder.tf_analysis(), tf);
                    assert_eq!(encoder.dynamic_allocation(), dynamic);
                }
            }
        }
    }
    assert_eq!(frames, 768);
    Ok(())
}

#[test]
fn allocation_setting_combinations_reset_and_small_budgets_are_transactional()
-> Result<(), Box<dyn std::error::Error>> {
    fn configure(encoder: &mut CeltEncoder, flags: u8) {
        encoder.set_transient_detection(flags & 1 != 0);
        encoder.set_tf_analysis(flags & 2 != 0);
        encoder.set_dynamic_allocation(flags & 4 != 0);
        encoder.set_spreading_analysis(flags & 8 != 0);
        encoder.set_allocation_analysis(flags & 16 != 0);
        encoder.set_pitch_prefilter(flags & 32 != 0);
    }
    let mut frames = 0;
    for flags in 0..64 {
        for rate in [InputRate::Khz8, InputRate::Khz48] {
            for channels in [1, 2] {
                let mut encoder = CeltEncoder::new(rate, channels)?;
                assert!(!encoder.allocation_analysis());
                assert!(!encoder.pitch_prefilter());
                configure(&mut encoder, flags);
                let mut decoder = OpusDecoder::new(
                    OutputRate::Khz48,
                    if channels == 1 {
                        PcmChannels::Mono
                    } else {
                        PcmChannels::Stereo
                    },
                )?;
                for size in SIZES {
                    let count = size.samples() / rate.factor() * channels;
                    let input: Vec<i16> = (0..count)
                        .map(|i| {
                            if i == count / 2 {
                                32767
                            } else {
                                (i as i16).wrapping_mul(7659)
                            }
                        })
                        .collect();
                    for budget in [2, 3, 6, 7, 15, 16, 32, 128] {
                        let before = encoder.clone();
                        assert_eq!(
                            encoder.encode(size, &[], budget),
                            Err(EncoderError::InvalidInput)
                        );
                        assert_eq!(encoder, before);
                        let packet = encoder.encode(size, &input, budget)?;
                        assert_eq!(
                            decoder.decode(packet.as_bytes())?.final_range(),
                            packet.final_range()
                        );
                        frames += 1;
                    }
                }
                let before = encoder.clone();
                assert_eq!(
                    encoder.encode(FrameSize::Ms20, &[], 1276),
                    Err(EncoderError::InvalidBudget)
                );
                assert_eq!(encoder, before);
                let mut toggled = encoder.clone();
                toggled.set_allocation_analysis(flags & 16 == 0);
                toggled.set_allocation_analysis(flags & 16 != 0);
                toggled.set_pitch_prefilter(flags & 32 == 0);
                toggled.set_pitch_prefilter(flags & 32 != 0);
                assert_eq!(toggled, encoder);
                encoder.reset();
                let mut fresh = CeltEncoder::new(rate, channels)?;
                configure(&mut fresh, flags);
                assert_eq!(encoder, fresh);
                assert_eq!(encoder.allocation_analysis(), flags & 16 != 0);
                assert_eq!(encoder.pitch_prefilter(), flags & 32 != 0);
                assert_eq!(encoder.transient_detection(), flags & 1 != 0);
                assert_eq!(encoder.tf_analysis(), flags & 2 != 0);
                assert_eq!(encoder.dynamic_allocation(), flags & 4 != 0);
                assert_eq!(encoder.spreading_analysis(), flags & 8 != 0);
                let input = vec![0; 960 / rate.factor() * channels];
                assert_eq!(
                    encoder.encode(FrameSize::Ms20, &input, 128)?,
                    fresh.encode(FrameSize::Ms20, &input, 128)?
                );
            }
        }
    }
    assert_eq!(frames, 8192);
    Ok(())
}
