use super::*;
use crate::{
    decoder::OpusDecoder,
    silk::{decoder::PcmChannels, resampler::OutputRate},
};
type TestResult = Result<(), Box<dyn std::error::Error>>;

fn hash(bytes: impl Iterator<Item = u8>) -> u64 {
    bytes.fold(0xcbf29ce484222325_u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
    })
}

#[test]
fn original_components_and_public_native_decoder_match_layout_transitions() -> TestResult {
    original_corpus(
        include_str!("../../tests/celt-channel-packet-fixtures.txt"),
        false,
    )
}

#[test]
fn original_components_and_public_native_decoder_match_bandwidth_transitions() -> TestResult {
    original_corpus(
        include_str!("../../tests/celt-bandwidth-packet-fixtures.txt"),
        true,
    )
}

fn original_corpus(corpus: &str, bandwidths: bool) -> TestResult {
    let rates = [
        InputRate::Khz8,
        InputRate::Khz12,
        InputRate::Khz16,
        InputRate::Khz24,
        InputRate::Khz48,
    ];
    let outputs = [
        OutputRate::Khz8,
        OutputRate::Khz12,
        OutputRate::Khz16,
        OutputRate::Khz24,
        OutputRate::Khz48,
    ];
    let sizes = [
        FrameSize::Ms2_5,
        FrameSize::Ms5,
        FrameSize::Ms10,
        FrameSize::Ms20,
    ];
    let mut lines = corpus.lines().filter(|line| !line.starts_with('#'));
    let mut encoder = CeltEncoder::new(InputRate::Khz48, 2)?;
    let mut previous_flags = usize::MAX;
    let mut decoders = Vec::new();
    for rate in outputs {
        for channels in [PcmChannels::Mono, PcmChannels::Stereo] {
            decoders.push(OpusDecoder::new(rate, channels)?);
        }
    }
    let mut rows = 0;
    while let Some(line) = lines.next() {
        let f: Vec<_> = line.split_whitespace().collect();
        assert_eq!(f[0], "frame");
        assert_eq!(f.len(), if bandwidths { 9 } else { 8 });
        let rate = rates
            .into_iter()
            .find(|r| r.hz().to_string() == f[2])
            .ok_or("rate")?;
        let size = sizes[f[3].parse::<usize>()?];
        let coded = f[4].parse::<usize>()?;
        let controls = f[5].parse::<usize>()?;
        if encoder.input_rate() != rate || previous_flags != controls {
            encoder = CeltEncoder::new(rate, 2)?;
            flags(&mut encoder, controls);
            previous_flags = controls;
        }
        if f[1] == "1" {
            encoder.reset();
            for decoder in &mut decoders {
                decoder.reset();
            }
        }
        encoder.set_encoded_channels(coded)?;
        if bandwidths {
            encoder.set_bandwidth(
                [
                    crate::Bandwidth::Narrowband,
                    crate::Bandwidth::Wideband,
                    crate::Bandwidth::SuperWideband,
                    crate::Bandwidth::Fullband,
                ][f[8].parse::<usize>()?],
            )?;
        }
        let bytes = f[6].parse::<usize>()?;
        let expected: Vec<u8> = (0..f[7].len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&f[7][i..i + 2], 16))
            .collect::<Result<_, _>>()?;
        let input = lines
            .next()
            .ok_or("input")?
            .strip_prefix("input ")
            .ok_or("input header")?
            .split_whitespace()
            .map(str::parse)
            .collect::<Result<Vec<i16>, _>>()?;
        let state = lines
            .next()
            .ok_or("state")?
            .strip_prefix("state ")
            .ok_or("state header")?
            .split_whitespace()
            .map(str::parse)
            .collect::<Result<Vec<i64>, _>>()?;
        assert_eq!(state.len(), 350);
        let packet = encoder.encode(size, &input, bytes)?;
        assert_eq!(packet.as_bytes(), expected, "packet row{rows}");
        assert_eq!(
            crate::Packet::parse(packet.as_bytes())?.bandwidth(),
            encoder.bandwidth()
        );
        assert_eq!(
            [
                i64::from(encoder.range),
                encoder.previous_coded as i64,
                i64::from(encoder.first),
                i64::from(encoder.energy.delayed_intra()),
                i64::from(encoder.preemphasis[0].memory()),
                i64::from(encoder.preemphasis[1].memory())
            ],
            state[..6],
            "prefix row{rows}"
        );
        for channel in 0..2 {
            assert_eq!(
                encoder.overlap[channel].map(i64::from),
                state[6 + 120 * channel..126 + 120 * channel],
                "overlap row{rows} C{channel}"
            );
            assert_eq!(
                encoder
                    .energy
                    .energies()
                    .channel(channel)
                    .ok_or("energy")?
                    .map(i64::from),
                state[246 + 21 * channel..267 + 21 * channel],
                "energy row{rows} C{channel}"
            );
            assert_eq!(
                encoder
                    .energy
                    .residuals(channel)
                    .ok_or("residual")?
                    .map(i64::from),
                state[288 + 21 * channel..309 + 21 * channel],
                "residual row{rows} C{channel}"
            );
            let raw = hash(
                encoder
                    .prefilter
                    .raw_history(channel)
                    .ok_or("raw history")?
                    .iter()
                    .flat_map(|v| v.to_le_bytes()),
            );
            let overlap = hash(
                encoder
                    .prefilter
                    .overlap(channel)
                    .ok_or("prefilter overlap")?
                    .iter()
                    .flat_map(|v| v.to_le_bytes()),
            );
            assert_eq!(
                [
                    i64::from(raw as u32),
                    i64::from((raw >> 32) as u32),
                    i64::from(overlap as u32),
                    i64::from((overlap >> 32) as u32)
                ],
                state[342 + 4 * channel..346 + 4 * channel],
                "pitch histories row{rows} C{channel}"
            );
        }
        assert_eq!(i64::from(encoder.transient_streak), state[330]);
        assert_eq!(
            [
                encoder.spreading.decision() as i64,
                i64::from(encoder.spreading.tonal_average()),
                i64::from(encoder.spreading.hf_average()),
                i64::from(encoder.spreading.tapset()),
                i64::from(encoder.trim.stereo_saving()),
                encoder.intensity.band() as i64
            ],
            state[331..337],
            "allocation histories row{rows}"
        );
        let params = encoder.prefilter.parameters();
        assert_eq!(
            [
                i64::from(params.gain() != 0),
                i64::from(params.period()),
                i64::from(params.gain()),
                if params.gain() == 0 {
                    0
                } else {
                    i64::from(params.gain() / 3072 - 1)
                },
                params.tapset() as i64
            ],
            state[337..342],
            "pitch parameters row{rows}"
        );
        for decoder in &mut decoders {
            let record: Vec<_> = lines.next().ok_or("decoded")?.split_whitespace().collect();
            assert_eq!(record.len(), 6);
            assert_eq!(record[0], "decoded");
            assert_eq!(decoder.output_rate().hz().to_string(), record[1]);
            assert_eq!(decoder.output_channels().count().to_string(), record[2]);
            let pcm = decoder.decode(packet.as_bytes())?;
            assert_eq!(pcm.samples_per_channel(), record[3].parse::<usize>()?);
            assert_eq!(pcm.final_range(), record[4].parse::<u32>()?);
            assert_eq!(
                hash(pcm.samples().iter().flat_map(|v| v.to_le_bytes())),
                record[5].parse::<u64>()?,
                "native PCM row{rows}"
            );
        }
        rows += 1;
    }
    assert_eq!(rows, 640);
    Ok(())
}

