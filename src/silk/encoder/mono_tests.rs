use super::*;
use crate::{Packet, decoder::OpusDecoder, silk::decoder::PcmChannels};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
#[test]
fn all_rates_bandwidths_and_packet_durations_decode_and_reset() -> Result<()> {
    for input in [
        OutputRate::Khz8,
        OutputRate::Khz12,
        OutputRate::Khz16,
        OutputRate::Khz24,
        OutputRate::Khz48,
    ] {
        for bandwidth in [SilkRate::Khz8, SilkRate::Khz12, SilkRate::Khz16] {
            for duration in [
                PacketDuration::Ms10,
                PacketDuration::Ms20,
                PacketDuration::Ms40,
                PacketDuration::Ms60,
            ] {
                for variable in [false, true] {
                    let mut encoder = MonoEncoder::new(input, bandwidth);
                    let mut decoder = OpusDecoder::new(OutputRate::Khz48, PcmChannels::Mono)?;
                    let n = input.hz() as usize * duration.milliseconds() as usize / 1000;
                    let bytes = duration.milliseconds() as usize * 4;
                    for frame in 0..3 {
                        let pcm: Vec<i16> = (0..n)
                            .map(|i| (((frame * n + i) * 991 % 30000) as i32 - 15000) as i16)
                            .collect();
                        let packet = encoder.encode(
                            duration,
                            &pcm,
                            bytes,
                            MonoOptions {
                                complexity: if bandwidth == SilkRate::Khz8 { 0 } else { 6 },
                                variable_bitrate: variable,
                                ..MonoOptions::default()
                            },
                        )?;
                        assert!(packet.as_bytes().len() <= bytes);
                        if !variable {
                            assert_eq!(packet.as_bytes().len(), bytes);
                        }
                        let parsed = Packet::parse(packet.as_bytes())?;
                        assert_eq!(
                            u32::from(parsed.duration_samples()),
                            duration.milliseconds() * 48
                        );
                        assert_eq!(
                            decoder.decode(packet.as_bytes())?.samples().len(),
                            duration.milliseconds() as usize * 48
                        );
                        assert_eq!(decoder.final_range(), packet.final_range());
                    }
                    encoder.reset();
                    assert_eq!(encoder, MonoEncoder::new(input, bandwidth));
                }
            }
        }
    }
    Ok(())
}
#[test]
fn padding_preserves_only_the_payload_at_all_packet_boundaries() -> Result<()> {
    for n in [1, 17, 253, 254, 255, 256, 509, 1000, 1275] {
        let payload = vec![0x55; n];
        for length in n + 1..=1276 {
            let encoded = packet(8, &payload, length, 123);
            let parsed = Packet::parse(encoded.as_bytes())?;
            assert_eq!(parsed.frames(), &[payload.as_slice()]);
            assert_eq!(encoded.as_bytes().len(), length);
        }
    }
    Ok(())
}
#[test]
fn packet_errors_preserve_input_and_codec_histories() -> Result<()> {
    let mut encoder = MonoEncoder::new(OutputRate::Khz48, SilkRate::Khz16);
    let input = vec![3000; 960];
    let options = MonoOptions::default();
    encoder.encode(PacketDuration::Ms20, &input, 80, options)?;
    let before = encoder.clone();
    assert_eq!(
        encoder.encode(PacketDuration::Ms20, &input[..959], 80, options),
        Err(MonoError::InvalidInputLength)
    );
    for invalid in [
        MonoOptions {
            complexity: 11,
            ..options
        },
        MonoOptions {
            packet_loss_percent: 101,
            ..options
        },
    ] {
        assert_eq!(
            encoder.encode(PacketDuration::Ms20, &input, 80, invalid),
            Err(MonoError::InvalidOptions)
        );
    }
    for bytes in [0, 2, 1277] {
        assert_eq!(
            encoder.encode(PacketDuration::Ms20, &input, bytes, options),
            Err(MonoError::InvalidOptions)
        );
    }
    assert!(
        encoder
            .encode(PacketDuration::Ms20, &input, 3, options)
            .is_err()
    );
    assert_eq!(encoder, before);
    Ok(())
}

#[test]
fn original_dc_rejection_matches_pcm_and_filter_history() -> Result<()> {
    let mut cases = 0;
    for line in include_str!("../../../tests/silk-encoder-dc-fixtures.txt").lines() {
        if line.starts_with('#') {
            continue;
        }
        let fields: Vec<_> = line.split_whitespace().collect();
        let rate = match fields[0] {
            "8000" => OutputRate::Khz8,
            "12000" => OutputRate::Khz12,
            "16000" => OutputRate::Khz16,
            "24000" => OutputRate::Khz24,
            _ => OutputRate::Khz48,
        };
        let mut state = fields[1].parse()?;
        let input: Vec<i16> = fields[4..]
            .iter()
            .map(|s| s.parse())
            .collect::<std::result::Result<_, _>>()?;
        let mut output = vec![0; input.len()];
        dc_reject(&mut state, rate, &input, &mut output);
        let hash = output
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .fold(14695981039346656037u64, |h, b| {
                (h ^ u64::from(b)).wrapping_mul(1099511628211)
            });
        assert_eq!(state, fields[2].parse::<i32>()?);
        assert_eq!(hash, fields[3].parse::<u64>()?);
        cases += 1;
    }
    assert_eq!(cases, 40);
    Ok(())
}
