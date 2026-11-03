use super::*;

#[test]
fn low_band_phase_leaves_celt_and_committed_input_untouched()
-> Result<(), Box<dyn std::error::Error>> {
    for channels in 1..=2 {
        let mut phased = HybridEncoder::new(InputRate::Khz48, channels, Bandwidth::Fullband)?;
        let mut regular = phased.clone();
        for frame in 0..12 {
            let duration = if frame % 2 == 0 {
                FrameDuration::Ms10
            } else {
                FrameDuration::Ms20
            };
            let input = pcm(InputRate::Khz48, channels, duration, frame);
            let before = phased.clone();
            let options = Options::default();
            let mut low = phased.frame_encoder().prepare_low_band(
                duration,
                &input,
                160 * channels,
                options,
                None,
                true,
            )?;
            // The outer owner must still be able to use old CELT and delayed
            // input for incoming redundancy before completing the high band.
            assert_eq!(phased.celt, before.celt);
            assert_eq!(phased.pcm.history(), before.pcm.history());
            assert_eq!(phased.high_gain, before.high_gain);
            assert_eq!(phased.width, before.width);
            assert_ne!(phased.speech, before.speech);
            low.no_redundancy()?;
            let prepared = phased.frame_encoder().complete_high_band(low)?;
            let actual =
                phased.finish_packet(duration, 160 * channels, prepared, RangeEncoder::finish)?;
            let expected = regular.encode(duration, &input, 160 * channels, options)?;
            assert_eq!(actual, expected);
            assert_eq!(phased, regular);
        }
    }
    Ok(())
}

#[test]
fn borrowed_frame_rejects_invalid_geometry_before_mutating_histories()
-> Result<(), Box<dyn std::error::Error>> {
    for case in 0..6 {
        let mut encoder = HybridEncoder::new(InputRate::Khz48, 2, Bandwidth::Fullband)?;
        let before = encoder.clone();
        let result = {
            let mut frame = encoder.frame_encoder();
            match case {
                0 => frame.channels = usize::MAX,
                1 => frame.rate = InputRate::Khz8,
                2 => frame.bandwidth = Bandwidth::Wideband,
                _ => {}
            }
            frame.prepare(
                FrameDuration::Ms20,
                if case == 3 { &[0; 1919] } else { &[0; 1920] },
                if case == 4 { 2 } else { 160 },
                Options::default(),
                if case == 5 { Some(0) } else { None },
                true,
            )
        };
        assert!(result.is_err(), "case{case}");
        assert_eq!(encoder, before, "case{case}");
    }
    let mut encoder = HybridEncoder::new(InputRate::Khz48, 2, Bandwidth::Fullband)?;
    encoder.celt.set_encoded_channels(1)?;
    let before = encoder.clone();
    assert_eq!(
        encoder
            .encode_inner(
                FrameDuration::Ms20,
                &[0; 1920],
                160,
                Options::default(),
                None,
                true
            )
            .err(),
        Some(EncoderError::InvalidConfiguration)
    );
    assert_eq!(encoder, before);
    Ok(())
}
use crate::{Mode, Packet, decoder::OpusDecoder, silk::decoder::PcmChannels};

#[test]
fn vbr_uses_nominal_rate_and_actual_shared_packet_length() -> Result<(), Box<dyn std::error::Error>>
{
    let mut lengths = std::collections::BTreeSet::new();
    for rate in [InputRate::Khz24, InputRate::Khz48] {
        for channels in 1..=2 {
            let mut encoder = HybridEncoder::new(rate, channels, Bandwidth::SuperWideband)?;
            encoder.set_inband_fec(true);
            encoder.set_dtx(true);
            let mut decoder = OpusDecoder::new(
                OutputRate::Khz48,
                if channels == 1 {
                    PcmChannels::Mono
                } else {
                    PcmChannels::Stereo
                },
            )?;
            for frame in 0..48 {
                let duration = if frame % 4 == 0 {
                    FrameDuration::Ms10
                } else {
                    FrameDuration::Ms20
                };
                let mut input = pcm(rate, channels, duration, frame);
                if (5..25).contains(&frame) {
                    input.fill(0);
                }
                let packet = encoder.encode_vbr(
                    duration,
                    &input,
                    32000 * channels as u32,
                    320,
                    Options::default(),
                )?;
                assert!((1..=320).contains(&packet.as_bytes().len()));
                assert_eq!(
                    decoder.decode(packet.as_bytes())?.final_range(),
                    packet.final_range()
                );
                lengths.insert(packet.as_bytes().len());
            }
        }
    }
    assert!(lengths.len() > 5);
    Ok(())
}

