//! End-to-end public opus_decode PCM/final range and atomic stream behavior.
use opura::{
    Bandwidth, Packet,
    hybrid::HybridDecoder,
    silk::{decoder::PcmChannels, resampler::OutputRate},
};
fn rate(hz: u32) -> OutputRate {
    match hz {
        8000 => OutputRate::Khz8,
        12000 => OutputRate::Khz12,
        16000 => OutputRate::Khz16,
        24000 => OutputRate::Khz24,
        48000 => OutputRate::Khz48,
        _ => panic!("fixture rate"),
    }
}
fn channels(n: usize) -> PcmChannels {
    if n == 1 {
        PcmChannels::Mono
    } else {
        PcmChannels::Stereo
    }
}
#[test]
fn real_hybrid_packets_match_public_opus_pcm_and_final_range()
-> Result<(), Box<dyn std::error::Error>> {
    let mut decoder = HybridDecoder::new(OutputRate::Khz48, PcmChannels::Mono)?;
    let mut count = 0;
    let mut seen = [[[[false; 2]; 2]; 5]; 4];
    let mut max_frames = 0;
    let mut bands = [false; 4];
    let mut lbrr = 0;
    let mut clipped = 0;
    let mut lines = include_str!("hybrid-packet-fixtures.txt")
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
        if decoder.output_rate().hz() != hz || decoder.output_channels().count() != api {
            decoder = HybridDecoder::new(rate(hz), channels(api))?;
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
        for index in 0..packet.frames().len() {
            let (header, _) = opura::silk::decode_header(&packet, index)?;
            for c in 0..header.channel_count() {
                lbrr += header
                    .redundancy(c)
                    .ok_or("redundancy")?
                    .iter()
                    .filter(|&&v| v)
                    .count();
            }
        }
        let lm = match packet.frame_samples() {
            120 => 0,
            240 => 1,
            480 => 2,
            960 => 3,
            _ => panic!("CELT frame size"),
        };
        let mut separate = decoder.clone();
        let mut individual = Vec::new();
        for payload in packet.frames() {
            let mut single = vec![data[0] & 0xfc];
            single.extend_from_slice(payload);
            individual.extend_from_slice(separate.decode(&single)?.samples());
        }
        let actual = decoder
            .decode(&data)
            .map_err(|e| format!("packet{count} {hz}Hz API{api} wire{packet:?}: {e}"))?;
        let expected_range = r[5].parse()?;
        let expected: Vec<i16> = r[7..].iter().map(|s| s.parse()).collect::<Result<_, _>>()?;
        assert_eq!(
            actual.final_range(),
            expected_range,
            "final range packet{count}"
        );
        assert_eq!(decoder.final_range(), expected_range);
        clipped += actual
            .samples()
            .iter()
            .filter(|&&v| matches!(v, i16::MIN | i16::MAX))
            .count();
        assert_eq!(actual.samples(), individual);
        assert_eq!(decoder, separate, "packet/single frame state");
        assert_eq!(decoder.celt().concealment().start_band(), 17);
        assert_eq!(
            actual.samples(),
            expected,
            "PCM packet{count} {hz}Hz API{api} wire{packet:?}"
        );
        assert_eq!(actual.samples_per_channel(), r[6].parse::<usize>()?);
        assert_eq!(actual.channels(), api);
        assert_eq!(actual.sample_rate(), hz);
        for c in 0..2 {
            for (kind, values) in [
                decoder.celt().energy().channel(c),
                decoder.celt().previous_energy(c),
                decoder.celt().previous_energy2(c),
                decoder.celt().background_energy(c),
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
        assert_eq!(
            decoder.celt().synthesis().deemphasis_memory(),
            &state[168..170]
        );
        for c in 0..api {
            let backend = decoder.celt().synthesis();
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
        seen[lm][ri][api - 1][usize::from(packet.channels()) - 1] = true;
        count += 1;
    }
    assert!(lbrr > 0, "missing in-band redundancy coverage");
    assert!(clipped > 0, "missing saturating accumulation coverage");
    assert_eq!(count, 960);
    assert_eq!(max_frames, 12);
    assert_eq!(bands, [false, false, true, true]);
    assert!(seen[2..].iter().flatten().flatten().flatten().all(|&v| v));
    Ok(())
}

fn unhex(hex: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    hex.as_bytes()
        .chunks_exact(2)
        .map(|v| Ok(u8::from_str_radix(std::str::from_utf8(v)?, 16)?))
        .collect()
}
#[test]
fn invalid_and_late_packet_failures_preserve_both_layers() -> Result<(), Box<dyn std::error::Error>>
{
    use opura::hybrid::DecoderError;
    let rows: Vec<_> = include_str!("hybrid-packet-fixtures.txt")
        .lines()
        .filter(|s| s.starts_with("packet "))
        .collect();
    let data = unhex(rows[1].split_whitespace().nth(4).ok_or("warm hex")?)?;
    let mut decoder = HybridDecoder::new(OutputRate::Khz48, PcmChannels::Stereo)?;
    decoder.decode(&data)?;
    let before = decoder.clone();
    for bad in [&[][..], &[0][..], &[0xf8, 0xff, 0xff][..], &[0x63, 0][..]] {
        assert!(decoder.decode(bad).is_err());
        assert_eq!(decoder, before);
    }
    let valid = Packet::parse(&data)?;
    assert_eq!(valid.frames().len(), 1);
    let first = valid.frames()[0];
    let mut changed = decoder.clone();
    changed.decode(&data)?;
    assert_ne!(changed, before, "first frame must advance histories");
    // A valid first frame advances both layers before the retained malformed
    // second frame fails; rollback covers the whole packet.
    let mut late = vec![(data[0] & 0xfc) | 2];
    if first.len() < 252 {
        late.push(first.len() as u8);
    } else {
        late.push((252 + first.len() % 4) as u8);
        late.push(((first.len() - 252) / 4) as u8);
    }
    late.extend_from_slice(first);
    // Retained xorshift malformed payload reaches the CELT pulse decoder after
    // SILK has already advanced channel, stereo and resampler state.
    let deep = unhex(concat!(
        "60acd8f723acf275f0ca9d7dcc6542af753459cef1b8d6581bd8baadaec3b6b77f961d40b95b5471",
        "aab324c9118b857453f5815eb242a80eda5b3e2dd9d9e983b363d01aace25d083468ef32a2e3a06a",
        "2d2ca74074d635f8f10872c31e62808425b3f8c5a5028773b0e1c04d92217fa13d1a95fefe90a774",
        "e9f6e3a4b19308da823e616d731ff6b1952548ce2d62bd80e04322586a9d2797741d0016b557dc3c",
        "a20d863767874443324c8d5a9c77809b665f3fae72f3a494d26a412b36ff20a4fe8ac8e4774884e1",
        "142d9bef3558534ab5470cfef4aca52129b9f9a171a29107a300e342fe134abcb679a4fa5a055000",
        "733d7e7824bd13b2",
    ))?;
    let expected = DecoderError::Celt(opura::celt::decoder::DecoderError::Partition(
        opura::celt::partition::PartitionError::Pvq(opura::celt::pvq::PvqError::Pulses(
            opura::celt::pulses::PulseError::Entropy(opura::range::RangeError::CorruptUniformValue),
        )),
    ));
    assert_eq!(decoder.decode(&deep), Err(expected));
    assert_eq!(decoder, before);
    late.extend_from_slice(&deep[1..]);
    assert!(matches!(decoder.decode(&late), Err(DecoderError::Celt(_))));
    assert_eq!(decoder, before);
    let mut reset = decoder.clone();
    reset.reset();
    let mut fresh = HybridDecoder::new(OutputRate::Khz48, PcmChannels::Stereo)?;
    assert_eq!(reset.decode(&data)?, fresh.decode(&data)?);
    assert_eq!(reset, fresh);
    Ok(())
}
#[test]
fn deterministic_corruption_is_bounded_and_transactional() -> Result<(), Box<dyn std::error::Error>>
{
    let mut seed = 0x48594252u32;
    let mut decoder = HybridDecoder::new(OutputRate::Khz24, PcmChannels::Stereo)?;
    let mut failures = 0;
    for i in 0..512 {
        let mut data = vec![0; 3 + (i * 37) % 384];
        data[0] = [0x60, 0x6c, 0x70, 0x7c][i % 4];
        for b in &mut data[1..] {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            *b = seed as u8;
        }
        let before = decoder.clone();
        match decoder.decode(&data) {
            Ok(pcm) => {
                assert_eq!(
                    pcm.samples_per_channel(),
                    usize::from(Packet::parse(&data)?.frame_samples()) / 2
                );
                assert_eq!(pcm.channels(), 2);
            }
            Err(_) => {
                failures += 1;
                assert_eq!(decoder, before);
            }
        }
    }
    assert!(failures > 0);
    Ok(())
}
