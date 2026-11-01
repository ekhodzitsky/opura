//! Real encoded audio reference checks and mono packet transaction boundaries.
use opura::{
    Packet,
    silk::{
        decode_header,
        mono::{MonoDecoder, MonoError},
    },
};

fn bytes(hex: &str) -> Result<Vec<u8>, std::num::ParseIntError> {
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16))
        .collect()
}

#[test]
fn real_audio_packets_match_reference_pcm_and_public_final_range()
-> Result<(), Box<dyn std::error::Error>> {
    let mut decoder = MonoDecoder::default();
    let mut count = 0;
    let mut rates = [false; 3];
    let mut durations = [false; 4];
    let mut masks = [false; 8];
    let mut kinds = [0; 3];
    let mut codes = [false; 4];
    let mut inactive = false;
    let mut maximum_samples = 0;
    for line in include_str!("silk-packet-fixtures.txt")
        .lines()
        .filter(|l| !l.starts_with('#'))
    {
        let columns: Vec<_> = line.split_whitespace().collect();
        let kind = columns[0];
        kinds[match kind {
            "packet" => 0,
            "symbols" => 1,
            "transition" => 2,
            _ => panic!("bad fixture kind"),
        }] += 1;
        let row = &columns[1..];
        if row[0] == "1" {
            decoder.reset();
        }
        let data = bytes(row[1])?;
        let packet = Packet::parse(&data)?;
        codes[(data[0] & 3) as usize] = true;
        let (header, _) = decode_header(&packet, 0)?;
        inactive |= header
            .voice_activity(0)
            .ok_or("missing VAD")?
            .iter()
            .any(|&v| !v);
        let mask = header
            .redundancy(0)
            .ok_or("missing mono header")?
            .iter()
            .enumerate()
            .fold(0, |mask, (i, &flag)| mask | (usize::from(flag) << i));
        masks[mask] = true;
        durations[match packet.frame_samples() {
            480 => 0,
            960 => 1,
            1920 => 2,
            2880 => 3,
            _ => panic!("bad duration"),
        }] = true;
        if kind == "transition" {
            let before = decoder.clone();
            assert_eq!(decoder.decode(&data), Err(MonoError::UnsupportedTransition));
            assert_eq!(decoder, before);
            count += 1;
            continue;
        }
        let pcm = decoder
            .decode(&data)
            .map_err(|e| format!("packet {count}: {e}"))?;
        assert_eq!(pcm.sample_rate(), row[2].parse::<u32>()? * 1000);
        rates[match pcm.sample_rate() {
            8000 => 0,
            12000 => 1,
            16000 => 2,
            _ => panic!("bad rate"),
        }] = true;
        assert_eq!(pcm.final_range(), row[3].parse::<u32>()?, "packet {count}");
        let expected = row[4..]
            .iter()
            .map(|v| v.parse::<i16>())
            .collect::<Result<Vec<_>, _>>()?;
        assert_eq!(pcm.samples(), expected, "packet {count}");
        maximum_samples = maximum_samples.max(pcm.samples().len());
        count += 1;
    }
    assert_eq!(count, 276);
    assert_eq!(rates, [true; 3]);
    assert_eq!(durations, [true; 4]);
    assert_eq!(masks, [true; 8]);
    assert_eq!(kinds, [222, 48, 6]);
    assert_eq!(codes, [true; 4]);
    assert!(inactive);
    assert_eq!(maximum_samples, 1920);
    Ok(())
}

fn first_packet_at_rate(rate: u32) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    for line in include_str!("silk-packet-fixtures.txt")
        .lines()
        .filter(|l| !l.starts_with('#'))
    {
        let row: Vec<_> = line.split_whitespace().collect();
        if row[0] == "packet" && row[1] == "1" && row[3].parse::<u32>()? == rate {
            return Ok(bytes(row[2])?);
        }
    }
    Err("missing rate fixture".into())
}

fn length_field(length: usize, data: &mut Vec<u8>) {
    assert!(length <= 1275);
    if length < 252 {
        data.push(length as u8);
    } else {
        let first = 252 + (length & 3);
        data.extend([first as u8, ((length - first) / 4) as u8]);
    }
}

fn repacketize(
    packets: &[Vec<u8>],
    code: u8,
    padded: bool,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let frames = packets
        .iter()
        .map(|p| Packet::parse(p))
        .collect::<Result<Vec<_>, _>>()?;
    let payloads: Vec<_> = frames
        .iter()
        .map(|p| {
            assert_eq!(p.frames().len(), 1);
            p.frames()[0]
        })
        .collect();
    let mut result = vec![(packets[0][0] & !3) | code];
    match code {
        1 => assert_eq!(payloads[0].len(), payloads[1].len()),
        2 => length_field(payloads[0].len(), &mut result),
        3 => {
            result.push(0x80 | if padded { 0x40 } else { 0 } | payloads.len() as u8);
            if padded {
                result.push(7);
            }
            for payload in &payloads[..payloads.len() - 1] {
                length_field(payload.len(), &mut result);
            }
        }
        _ => panic!("bad packet code"),
    }
    for payload in payloads {
        result.extend_from_slice(payload);
    }
    if padded {
        result.extend_from_slice(&[0x9a; 7]);
    }
    Ok(result)
}