fn flags(encoder: &mut CeltEncoder, value: usize) {
    encoder.set_transient_detection(value & 1 != 0);
    encoder.set_tf_analysis(value & 2 != 0);
    encoder.set_dynamic_allocation(value & 4 != 0);
    encoder.set_spreading_analysis(value & 8 != 0);
    encoder.set_allocation_analysis(value & 16 != 0);
    encoder.set_pitch_prefilter(value & 32 != 0);
}

#[test]
fn band_controls_and_late_errors_preserve_the_complete_owner() -> TestResult {
    use crate::Bandwidth::*;
    let bands = [Narrowband, Wideband, SuperWideband, Fullband];
    for (rate, available) in [
        (InputRate::Khz8, 1),
        (InputRate::Khz12, 2),
        (InputRate::Khz16, 2),
        (InputRate::Khz24, 3),
        (InputRate::Khz48, 4),
    ] {
        let mut encoder = CeltEncoder::new(rate, 2)?;
        flags(&mut encoder, 63);
        let default = bands[available - 1];
        assert_eq!(encoder.bandwidth(), default);
        let pcm = input(960 / rate.factor(), 3);
        encoder.encode(FrameSize::Ms20, &pcm, 128)?;
        let before = encoder.clone();
        encoder.set_bandwidth(Narrowband)?;
        encoder.set_bandwidth(default)?;
        assert_eq!(
            encoder, before,
            "changing controls without encoding must retain histories"
        );
        for band in bands[available..].iter().copied().chain([Mediumband]) {
            let before = encoder.clone();
            assert_eq!(
                encoder.set_bandwidth(band),
                Err(EncoderError::InvalidBandwidth)
            );
            assert_eq!(encoder, before);
        }
        encoder.set_bandwidth(Narrowband)?;
        encoder.set_encoded_channels(1)?;
        let before = encoder.clone();
        for options in [
            None,
            Some(VbrOptions {
                bitrate: 64000,
                constrained: false,
                payload_bytes: 256,
            }),
            Some(VbrOptions {
                bitrate: 64000,
                constrained: true,
                payload_bytes: 256,
            }),
        ] {
            let bytes = options.map_or(128, |o| o.payload_bytes);
            let result = encoder.encode_with_rate(FrameSize::Ms20, &pcm, bytes, options, |_| {
                Err(EntropyError::CapacityExhausted)
            });
            assert!(result.is_err());
            assert_eq!(encoder, before);
        }
        let packet = encoder.encode(FrameSize::Ms20, &pcm, 128)?;
        assert_eq!(
            crate::Packet::parse(packet.as_bytes())?.bandwidth(),
            Narrowband
        );
        for channel in 0..2 {
            assert_eq!(
                &encoder.energy.energies().channel(channel).ok_or("energy")?[13..],
                &[0; 8]
            );
        }
        encoder.reset();
        let mut fresh = CeltEncoder::new(rate, 2)?;
        flags(&mut fresh, 63);
        fresh.set_encoded_channels(1)?;
        fresh.set_bandwidth(Narrowband)?;
        assert_eq!(encoder, fresh);
    }
    Ok(())
}
fn input(n: usize, frame: usize) -> Vec<i16> {
    (0..n)
        .flat_map(|i| {
            let phase = ((i + frame * n) % 160) as i32;
            let value = (phase.min(160 - phase) - 40) * 500;
            [
                value as i16,
                if frame % 4 == 0 {
                    -value as i16
                } else {
                    ((i * 719 + frame * 1231) % 32768) as i16 - 16384
                },
            ]
        })
        .collect()
}

