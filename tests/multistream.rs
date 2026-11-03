//! Multistream framing, resource and channel-map boundary contracts.
use opura::{
    Packet, PacketError,
    multistream::{ChannelMapping, MAX_PACKET_BYTES, MultistreamError, MultistreamPacket},
    repacketizer::Repacketizer,
};

#[test]
fn explicit_frame_lengths_and_consumed_padding() -> Result<(), Box<dyn std::error::Error>> {
    type Case<'a> = (&'a [u8], usize, &'a [&'a [u8]]);
    let cases: &[Case<'_>] = &[
        (&[0x98, 2, 11, 22, 0xe0], 4, &[&[11, 22]]),
        (&[0x99, 2, 11, 22, 33, 44, 0xe0], 6, &[&[11, 22], &[33, 44]]),
        (&[0x9a, 1, 2, 11, 22, 33, 0xe0], 6, &[&[11], &[22, 33]]),
        (
            &[0x9b, 0x83, 0, 1, 2, 11, 22, 33, 0xe0],
            8,
            &[&[], &[11], &[22, 33]],
        ),
        (&[0x98, 0, 0xe0], 2, &[&[]]),
    ];
    for &(input, expected, frames) in cases {
        let (packet, consumed) = Packet::parse_self_delimited(input)?;
        assert_eq!(consumed, expected);
        assert_eq!(packet.frames(), frames);
        assert_eq!(input[consumed], 0xe0);
    }
    let mut padded = vec![0x9b, 0x41, 255, 1, 2, 11, 22];
    padded.extend([0xaa; 255]);
    let consumed = padded.len();
    padded.extend([0xe0, 44]);
    let (packet, actual) = Packet::parse_self_delimited(&padded)?;
    assert_eq!(actual, consumed);
    assert_eq!(packet.frames(), &[&[11, 22][..]]);
    assert_eq!(&padded[actual..], &[0xe0, 44]);
    let (packet, consumed) = Packet::parse_self_delimited(&[0x83, 48, 0, 0xe0])?;
    assert_eq!(packet.frames().len(), 48);
    assert!(packet.frames().iter().all(|f| f.is_empty()));
    assert_eq!(packet.duration_samples(), 5760);
    assert_eq!(consumed, 3);
    Ok(())
}

#[test]
fn self_delimited_repacking_boundaries_and_atomic_failures()
-> Result<(), Box<dyn std::error::Error>> {
    for count in [1, 2, 3, 8, 9, 48] {
        for length in [0, 1, 251, 252, 255, 1275] {
            for variable in [false, true] {
                let packets: Vec<Vec<u8>> = (0..count)
                    .map(|i| {
                        let n = if variable && i % 2 == 1 {
                            length / 2
                        } else {
                            length
                        };
                        let mut bytes = vec![0xe0];
                        bytes.extend(std::iter::repeat_n(i as u8, n));
                        bytes
                    })
                    .collect();
                let mut rp = Repacketizer::new();
                for bytes in &packets {
                    rp.add_packet(bytes)?;
                }
                let mut out = vec![0xcc; 65536];
                let n = rp.write_self_delimited(&mut out)?;
                out[n] = 0xe0;
                let (parsed, consumed) = Packet::parse_self_delimited(&out[..n + 1])?;
                assert_eq!(consumed, n);
                assert_eq!(parsed.frames(), rp.frames());
                for extra in [1, 2, 254, 255, 256, 510, 511] {
                    let padded = n + extra;
                    rp.write_self_delimited_padded(&mut out, padded)?;
                    out[padded] = 0xe0;
                    let (parsed, consumed) = Packet::parse_self_delimited(&out[..padded + 1])?;
                    assert_eq!(consumed, padded);
                    assert_eq!(parsed.frames(), rp.frames());
                    assert_eq!(out[padded], 0xe0);
                }
                let mut small = vec![0x5a; n - 1];
                let before = small.clone();
                assert!(rp.write_self_delimited(&mut small).is_err());
                assert_eq!(small, before);
                let before = out.clone();
                assert!(rp.write_self_delimited_padded(&mut out, n - 1).is_err());
                assert_eq!(out, before);
            }
        }
    }
    Ok(())
}

