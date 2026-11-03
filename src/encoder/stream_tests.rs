use super::*;
use crate::{Packet, decoder::OpusDecoder, silk::decoder::PcmChannels};
type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

#[test]
fn original_cross_mode_prefill_mid_pcm_and_resamplers_match() -> Result {
    check_original_inputs(
        include_str!("../../tests/opus-stream-input-fixtures.txt"),
        false,
    )?;
    Ok(())
}

#[test]
fn original_bandwidth_prefill_mid_pcm_and_resamplers_match() -> Result {
    check_original_inputs(
        include_str!("../../tests/opus-stream-band-input-fixtures.txt"),
        true,
    )?;
    Ok(())
}

fn check_original_inputs(fixture: &str, band_profile: bool) -> Result {
    let mut encoder = OpusEncoder::new(InputRate::Khz8, 1)?;
    let mut cases = 0;
    let call_fixture = if band_profile {
        include_str!("../../tests/opus-stream-band-celt-calls-fixtures.txt")
    } else {
        include_str!("../../tests/opus-stream-celt-calls-fixtures.txt")
    };
    let mut call_rows = call_fixture.lines().filter(|l| !l.starts_with('#'));
    let mut transform = include_str!("../../tests/opus-stream-celt-input-fixtures.txt")
        .lines()
        .filter(|l| !l.starts_with('#'));
    for line in fixture.lines().filter(|l| !l.starts_with('#')) {
        let v = line
            .split_whitespace()
            .map(str::parse::<u64>)
            .collect::<std::result::Result<Vec<_>, _>>()?;
        assert_eq!(v.len(), 16);
        let rate = match v[0] {
            8000 => InputRate::Khz8,
            12000 => InputRate::Khz12,
            16000 => InputRate::Khz16,
            24000 => InputRate::Khz24,
            _ => InputRate::Khz48,
        };
        let channels = v[1] as usize;
        if v[3] == 0 {
            encoder = OpusEncoder::new(rate, channels)?;
        }
        let size = if v[4] == 5 {
            FrameSize::Ms5
        } else if v[4] == 10 {
            FrameSize::Ms10
        } else {
            FrameSize::Ms20
        };
        let mode = match v[5] {
            1000 => Mode::Silk,
            1001 => Mode::Hybrid,
            _ => Mode::Celt,
        };
        let mut seed = v[7] as u32;
        let input: Vec<i16> = (0..size.samples() / rate.factor() * channels)
            .map(|i| {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                if v[8] == 0 {
                    0
                } else if v[8] == 1 {
                    if i % 2 == 0 { i16::MIN } else { i16::MAX }
                } else {
                    ((seed & 65535) as i32 - 32768) as i16
                }
            })
            .collect();
        let packet = encoder.encode(
            size,
            &input,
            640,
            EncodeOptions {
                bitrate: 64000 * channels as u32,
                controls: control::Controls {
                    variable: v[2] != 0,
                    complexity: 0,
                    mode: Some(mode),
                    channels: Some(v[6] as usize),
                    bandwidth: Some(if band_profile {
                        if (64..192).contains(&v[3]) {
                            Bandwidth::Wideband
                        } else {
                            Bandwidth::Narrowband
                        }
                    } else if mode == Mode::Silk {
                        Bandwidth::Wideband
                    } else {
                        Bandwidth::Fullband
                    }),
                    ..control::Controls::default()
                },
                ..EncodeOptions::default()
            },
        )?;
        let calls = call_rows
            .next()
            .ok_or("missing CELT call row")?
            .split_whitespace()
            .map(str::parse::<u64>)
            .collect::<std::result::Result<Vec<_>, _>>()?;
        assert_eq!(calls.len(), 1 + 5 * calls[0] as usize);
        let expected_calls = calls[1..]
            .chunks_exact(5)
            .map(<[u64; 5]>::try_from)
            .collect::<std::result::Result<Vec<_>, _>>()?;
        assert_eq!(
            encoder.celt.call_trace, expected_calls,
            "CELT calls: {line}"
        );
        assert_eq!(packet.decision.channels, v[9] as usize, "wire: {line}");
        assert_eq!(packet.decision.to_mono, v[10] != 0, "toMono: {line}");
        if v[11] != 0 {
            assert_eq!(
                encoder.internal,
                match v[11] {
                    8 => SilkRate::Khz8,
                    12 => SilkRate::Khz12,
                    _ => SilkRate::Khz16,
                },
                "internal: {line}"
            );
        }
        assert_eq!(
            encoder.speech_channels, v[12] as usize,
            "speech channels: {line}"
        );
        for c in 0..2 {
            if v[13 + c] != 0 {
                assert_eq!(
                    encoder.speech.resampler[c].native_state_fingerprint(),
                    v[13 + c],
                    "resampler{c}: {line}"
                );
            }
        }
        assert_eq!(encoder.speech.mid_input_hash, v[15], "mid PCM: {line}");
        if !band_profile {
            let hashes = transform
                .next()
                .ok_or("missing transform input row")?
                .split_whitespace()
                .map(str::parse::<u64>)
                .collect::<std::result::Result<Vec<_>, _>>()?;
            assert_eq!(hashes.len(), 2);
            for (c, &hash) in hashes.iter().enumerate().take(channels) {
                assert_eq!(
                    encoder.celt.input_fingerprint(c),
                    Some(hash),
                    "CELT input{c}: {line}"
                );
            }
        }
        cases += 1;
    }
    assert_eq!(cases, if band_profile { 3072 } else { 1408 });
    assert!(call_rows.next().is_none());
    if !band_profile {
        assert!(transform.next().is_none());
    }
    Ok(())
}