#[test]
fn actual_packets_keep_both_input_histories_across_wire_layout_changes() -> TestResult {
    for rate in [
        InputRate::Khz8,
        InputRate::Khz12,
        InputRate::Khz16,
        InputRate::Khz24,
        InputRate::Khz48,
    ] {
        for variable in 0..3 {
            let mut encoder = CeltEncoder::new(rate, 2)?;
            flags(&mut encoder, 63);
            let mut decoder = OpusDecoder::new(OutputRate::Khz48, PcmChannels::Stereo)?;
            for frame in 0..32 {
                let size = [
                    FrameSize::Ms2_5,
                    FrameSize::Ms5,
                    FrameSize::Ms10,
                    FrameSize::Ms20,
                ][frame % 4];
                let channels = if frame % 6 < 3 { 2 } else { 1 };
                encoder.set_encoded_channels(channels)?;
                let pcm = input(size.samples() / rate.factor(), frame);
                let mut emphasis = encoder.preemphasis.clone();
                for (c, state) in emphasis.iter_mut().enumerate() {
                    state.process(size, rate, &pcm, 2, c, &mut vec![0; size.samples()])?;
                }
                let packet = if variable == 0 {
                    encoder.encode(size, &pcm, 128)?
                } else {
                    encoder.encode_vbr(
                        size,
                        &pcm,
                        VbrOptions {
                            bitrate: 64000,
                            constrained: variable == 2,
                            payload_bytes: 256,
                        },
                    )?
                };
                assert_eq!(packet.channels(), channels);
                assert_eq!(
                    crate::Packet::parse(packet.as_bytes())?.channels() as usize,
                    channels
                );
                assert_eq!(encoder.preemphasis, emphasis);
                assert_eq!(encoder.channels(), 2);
                assert_eq!(encoder.encoded_channels(), channels);
                if channels == 1 {
                    assert_eq!(
                        encoder.energy.energies().channel(0),
                        encoder.energy.energies().channel(1)
                    );
                    assert_eq!(encoder.energy.residuals(1), Some(&[0; 21]));
                }
                assert_eq!(
                    decoder.decode(packet.as_bytes())?.samples_per_channel(),
                    size.samples()
                );
                assert_eq!(decoder.final_range(), packet.final_range());
            }
        }
    }
    Ok(())
}

