use super::*;
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
fn state(v: &[i64]) -> StereoAnalysisState {
    StereoAnalysisState {
        prediction: [v[0] as i16, v[1] as i16],
        mid: [v[2] as i16, v[3] as i16],
        side: [v[4] as i16, v[5] as i16],
        amplitude: [v[6] as i32, v[7] as i32, v[8] as i32, v[9] as i32],
        smoothed_width: v[10] as i16,
        width: v[11] as i16,
        silent_side: v[12] as i16,
    }
}
fn values(s: &StereoAnalysisState) -> Vec<i64> {
    s.prediction
        .map(i64::from)
        .into_iter()
        .chain(s.mid.map(i64::from))
        .chain(s.side.map(i64::from))
        .chain(s.amplitude.map(i64::from))
        .chain([
            i64::from(s.smoothed_width),
            i64::from(s.width),
            i64::from(s.silent_side),
        ])
        .collect()
}
#[test]
fn original_public_stereo_transform_state_and_prefix_match() -> Result<()> {
    let mut calls = 0;
    let mut omitted = 0;
    let mut zero_width = 0;
    for (line_number, line) in include_str!("../../../tests/silk-encoder-stereo-fixtures.txt")
        .lines()
        .enumerate()
    {
        if line.starts_with('#') {
            continue;
        }
        let fields: Vec<_> = line.split_whitespace().collect();
        let v: Vec<i64> = fields[..fields.len() - 1]
            .iter()
            .map(|s| s.parse())
            .collect::<std::result::Result<_, _>>()?;
        let rate = match v[0] {
            8 => SilkRate::Khz8,
            12 => SilkRate::Khz12,
            _ => SilkRate::Khz16,
        };
        let length = v[1] as usize;
        let left: Vec<i16> = v[40..40 + length].iter().map(|&v| v as i16).collect();
        let right: Vec<i16> = v[40 + length..40 + 2 * length]
            .iter()
            .map(|&v| v as i16)
            .collect();
        let mut before = state(&v[8..21]);
        let result = before.analyze(
            rate,
            if length == 10 * v[0] as usize {
                FrameDuration::Ms10
            } else {
                FrameDuration::Ms20
            },
            &left,
            &right,
            StereoControls {
                total_rate_bps: v[2] as i32,
                previous_activity_q8: v[3] as u16,
                to_mono: v[4] != 0,
            },
        )?;
        assert_eq!(
            values(&before),
            v[21..34],
            "stereo history line {}",
            line_number + 1
        );
        assert_eq!(result.mid_only(), v[5] != 0);
        assert_eq!(result.rates_bps().map(i64::from), v[6..8]);
        assert_eq!(
            result
                .indices()
                .iter()
                .flatten()
                .map(|&v| i64::from(v))
                .collect::<Vec<_>>(),
            v[34..40]
        );
        assert_eq!(
            result.mid(),
            v[40 + 2 * length..40 + 3 * length]
                .iter()
                .map(|&v| v as i16)
                .collect::<Vec<_>>(),
            "mid line {}",
            line_number + 1
        );
        assert_eq!(
            result.side(),
            v[40 + 3 * length..40 + 4 * length]
                .iter()
                .map(|&v| v as i16)
                .collect::<Vec<_>>(),
            "side line {}",
            line_number + 1
        );
        let mut writer = RangeEncoder::new(32)?;
        result.encode_prefix(&mut writer, false)?;
        let at = 40 + 4 * length;
        assert_eq!(
            [
                i64::from(writer.range()),
                i64::from(writer.tell()),
                i64::from(writer.tell_frac())
            ],
            v[at..]
        );
        let hex = fields.last().ok_or("missing prefix bytes")?;
        let expected: Vec<u8> = (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16))
            .collect::<std::result::Result<_, _>>()?;
        assert_eq!(writer.finish()?, expected);
        calls += 1;
        omitted += usize::from(result.mid_only());
        zero_width += usize::from(before.width == 0);
    }
    assert_eq!(calls, 144);
    assert!(omitted > 6);
    assert!(zero_width > 12);
    Ok(())
}
#[test]
fn full_scale_stereo_transitions_and_errors_preserve_state() -> Result<()> {
    let mut state = StereoAnalysisState::default();
    for rate in [SilkRate::Khz8, SilkRate::Khz12, SilkRate::Khz16] {
        for duration in [FrameDuration::Ms10, FrameDuration::Ms20] {
            let n = rate.khz() as usize
                * if duration == FrameDuration::Ms10 {
                    10
                } else {
                    20
                };
            let left: Vec<i16> = (0..n)
                .map(|i| if i % 2 == 0 { i16::MIN } else { i16::MAX })
                .collect();
            let right: Vec<i16> = left.iter().map(|&v| v.saturating_neg()).collect();
            for total_rate_bps in [1, 8000, 16000, 32000, 1024000] {
                for to_mono in [false, true] {
                    let c = StereoControls {
                        total_rate_bps,
                        previous_activity_q8: 256,
                        to_mono,
                    };
                    let result = state.analyze(rate, duration, &left, &right, c)?;
                    assert!(result.rates_bps()[0] > 0);
                    assert_eq!(result.rates_bps()[1] == 0, result.mid_only());
                    let saved = state.clone();
                    assert_eq!(
                        state.analyze(rate, duration, &left[..n - 1], &right, c),
                        Err(StereoEncodeError::InvalidInputLength)
                    );
                    for invalid in [
                        StereoControls {
                            total_rate_bps: 0,
                            ..c
                        },
                        StereoControls {
                            total_rate_bps: 1024001,
                            ..c
                        },
                        StereoControls {
                            previous_activity_q8: 257,
                            ..c
                        },
                    ] {
                        assert_eq!(
                            state.analyze(rate, duration, &left, &right, invalid),
                            Err(StereoEncodeError::InvalidControls)
                        );
                    }
                    assert_eq!(state, saved);
                    let mut writer = RangeEncoder::new(32)?;
                    result.encode_prefix(&mut writer, false)?;
                    writer.finish()?;
                    let original = writer.clone();
                    assert_eq!(
                        result.encode_prefix(&mut writer, false),
                        Err(StereoEncodeError::Entropy(EncoderError::Finished))
                    );
                    assert_eq!(writer, original);
                }
            }
        }
    }
    state.reset();
    assert_eq!(state, StereoAnalysisState::default());
    Ok(())
}