fn pcm(rate: InputRate, channels: usize, size: FrameSize, frame: usize) -> Vec<i16> {
    let n = size.samples() / rate.factor();
    (0..n * channels)
        .map(|i| {
            let p = (i / channels + frame * n + i % channels * 13) % 80;
            ((p.min(80 - p) as i32 - 20) * 500) as i16
        })
        .collect()
}

#[test]
fn common_owner_encodes_mode_and_wire_changes_with_transition_ranges() -> Result {
    let mut tails = [0; 2];
    let mut modes = [0; 3];
    for rate in [
        InputRate::Khz8,
        InputRate::Khz12,
        InputRate::Khz16,
        InputRate::Khz24,
        InputRate::Khz48,
    ] {
        for channels in 1..=2 {
            for variable in [false, true] {
                let mut encoder = OpusEncoder::new(rate, channels)?;
                let mut decoder = OpusDecoder::new(
                    OutputRate::Khz48,
                    if channels == 1 {
                        PcmChannels::Mono
                    } else {
                        PcmChannels::Stereo
                    },
                )?;
                for frame in 0..48 {
                    let requested =
                        [Mode::Celt, Mode::Silk, Mode::Hybrid, Mode::Celt][frame / 3 % 4];
                    let size = if frame % 2 == 0 {
                        FrameSize::Ms10
                    } else {
                        FrameSize::Ms20
                    };
                    let options = EncodeOptions {
                        bitrate: 64000 * channels as u32,
                        controls: control::Controls {
                            mode: Some(requested),
                            variable,
                            complexity: 0,
                            channels: Some(if frame / 8 % 2 == 0 { 1 } else { channels }),
                            bandwidth: Some(if requested == Mode::Silk {
                                Bandwidth::Wideband
                            } else {
                                Bandwidth::Fullband
                            }),
                            ..control::Controls::default()
                        },
                        ..EncodeOptions::default()
                    };
                    let previous = encoder.history.previous_mode;
                    let packet = encoder
                        .encode(size, &pcm(rate, channels, size, frame), 640, options)
                        .map_err(|e| {
                            format!("{rate:?}/{channels}/{variable}/frame{frame}: {e:?}")
                        })?;
                    let parsed = Packet::parse(packet.as_bytes())?;
                    assert_eq!(parsed.mode(), packet.decision.mode);
                    assert_eq!(
                        decoder.decode(packet.as_bytes())?.final_range(),
                        packet.final_range(),
                        "{rate:?}/{channels}/{variable}/frame{frame}"
                    );
                    modes[match packet.decision.mode {
                        Mode::Silk => 0,
                        Mode::Hybrid => 1,
                        Mode::Celt => 2,
                    }] += 1;
                    if packet.tail_bytes > 0 {
                        tails[usize::from(previous == Some(Mode::Celt))] += 1;
                    }
                }
            }
        }
    }
    assert!(modes.iter().all(|&n| n > 0), "{modes:?}");
    assert!(tails.iter().all(|&n| n > 0), "{tails:?}");
    Ok(())
}