#[test]
fn vbr_caps_invalid_targets_and_late_failure_preserve_full_state()
-> Result<(), Box<dyn std::error::Error>> {
    for channels in 1..=2 {
        let mut encoder = HybridEncoder::new(InputRate::Khz48, channels, Bandwidth::Fullband)?;
        encoder.set_dtx(true);
        encoder.set_inband_fec(true);
        let input = pcm(InputRate::Khz48, channels, FrameDuration::Ms20, 0);
        let options = Options {
            complexity: 10,
            packet_loss_percent: 20,
        };
        encoder.encode_vbr(
            FrameDuration::Ms20,
            &input,
            48000 * channels as u32,
            320,
            options,
        )?;
        let before = encoder.clone();
        for rate in [0, 1, 1199, 1024001, u32::MAX] {
            assert!(
                encoder
                    .encode_vbr(FrameDuration::Ms20, &input, rate, 320, options)
                    .is_err()
            );
            assert_eq!(encoder, before);
        }
        let mut calls = 0;
        let error = encoder.encode_with_rate(
            FrameDuration::Ms20,
            &input,
            320,
            options,
            Some(48000 * channels as u32),
            |writer| {
                calls += 1;
                assert!(writer.capacity() < 319);
                writer.finish()?;
                Err(EntropyError::CapacityExhausted)
            },
        );
        assert_eq!(calls, 1);
        assert_eq!(
            error,
            Err(EncoderError::Entropy(EntropyError::CapacityExhausted))
        );
        assert_eq!(encoder, before);
        let mut admitted = 0;
        for capacity in [3, 8, 16, 32, 64, 1276] {
            let mut trial = before.clone();
            match trial.encode_vbr(FrameDuration::Ms20, &input, 64000, capacity, options) {
                Ok(packet) => {
                    assert!(packet.as_bytes().len() <= capacity);
                    admitted += 1;
                }
                Err(_) => assert_eq!(trial, before),
            }
        }
        assert!(admitted > 0);
    }
    Ok(())
}

#[test]
fn queued_fec_budget_retry_and_finalization_errors_preserve_state()
-> Result<(), Box<dyn std::error::Error>> {
    for channels in 1..=2 {
        let mut encoder = HybridEncoder::new(InputRate::Khz48, channels, Bandwidth::Fullband)?;
        encoder.set_inband_fec(true);
        let options = Options {
            complexity: 10,
            packet_loss_percent: 20,
        };
        let mut decoder = OpusDecoder::new(
            OutputRate::Khz48,
            if channels == 1 {
                PcmChannels::Mono
            } else {
                PcmChannels::Stereo
            },
        )?;
        for frame in 0..12 {
            let input = pcm(InputRate::Khz48, channels, FrameDuration::Ms20, frame);
            let packet = encoder.encode(FrameDuration::Ms20, &input, 160 * channels, options)?;
            decoder.decode(packet.as_bytes())?;
        }
        assert!(encoder.speech.fec.has_pending(PacketDuration::Ms20));
        let before = encoder.clone();
        let input = pcm(InputRate::Khz48, channels, FrameDuration::Ms20, 12);
        let mut dropped = 0;
        let mut failed = 0;
        for bytes in (3..160 * channels).step_by(3) {
            let mut trial = before.clone();
            let mut finalizations = 0;
            match trial.encode_with(FrameDuration::Ms20, &input, bytes, options, |writer| {
                finalizations += 1;
                writer.finish()
            }) {
                Ok(packet) => {
                    assert_eq!(finalizations, 1);
                    if trial.speech.fec.dropped {
                        assert!(!trial.has_fec());
                        let mut expected = before.clone();
                        let prepared = expected.encode_inner(
                            FrameDuration::Ms20,
                            &input,
                            bytes,
                            options,
                            None,
                            false,
                        )?;
                        let expected_packet = expected.finish_packet(
                            FrameDuration::Ms20,
                            bytes,
                            prepared,
                            RangeEncoder::finish,
                        )?;
                        assert_eq!(packet, expected_packet);
                        assert_eq!(trial, expected);
                        dropped += 1;
                    }
                    let mut check = decoder.clone();
                    assert_eq!(
                        check.decode(packet.as_bytes())?.final_range(),
                        packet.final_range()
                    );
                }
                Err(_) => {
                    assert_eq!(trial, before);
                    failed += 1;
                }
            }
        }
        assert!(dropped > 0 && failed > 0);
        let mut trial = before.clone();
        let mut finalizations = 0;
        assert_eq!(
            trial.encode_with(
                FrameDuration::Ms20,
                &input,
                160 * channels,
                options,
                |writer| {
                    finalizations += 1;
                    writer.finish()?;
                    Err(EntropyError::CapacityExhausted)
                }
            ),
            Err(EncoderError::Entropy(EntropyError::CapacityExhausted))
        );
        assert_eq!(finalizations, 1);
        assert_eq!(trial, before);
        // Native outer digital-zero VAD must prevent capturing a new LBRR,
        // even with DTX disabled and a voiced resampler/VAD tail still present.
        let mut quiet = before.clone();
        quiet.encode(
            FrameDuration::Ms20,
            &vec![0; 960 * channels],
            160 * channels,
            options,
        )?;
        assert!(!quiet.speech.fec.has_pending(PacketDuration::Ms20));
    }
    Ok(())
}