#[test]
fn layout_controls_reset_and_failed_encoding_preserve_complete_state() -> TestResult {
    let mut encoder = CeltEncoder::new(InputRate::Khz48, 2)?;
    flags(&mut encoder, 63);
    encoder.encode(FrameSize::Ms20, &input(960, 1), 128)?;
    for channels in [0, 3, usize::MAX] {
        let before = encoder.clone();
        assert_eq!(
            encoder.set_encoded_channels(channels),
            Err(EncoderError::InvalidChannels)
        );
        assert_eq!(encoder, before);
    }
    encoder.set_encoded_channels(1)?;
    let before = encoder.clone();
    assert!(encoder.encode(FrameSize::Ms20, &[0; 960], 128).is_err());
    assert_eq!(encoder, before);
    let forced = encoder.encode_with(FrameSize::Ms20, &input(960, 2), 128, |_| {
        Err(EntropyError::CapacityExhausted)
    });
    assert!(forced.is_err());
    assert_eq!(encoder, before);
    for constrained in [false, true] {
        let options = VbrOptions {
            bitrate: 64000,
            constrained,
            payload_bytes: 256,
        };
        let forced = encoder.encode_with_rate(
            FrameSize::Ms20,
            &input(960, 2),
            options.payload_bytes,
            Some(options),
            |_| Err(EntropyError::CapacityExhausted),
        );
        assert!(forced.is_err());
        assert_eq!(encoder, before);
        for bytes in [0, 1, 1276, usize::MAX] {
            assert!(
                encoder
                    .encode_vbr(
                        FrameSize::Ms20,
                        &input(960, 2),
                        VbrOptions {
                            payload_bytes: bytes,
                            ..options
                        }
                    )
                    .is_err()
            );
            assert_eq!(encoder, before);
        }
    }
    encoder.reset();
    assert_eq!(encoder.encoded_channels(), 1);
    let mut fresh = CeltEncoder::new(InputRate::Khz48, 2)?;
    flags(&mut fresh, 63);
    fresh.set_encoded_channels(1)?;
    assert_eq!(encoder, fresh);
    let mut mono = CeltEncoder::new(InputRate::Khz48, 1)?;
    let before = mono.clone();
    assert_eq!(
        mono.set_encoded_channels(2),
        Err(EncoderError::InvalidChannels)
    );
    assert_eq!(mono, before);
    Ok(())
}