#[test]
fn packet_codes_padding_and_120_ms_bound_match_sequential_decoding()
-> Result<(), Box<dyn std::error::Error>> {
    for rate in [8, 12, 16] {
        let data = first_packet_at_rate(rate)?;
        assert_eq!(Packet::parse(&data)?.frame_samples(), 480);
        for (code, count, padding) in [(1, 2, false), (2, 2, false), (3, 3, false), (3, 12, true)] {
            let packets = vec![data.clone(); count];
            let mut sequential = MonoDecoder::default();
            let mut expected = Vec::new();
            let mut final_range = 0;
            for packet in &packets {
                let pcm = sequential.decode(packet)?;
                expected.extend_from_slice(pcm.samples());
                final_range = pcm.final_range();
            }
            let mut together = MonoDecoder::default();
            let packed = repacketize(&packets, code, padding)?;
            let result = together.decode(&packed)?;
            assert_eq!(result.samples(), expected);
            assert_eq!(result.final_range(), final_range);
            assert_eq!(result.sample_rate(), rate * 1000);
            assert_eq!(together, sequential);
            if count == 12 {
                assert_eq!(result.samples().len(), (rate * 120) as usize);
            }
        }
    }
    Ok(())
}

#[test]
fn errors_preserve_full_packet_state_and_retry_matches_clean_decoder()
-> Result<(), Box<dyn std::error::Error>> {
    let good = first_packet_at_rate(8)?;
    let mut decoder = MonoDecoder::default();
    decoder.decode(&good)?;
    let before = decoder.clone();
    for (data, error) in [
        (vec![], MonoError::Framing(opura::PacketError::EmptyPacket)),
        (vec![0x80, 0, 0], MonoError::UnsupportedMode),
        (vec![0x60, 0, 0], MonoError::UnsupportedMode),
        (vec![0x04, 0, 0], MonoError::UnsupportedChannels),
        (vec![0x00], MonoError::MissingPayload),
        (vec![0x00, 0], MonoError::MissingPayload),
    ] {
        assert_eq!(decoder.decode(&data), Err(error));
        assert_eq!(decoder, before);
    }
    // Valid first Opus frame, unsupported transition tail in the second.
    // The error is after both frames have run synthesis in the temporary state.
    let mut bad = good.clone();
    bad.extend_from_slice(&[0; 32]);
    let late_failure = repacketize(&[good.clone(), bad], 2, false)?;
    assert_eq!(
        decoder.decode(&late_failure),
        Err(MonoError::UnsupportedTransition)
    );
    assert_eq!(decoder, before);
    let mut control = before;
    assert_eq!(decoder.decode(&good)?, control.decode(&good)?);
    assert_eq!(decoder, control);
    decoder.reset();
    assert_eq!(decoder, MonoDecoder::default());
    assert_eq!(
        decoder.decode(&good)?,
        MonoDecoder::default().decode(&good)?
    );
    Ok(())
}

#[test]
fn short_and_seeded_inputs_have_bounded_outputs_or_transactional_errors()
-> Result<(), Box<dyn std::error::Error>> {
    let good = first_packet_at_rate(16)?;
    let mut decoder = MonoDecoder::default();
    decoder.decode(&good)?;
    let before = decoder.clone();
    let mut seed = 0x73696c6bu32;
    for configuration in 0..12 {
        for length in 0..=64 {
            let mut data = vec![(configuration << 3) as u8];
            for _ in 0..length {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                data.push(seed as u8);
            }
            match decoder.decode(&data) {
                Ok(pcm) => {
                    assert!(pcm.samples().len() <= 1920);
                    let packet = Packet::parse(&data)?;
                    assert_eq!(
                        pcm.samples().len() as u32,
                        u32::from(packet.duration_samples()) * pcm.sample_rate() / 48000
                    );
                }
                Err(_) => assert_eq!(decoder, before),
            }
            decoder = before.clone();
        }
    }
    Ok(())
}

#[test]
fn encoder_trimmed_zero_bytes_use_packet_budget_without_changing_primitive_contract()
-> Result<(), Box<dyn std::error::Error>> {
    use opura::silk::{
        channel::{ChannelError, ChannelState},
        excitation::ExcitationError,
        gains::{FrameActivity, FrameDuration},
        pitch::{PitchCoding, SilkRate},
    };
    // Real pinned encoder packet: upstream consumes 324 whole bits from a
    // 320-bit transmitted payload after stripping its trailing zero byte.
    let data = bytes(
        "08a9401ce93e5b32a2a43f35964fab23eb1a544c513efc1cd632007490590c8606cd1cdd71f9c69343",
    )?;
    let packet = Packet::parse(&data)?;
    let (_, mut entropy) = decode_header(&packet, 0)?;
    let mut primitive = ChannelState::default();
    let before = primitive.clone();
    let cursor = entropy.clone();
    assert_eq!(
        primitive.decode(
            &mut entropy,
            SilkRate::Khz8,
            FrameDuration::Ms20,
            FrameActivity::Active,
            PitchCoding::Independent
        ),
        Err(ChannelError::Excitation(ExcitationError::BitBudgetExceeded))
    );
    assert_eq!(primitive, before);
    assert_eq!(entropy, cursor);
    let pcm = MonoDecoder::default().decode(&data)?;
    assert_eq!(pcm.final_range(), 522643712);
    assert_eq!(pcm.samples().len(), 160);
    Ok(())
}