#[test]
fn integrated_fec_primary_recovery_and_dtx_are_decodable() -> Result<(), Box<dyn std::error::Error>>
{
    let mut protected = 0;
    let mut suppressed = 0;
    for rate in [InputRate::Khz24, InputRate::Khz48] {
        for channels in 1..=2 {
            let mut encoder = HybridEncoder::new(rate, channels, Bandwidth::SuperWideband)?;
            encoder.set_inband_fec(true);
            encoder.set_dtx(true);
            let mut decoder = OpusDecoder::new(
                OutputRate::Khz48,
                if channels == 1 {
                    PcmChannels::Mono
                } else {
                    PcmChannels::Stereo
                },
            )?;
            let mut previous_samples = 960;
            for frame in 0..100 {
                let short = (45..50).contains(&frame);
                let duration = if short {
                    FrameDuration::Ms10
                } else {
                    FrameDuration::Ms20
                };
                let ms = if short { 10 } else { 20 };
                let mut input = pcm(rate, channels, duration, frame);
                if (8..40).contains(&frame) {
                    input.fill(0);
                }
                encoder.set_inband_fec(!(65..70).contains(&frame));
                let packet = encoder.encode(
                    duration,
                    &input,
                    ms * 8 * channels,
                    Options {
                        complexity: 10,
                        packet_loss_percent: 20,
                    },
                )?;
                protected += usize::from(encoder.has_fec());
                suppressed += usize::from(encoder.in_dtx());
                if encoder.in_dtx() {
                    assert!(!encoder.has_fec());
                }
                if packet.as_bytes().len() > 1 {
                    let (header, _) =
                        crate::silk::decode_header(&Packet::parse(packet.as_bytes())?, 0)?;
                    let advertised = (0..channels)
                        .any(|c| header.redundancy(c).is_some_and(|v| v.iter().any(|&v| v)));
                    assert_eq!(advertised, encoder.has_fec());
                }
                if frame % 3 != 1 {
                    if frame % 3 == 2 {
                        decoder.decode_fec(packet.as_bytes(), previous_samples)?;
                    }
                    let decoded = decoder.decode(packet.as_bytes())?;
                    assert_eq!(decoded.final_range(), packet.final_range());
                }
                previous_samples = (ms * 48) as u16;
            }
            encoder.reset();
            let mut fresh = HybridEncoder::new(rate, channels, Bandwidth::SuperWideband)?;
            fresh.set_inband_fec(true);
            fresh.set_dtx(true);
            assert_eq!(encoder, fresh);
        }
    }
    assert!(protected > 40);
    assert!(suppressed > 20);
    Ok(())
}

fn pcm(rate: InputRate, channels: usize, duration: FrameDuration, frame: usize) -> Vec<i16> {
    let n = rate.hz() as usize
        / if duration == FrameDuration::Ms10 {
            100
        } else {
            50
        };
    (0..n * channels)
        .map(|i| {
            let phase = (i / channels + frame * n + i % channels * 17) % 160;
            ((phase.min(160 - phase) as i32 - 40) * 200) as i16
        })
        .collect()
}

