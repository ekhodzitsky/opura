use super::*;
type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

#[test]
fn original_partial_stereo_restart_retains_mid_and_silence_memory() -> Result {
    let mut cases = 0;
    for line in include_str!("../../../tests/silk-channel-restart-fixtures.txt").lines() {
        if line.starts_with('#') {
            continue;
        }
        let v: Vec<i32> = line
            .split_whitespace()
            .map(|s| s.parse())
            .collect::<std::result::Result<_, _>>()?;
        let mut s = StereoAnalysisState {
            prediction: [v[0] as i16, v[1] as i16],
            mid: [v[2] as i16, v[3] as i16],
            side: [v[4] as i16, v[5] as i16],
            amplitude: [v[6], v[7], v[8], v[9]],
            smoothed_width: v[10] as i16,
            width: v[11] as i16,
            silent_side: v[12] as i16,
        };
        s.restart_stereo();
        let actual: Vec<_> = s
            .prediction
            .into_iter()
            .chain(s.mid)
            .chain(s.side)
            .map(i32::from)
            .chain(s.amplitude)
            .chain([
                i32::from(s.smoothed_width),
                i32::from(s.width),
                i32::from(s.silent_side),
            ])
            .collect();
        assert_eq!(actual, &v[13..], "{line}");
        cases += 1;
    }
    assert_eq!(cases, 32);
    Ok(())
}

fn rate(hz: i64) -> OutputRate {
    match hz {
        8000 => OutputRate::Khz8,
        12000 => OutputRate::Khz12,
        16000 => OutputRate::Khz16,
        24000 => OutputRate::Khz24,
        _ => OutputRate::Khz48,
    }
}
fn duration(ms: i64) -> PacketDuration {
    match ms {
        10 => PacketDuration::Ms10,
        20 => PacketDuration::Ms20,
        40 => PacketDuration::Ms40,
        _ => PacketDuration::Ms60,
    }
}
fn signal(mut seed: u32, count: usize, pattern: i64) -> Vec<i16> {
    (0..count)
        .map(|i| {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            if pattern == 0 {
                0
            } else if pattern == 1 {
                if i % 2 == 0 { i16::MIN } else { i16::MAX }
            } else {
                ((seed & 65535) as i32 - 32768) as i16
            }
        })
        .collect()
}

#[test]
fn original_public_wire_layout_mid_and_resampler_states_match() -> Result {
    let mut encoder = StereoEncoder::new(OutputRate::Khz8, SilkRate::Khz8);
    let mut cases = 0;
    let mut changes = 0;
    for line in include_str!("../../../tests/silk-channel-layout-fixtures.txt").lines() {
        if line.starts_with('#') {
            continue;
        }
        let fields: Vec<_> = line.split_whitespace().collect();
        let v: Vec<i64> = fields[..15]
            .iter()
            .map(|s| s.parse())
            .collect::<std::result::Result<_, _>>()?;
        let input_rate = rate(v[0]);
        let band = [SilkRate::Khz8, SilkRate::Khz12, SilkRate::Khz16][v[1] as usize];
        if v[4] == 0 {
            encoder = StereoEncoder::new(input_rate, band);
        }
        let before = encoder.encoded_channels();
        encoder.set_encoded_channels(v[5] as usize)?;
        encoder.set_dtx(v[6] != 0);
        encoder.set_inband_fec(v[7] != 0);
        let pcm = signal(v[8] as u32, (v[0] * v[2] / 1000 * 2) as usize, v[9]);
        let packet = encoder.encode(
            duration(v[2]),
            &pcm,
            v[2] as usize * 6,
            StereoOptions {
                complexity: 0,
                variable_bitrate: v[3] != 0,
                packet_loss_percent: 20,
            },
        )?;
        assert_eq!(encoder.encoded_channels(), v[10] as usize, "wire: {line}");
        assert_eq!(
            usize::from((packet.as_bytes()[0] & 4) != 0) + 1,
            v[10] as usize,
            "TOC: {line}"
        );
        assert_eq!(encoder.to_mono, v[11] != 0, "smooth request: {line}");
        if encoder.to_mono {
            assert_eq!(
                encoder.speech.stereo.width, 0,
                "native width collapse: {line}"
            );
            assert_eq!(
                encoder.speech.stereo.prediction, [0; 2],
                "native prediction collapse: {line}"
            );
        }
        assert_eq!(
            encoder.speech.stereo.mid,
            [v[12] as i16, v[13] as i16],
            "mid: {line}"
        );
        for ch in 0..2 {
            let expected: u64 = fields[15 + ch].parse()?;
            if expected != 0 {
                assert_eq!(
                    encoder.speech.resampler[ch].native_state_fingerprint(),
                    expected,
                    "resampler{ch}: {line}"
                );
            }
        }
        assert_eq!(
            encoder.speech.mid_input_hash,
            fields[17].parse::<u64>()?,
            "complete mid PCM: {line}"
        );
        changes += usize::from(before != encoder.encoded_channels());
        cases += 1;
    }
    assert_eq!(cases, 4608);
    assert!(changes > 600);
    Ok(())
}

