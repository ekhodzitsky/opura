//! Complete SILK packet output against public reference PCM.
use opura::silk::{
    decoder::{DecoderError, PcmChannels, SilkDecoder},
    resampler::OutputRate,
};

fn bytes(hex: &str) -> Result<Vec<u8>, std::num::ParseIntError> {
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16))
        .collect()
}
fn rate(hz: u32) -> OutputRate {
    match hz {
        8000 => OutputRate::Khz8,
        12000 => OutputRate::Khz12,
        16000 => OutputRate::Khz16,
        24000 => OutputRate::Khz24,
        _ => OutputRate::Khz48,
    }
}

#[test]
fn mono_reference_outputs_remain_exact_in_channel_general_decoder()
-> Result<(), Box<dyn std::error::Error>> {
    let mut decoder = SilkDecoder::new(OutputRate::Khz8, PcmChannels::Mono);
    let mut count = 0;
    for line in include_str!("silk-output-fixtures.txt")
        .lines()
        .filter(|l| !l.starts_with('#'))
    {
        let row: Vec<_> = line.split_whitespace().collect();
        let target = rate(row[2].parse()?);
        if decoder.output_rate() != target {
            decoder = SilkDecoder::new(target, PcmChannels::Mono);
        }
        if row[1] == "1" {
            decoder.reset();
        }
        let data = if row[5] == "-" {
            Vec::new()
        } else {
            bytes(row[5])?
        };
        let result = match row[0] {
            "packet" => decoder.decode(&data)?,
            "fec" => decoder.decode_fec(&data)?,
            "loss" => decoder.conceal(row[4].parse()?)?,
            "transition" => {
                let before = decoder.clone();
                assert_eq!(
                    decoder.decode(&data),
                    Err(DecoderError::UnsupportedTransition)
                );
                assert_eq!(decoder, before);
                count += 1;
                continue;
            }
            _ => panic!("bad reference kind"),
        };
        let expected: Vec<_> = row[7..]
            .iter()
            .map(|s| s.parse::<i16>())
            .collect::<Result<_, _>>()?;
        assert_eq!(result.samples(), expected, "PCM case {count}");
        assert_eq!(result.sample_rate(), target.hz());
        assert_eq!(result.channels(), 1);
        assert_eq!(result.samples_per_channel(), expected.len());
        assert_eq!(result.final_range(), row[6].parse::<u32>()?);
        count += 1;
    }
    assert_eq!(count, 260);
    Ok(())
}

#[test]
fn stereo_public_pcm_and_range_match_across_layout_loss_and_fec()
-> Result<(), Box<dyn std::error::Error>> {
    let mut decoder = SilkDecoder::new(OutputRate::Khz8, PcmChannels::Mono);
    let mut count = 0;
    let mut coverage = 0u32;
    let mut geometry = [[false; 2]; 5];
    let mut masks = [[[[false; 8]; 8]; 3]; 2];
    let mut rate_pairs = [[false; 3]; 5];
    let mut maximum = 0;
    for line in include_str!("silk-decoder-fixtures.txt")
        .lines()
        .filter(|l| !l.starts_with('#'))
    {
        let row: Vec<_> = line.split_whitespace().collect();
        let target = rate(row[2].parse()?);
        let channels = match row[3] {
            "1" => PcmChannels::Mono,
            "2" => PcmChannels::Stereo,
            _ => panic!("channels"),
        };
        if row[1] == "1" || decoder.output_rate() != target || decoder.output_channels() != channels
        {
            decoder = SilkDecoder::new(target, channels);
        }
        let data = if row[6] == "-" {
            Vec::new()
        } else {
            bytes(row[6])?
        };
        if count >= 1000 && row[0] == "fec" {
            let packet = opura::Packet::parse(&data)?;
            let (header, _) = opura::silk::decode_header(&packet, 0)?;
            let mut value = [0usize; 2];
            for (channel, mask) in value.iter_mut().enumerate() {
                *mask = header
                    .redundancy(channel)
                    .ok_or("missing redundancy")?
                    .iter()
                    .enumerate()
                    .fold(0, |v, (i, flag)| v | (usize::from(*flag) << i));
            }
            let input = match row[4] {
                "8" => 0,
                "12" => 1,
                "16" => 2,
                _ => panic!("rate"),
            };
            masks[channels.count() - 1][input][value[0]][value[1]] = true;
        }
        let before = decoder.clone();
        let result = match row[0] {
            "packet" => decoder.decode(&data),
            "fec" => decoder.decode_fec(&data),
            "loss" => decoder.conceal(row[5].parse()?),
            "transition" => {
                assert_eq!(
                    decoder.decode(&data),
                    Err(DecoderError::UnsupportedTransition)
                );
                assert_eq!(decoder, before);
                count += 1;
                continue;
            }
            _ => panic!("kind"),
        };
        let pcm = result.map_err(|e| format!("case {count} {}: {e}", row[0]))?;
        let expected: Vec<_> = row[9..]
            .iter()
            .map(|s| s.parse::<i16>())
            .collect::<Result<_, _>>()?;
        assert_eq!(pcm.final_range(), row[7].parse::<u32>()?, "range {count}");
        assert_eq!(pcm.samples().len(), expected.len(), "length {count}");
        if let Some(i) = pcm
            .samples()
            .iter()
            .zip(&expected)
            .position(|(a, b)| a != b)
        {
            panic!(
                "PCM case {count} {} at {i}: {} != {}",
                row[0],
                pcm.samples()[i],
                expected[i]
            );
        }
        assert_eq!(pcm.channels(), channels.count());
        assert_eq!(pcm.sample_rate(), target.hz());
        assert_eq!(
            pcm.samples_per_channel(),
            row[5].parse::<usize>()? * target.hz() as usize / 1000
        );
        coverage |= row[8].parse::<u32>()?;
        let r = match target {
            OutputRate::Khz8 => 0,
            OutputRate::Khz12 => 1,
            OutputRate::Khz16 => 2,
            OutputRate::Khz24 => 3,
            OutputRate::Khz48 => 4,
        };
        geometry[r][channels.count() - 1] = true;
        let input = match row[4] {
            "8" => 0,
            "12" => 1,
            "16" => 2,
            _ => panic!("rate"),
        };
        rate_pairs[r][input] = true;
        maximum = maximum.max(pcm.samples().len());
        count += 1;
    }
    assert_eq!(count, 1768);
    assert_eq!(coverage, 255, "observed reference layout branches");
    assert_eq!(geometry, [[true; 2]; 5]);
    assert_eq!(rate_pairs, [[true; 3]; 5]);
    assert_eq!(masks, [[[[true; 8]; 8]; 3]; 2]);
    assert_eq!(maximum, 11520);
    Ok(())
}

