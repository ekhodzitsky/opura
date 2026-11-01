//! End-to-end public opus_decode PCM/final range and atomic stream behavior.
use opura::{
    Bandwidth, Packet,
    celt::{
        FrameSize,
        decoder::{CeltDecoder, DecoderError, FrameConfig},
        output::Decimation,
    },
};
const SIZES: [FrameSize; 4] = [
    FrameSize::Ms2_5,
    FrameSize::Ms5,
    FrameSize::Ms10,
    FrameSize::Ms20,
];
fn rate(hz: u32) -> Decimation {
    match hz {
        8000 => Decimation::By6,
        12000 => Decimation::By4,
        16000 => Decimation::By3,
        24000 => Decimation::By2,
        48000 => Decimation::By1,
        _ => panic!("fixture rate"),
    }
}
#[test]
fn real_celt_packets_match_public_opus_pcm_and_final_range()
-> Result<(), Box<dyn std::error::Error>> {
    let mut decoder = CeltDecoder::new(Decimation::By1, 1)?;
    let mut count = 0;
    let mut seen = [[[[false; 2]; 2]; 5]; 4];
    let mut max_frames = 0;
    let mut bands = [false; 4];
    let mut lines = include_str!("celt-packet-fixtures.txt")
        .lines()
        .filter(|s| !s.starts_with('#'));
    while let Some(line) = lines.next() {
        let r: Vec<_> = line.split_whitespace().collect();
        assert_eq!(r[0], "packet");
        let state: Vec<i32> = lines
            .next()
            .ok_or("missing state")?
            .split_whitespace()
            .skip(1)
            .map(|s| s.parse())
            .collect::<Result<_, _>>()?;
        let hz: u32 = r[2].parse()?;
        let api: usize = r[3].parse()?;
        if decoder.sample_rate() != hz || decoder.channels() != api {
            decoder = CeltDecoder::new(rate(hz), api)?;
        }
        if r[1] == "1" {
            decoder.reset();
        }
        let data: Vec<u8> = r[4]
            .as_bytes()
            .chunks_exact(2)
            .map(|s| Ok(u8::from_str_radix(std::str::from_utf8(s)?, 16)?))
            .collect::<Result<_, Box<dyn std::error::Error>>>()?;
        let packet = Packet::parse(&data)?;
        let lm = match packet.frame_samples() {
            120 => 0,
            240 => 1,
            480 => 2,
            960 => 3,
            _ => panic!("CELT frame size"),
        };
        let config = FrameConfig {
            size: SIZES[lm],
            channels: usize::from(packet.channels()),
            bandwidth: packet.bandwidth(),
        };
        let mut separate = decoder.clone();
        let mut frame_pcm = Vec::new();
        for frame in packet.frames() {
            frame_pcm.extend_from_slice(separate.decode_frame(frame, config)?.samples());
        }
        let actual = decoder
            .decode(&data)
            .map_err(|e| format!("packet{count} {hz}Hz API{api} {config:?}: {e}"))?;
        let expected_range = r[5].parse()?;
        let expected: Vec<i16> = r[7..].iter().map(|s| s.parse()).collect::<Result<_, _>>()?;
        assert_eq!(
            actual.final_range(),
            expected_range,
            "final range packet{count}"
        );
        assert_eq!(decoder.final_range(), expected_range);
        assert_eq!(
            actual.samples(),
            expected,
            "PCM packet{count} {hz}Hz API{api} {config:?}"
        );
        assert_eq!(actual.samples(), frame_pcm);
        assert_eq!(decoder, separate, "packet/frame history");
        assert_eq!(actual.samples_per_channel(), r[6].parse::<usize>()?);
        assert_eq!(actual.channels(), api);
        assert_eq!(actual.sample_rate(), hz);
        for c in 0..2 {
            for (kind, values) in [
                decoder.energy().channel(c),
                decoder.previous_energy(c),
                decoder.previous_energy2(c),
                decoder.background_energy(c),
            ]
            .into_iter()
            .enumerate()
            {
                assert_eq!(
                    values.ok_or("energy history")?.as_slice(),
                    &state[42 * kind + c * 21..42 * kind + (c + 1) * 21],
                    "energy state packet{count} history{kind} channel{c}"
                );
            }
        }
        assert_eq!(decoder.synthesis().deemphasis_memory(), &state[168..170]);
        for c in 0..api {
            let backend = decoder.synthesis();
            let post = backend.postfilter(c).ok_or("postfilter channel")?;
            for (k, p) in post.parameters().iter().enumerate() {
                assert_eq!(
                    [
                        i32::from(p.period()),
                        i32::from(p.gain()),
                        p.tapset() as i32
                    ],
                    state[170 + 3 * k..173 + 3 * k],
                    "filter params packet{count}"
                );
            }
            assert_eq!(
                backend.overlap(c).ok_or("overlap")?.as_slice(),
                &state[176 + 60 * c..176 + 60 * (c + 1)],
                "overlap packet{count} channel{c}"
            );
            assert_eq!(
                post.history().as_slice(),
                &state[176 + 60 * api + 1026 * c..176 + 60 * api + 1026 * (c + 1)],
                "filter history packet{count} channel{c}"
            );
        }
        let b = match packet.bandwidth() {
            Bandwidth::Narrowband => 0,
            Bandwidth::Wideband => 1,
            Bandwidth::SuperWideband => 2,
            Bandwidth::Fullband => 3,
            _ => panic!("CELT bandwidth"),
        };
        bands[b] = true;
        max_frames = max_frames.max(packet.frames().len());
        let ri = match hz {
            8000 => 0,
            12000 => 1,
            16000 => 2,
            24000 => 3,
            48000 => 4,
            _ => unreachable!(),
        };
        seen[lm][ri][api - 1][config.channels - 1] = true;
        count += 1;
    }
    assert_eq!(count, 960);
    assert_eq!(max_frames, 48);
    assert_eq!(bands, [true; 4]);
    assert!(seen.iter().flatten().flatten().flatten().all(|&v| v));
    Ok(())
}
#[test]
fn invalid_packets_frames_and_late_failures_preserve_every_history()
-> Result<(), Box<dyn std::error::Error>> {
    let mut decoder = CeltDecoder::new(Decimation::By1, 2)?;
    let warm = include_str!("celt-packet-fixtures.txt")
        .lines()
        .filter(|s| s.starts_with("packet "))
        .nth(1)
        .ok_or("warm packet")?;
    let hex = warm.split_whitespace().nth(4).ok_or("warm hex")?;
    let data: Vec<u8> = hex
        .as_bytes()
        .chunks_exact(2)
        .map(|s| Ok(u8::from_str_radix(std::str::from_utf8(s)?, 16)?))
        .collect::<Result<_, Box<dyn std::error::Error>>>()?;
    decoder.decode(&data)?;
    let before = decoder.clone();
    for data in [&[][..], &[0][..], &[0xfb, 0][..]] {
        assert!(decoder.decode(data).is_err());
        assert_eq!(decoder, before);
    }
    let config = FrameConfig {
        size: FrameSize::Ms20,
        channels: 1,
        bandwidth: Bandwidth::Fullband,
    };
    let mut after_first = before.clone();
    after_first.decode(&[0xf8, 0xff, 0xff])?;
    assert_ne!(after_first, before);
    // Retained deterministic LCG malformed payload: a corrupt uniform code
    // fails after the first frame has already changed synthesis/energy history.
    let hex = concat!(
        "5048495da955d7c2136e1692dc1407f3bfeefe758a16b24819ffee3e16e47c202ddc",
        "1b0eba018dfc063c2482acb14d92badbfbcb92c80172c413aa5b41863138b081dc24",
        "d08edb5614c43b62fde27d63d93fa7f8def6ea00daf5285fe4cff053ca73c71e2c34",
        "d00272c227da22ef84cf3877f6f1781cd4b7fcc72aa75f8b3fac",
    );
    let bad: Vec<u8> = hex
        .as_bytes()
        .chunks_exact(2)
        .map(|s| Ok(u8::from_str_radix(std::str::from_utf8(s)?, 16)?))
        .collect::<Result<_, Box<dyn std::error::Error>>>()?;
    let failure = after_first
        .decode_frame(&bad, config)
        .err()
        .ok_or("malformed payload accepted")?;
    let pulse_error =
        opura::celt::pulses::PulseError::Entropy(opura::range::RangeError::CorruptUniformValue);
    let expected_failure = DecoderError::Partition(opura::celt::partition::PartitionError::Pvq(
        opura::celt::pvq::PvqError::Pulses(pulse_error),
    ));
    assert_eq!(failure, expected_failure);
    let mut packet = vec![0xfa, 2, 0xff, 0xff];
    packet.extend_from_slice(&bad);
    assert_eq!(decoder.decode(&packet), Err(failure));
    assert_eq!(decoder, before);
    for c in [
        FrameConfig {
            channels: 0,
            ..config
        },
        FrameConfig {
            channels: 3,
            ..config
        },
        FrameConfig {
            bandwidth: Bandwidth::Mediumband,
            ..config
        },
    ] {
        assert_eq!(
            decoder.decode_frame(&[0xff; 2], c),
            Err(DecoderError::InvalidConfiguration)
        );
        assert_eq!(decoder, before);
    }
    assert_eq!(
        decoder.decode_frame(&vec![0; 1276], config),
        Err(DecoderError::MissingPayload)
    );
    assert_eq!(decoder, before);
    decoder.reset();
    assert_eq!(decoder, CeltDecoder::new(Decimation::By1, 2)?);
    assert!(CeltDecoder::new(Decimation::By1, 0).is_err());
    assert!(CeltDecoder::new(Decimation::By1, 3).is_err());
    Ok(())
}
#[test]
fn deterministic_adversarial_payloads_are_bounded_and_atomic()
-> Result<(), Box<dyn std::error::Error>> {
    let mut decoder = CeltDecoder::new(Decimation::By2, 2)?;
    let mut seed = 0x43454c54u32;
    for i in 0..512 {
        let mut data = vec![0u8; [2, 3, 8, 16, 64, 255, 1275][i % 7]];
        for b in &mut data {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            *b = seed as u8;
        }
        let config = FrameConfig {
            size: SIZES[i % 4],
            channels: 1 + i % 2,
            bandwidth: [
                Bandwidth::Narrowband,
                Bandwidth::Wideband,
                Bandwidth::SuperWideband,
                Bandwidth::Fullband,
            ][i % 4],
        };
        let saved = decoder.clone();
        match decoder.decode_frame(&data, config) {
            Ok(pcm) => {
                assert_eq!(pcm.samples_per_channel(), config.size.samples() / 2);
                assert_eq!(pcm.channels(), 2);
            }
            Err(_) => assert_eq!(decoder, saved),
        }
    }
    Ok(())
}
