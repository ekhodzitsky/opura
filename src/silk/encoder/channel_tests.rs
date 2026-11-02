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
    native_channel_corpus(
        include_str!("../../../tests/silk-encoder-channel-fixtures.txt"),
        false,
    )
}
#[test]
fn unchanged_native_redundancy_and_main_channel_frames_match() -> Result<()> {
    native_channel_corpus(
        include_str!("../../../tests/silk-encoder-fec-fixtures.txt"),
        true,
    )
}
fn native_channel_corpus(fixtures: &str, with_fec: bool) -> Result<()> {
    let mut fec_metadata = Vec::<i32>::new();
    let mut redundant_frames = Vec::<Option<RedundantFrame>>::new();
    let mut redundant_count = 0;
    let mut soft_target_overruns = 0;
    let mut native_overflows = 0;
    let mut recovery_count = 0;
    let mut recovered_losses = 0;
    let mut protected_recoveries = 0;
    let mut recovery_decoder = OpusDecoder::new(OutputRate::Khz48, PcmChannels::Mono)?;
    let mut current_packet = Vec::new();
    let mut lbrr_flag = false;
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
    for (line_number, line) in fixtures.lines().enumerate() {
        if line.starts_with('#') {
            continue;
        }
        let f: Vec<_> = line.split_whitespace().collect();
        if f[0] == "recover" {
            assert!(with_fec);
            let count: u16 = f[2].parse()?;
            let pcm = if f[1] == "1" {
                recovered_losses += 1;
                protected_recoveries += usize::from(lbrr_flag);
                recovery_decoder.decode_fec(&current_packet, count)?
            } else {
                recovery_decoder.decode(&current_packet)?
            };
            assert_eq!(pcm.samples().len(), usize::from(count));
            assert_eq!(pcm.final_range(), f[3].parse()?);
            assert_eq!(fingerprint(pcm.samples()), f[4].parse()?);
            recovery_count += 1;
            continue;
        }
        if f[0] == "overflow" {
            assert!(with_fec);
            let tell: u32 = f[5].parse()?;
            let maximum: u32 = f[6].parse()?;
            let storage: u32 = f[7].parse()?;
            assert!(tell > maximum && tell > storage * 8);
            let fallback = hex(f[9])?;
            assert_eq!(fallback.len(), f[8].parse::<usize>()?);
            let packet = crate::Packet::parse(&fallback)?;
            assert_eq!(packet.frame_samples(), 2880);
            assert_eq!(packet.frames(), &[&[0u8][..]]);
            native_overflows += 1;
            continue;
        }
        if f[0] == "fec" {
            fec_metadata = f[1..]
                .iter()
                .map(|v| v.parse())
                .collect::<std::result::Result<_, _>>()?;
            continue;
        }
        if f[0] == "packet" {
            if f[1] == "1" {
                native_decoder = OpusDecoder::new(OutputRate::Khz48, PcmChannels::Mono)?;
                own_decoder = OpusDecoder::new(OutputRate::Khz48, PcmChannels::Mono)?;
                recovery_decoder = OpusDecoder::new(OutputRate::Khz48, PcmChannels::Mono)?;
            }
            let original = hex(f[5])?;
            writer.patch_initial_bits((flags << 1) | u32::from(lbrr_flag), header_bits)?;
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
            current_packet = own;
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
            redundant_frames.clear();
        }
        if channel.rate != r {
            // The public native corpus uses48kHz API input. A rate change
            // replaces the old live resampler; it does not read that state.
            let mut resampler = EncoderResamplerState::new(OutputRate::Khz48, channel.rate);
            channel.change_rate(
                OutputRate::Khz48,
                r,
                if count == 2 {
                    FrameDuration::Ms10
                } else {
                    FrameDuration::Ms20
                },
                &mut resampler,
            )?;
            redundant_frames.clear();
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
            let previous = std::mem::take(&mut redundant_frames);
            let mask = previous
                .iter()
                .enumerate()
                .fold(0, |mask, (i, f)| mask | (usize::from(f.is_some()) << i));
            lbrr_flag = mask != 0;
            assert_eq!(lbrr_flag, v[13] != 0);
            if lbrr_flag && v[12] > 1 {
                writer.encode_icdf(
                    mask - 1,
                    if v[12] == 2 {
                        &[203, 150, 0]
                    } else {
                        &[215, 195, 166, 125, 110, 82, 0]
                    },
                    8,
                )?;
            }
            for (i, frame) in previous.iter().enumerate() {
                if let Some(frame) = frame {
                    channel.encode_redundant(
                        &mut writer,
                        frame,
                        if i > 0 && previous[i - 1].is_some() {
                            PitchCoding::Conditional
                        } else {
                            PitchCoding::Independent
                        },
                    )?;
                }
            }
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
        let fec_controls = if with_fec && fec_metadata[0] != 0 {
            Some(RedundancyControls {
                gain_increase: fec_metadata[1] as u8,
                reset_gain_history: fec_metadata[2] != 0,
            })
        } else {
            None
        };
        let result = channel
            .encode_with_redundancy(
                &mut writer,
                &input,
                controls,
                if with_fec {
                    1275 * 8
                } else {
                    controls.max_bits
                },
                fec_controls,
            )
            .map_err(|e| format!("frame line {}: {e}", line_number + 1))?;
        assert_eq!(
            [writer.tell(), writer.tell_frac(), writer.range()],
            [v[24] as u32, v[25] as u32, v[26] as u32],
            "entropy line {}",
            line_number + 1
        );
        assert_eq!(result.bits(), writer.tell());
        soft_target_overruns += usize::from(writer.tell() > controls.max_bits);
        if with_fec {
            assert_eq!(
                result.redundancy().is_some(),
                fec_metadata[4] != 0,
                "redundancy presence line{}",
                line_number + 1
            );
            if let Some(redundant) = result.redundancy() {
                assert_eq!(
                    indices(redundant.indices()),
                    fec_metadata[5..38],
                    "redundant indices line{}",
                    line_number + 1
                );
                assert_eq!(
                    redundant
                        .pulses()
                        .iter()
                        .map(|&v| i32::from(v))
                        .collect::<Vec<_>>(),
                    fec_metadata[38..],
                    "redundant pulses line{}",
                    line_number + 1
                );
                assert_eq!(i32::from(channel.redundant_gain), fec_metadata[3]);
                redundant_count += 1;
            }
            redundant_frames.push(result.redundancy().cloned());
        }
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
    if with_fec {
        assert_eq!((frames, packets, conditional), (483, 281, 202));
        assert_eq!(native_overflows, 1);
        assert_eq!(recovery_count, 280);
        assert_eq!(recovered_losses, 93);
        assert_eq!(protected_recoveries, 46);
        assert!(soft_target_overruns > 0);
        assert!(redundant_count > 100);
        assert!(retries > 0);
    } else {
        assert_eq!((frames, packets, conditional), (504, 288, 216));
        assert_eq!(native_overflows, 0);
        assert!(retries > 100);
        assert_eq!(damage, 1);
        assert!(rate_changes > 0);
    }
    Ok(())
}
#[test]
fn redundant_quantization_isolated_from_main_and_errors_are_atomic() -> Result<()> {
    let mut main = ChannelFrameEncoder::new(SilkRate::Khz8);
    let mut redundant = main.clone();
    let mut main_writer = header(1275)?;
    let mut redundant_writer = main_writer.clone();
    let input: Vec<i16> = (0..160).map(|i| ((i % 80) as i16 - 40) * 200).collect();
    let c = controls(FrameDuration::Ms20);
    let fec = Some(RedundancyControls {
        gain_increase: 7,
        reset_gain_history: true,
    });
    let ordinary = main.encode(&mut main_writer, &input, c)?;
    let protected =
        redundant.encode_with_redundancy(&mut redundant_writer, &input, c, c.max_bits, fec)?;
    assert_eq!(ordinary.indices(), protected.indices());
    assert_eq!(ordinary.pulses(), protected.pulses());
    assert_eq!(main.nsq, redundant.nsq);
    assert_eq!(main.gains, redundant.gains);
    assert_eq!(main_writer, redundant_writer);
    let frame = protected.redundancy().ok_or("missing redundant frame")?;
    let saved = redundant.clone();
    let original_writer = redundant_writer.clone();
    assert!(
        redundant
            .encode_with_redundancy(
                &mut redundant_writer,
                &input,
                c,
                c.max_bits,
                Some(RedundancyControls {
                    gain_increase: 8,
                    reset_gain_history: true
                })
            )
            .is_err()
    );
    assert!(
        redundant
            .encode_with_redundancy(&mut redundant_writer, &input, c, c.max_bits - 1, fec)
            .is_err()
    );
    let tiny = FrameControls { max_bits: 16, ..c };
    assert!(
        redundant
            .encode_with_redundancy(&mut redundant_writer, &input, tiny, 16, fec)
            .is_err()
    );
    assert_eq!(redundant, saved);
    assert_eq!(redundant_writer, original_writer);
    let mut fresh = ChannelFrameEncoder::new(SilkRate::Khz8);
    let before = fresh.clone();
    let mut writer = header(1275)?;
    let before_writer = writer.clone();
    assert!(
        fresh
            .encode_redundant(&mut writer, frame, PitchCoding::Conditional)
            .is_err()
    );
    assert_eq!(fresh, before);
    assert_eq!(writer, before_writer);
    writer.finish()?;
    let finished = writer.clone();
    assert!(
        redundant
            .encode_redundant(&mut writer, frame, PitchCoding::Independent)
            .is_err()
    );
    assert_eq!(redundant, saved);
    assert_eq!(writer, finished);
    Ok(())
}
#[test]
fn native_history_migration_and_primed_resampler_pcm_match() -> Result<()> {
    let mut cases = 0;
    for line in include_str!("../../../tests/silk-encoder-rate-change-fixtures.txt").lines() {
        if line.starts_with('#') {
            continue;
        }
        let values: Vec<u64> = line
            .split_whitespace()
            .map(|s| s.parse())
            .collect::<std::result::Result<_, _>>()?;
        let old = rate(values[0] as i64);
        let next = rate(values[1] as i64);
        let input = match values[2] {
            8000 => OutputRate::Khz8,
            12000 => OutputRate::Khz12,
            16000 => OutputRate::Khz16,
            24000 => OutputRate::Khz24,
            _ => OutputRate::Khz48,
        };
        let duration = if values[3] == 10 {
            FrameDuration::Ms10
        } else {
            FrameDuration::Ms20
        };
        let mut channel = ChannelFrameEncoder::new(old);
        channel.counter = 41;
        let mut history_random = values[4] as u32;
        for sample in &mut channel.history {
            history_random = history_random
                .wrapping_mul(1664525)
                .wrapping_add(1013904223);
            *sample = ((history_random >> 16) as i32 - 32768) as i16;
        }
        let mut random = values[5] as u32;
        let mut samples = |length: usize| -> Vec<i16> {
            (0..length)
                .map(|_| {
                    random = random.wrapping_mul(1664525).wrapping_add(1013904223);
                    ((random >> 16) as i32 - 32768) as i16
                })
                .collect()
        };
        let mut resampler = EncoderResamplerState::new(input, old);
        let mut out = [0i16; 320];
        resampler.process(
            &samples(values[3] as usize * input.hz() as usize / 1000),
            &mut out,
        )?;
        channel.change_rate(input, next, duration, &mut resampler)?;
        assert_eq!(channel.internal_rate(), next);
        assert_eq!(channel.counter, 41);
        assert_eq!(fingerprint(&channel.history), values[6]);
        for block in 0..8 {
            let ms = if block % 2 == 0 { 10 } else { 20 };
            let n = resampler.process(&samples(ms * input.hz() as usize / 1000), &mut out)?;
            assert_eq!(n, ms * next.khz() as usize);
            assert_eq!(fingerprint(&out[..n]), values[7 + block]);
        }
        cases += 1;
    }
    assert_eq!(cases, 90);
    Ok(())
}
#[test]
fn rate_change_clears_prediction_keeps_stream_state_and_validates_resampler() -> Result<()> {
    let mut channel = ChannelFrameEncoder::new(SilkRate::Khz8);
    let pcm: Vec<i16> = (0..160).map(|i| ((i % 80) as i16 - 40) * 200).collect();
    let mut writer = header(1275)?;
    channel.encode(&mut writer, &pcm, controls(FrameDuration::Ms20))?;
    let before = channel.clone();
    let mut mismatch = EncoderResamplerState::new(OutputRate::Khz48, SilkRate::Khz16);
    let saved = mismatch.clone();
    assert_eq!(
        channel.change_rate(
            OutputRate::Khz48,
            SilkRate::Khz12,
            FrameDuration::Ms20,
            &mut mismatch
        ),
        Err(FrameError::InvalidControls)
    );
    assert_eq!(channel, before);
    assert_eq!(mismatch, saved);
    let mut resampler = EncoderResamplerState::new(OutputRate::Khz48, SilkRate::Khz8);
    let saved = resampler.clone();
    channel.change_rate(
        OutputRate::Khz48,
        SilkRate::Khz8,
        FrameDuration::Ms20,
        &mut resampler,
    )?;
    assert_eq!(channel, before);
    assert_eq!(resampler, saved);
    channel.change_rate(
        OutputRate::Khz48,
        SilkRate::Khz16,
        FrameDuration::Ms20,
        &mut resampler,
    )?;
    assert_eq!(channel.counter, before.counter);
    assert_eq!(channel.entropy, before.entropy);
    assert_eq!(channel.redundant_gain, before.redundant_gain);
    assert_eq!(channel.ltp, before.ltp);
    assert!(channel.first);
    assert_eq!(channel.nsq, NoiseShapingQuantizer::default());
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