#[test]
fn layout_limits_and_encoder_map_coverage() -> Result<(), MultistreamError> {
    for (streams, coupled, map) in [
        (0, 0, &[0][..]),
        (256, 0, &[0]),
        (1, 2, &[0]),
        (255, 1, &[0]),
        (usize::MAX, usize::MAX, &[0]),
        (1, 0, &[]),
    ] {
        assert_eq!(
            ChannelMapping::new(streams, coupled, map),
            Err(MultistreamError::InvalidLayout)
        );
    }
    assert_eq!(
        ChannelMapping::new(1, 0, &[1]),
        Err(MultistreamError::InvalidChannelMap)
    );
    assert_eq!(
        ChannelMapping::new(1, 0, &[0; 256]),
        Err(MultistreamError::InvalidLayout)
    );
    let map = ChannelMapping::new(2, 1, &[2, 0, 1, 255, 0])?;
    assert_eq!(map.channels(), 5);
    assert_eq!(map.streams(), 2);
    assert_eq!(map.coupled_streams(), 1);
    assert_eq!(map.coded_channels(), 3);
    map.validate_encoder()?;
    let omitted = ChannelMapping::new(2, 1, &[0, 255, 0])?;
    assert_eq!(
        omitted.validate_encoder(),
        Err(MultistreamError::IncompleteEncoderMap)
    );
    let silent = ChannelMapping::new(255, 0, &[255])?;
    assert_eq!(silent.mapping(), &[255]);
    assert_eq!(
        silent.validate_encoder(),
        Err(MultistreamError::IncompleteEncoderMap)
    );
    let full: Vec<u8> = (0..255).collect();
    ChannelMapping::new(255, 0, &full)?.validate_encoder()?;
    Ok(())
}

#[test]
fn whole_stream_duration_not_frame_count_must_match() -> Result<(), MultistreamError> {
    // One20ms CELT frame followed by two10ms frames: compatible durations.
    let packet = MultistreamPacket::parse(&[0x98, 0, 0x91], 2)?;
    assert_eq!(packet.duration_samples(), 960);
    assert_eq!(packet.streams()[0].frames().len(), 1);
    assert_eq!(packet.streams()[1].frames().len(), 2);
    assert!(matches!(
        MultistreamPacket::parse(&[0x98, 0, 0x90], 2),
        Err(MultistreamError::DurationMismatch)
    ));
    assert!(matches!(
        MultistreamPacket::parse(&[0x98, 0], 2),
        Err(MultistreamError::Framing {
            stream: 1,
            error: PacketError::EmptyPacket
        })
    ));
    for streams in [0, 256, usize::MAX] {
        assert!(matches!(
            MultistreamPacket::parse(&[], streams),
            Err(MultistreamError::InvalidLayout)
        ));
    }
    let largest = vec![0x80; MAX_PACKET_BYTES + 1];
    assert!(matches!(
        MultistreamPacket::parse(&largest, 1),
        Err(MultistreamError::PacketTooLarge)
    ));
    Ok(())
}

#[test]
fn malformed_sizes_counts_durations_and_padding_reject() {
    for bytes in [
        &[][..],
        &[0x98],
        &[0x98, 252],
        &[0x99, 2, 1, 2, 3],
        &[0x9a, 1, 2, 1, 2],
        &[0x9b, 0, 0],
        &[0x9b, 49, 0],
        &[0x9b, 7, 0],
        &[0x9b, 0x41, 255],
        &[0x9b, 0x41, 254, 0],
    ] {
        assert!(Packet::parse_self_delimited(bytes).is_err(), "{bytes:?}");
    }
}

fn active_celt_packet() -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    for line in include_str!("celt-packet-fixtures.txt")
        .lines()
        .filter(|l| l.starts_with("packet "))
    {
        let fields: Vec<_> = line.split_whitespace().collect();
        if fields[2] == "48000"
            && fields[3] == "2"
            && fields[6] == "960"
            && fields[7..].iter().any(|&v| v != "0")
        {
            return fields[4]
                .as_bytes()
                .chunks_exact(2)
                .map(|p| Ok(u8::from_str_radix(std::str::from_utf8(p)?, 16)?))
                .collect();
        }
    }
    Err("missing active native CELT packet".into())
}
fn join_streams(first: &[u8], last: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut rp = Repacketizer::new();
    rp.add_packet(first)?;
    let mut out = vec![0; 65536];
    let n = rp.write_self_delimited(&mut out)?;
    out.truncate(n);
    out.extend_from_slice(last);
    Ok(out)
}