#[test]
fn short_frame_silence_uses_half_millisecond_dtx_clock() -> Result {
    let mut encoder = OpusEncoder::new(InputRate::Khz48, 1)?;
    let options = EncodeOptions {
        controls: control::Controls {
            dtx: true,
            complexity: 0,
            ..control::Controls::default()
        },
        ..EncodeOptions::default()
    };
    for frame in 0..90 {
        let packet = encoder.encode(FrameSize::Ms2_5, &[0; 120], 80, options)?;
        assert_eq!(packet.as_bytes().len() == 1, frame >= 80, "frame{frame}");
    }
    Ok(())
}

#[test]
fn invalid_and_budget_failures_preserve_common_owner() -> Result {
    let mut encoder = OpusEncoder::new(InputRate::Khz48, 2)?;
    encoder.encode(
        FrameSize::Ms20,
        &pcm(InputRate::Khz48, 2, FrameSize::Ms20, 0),
        320,
        EncodeOptions::default(),
    )?;
    let before = encoder.clone();
    for cap in 0..3 {
        assert!(
            encoder
                .encode(FrameSize::Ms20, &[0; 1920], cap, EncodeOptions::default())
                .is_err()
        );
        assert_eq!(encoder, before);
    }
    let options = EncodeOptions {
        controls: control::Controls {
            mode: Some(Mode::Silk),
            bandwidth: Some(Bandwidth::Wideband),
            ..control::Controls::default()
        },
        ..EncodeOptions::default()
    };
    let mut failed = 0;
    for cap in 3..30 {
        let mut trial = before.clone();
        if trial
            .encode(
                FrameSize::Ms20,
                &pcm(InputRate::Khz48, 2, FrameSize::Ms20, 1),
                cap,
                options,
            )
            .is_err()
        {
            assert_eq!(trial, before);
            failed += 1;
        }
    }
    assert!(failed > 0);
    Ok(())
}

#[test]
fn silk_early_dtx_preserves_celt_controls_and_committed_input_through_wire_changes() -> Result {
    let mut encoder = OpusEncoder::new(InputRate::Khz48, 2)?;
    let mut suppressed = 0;
    let mut suppressed_mono = 0;
    for frame in 0..64 {
        let before_history = encoder.pcm.history().to_vec();
        let before_mode = encoder.history.previous_mode;
        let before_channels = encoder.history.previous_channels;
        let mut before_celt = encoder.celt.clone();
        before_celt.call_trace.clear();
        let packet = encoder.encode(
            FrameSize::Ms20,
            &[1; 1920],
            320,
            EncodeOptions {
                controls: control::Controls {
                    mode: Some(Mode::Hybrid),
                    bandwidth: Some(Bandwidth::Fullband),
                    channels: Some(if frame < 32 { 2 } else { 1 }),
                    dtx: true,
                    complexity: 0,
                    ..control::Controls::default()
                },
                ..EncodeOptions::default()
            },
        )?;
        if packet.as_bytes().len() == 1 {
            // Nonzero raw PCM rules out the outer digital-silence return.
            let mut after_celt = encoder.celt.clone();
            after_celt.call_trace.clear();
            assert_eq!(after_celt, before_celt, "frame{frame}");
            assert_eq!(encoder.pcm.history(), before_history, "frame{frame}");
            assert_eq!(encoder.history.previous_mode, before_mode);
            assert_eq!(encoder.history.previous_channels, before_channels);
            suppressed += 1;
            suppressed_mono += usize::from(packet.decision.channels == 1);
        }
    }
    assert!(suppressed > 20);
    assert!(suppressed_mono > 5);
    Ok(())
}
