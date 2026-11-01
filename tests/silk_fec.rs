//! In-band FEC and subsequent primary packet decoding against upstream PCM.
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
fn fec_and_primary_pcm_final_range_match_real_streams_and_every_mask()
-> Result<(), Box<dyn std::error::Error>> {
    let mut decoder = MonoDecoder::default();
    let mut count = 0;
    let mut fec_count = 0;
    let mut masks = [false; 8];
    let mut rates = [false; 3];
    for line in include_str!("silk-fec-fixtures.txt")
        .lines()
        .filter(|l| !l.starts_with('#'))
    {
        let row: Vec<_> = line.split_whitespace().collect();
        if row[1] == "1" {
            decoder.reset();
        }
        let data = bytes(row[2])?;
        let packet = Packet::parse(&data)?;
        if row[0] == "budget" {
            let before = decoder.clone();
            assert_eq!(
                decoder.decode_fec(&data),
                Err(MonoError::Channel(
                    opura::silk::channel::ChannelError::Excitation(
                        opura::silk::excitation::ExcitationError::BitBudgetExceeded
                    )
                ))
            );
            assert_eq!(decoder, before);
            count += 1;
            continue;
        }
        let pcm = if row[0] == "fec" {
            fec_count += 1;
            let (header, _) = decode_header(&packet, 0)?;
            let mask = header
                .redundancy(0)
                .ok_or("missing FEC flags")?
                .iter()
                .enumerate()
                .fold(0, |value, (i, &flag)| value | (usize::from(flag) << i));
            masks[mask] = true;
            decoder.decode_fec(&data)?
        } else if row[0] == "transition" {
            let before = decoder.clone();
            assert_eq!(decoder.decode(&data), Err(MonoError::UnsupportedTransition));
            assert_eq!(decoder, before);
            count += 1;
            continue;
        } else {
            assert_eq!(row[0], "packet");
            decoder.decode(&data)?
        };
        let rate: u32 = row[3].parse()?;
        rates[match rate {
            8 => 0,
            12 => 1,
            16 => 2,
            _ => panic!("bad reference rate"),
        }] = true;
        let expected: Vec<_> = row[5..]
            .iter()
            .map(|s| s.parse::<i16>())
            .collect::<Result<_, _>>()?;
        assert_eq!(pcm.sample_rate(), rate * 1000);
        assert_eq!(
            pcm.final_range(),
            row[4].parse::<u32>()?,
            "range step {count}"
        );
        assert_eq!(pcm.samples(), expected, "PCM step {count} {}", row[0]);
        count += 1;
    }
    assert_eq!(count, 328);
    assert_eq!(fec_count, 111);
    assert_eq!(masks, [true; 8]);
    assert_eq!(rates, [true; 3]);
    Ok(())
}

#[test]
fn invalid_fec_does_not_change_history_and_reset_is_repeatable()
-> Result<(), Box<dyn std::error::Error>> {
    let line = include_str!("silk-fec-fixtures.txt")
        .lines()
        .find(|l| l.starts_with("fec "))
        .ok_or("missing FEC fixture")?;
    let row: Vec<_> = line.split_whitespace().collect();
    let good = bytes(row[2])?;
    let mut decoder = MonoDecoder::default();
    decoder.decode(&good)?;
    let before = decoder.clone();
    for (data, error) in [
        (vec![], MonoError::Framing(opura::PacketError::EmptyPacket)),
        (vec![0x80, 0, 0], MonoError::UnsupportedMode),
        (vec![0x60, 0, 0], MonoError::UnsupportedMode),
        (vec![0x04, 0, 0], MonoError::UnsupportedChannels),
        (vec![0], MonoError::MissingPayload),
        (vec![0, 0], MonoError::MissingPayload),
    ] {
        assert_eq!(decoder.decode_fec(&data), Err(error));
        assert_eq!(decoder, before);
    }
    let mut control = before;
    assert_eq!(decoder.decode_fec(&good)?, control.decode_fec(&good)?);
    assert_eq!(decoder, control);
    decoder.reset();
    assert_eq!(decoder, MonoDecoder::default());
    assert_eq!(
        decoder.decode_fec(&good)?,
        MonoDecoder::default().decode_fec(&good)?
    );
    Ok(())
}

#[test]
fn multi_frame_fec_recovers_only_first_opus_frame_and_primary_can_follow()
-> Result<(), Box<dyn std::error::Error>> {
    let row: Vec<_> = include_str!("silk-fec-fixtures.txt")
        .lines()
        .filter(|l| l.starts_with("fec "))
        .next_back()
        .ok_or("missing FEC packet")?
        .split_whitespace()
        .collect();
    let single = bytes(row[2])?;
    let parsed = Packet::parse(&single)?;
    assert_eq!(parsed.frames().len(), 1);
    assert_eq!(parsed.frame_samples(), 2880);
    let mut multiple = vec![(single[0] & !3) | 1];
    multiple.extend_from_slice(parsed.frames()[0]);
    multiple.extend_from_slice(parsed.frames()[0]);
    let mut a = MonoDecoder::default();
    let mut b = MonoDecoder::default();
    let recovered = a.decode_fec(&multiple)?;
    assert_eq!(recovered, b.decode_fec(&single)?);
    assert_eq!(recovered.samples().len(), 960);
    assert_eq!(a, b);
    let combined = a.decode(&multiple)?;
    let mut expected = b.decode(&single)?.samples().to_vec();
    let second = b.decode(&single)?;
    expected.extend_from_slice(second.samples());
    assert_eq!(combined.samples(), expected);
    assert_eq!(combined.final_range(), second.final_range());
    assert_eq!(a, b);
    Ok(())
}