#[test]
fn persistent_mapped_normal_loss_fec_and_reset_match_constituent_decoders()
-> Result<(), Box<dyn std::error::Error>> {
    use opura::{
        decoder::OpusDecoder,
        multistream::MultistreamDecoder,
        silk::{decoder::PcmChannels, resampler::OutputRate},
    };
    let bytes = active_celt_packet()?;
    let transport = join_streams(&bytes, &bytes)?;
    let layout = ChannelMapping::new(2, 1, &[2, 0, 1, 255, 0])?;
    for rate in [
        OutputRate::Khz8,
        OutputRate::Khz12,
        OutputRate::Khz16,
        OutputRate::Khz24,
        OutputRate::Khz48,
    ] {
        let mut ms = MultistreamDecoder::new(rate, layout.clone())?;
        let mut stereo = OpusDecoder::new(rate, PcmChannels::Stereo)?;
        let mut mono = OpusDecoder::new(rate, PcmChannels::Mono)?;
        for step in 0..7 {
            let (actual, left, right) = match step {
                3 => (
                    ms.conceal_samples(120)?,
                    stereo.conceal_samples(120)?,
                    mono.conceal_samples(120)?,
                ),
                4 => (
                    ms.decode_fec(&transport, 960)?,
                    stereo.decode_fec(&bytes, 960)?,
                    mono.decode_fec(&bytes, 960)?,
                ),
                _ => (
                    ms.decode(&transport)?,
                    stereo.decode(&bytes)?,
                    mono.decode(&bytes)?,
                ),
            };
            assert_eq!(actual.sample_rate(), rate.hz());
            assert_eq!(actual.channels(), 5);
            assert_eq!(actual.samples_per_channel(), left.samples_per_channel());
            assert_eq!(
                actual.final_range(),
                left.final_range() ^ right.final_range()
            );
            assert_eq!(ms.final_range(), actual.final_range());
            for ((frame, s), &m) in actual
                .samples()
                .chunks_exact(5)
                .zip(left.samples().chunks_exact(2))
                .zip(right.samples())
            {
                assert_eq!(frame, &[m, s[0], s[1], 0, s[0]]);
            }
            assert_eq!(ms.last_packet_duration(), if step == 3 { 120 } else { 960 });
        }
        let before = ms.clone();
        for duration in [0, 1, 119, 121, 5880, u16::MAX] {
            assert_eq!(
                ms.conceal_samples(duration),
                Err(MultistreamError::InvalidDuration)
            );
            assert_eq!(ms, before);
            assert_eq!(
                ms.decode_fec(&transport, duration),
                Err(MultistreamError::InvalidDuration)
            );
            assert_eq!(ms, before);
        }
        assert!(ms.decode(&[0x98, 0]).is_err());
        assert_eq!(ms, before);
        ms.reset();
        assert_eq!(ms, MultistreamDecoder::new(rate, layout.clone())?);
        assert_eq!(ms.output_rate(), rate);
        assert_eq!(ms.channel_mapping(), &layout);
    }
    Ok(())
}

#[test]
fn omitted_streams_still_decode_and_contribute_range() -> Result<(), Box<dyn std::error::Error>> {
    use opura::{
        decoder::OpusDecoder,
        multistream::MultistreamDecoder,
        silk::{decoder::PcmChannels, resampler::OutputRate},
    };
    let bytes = active_celt_packet()?;
    let transport = join_streams(&bytes, &[0x98])?;
    let layout = ChannelMapping::new(2, 0, &[255, 255])?;
    let mut ms = MultistreamDecoder::new(OutputRate::Khz48, layout)?;
    let mut source = OpusDecoder::new(OutputRate::Khz48, PcmChannels::Mono)?;
    for _ in 0..3 {
        let expected = source.decode(&bytes)?;
        let actual = ms.decode(&transport)?;
        assert!(actual.samples().iter().all(|&n| n == 0));
        assert_eq!(actual.final_range(), expected.final_range());
        assert_ne!(actual.final_range(), 0);
        assert_eq!(actual.samples_per_channel(), 960);
    }
    Ok(())
}