#[test]
fn failures_roll_back_both_channels_output_history_and_configuration()
-> Result<(), Box<dyn std::error::Error>> {
    let row: Vec<_> = include_str!("silk-decoder-fixtures.txt")
        .lines()
        .find(|l| l.starts_with("packet 1 48000 2 "))
        .ok_or("missing stereo seed")?
        .split_whitespace()
        .collect();
    let good = bytes(row[6])?;
    let mut decoder = SilkDecoder::new(OutputRate::Khz48, PcmChannels::Stereo);
    assert_eq!(decoder.conceal(20), Err(DecoderError::MissingHistory));
    decoder.decode(&good)?;
    let before = decoder.clone();
    for data in [
        vec![],
        vec![0x80, 0, 0],
        vec![0x60, 0, 0],
        vec![0],
        vec![0, 0],
        vec![0x03, 0],
    ] {
        assert!(decoder.decode(&data).is_err());
        assert_eq!(decoder, before);
        assert!(decoder.decode_fec(&data).is_err());
        assert_eq!(decoder, before);
    }
    for duration in [0, 1, 9, 11, 121, u16::MAX] {
        assert_eq!(
            decoder.conceal(duration),
            Err(DecoderError::InvalidLossDuration)
        );
        assert_eq!(decoder, before);
    }
    let budget_row: Vec<_> = include_str!("silk-fec-fixtures.txt")
        .lines()
        .find(|l| l.starts_with("budget "))
        .ok_or("missing budget regression")?
        .split_whitespace()
        .collect();
    let budget = bytes(budget_row[2])?;
    assert!(decoder.decode_fec(&budget).is_err());
    assert_eq!(decoder, before);
    // The first compressed frame succeeds before the second exceeds its budget.
    let first_row: Vec<_> = include_str!("silk-fec-fixtures.txt")
        .lines()
        .find(|l| {
            l.starts_with("packet ")
                && l.split_whitespace()
                    .nth(2)
                    .is_some_and(|s| s.starts_with("58"))
        })
        .ok_or("missing matching first frame")?
        .split_whitespace()
        .collect();
    let first = bytes(first_row[2])?;
    let payload = &first[1..];
    let mut multiple = vec![0x5a];
    if payload.len() < 252 {
        multiple.push(payload.len() as u8);
    } else {
        multiple.push((252 + (payload.len() - 252) % 4) as u8);
        multiple.push(((payload.len() - 252) / 4) as u8);
    }
    multiple.extend_from_slice(payload);
    multiple.extend_from_slice(&budget[1..]);
    assert!(decoder.decode(&multiple).is_err());
    assert_eq!(decoder, before);
    let mut control = before;
    assert_eq!(decoder.decode(&good)?, control.decode(&good)?);
    assert_eq!(decoder, control);
    decoder.reset();
    assert_eq!(
        decoder,
        SilkDecoder::new(OutputRate::Khz48, PcmChannels::Stereo)
    );
    assert_eq!(
        decoder.decode(&good)?,
        SilkDecoder::new(OutputRate::Khz48, PcmChannels::Stereo).decode(&good)?
    );
    Ok(())
}

#[test]
fn bounded_adversarial_packets_preserve_state_on_every_error()
-> Result<(), Box<dyn std::error::Error>> {
    let mut random = 0x5349_4445u32;
    let mut decoder = SilkDecoder::new(OutputRate::Khz48, PcmChannels::Stereo);
    let mut rejected = 0;
    for index in 0..512 {
        let mut data = vec![0u8; 2 + index % 1274];
        for byte in &mut data {
            random ^= random << 13;
            random ^= random >> 17;
            random ^= random << 5;
            *byte = random as u8;
        }
        // SILK configurations, both encoded layouts and all framing codes.
        data[0] %= 96;
        for fec in [false, true] {
            let before = decoder.clone();
            let result = if fec {
                decoder.decode_fec(&data)
            } else {
                decoder.decode(&data)
            };
            match result {
                Ok(pcm) => {
                    assert!(pcm.samples().len() <= 11520);
                    assert_eq!(pcm.samples().len(), pcm.samples_per_channel() * 2);
                }
                Err(_) => {
                    assert_eq!(decoder, before);
                    rejected += 1;
                }
            }
        }
    }
    assert!(rejected > 100);
    Ok(())
}