#[test]
fn shared_stream_packets_decode_and_preserve_duration_range()
-> Result<(), Box<dyn std::error::Error>> {
    for rate in [InputRate::Khz24, InputRate::Khz48] {
        for channels in 1..=2 {
            for bandwidth in [Bandwidth::SuperWideband, Bandwidth::Fullband] {
                if rate == InputRate::Khz24 && bandwidth == Bandwidth::Fullband {
                    continue;
                }
                for complexity in [0, 1, 6, 10] {
                    let mut encoder = HybridEncoder::new(rate, channels, bandwidth)?;
                    let mut decoder = OpusDecoder::new(
                        OutputRate::Khz48,
                        if channels == 1 {
                            PcmChannels::Mono
                        } else {
                            PcmChannels::Stereo
                        },
                    )?;
                    for frame in 0..6 {
                        let duration = if frame % 2 == 0 {
                            FrameDuration::Ms10
                        } else {
                            FrameDuration::Ms20
                        };
                        let input = pcm(rate, channels, duration, frame);
                        let bytes = if duration == FrameDuration::Ms10 {
                            80
                        } else {
                            160
                        };
                        let encoded = encoder.encode(
                            duration,
                            &input,
                            bytes,
                            Options {
                                complexity,
                                packet_loss_percent: 20,
                            },
                        )?;
                        let packet = Packet::parse(encoded.as_bytes())?;
                        assert_eq!(packet.mode(), Mode::Hybrid);
                        assert_eq!(packet.bandwidth(), bandwidth);
                        assert_eq!(usize::from(packet.channels()), channels);
                        let decoded = decoder.decode(encoded.as_bytes())?;
                        assert_eq!(
                            decoded.samples().len(),
                            packet.duration_samples() as usize * channels
                        );
                        assert_eq!(decoded.final_range(), encoded.final_range());
                        assert_eq!(encoded.as_bytes().len(), bytes);
                    }
                }
            }
        }
    }
    Ok(())
}

#[test]
fn finalization_failure_and_tiny_budget_roll_back_both_layers()
-> Result<(), Box<dyn std::error::Error>> {
    for channels in 1..=2 {
        let mut encoder = HybridEncoder::new(InputRate::Khz48, channels, Bandwidth::Fullband)?;
        let input = pcm(InputRate::Khz48, channels, FrameDuration::Ms20, 0);
        encoder.encode(FrameDuration::Ms20, &input, 160, Options::default())?;
        let before = encoder.clone();
        let mut called = false;
        let failure = encoder.encode_with(
            FrameDuration::Ms20,
            &input,
            160,
            Options::default(),
            |writer| {
                called = true;
                writer.finish()?;
                Err(EntropyError::CapacityExhausted)
            },
        );
        assert!(called);
        assert_eq!(
            failure,
            Err(EncoderError::Entropy(EntropyError::CapacityExhausted))
        );
        assert_eq!(encoder, before);
        assert!(
            encoder
                .encode(FrameDuration::Ms20, &input, 3, Options::default())
                .is_err()
        );
        assert_eq!(encoder, before);
        encoder.reset();
        let fresh = HybridEncoder::new(InputRate::Khz48, channels, Bandwidth::Fullband)?;
        assert_eq!(encoder, fresh);
    }
    Ok(())
}

#[test]
fn native_high_band_gain_and_in_place_fades_match() -> Result<(), Box<dyn std::error::Error>> {
    let mut gains = 0;
    let mut fades = 0;
    for line in include_str!("../../tests/hybrid-encoder-fade-fixtures.txt")
        .lines()
        .filter(|l| !l.starts_with('#'))
    {
        let mut fields = line.split_whitespace();
        let kind = fields.next().ok_or("missing kind")?;
        let v = fields.map(str::parse).collect::<Result<Vec<i128>, _>>()?;
        if kind == "gain" {
            assert_eq!(v.len(), 2);
            let gain = (32767 - (crate::celt::math::exp2((-v[0]) as i16) >> 1)) as i16;
            assert_eq!(i128::from(gain), v[1]);
            gains += 1;
        } else {
            assert_eq!(kind, "fade");
            assert_eq!(v.len(), 8);
            let rate = if v[1] == 24000 {
                InputRate::Khz24
            } else {
                InputRate::Khz48
            };
            let channels = usize::try_from(v[2])?;
            let n = usize::try_from(v[1] * v[3] / 1000)? * channels;
            let mut seed = u32::try_from(v[6])?;
            let mut pcm: Vec<i16> = (0..n)
                .map(|_| {
                    seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                    (seed >> 16) as i16
                })
                .collect();
            fade(
                &mut pcm,
                rate,
                channels,
                i16::try_from(v[4])?,
                i16::try_from(v[5])?,
                v[0] != 0,
            );
            let hash = pcm
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .fold(14695981039346656037u64, |h, b| {
                    (h ^ u64::from(b)).wrapping_mul(1099511628211)
                });
            // The wider signed parser retains negative controls and all64 hash bits.
            assert_eq!(hash, v[7] as u64, "{line}");
            fades += 1;
        }
    }
    assert_eq!((gains, fades), (15, 656));
    Ok(())
}