#[test]
fn late_audio_failure_rolls_back_every_stream_and_resume() -> Result<(), Box<dyn std::error::Error>>
{
    use opura::{multistream::MultistreamDecoder, silk::resampler::OutputRate};
    let bytes = active_celt_packet()?;
    let valid = join_streams(&bytes, &bytes)?;
    let mut ms =
        MultistreamDecoder::new(OutputRate::Khz48, ChannelMapping::new(2, 2, &[0, 1, 2, 3])?)?;
    let warm = include_str!("celt-packet-fixtures.txt")
        .lines()
        .filter(|l| l.starts_with("packet "))
        .nth(1)
        .ok_or("missing warm CELT packet")?;
    let warm_hex = warm
        .split_whitespace()
        .nth(4)
        .ok_or("missing warm CELT hex")?;
    let warm_bytes: Vec<u8> = warm_hex
        .as_bytes()
        .chunks_exact(2)
        .map(|p| Ok(u8::from_str_radix(std::str::from_utf8(p)?, 16)?))
        .collect::<Result<_, Box<dyn std::error::Error>>>()?;
    ms.decode(&join_streams(&warm_bytes, &warm_bytes)?)?;
    let before = ms.clone();
    let hybrid = include_str!("hybrid-packet-fixtures.txt")
        .lines()
        .filter(|l| l.starts_with("packet "))
        .nth(1)
        .ok_or("missing Hybrid packet")?;
    let hex = hybrid
        .split_whitespace()
        .nth(4)
        .ok_or("missing Hybrid hex")?;
    let hybrid_bytes: Vec<u8> = hex
        .as_bytes()
        .chunks_exact(2)
        .map(|p| Ok(u8::from_str_radix(std::str::from_utf8(p)?, 16)?))
        .collect::<Result<_, Box<dyn std::error::Error>>>()?;
    // Same deep malformed Hybrid payload retained by the single-stream late
    // failure regression: framing succeeds, synthesis fails after state work.
    let deep = concat!(
        "60acd8f723acf275f0ca9d7dcc6542af753459cef1b8d6581bd8baadaec3b6b77f961d40b95b5471",
        "aab324c9118b857453f5815eb242a80eda5b3e2dd9d9e983b363d01aace25d083468ef32a2e3a06a",
        "2d2ca74074d635f8f10872c31e62808425b3f8c5a5028773b0e1c04d92217fa13d1a95fefe90a774",
        "e9f6e3a4b19308da823e616d731ff6b1952548ce2d62bd80e04322586a9d2797741d0016b557dc3c",
        "a20d863767874443324c8d5a9c77809b665f3fae72f3a494d26a412b36ff20a4fe8ac8e4774884e1",
        "142d9bef3558534ab5470cfef4aca52129b9f9a171a29107a300e342fe134abcb679a4fa5a055000",
        "733d7e7824bd13b2",
    );
    let mut bad = Vec::new();
    for pair in deep.as_bytes().chunks_exact(2) {
        bad.push(u8::from_str_radix(std::str::from_utf8(pair)?, 16)?);
    }
    let mut rp = Repacketizer::new();
    rp.add_packet(&hybrid_bytes)?;
    rp.add_packet(&bad)?;
    let mut late_stream = vec![0; 65536];
    let n = rp.write(&mut late_stream)?;
    late_stream.truncate(n);
    let late = join_streams(&bytes, &late_stream)?;
    // This assertion ensures the test reaches the audio transaction, not just
    // the all-stream framing preflight.
    MultistreamPacket::parse(&late, 2)?;
    assert!(matches!(
        ms.decode(&late),
        Err(MultistreamError::Decode { stream: 1, .. })
    ));
    assert_eq!(ms, before);
    let mut pristine = before.clone();
    assert_eq!(ms.decode(&valid)?, pristine.decode(&valid)?);
    assert_eq!(ms, pristine);
    Ok(())
}

#[test]
fn common_gain_controls_preserve_histories_and_reset_setting()
-> Result<(), Box<dyn std::error::Error>> {
    use opura::{
        decoder::OpusDecoder,
        multistream::MultistreamDecoder,
        silk::{decoder::PcmChannels, resampler::OutputRate},
    };
    let bytes = active_celt_packet()?;
    let transport = join_streams(&bytes, &bytes)?;
    let layout = ChannelMapping::new(2, 1, &[2, 0, 1, 255, 0])?;
    let mut ms = MultistreamDecoder::new(OutputRate::Khz48, layout.clone())?;
    let mut stereo = OpusDecoder::new(OutputRate::Khz48, PcmChannels::Stereo)?;
    let mut mono = OpusDecoder::new(OutputRate::Khz48, PcmChannels::Mono)?;
    assert_eq!(ms.gain_q8(), 0);
    for gain in [0, 1536, -1536, 1, -1, i16::MAX, i16::MIN, 0] {
        ms.set_gain_q8(gain);
        stereo.set_gain_q8(gain);
        mono.set_gain_q8(gain);
        assert_eq!(ms.gain_q8(), gain);
        assert_eq!(stereo.gain_q8(), gain);
        for loss in [false, true] {
            let (actual, a, b) = if loss {
                (
                    ms.conceal_samples(960)?,
                    stereo.conceal_samples(960)?,
                    mono.conceal_samples(960)?,
                )
            } else {
                (
                    ms.decode(&transport)?,
                    stereo.decode(&bytes)?,
                    mono.decode(&bytes)?,
                )
            };
            assert_eq!(actual.final_range(), a.final_range() ^ b.final_range());
            for ((frame, s), &m) in actual
                .samples()
                .chunks_exact(5)
                .zip(a.samples().chunks_exact(2))
                .zip(b.samples())
            {
                assert_eq!(frame, &[m, s[0], s[1], 0, s[0]]);
            }
            if gain == i16::MIN {
                assert!(actual.samples().iter().all(|&n| n == 0));
            }
        }
    }
    ms.set_gain_q8(1536);
    ms.reset();
    let mut fresh = MultistreamDecoder::new(OutputRate::Khz48, layout)?;
    fresh.set_gain_q8(1536);
    assert_eq!(ms, fresh);
    assert_eq!(ms.gain_q8(), 1536);
    stereo.set_gain_q8(-1536);
    stereo.reset();
    let mut fresh = OpusDecoder::new(OutputRate::Khz48, PcmChannels::Stereo)?;
    fresh.set_gain_q8(-1536);
    assert_eq!(stereo, fresh);
    Ok(())
}