#[test]
fn stereo_packets_decode_through_mid_only_and_side_restart() -> Result<()> {
    use crate::{decoder::OpusDecoder, silk::decoder::PcmChannels};
    let mut omissions = 0;
    let mut restarts = 0;
    for input in [
        OutputRate::Khz8,
        OutputRate::Khz12,
        OutputRate::Khz16,
        OutputRate::Khz24,
        OutputRate::Khz48,
    ] {
        for internal in [SilkRate::Khz8, SilkRate::Khz12, SilkRate::Khz16] {
            for duration in [
                PacketDuration::Ms10,
                PacketDuration::Ms20,
                PacketDuration::Ms40,
                PacketDuration::Ms60,
            ] {
                for variable in [false, true] {
                    let mut encoder = StereoEncoder::new(input, internal);
                    let mut decoder = OpusDecoder::new(OutputRate::Khz48, PcmChannels::Stereo)?;
                    let n = input.hz() as usize * duration.milliseconds() as usize / 1000;
                    for frame in 0..20 {
                        let mut pcm = vec![0i16; 2 * n];
                        for (i, pair) in pcm.chunks_exact_mut(2).enumerate() {
                            let value = (((frame * n + i) % 80) as i32 - 40) * 300;
                            pair[0] = value as i16;
                            pair[1] = if frame < 12 {
                                value as i16
                            } else {
                                (((frame * n + i) * 7919 % 50000) as i32 - 25000) as i16
                            };
                        }
                        let old = encoder.previous_mid_only;
                        let packet = encoder.encode(
                            duration,
                            &pcm,
                            duration.milliseconds() as usize * 8,
                            StereoOptions {
                                complexity: if internal == SilkRate::Khz8 { 0 } else { 6 },
                                variable_bitrate: variable,
                                ..StereoOptions::default()
                            },
                        )?;
                        assert!(packet.as_bytes().len() <= duration.milliseconds() as usize * 8);
                        if !variable {
                            assert_eq!(
                                packet.as_bytes().len(),
                                duration.milliseconds() as usize * 8
                            );
                        }
                        assert_eq!(
                            decoder.decode(packet.as_bytes())?.samples().len(),
                            duration.milliseconds() as usize * 96
                        );
                        assert_eq!(decoder.final_range(), packet.final_range());
                        omissions += usize::from(encoder.previous_mid_only);
                        restarts += usize::from(old && !encoder.previous_mid_only);
                    }
                    let before = encoder.clone();
                    let pcm = vec![0; 2 * n];
                    assert_eq!(
                        encoder.encode(
                            duration,
                            &pcm[..pcm.len() - 1],
                            100,
                            StereoOptions::default()
                        ),
                        Err(StereoPacketError::InvalidInputLength)
                    );
                    for invalid in [
                        StereoOptions {
                            complexity: 11,
                            ..StereoOptions::default()
                        },
                        StereoOptions {
                            packet_loss_percent: 101,
                            ..StereoOptions::default()
                        },
                    ] {
                        assert_eq!(
                            encoder.encode(duration, &pcm, 100, invalid),
                            Err(StereoPacketError::InvalidOptions)
                        );
                    }
                    assert!(
                        encoder
                            .encode(duration, &pcm, 3, StereoOptions::default())
                            .is_err()
                    );
                    assert_eq!(encoder, before);
                    encoder.reset();
                    assert_eq!(encoder, StereoEncoder::new(input, internal));
                }
            }
        }
    }
    assert!(omissions > 100);
    assert!(restarts > 30);
    Ok(())
}