#[test]
fn request_reset_failures_and_cancellation_preserve_live_histories() -> Result {
    let mut encoder = StereoEncoder::new(OutputRate::Khz48, SilkRate::Khz16);
    let options = StereoOptions {
        complexity: 0,
        ..StereoOptions::default()
    };
    let pcm = signal(123456789, 1920, 3);
    encoder.encode(PacketDuration::Ms20, &pcm, 160, options)?;
    for invalid in [0, 3, usize::MAX] {
        let before = encoder.clone();
        assert!(encoder.set_encoded_channels(invalid).is_err());
        assert_eq!(encoder, before);
    }
    let original = encoder.clone();
    encoder.set_encoded_channels(1)?;
    encoder.set_encoded_channels(2)?;
    assert_eq!(encoder, original);
    encoder.set_encoded_channels(1)?;
    let requested = encoder.clone();
    assert!(
        encoder
            .encode(PacketDuration::Ms20, &pcm[..1919], 160, options)
            .is_err()
    );
    assert_eq!(encoder, requested);
    assert!(
        encoder
            .encode(PacketDuration::Ms20, &pcm, 3, options)
            .is_err()
    );
    assert_eq!(encoder, requested);
    encoder.encode(PacketDuration::Ms20, &pcm, 160, options)?;
    assert_eq!(encoder.encoded_channels(), 2);
    assert!(encoder.to_mono);
    let ramped = encoder.clone();
    assert!(
        encoder
            .encode(PacketDuration::Ms20, &pcm, 3, options)
            .is_err()
    );
    assert_eq!(encoder, ramped);
    encoder.encode(PacketDuration::Ms20, &pcm, 160, options)?;
    assert_eq!(encoder.encoded_channels(), 1);
    encoder.reset();
    let mut expected = StereoEncoder::new(OutputRate::Khz48, SilkRate::Khz16);
    expected.set_encoded_channels(1)?;
    assert_eq!(encoder, expected);
    encoder.encode(PacketDuration::Ms20, &pcm, 160, options)?;
    assert_eq!(encoder.encoded_channels(), 1);
    Ok(())
}

#[test]
fn channel_requests_compose_with_automatic_bandwidth_and_budget_errors() -> Result {
    use crate::{Packet, decoder::OpusDecoder, silk::decoder::PcmChannels};
    let mut changes = 0;
    let mut failures = 0;
    for hz in [8000, 12000, 16000, 24000, 48000] {
        for variable in [false, true] {
            let mut encoder = StereoEncoder::new(rate(hz), SilkRate::Khz16);
            encoder.set_automatic_bandwidth(true);
            encoder.set_inband_fec(true);
            let mut decoder = OpusDecoder::new(OutputRate::Khz48, PcmChannels::Stereo)?;
            for frame in 0..64 {
                let d = duration([10, 20, 40, 60][frame % 4]);
                encoder.set_encoded_channels(1 + (frame / 7) % 2)?;
                let previous = encoder.encoded_channels();
                let pcm = signal(
                    0x73746572 + frame as u32,
                    hz as usize * d.milliseconds() as usize / 1000 * 2,
                    if (20..40).contains(&frame) { 0 } else { 3 },
                );
                let options = StereoOptions {
                    complexity: 0,
                    variable_bitrate: variable,
                    packet_loss_percent: 20,
                };
                let before = encoder.clone();
                assert!(encoder.encode(d, &pcm, 2, options).is_err());
                assert_eq!(encoder, before);
                failures += 1;
                let packet = encoder.encode(
                    d,
                    &pcm,
                    d.milliseconds() as usize * if frame < 32 { 3 } else { 6 },
                    options,
                )?;
                let parsed = Packet::parse(packet.as_bytes())?;
                assert_eq!(u32::from(parsed.duration_samples()), d.milliseconds() * 48);
                let decoded = decoder.decode(packet.as_bytes())?;
                assert_eq!(decoded.final_range(), packet.final_range());
                changes += usize::from(previous != encoder.encoded_channels());
            }
        }
    }
    assert_eq!(failures, 640);
    assert!(changes > 60);
    Ok(())
}
