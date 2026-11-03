//! Original native self-delimited bytes and public multistream PCM/range/gain.
use opura::{
    Packet,
    multistream::{ChannelMapping, MultistreamDecoder},
    repacketizer::Repacketizer,
    silk::resampler::OutputRate,
};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
fn hash_bytes(bytes: impl IntoIterator<Item = u8>) -> u64 {
    bytes.into_iter().fold(14695981039346656037u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(1099511628211)
    })
}

#[test]
fn original_native_self_delimited_bytes_sizes_and_consumed_offsets() -> Result<()> {
    let boundaries = [0, 1, 251, 252, 253, 254, 255, 256, 1274, 1275];
    let mut rows = 0;
    for line in include_str!("multistream-framing-fixtures.txt")
        .lines()
        .filter(|l| !l.starts_with('#'))
    {
        let [toc, count, pattern, extra, length, expected, consumed]: [u64; 7] = line
            .split_whitespace()
            .map(str::parse)
            .collect::<std::result::Result<Vec<_>, _>>()?
            .try_into()
            .map_err(|_| "invalid framing fixture")?;
        let toc = toc as u8;
        let count = count as usize;
        let length = length as usize;
        let sources: Vec<Vec<u8>> = (0..count)
            .map(|frame| {
                let n = match pattern {
                    0..=4 => [0, 1, 251, 252, 1275][pattern as usize],
                    5 => [0, 1275][frame % 2],
                    6 => boundaries[(frame + usize::from(toc >> 3)) % boundaries.len()],
                    _ => (frame * 17 + usize::from(toc)) % 1276,
                };
                std::iter::once(toc)
                    .chain((0..n).map(|i| (i * 37 + frame * 19 + usize::from(toc) * 3) as u8))
                    .collect()
            })
            .collect();
        let mut rp = Repacketizer::new();
        for packet in &sources {
            rp.add_packet(packet)?;
        }
        let before = rp.clone();
        let mut output = vec![0xa5; 65544];
        let n = if extra == 0 {
            rp.write_self_delimited(&mut output)?
        } else {
            rp.write_self_delimited_padded(&mut output, length)?
        };
        assert_eq!(n, length, "{line}");
        assert_eq!(hash_bytes(output[..n].iter().copied()), expected, "{line}");
        assert_eq!(&output[n..n + 8], &[0xa5; 8]);
        assert_eq!(rp, before);
        // The following valid packet belongs to a separate stream and must not
        // affect this stream's inferred frame lengths or consumed padding.
        output[n] = 0xe0;
        output[n + 1] = 99;
        let (packet, actual) = Packet::parse_self_delimited(&output[..n + 2])?;
        assert_eq!(actual, consumed as usize, "{line}");
        assert_eq!(packet.frames(), rp.frames(), "{line}");
        rows += 1;
    }
    assert_eq!(rows, 13328);
    Ok(())
}

fn rate(hz: u32) -> Result<OutputRate> {
    Ok(match hz {
        8000 => OutputRate::Khz8,
        12000 => OutputRate::Khz12,
        16000 => OutputRate::Khz16,
        24000 => OutputRate::Khz24,
        48000 => OutputRate::Khz48,
        _ => return Err("invalid fixture rate".into()),
    })
}
fn unhex(hex: &str) -> Result<Vec<u8>> {
    if hex.len() % 2 != 0 {
        return Err("odd fixture hex".into());
    }
    hex.as_bytes()
        .chunks_exact(2)
        .map(|p| Ok(u8::from_str_radix(std::str::from_utf8(p)?, 16)?))
        .collect()
}
#[test]
fn original_public_multistream_pcm_ranges_gain_loss_fec_and_reset() -> Result<()> {
    let mut decoder = None;
    let mut cases = 0;
    let mut rows = 0;
    let mut operations = [0; 3];
    let mut modes = [0; 3];
    let mut fec_backups = 0;
    for line in include_str!("multistream-decoder-fixtures.txt")
        .lines()
        .filter(|l| !l.starts_with('#'))
    {
        let fields: Vec<_> = line.split_whitespace().collect();
        match fields[0] {
            "case" => {
                assert_eq!(fields.len(), 5);
                let map: Vec<u8> = fields[4]
                    .split(',')
                    .map(str::parse)
                    .collect::<std::result::Result<_, _>>()?;
                decoder = Some(MultistreamDecoder::new(
                    rate(fields[1].parse()?)?,
                    ChannelMapping::new(fields[2].parse()?, fields[3].parse()?, &map)?,
                )?);
                cases += 1;
            }
            "frame" => {
                assert_eq!(fields.len(), 9);
                let decoder = decoder.as_mut().ok_or("frame without case")?;
                let gain: i16 = fields[3].parse()?;
                decoder.set_gain_q8(gain);
                if fields[2] == "1" {
                    decoder.reset();
                }
                assert_eq!(decoder.gain_q8(), gain);
                let duration: u16 = fields[4].parse()?;
                let packet = if fields[5] == "-" {
                    vec![]
                } else {
                    unhex(fields[5])?
                };
                if fields[1] != "plc" {
                    let parsed = opura::multistream::MultistreamPacket::parse(
                        &packet,
                        decoder.channel_mapping().streams(),
                    )?;
                    for stream in parsed.streams() {
                        let mode = match stream.mode() {
                            opura::Mode::Silk => 0,
                            opura::Mode::Hybrid => 1,
                            opura::Mode::Celt => 2,
                        };
                        if fields[1] == "normal" {
                            modes[mode] += 1;
                        }
                        if fields[1] == "fec"
                            && stream.mode() != opura::Mode::Celt
                            && stream.frames()[0].len() > 1
                        {
                            let (header, _) = opura::silk::decode_header(stream, 0)?;
                            for channel in 0..usize::from(stream.channels()) {
                                if header
                                    .redundancy(channel)
                                    .ok_or("missing SILK redundancy channel")?
                                    .contains(&true)
                                {
                                    fec_backups += 1;
                                }
                            }
                        }
                    }
                }
                let output = match fields[1] {
                    "normal" => {
                        operations[0] += 1;
                        decoder.decode(&packet)?
                    }
                    "plc" => {
                        operations[1] += 1;
                        decoder.conceal_samples(duration)?
                    }
                    "fec" => {
                        operations[2] += 1;
                        decoder.decode_fec(&packet, duration)?
                    }
                    _ => return Err("invalid fixture operation".into()),
                };
                let expected_range: u32 = fields[6].parse()?;
                let expected_samples: usize = fields[7].parse()?;
                let expected_hash: u64 = fields[8].parse()?;
                assert_eq!(output.final_range(), expected_range, "{line}");
                assert_eq!(decoder.final_range(), expected_range, "{line}");
                assert_eq!(output.samples_per_channel(), expected_samples, "{line}");
                assert_eq!(decoder.last_packet_duration(), duration);
                assert_eq!(
                    hash_bytes(output.samples().iter().flat_map(|n| n.to_le_bytes())),
                    expected_hash,
                    "{line}"
                );
                rows += 1;
            }
            _ => return Err("unknown fixture row".into()),
        }
    }
    assert_eq!(cases, 40);
    assert_eq!(rows, 560);
    assert_eq!(operations, [400, 80, 80]);
    assert!(
        modes.iter().all(|&n| n > 0),
        "actual mode coverage: {modes:?}"
    );
    assert!(
        fec_backups > 0,
        "FEC steps must contain actual LBRR, not only fallbacks"
    );
    Ok(())
}