#[test]
fn invalid_geometry_and_budget_boundaries_preserve_stream() -> Result<(), Box<dyn std::error::Error>>
{
    for rate in [InputRate::Khz8, InputRate::Khz12, InputRate::Khz16] {
        assert_eq!(
            HybridEncoder::new(rate, 1, Bandwidth::Fullband),
            Err(EncoderError::InvalidConfiguration)
        );
    }
    for channels in [0, 3, usize::MAX] {
        assert_eq!(
            HybridEncoder::new(InputRate::Khz48, channels, Bandwidth::Fullband),
            Err(EncoderError::InvalidConfiguration)
        );
    }
    for bandwidth in [
        Bandwidth::Narrowband,
        Bandwidth::Mediumband,
        Bandwidth::Wideband,
    ] {
        assert_eq!(
            HybridEncoder::new(InputRate::Khz48, 1, bandwidth),
            Err(EncoderError::InvalidConfiguration)
        );
    }
    assert_eq!(
        HybridEncoder::new(InputRate::Khz24, 1, Bandwidth::Fullband),
        Err(EncoderError::InvalidConfiguration)
    );
    for channels in 1..=2 {
        let mut encoder = HybridEncoder::new(InputRate::Khz48, channels, Bandwidth::Fullband)?;
        let input = pcm(InputRate::Khz48, channels, FrameDuration::Ms20, 7);
        let original_input = input.clone();
        encoder.encode(FrameDuration::Ms20, &input, 160, Options::default())?;
        let before = encoder.clone();
        assert_eq!(
            encoder.encode(FrameDuration::Ms10, &input, 160, Options::default()),
            Err(EncoderError::InvalidInput)
        );
        for bytes in [0, 1, 2, 1277, usize::MAX] {
            assert_eq!(
                encoder.encode(FrameDuration::Ms20, &input, bytes, Options::default()),
                Err(EncoderError::InvalidOptions)
            );
        }
        for options in [
            Options {
                complexity: 11,
                packet_loss_percent: 0,
            },
            Options {
                complexity: 10,
                packet_loss_percent: 101,
            },
        ] {
            assert_eq!(
                encoder.encode(FrameDuration::Ms20, &input, 160, options),
                Err(EncoderError::InvalidOptions)
            );
        }
        assert_eq!(encoder, before);
        let mut accepted = 0;
        let mut rejected = 0;
        for bytes in [
            3, 4, 5, 8, 15, 16, 24, 31, 32, 40, 80, 160, 320, 640, 1275, 1276,
        ] {
            let mut trial = before.clone();
            match trial.encode(FrameDuration::Ms20, &input, bytes, Options::default()) {
                Ok(packet) => {
                    assert_eq!(packet.as_bytes().len(), bytes);
                    accepted += 1;
                }
                Err(_) => {
                    assert_eq!(trial, before);
                    rejected += 1;
                }
            }
        }
        assert!(accepted > 0 && rejected > 0);
        assert_eq!(input, original_input);
    }
    Ok(())
}

