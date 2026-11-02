use super::*;
use crate::{
    decoder::OpusDecoder,
    silk::{decoder::PcmChannels, resampler::OutputRate},
};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
fn rate(khz: i64) -> SilkRate {
    match khz {
        8 => SilkRate::Khz8,
        12 => SilkRate::Khz12,
        _ => SilkRate::Khz16,
    }
}
fn hex(input: &str) -> Result<Vec<u8>> {
    (0..input.len())
        .step_by(2)
        .map(|i| Ok(u8::from_str_radix(&input[i..i + 2], 16)?))
        .collect()
}
fn fingerprint(samples: &[i16]) -> u64 {
    samples
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .fold(14695981039346656037, |h, b| {
            (h ^ u64::from(b)).wrapping_mul(1099511628211)
        })
}
fn indices(i: &FrameIndices) -> Vec<i32> {
    [
        match i.signal {
            SignalType::Inactive => 0,
            SignalType::Unvoiced => 1,
            SignalType::Voiced => 2,
        },
        i32::from(i.offset),
    ]
    .into_iter()
    .chain(i.gains.map(i32::from))
    .chain(i.nlsf.map(i32::from))
    .chain([
        i32::from(i.interpolation_q2),
        i32::from(i.lag),
        i32::from(i.contour),
        i32::from(i.periodicity),
    ])
    .chain(i.ltp.map(i32::from))
    .chain([i32::from(i.ltp_scale), i32::from(i.seed)])
    .collect()
}
#[test]
fn unchanged_public_native_channel_frames_packets_and_pcm_match() -> Result<()> {
    let mut channel = ChannelFrameEncoder::new(SilkRate::Khz8);
    let mut writer = RangeEncoder::new(1275)?;
    let mut flags = 0;
    let mut frames = 0;
    let mut packets = 0;
    let mut retries = 0;
    let mut damage = 0;
    let mut conditional = 0;
    let mut rate_changes = 0;
    let mut native_decoder = OpusDecoder::new(OutputRate::Khz48, PcmChannels::Mono)?;
    let mut own_decoder = OpusDecoder::new(OutputRate::Khz48, PcmChannels::Mono)?;
    let mut header_bits = 0;
    for (line_number, line) in include_str!("../../../tests/silk-encoder-channel-fixtures.txt")
        .lines()
        .enumerate()
    {
        if line.starts_with('#') {
            continue;
        }
        let f: Vec<_> = line.split_whitespace().collect();
        if f[0] == "packet" {
            if f[1] == "1" {
                native_decoder = OpusDecoder::new(OutputRate::Khz48, PcmChannels::Mono)?;
                own_decoder = OpusDecoder::new(OutputRate::Khz48, PcmChannels::Mono)?;
            }
            let original = hex(f[5])?;
            writer.patch_initial_bits(flags << 1, header_bits)?;
            let mut finalized = writer.clone();
            finalized.shrink(finalized.tell().div_ceil(8) as usize)?;
            let mut own = vec![original[0] & 0xf8];
            own.extend_from_slice(finalized.finish()?);
            let expected_count: usize = f[2].parse()?;
            let expected_range: u32 = f[3].parse()?;
            let expected_hash: u64 = f[4].parse()?;
            let original_pcm = native_decoder.decode(&original)?;
            assert_eq!(original_pcm.samples().len(), expected_count);
            assert_eq!(
                fingerprint(original_pcm.samples()),
                expected_hash,
                "native decoder line {}",
                line_number + 1
            );
            assert_eq!(native_decoder.final_range(), expected_range);
            let own_pcm = own_decoder.decode(&own)?;
            assert_eq!(own_pcm.samples().len(), expected_count);
            assert_eq!(
                fingerprint(own_pcm.samples()),
                expected_hash,
                "own encoded packet PCM line {}",
                line_number + 1
            );
            assert_eq!(own_decoder.final_range(), expected_range);
            packets += 1;
            continue;
        }
        assert_eq!(f[0], "frame");
        let v: Vec<i64> = f[1..30]
            .iter()
            .map(|s| s.parse())
            .collect::<std::result::Result<_, _>>()?;
        let nsq_hash: u64 = f[30].parse()?;
        let r = rate(v[0]);
        let khz = v[0] as usize;
        let count = v[1] as usize;
        let length = count * 5 * khz;
        let history: Vec<i16> = f[31..31 + 25 * khz]
            .iter()
            .map(|s| s.parse())
            .collect::<std::result::Result<_, _>>()?;
        if v[14] == 0 {
            channel = ChannelFrameEncoder::new(r);
        }
        if channel.rate != r {
            // Midstream input-rate history conversion belongs to the enclosing
            // frontend; supply its unchanged native result to this fixed-rate core.
            channel = ChannelFrameEncoder::new(r);
            channel.history[..25 * khz].copy_from_slice(&history);
            channel.counter = v[14] as u32;
            rate_changes += 1;
        }
        assert_eq!(channel.counter, v[14] as u32);
        assert_eq!(channel.first, v[15] != 0);
        assert_eq!(
            &channel.history[..25 * khz],
            history,
            "input history line {}",
            line_number + 1
        );
        if v[19] == 0 {
            writer = RangeEncoder::new(1275)?;
            header_bits = v[12] as u32 + 1;
            let cdf = [(256 - (256 >> header_bits)) as u8, 0];
            writer.encode_icdf(0, &cdf, 8)?;
            flags = 0;
        }
        assert_eq!(writer.tell(), v[16] as u32);
        assert_eq!(writer.range(), v[17] as u32);
        assert_eq!(writer.tell_frac(), v[18] as u32);
        let start = 31 + 25 * khz;
        let input: Vec<i16> = f[start..start + length]
            .iter()
            .map(|s| s.parse())
            .collect::<std::result::Result<_, _>>()?;
        let controls = FrameControls {
            duration: if count == 2 {
                FrameDuration::Ms10
            } else {
                FrameDuration::Ms20
            },
            complexity: v[2] as u8,
            snr_db_q7: v[3] as u16,
            activity: SpeechActivity {
                probability_q8: v[8] as u8,
                tilt_q15: v[9] as i16,
                quality_bands_q15: [v[20] as i16, v[21] as i16, v[22] as i16, v[23] as i16],
            },
            active: v[10] != 0,
            constant_bitrate: v[4] != 0,
            fill_budget: v[5] != 0,
            coding: match v[6] {
                0 => PitchCoding::Independent,
                1 => PitchCoding::IndependentWithoutScaling,
                _ => PitchCoding::Conditional,
            },
            max_bits: v[7] as u32,
            packet_loss_percent: v[11] as u8,
            frames_per_packet: v[12] as u8,
            redundancy: v[13] != 0,
        };
        let result = channel
            .encode(&mut writer, &input, controls)
            .map_err(|e| format!("frame line {}: {e}", line_number + 1))?;
        assert_eq!(
            [writer.tell(), writer.tell_frac(), writer.range()],
            [v[24] as u32, v[25] as u32, v[26] as u32],
            "entropy line {}",
            line_number + 1
        );
        assert_eq!(result.bits(), writer.tell());
        assert_eq!(i64::from(channel.gains.previous_index()), v[27]);
        assert_eq!(
            i64::from(result.quantizer_calls()) + i64::from(result.used_damage_control()),
            v[28],
            "native trial calls line {}",
            line_number + 1
        );
        assert_eq!(
            super::super::nsq::tests::fingerprint(&channel.nsq),
            nsq_hash,
            "NSQ history line {}",
            line_number + 1
        );
        let start = start + length;
        let expected: Vec<i32> = f[start..start + 33]
            .iter()
            .map(|s| s.parse())
            .collect::<std::result::Result<_, _>>()?;
        assert_eq!(
            indices(result.indices()),
            expected,
            "selected indices line {}",
            line_number + 1
        );
        let pulses: Vec<i8> = f[start + 33..start + 33 + length]
            .iter()
            .map(|s| s.parse())
            .collect::<std::result::Result<_, _>>()?;
        assert_eq!(
            result.pulses(),
            pulses,
            "selected pulses line {}",
            line_number + 1
        );
        let expected = hex(f.last().ok_or("missing native payload")?)?;
        let mut completed = writer.clone();
        completed.shrink(completed.tell().div_ceil(8) as usize)?;
        assert_eq!(
            completed.finish()?,
            expected,
            "complete frame bytes line {}",
            line_number + 1
        );
        flags = (flags << 1) | u32::from(controls.active);
        frames += 1;
        retries += usize::from(result.quantizer_calls() > 1);
        damage += usize::from(result.used_damage_control());
        conditional += usize::from(controls.coding == PitchCoding::Conditional);
    }
    assert_eq!((frames, packets, conditional), (504, 288, 216));
    assert!(retries > 100);
    assert_eq!(damage, 1);
    assert!(rate_changes > 0);
    Ok(())
}
fn controls(duration: FrameDuration) -> FrameControls {
    FrameControls {
        duration,
        complexity: 0,
        snr_db_q7: 3072,
        activity: SpeechActivity {
            probability_q8: 255,
            tilt_q15: 0,
            quality_bands_q15: [20000; 4],
        },
        active: true,
        constant_bitrate: true,
        fill_budget: true,
        coding: PitchCoding::Independent,
        max_bits: 256,
        packet_loss_percent: 10,
        frames_per_packet: 1,
        redundancy: false,
    }
}
fn header(capacity: usize) -> Result<RangeEncoder> {
    let mut writer = RangeEncoder::new(capacity)?;
    writer.encode_icdf(0, &[192, 0], 8)?;
    Ok(writer)
}
#[test]
fn pure_rust_buffered_pcm_frames_decode_across_rates_durations_and_complexities() -> Result<()> {
    for r in [SilkRate::Khz8, SilkRate::Khz12, SilkRate::Khz16] {
        for duration in [FrameDuration::Ms10, FrameDuration::Ms20] {
            for complexity in [0, 1, 4, 10] {
                let mut channel = ChannelFrameEncoder::new(r);
                let mut vad = super::super::analysis::VoiceActivityDetector::default();
                let mut decoder = OpusDecoder::new(OutputRate::Khz48, PcmChannels::Mono)?;
                let length = r.khz() as usize * 5 * subframes(duration);
                for frame in 0..6 {
                    let input: Vec<i16> = (0..length)
                        .map(|i| {
                            let phase = (frame * length + i) % 80;
                            ((phase.min(80 - phase) as i32 - 20) * 250) as i16
                        })
                        .collect();
                    let activity = vad.analyze(r, duration, &input)?;
                    let c = FrameControls {
                        complexity,
                        activity,
                        active: activity.probability_q8 >= 13,
                        ..controls(duration)
                    };
                    let mut writer = header(1275)?;
                    let result = channel.encode(&mut writer, &input, c)?;
                    assert!((1..=7).contains(&result.iterations()));
                    assert!((1..=result.iterations()).contains(&result.quantizer_calls()));
                    assert!(result.bits() <= c.max_bits);
                    assert!(!writer.is_finished());
                    writer.patch_initial_bits(u32::from(c.active) << 1, 2)?;
                    writer.shrink(writer.tell().div_ceil(8) as usize)?;
                    let base = match r {
                        SilkRate::Khz8 => 0,
                        SilkRate::Khz12 => 4,
                        SilkRate::Khz16 => 8,
                    };
                    let config = base + u8::from(duration == FrameDuration::Ms20);
                    let mut packet = vec![config << 3];
                    packet.extend_from_slice(writer.finish()?);
                    assert_eq!(
                        decoder.decode(&packet)?.samples().len(),
                        if duration == FrameDuration::Ms10 {
                            480
                        } else {
                            960
                        }
                    );
                }
            }
        }
    }
    Ok(())
}
#[test]
fn invalid_controls_late_budget_failure_and_finished_writers_are_atomic() -> Result<()> {
    let mut channel = ChannelFrameEncoder::new(SilkRate::Khz16);
    let c = controls(FrameDuration::Ms20);
    let input: Vec<i16> = (0..320)
        .map(|i| ((i * 733) % 20000 - 10000) as i16)
        .collect();
    let mut writer = header(1275)?;
    channel.encode(&mut writer, &input, c)?;
    let saved = channel.clone();
    let mut writer = header(1275)?;
    let original = writer.clone();
    assert_eq!(
        channel.encode(&mut writer, &input[..319], c),
        Err(FrameError::InvalidInputLength)
    );
    for invalid in [
        FrameControls {
            complexity: 11,
            ..c
        },
        FrameControls {
            snr_db_q7: 8193,
            ..c
        },
        FrameControls {
            packet_loss_percent: 101,
            ..c
        },
        FrameControls {
            frames_per_packet: 0,
            ..c
        },
        FrameControls {
            frames_per_packet: 4,
            ..c
        },
        FrameControls { max_bits: 0, ..c },
        FrameControls {
            max_bits: 10201,
            ..c
        },
        FrameControls {
            activity: SpeechActivity {
                quality_bands_q15: [-1; 4],
                ..c.activity
            },
            ..c
        },
    ] {
        assert_eq!(
            channel.encode(&mut writer, &input, invalid),
            Err(FrameError::InvalidControls)
        );
        assert_eq!(channel, saved);
        assert_eq!(writer, original);
    }
    assert_eq!(
        channel.encode(
            &mut writer,
            &input,
            FrameControls {
                max_bits: original.tell() + 1,
                ..c
            }
        ),
        Err(FrameError::BitBudgetExceeded)
    );
    assert_eq!(channel, saved);
    assert_eq!(writer, original);
    let mut fresh = ChannelFrameEncoder::new(SilkRate::Khz16);
    assert_eq!(
        fresh.encode(
            &mut writer,
            &input,
            FrameControls {
                coding: PitchCoding::Conditional,
                ..c
            }
        ),
        Err(FrameError::Serialization(SilkEncodeError::MissingHistory))
    );
    assert_eq!(fresh, ChannelFrameEncoder::new(SilkRate::Khz16));
    assert_eq!(writer, original);
    writer.finish()?;
    let finished = writer.clone();
    assert_eq!(
        channel.encode(&mut writer, &input, c),
        Err(FrameError::Serialization(SilkEncodeError::Entropy(
            EncoderError::Finished
        )))
    );
    assert_eq!(channel, saved);
    assert_eq!(writer, finished);
    channel.reset();
    assert_eq!(channel, ChannelFrameEncoder::new(SilkRate::Khz16));
    Ok(())
}
#[test]
fn prefill_keeps_fixed_five_ms_delay_and_physical_overflow_is_bounded() -> Result<()> {
    let r = SilkRate::Khz8;
    let mut channel = ChannelFrameEncoder::new(r);
    let input: Vec<i16> = (0..80).map(|i| i * 100 - 4000).collect();
    let saved = channel.clone();
    assert_eq!(
        channel.prefill(FrameDuration::Ms10, &input[..79]),
        Err(FrameError::InvalidInputLength)
    );
    assert_eq!(channel, saved);
    channel.prefill(FrameDuration::Ms10, &input)?;
    assert_eq!(channel.counter, 1);
    assert!(channel.first);
    assert_eq!(&channel.history[160..200], &input[40..]);
    let mut writer = header(1275)?;
    let result = channel.encode(&mut writer, &input, controls(FrameDuration::Ms10))?;
    assert_eq!(result.indices().seed, 1);
    assert!(!channel.first);
    let c = FrameControls {
        snr_db_q7: 4608,
        fill_budget: false,
        max_bits: 10200,
        ..controls(FrameDuration::Ms10)
    };
    let noise: Vec<i16> = (0..80)
        .map(|i| ((i * 7919) % 50000 - 25000) as i16)
        .collect();
    let mut unbounded = channel.clone();
    let mut ample = header(1275)?;
    let baseline = unbounded.encode(&mut ample, &noise, c)?;
    assert!(baseline.bits() > 160);
    for capacity in [3, 8, 16, 24, 32] {
        let mut candidate = channel.clone();
        let mut writer = header(capacity)?;
        let saved = writer.clone();
        match candidate.encode(
            &mut writer,
            &noise,
            FrameControls {
                max_bits: capacity as u32 * 8,
                ..c
            },
        ) {
            Ok(frame) => {
                assert!(frame.bits() <= capacity as u32 * 8);
                assert!(frame.iterations() <= 7);
                writer.finish()?;
            }
            Err(FrameError::BitBudgetExceeded) => {
                assert_eq!(candidate, channel);
                assert_eq!(writer, saved);
            }
            Err(e) => return Err(format!("physical capacity {capacity}: {e}").into()),
        }
    }
    Ok(())
}