#[test]
fn original_hybrid_dtx_schedule_and_layer_order_match() -> Result<(), Box<dyn std::error::Error>> {
    let mut encoder = HybridEncoder::new(InputRate::Khz48, 1, Bandwidth::Fullband)?;
    let mut counts = [0; 3];
    let mut rows = 0;
    for line in include_str!("../../tests/hybrid-encoder-dtx-fixtures.txt")
        .lines()
        .filter(|l| !l.starts_with('#'))
    {
        let v = line
            .split_whitespace()
            .map(str::parse)
            .collect::<Result<Vec<usize>, _>>()?;
        assert_eq!(v.len(), 11);
        let [
            rate,
            band,
            channels,
            ms,
            complexity,
            _,
            frame,
            enabled,
            suppressed,
            silk_zero,
            celt_calls,
        ]: [usize; 11] = v.try_into().map_err(|_| "bad row")?;
        if frame == 0 {
            encoder = HybridEncoder::new(
                if rate == 24000 {
                    InputRate::Khz24
                } else {
                    InputRate::Khz48
                },
                channels,
                if band == 1104 {
                    Bandwidth::SuperWideband
                } else {
                    Bandwidth::Fullband
                },
            )?;
        }
        encoder.set_dtx(enabled != 0);
        let n = rate * ms / 1000;
        let input: Vec<i16> = (0..n * channels)
            .map(|i| {
                let phase = (frame * n + i / channels) % 240;
                if !(5..110).contains(&frame) {
                    ((phase.min(240 - phase) as i32 - 60) * 200) as i16
                } else if frame < 55 {
                    0
                } else {
                    ((i / channels + i % channels) % 2) as i16
                }
            })
            .collect();
        let before = encoder.clone();
        let mut finished = false;
        let duration = if ms == 10 {
            FrameDuration::Ms10
        } else {
            FrameDuration::Ms20
        };
        let packet = encoder.encode_with(
            duration,
            &input,
            ms * 4 * channels,
            Options {
                complexity: complexity as u8,
                packet_loss_percent: 0,
            },
            |writer| {
                finished = true;
                writer.finish()
            },
        )?;
        assert_eq!(packet.as_bytes().len() == 1, suppressed != 0, "{line}");
        assert_eq!(encoder.in_dtx(), suppressed != 0, "{line}");
        assert_eq!(usize::from(finished), celt_calls, "{line}");
        assert_eq!(silk_zero != 0, celt_calls == 0);
        if silk_zero != 0 {
            assert_eq!(encoder.celt, before.celt);
            assert_eq!(encoder.pcm.history(), before.pcm.history());
            assert_eq!(encoder.high_gain, before.high_gain);
            assert_eq!(encoder.width, before.width);
            counts[0] += 1;
        } else if suppressed != 0 {
            counts[1] += 1;
        } else {
            counts[2] += 1;
        }
        if suppressed != 0 {
            assert_eq!(packet.final_range(), 0);
        }
        rows += 1;
    }
    assert_eq!(rows, 10800);
    assert!(counts.iter().all(|&n| n > 0));
    Ok(())
}

#[test]
fn dtx_finalization_failure_toggles_and_reset_preserve_contracts()
-> Result<(), Box<dyn std::error::Error>> {
    for channels in 1..=2 {
        let mut encoder = HybridEncoder::new(InputRate::Khz48, channels, Bandwidth::Fullband)?;
        encoder.set_dtx(true);
        let silence = vec![0; 960 * channels];
        for _ in 0..12 {
            encoder.encode(
                FrameDuration::Ms20,
                &silence,
                160 * channels,
                Options::default(),
            )?;
        }
        assert!(encoder.in_dtx());
        let before = encoder.clone();
        let mut called = false;
        let result = encoder.encode_with(
            FrameDuration::Ms20,
            &silence,
            160 * channels,
            Options::default(),
            |writer| {
                called = true;
                writer.finish()?;
                Err(EntropyError::CapacityExhausted)
            },
        );
        assert!(called);
        assert_eq!(
            result,
            Err(EncoderError::Entropy(EntropyError::CapacityExhausted))
        );
        assert_eq!(encoder, before);
        let packet = encoder.encode(
            FrameDuration::Ms20,
            &silence,
            160 * channels,
            Options::default(),
        )?;
        assert_eq!((packet.as_bytes().len(), packet.final_range()), (1, 0));
        encoder.reset();
        let mut fresh = HybridEncoder::new(InputRate::Khz48, channels, Bandwidth::Fullband)?;
        fresh.set_dtx(true);
        assert_eq!(encoder, fresh);
        let quiet = vec![1; 960 * channels];
        for _ in 0..15 {
            encoder.encode(
                FrameDuration::Ms20,
                &quiet,
                160 * channels,
                Options::default(),
            )?;
        }
        assert!(encoder.in_dtx());
        let before = encoder.clone();
        let mut called = false;
        let packet = encoder.encode_with(
            FrameDuration::Ms20,
            &quiet,
            160 * channels,
            Options::default(),
            |_| {
                called = true;
                Err(EntropyError::CapacityExhausted)
            },
        )?;
        assert!(!called);
        assert_eq!(packet.as_bytes().len(), 1);
        assert_eq!(encoder.celt, before.celt);
        encoder.set_dtx(false);
        let packet = encoder.encode(
            FrameDuration::Ms10,
            &quiet[..480 * channels],
            80 * channels,
            Options::default(),
        )?;
        assert_eq!(packet.as_bytes().len(), 80 * channels);
        assert!(!encoder.in_dtx());
    }
    Ok(())
}
