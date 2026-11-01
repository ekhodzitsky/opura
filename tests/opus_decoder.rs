//! Public Hybrid libopus PLC/DTX/FEC/recovery PCM and independent state snapshots.
use opura::{
    Packet,
    celt::decoder::ConcealmentMode,
    decoder::OpusDecoder,
    silk::{decoder::PcmChannels, resampler::OutputRate},
};
fn rate(hz: u32) -> OutputRate {
    match hz {
        8000 => OutputRate::Khz8,
        12000 => OutputRate::Khz12,
        16000 => OutputRate::Khz16,
        24000 => OutputRate::Khz24,
        48000 => OutputRate::Khz48,
        _ => panic!("rate"),
    }
}
fn channels(n: usize) -> PcmChannels {
    if n == 1 {
        PcmChannels::Mono
    } else {
        PcmChannels::Stereo
    }
}
fn reference_stream(
    fixtures: &str,
    expected: (usize, [usize; 4], usize),
) -> Result<(), Box<dyn std::error::Error>> {
    let mut decoder = OpusDecoder::new(OutputRate::Khz48, PcmChannels::Mono)?;
    let mut lines = fixtures.lines().filter(|s| !s.starts_with('#'));
    let mut count = 0;
    let mut modes = [0; 4];
    let mut capped = 0;
    let with_context = fixtures.lines().any(|s| s.starts_with("# context"));
    let mut transition_flags = 0;
    while let Some(line) = lines.next() {
        let r: Vec<_> = line.split_whitespace().collect();
        assert_eq!(r[0], "frame");
        let kind = r[1];
        let reset: u8 = r[2].parse()?;
        let hz: u32 = r[3].parse()?;
        let api: usize = r[4].parse()?;
        let duration: u16 = r[5].parse()?;
        if decoder.output_rate().hz() != hz || decoder.output_channels().count() != api {
            decoder = OpusDecoder::new(rate(hz), channels(api))?;
        }
        if reset == 2 {
            decoder = OpusDecoder::new(rate(hz), channels(api))?;
        } else if reset == 1 {
            decoder.reset();
        }
        let state: Vec<i32> = lines
            .next()
            .ok_or("missing normal state")?
            .split_whitespace()
            .skip(1)
            .map(|s| s.parse())
            .collect::<Result<_, _>>()?;
        let plc: Vec<i64> = lines
            .next()
            .ok_or("missing PLC state")?
            .split_whitespace()
            .skip(1)
            .map(|s| s.parse())
            .collect::<Result<_, _>>()?;
        let context: Option<Vec<i32>> = if with_context {
            Some(
                lines
                    .next()
                    .ok_or("missing context")?
                    .split_whitespace()
                    .skip(1)
                    .map(str::parse)
                    .collect::<Result<_, _>>()?,
            )
        } else {
            None
        };
        let data: Option<Vec<u8>> = if r[6] == "-" {
            None
        } else {
            Some(
                r[6].as_bytes()
                    .chunks_exact(2)
                    .map(|s| Ok(u8::from_str_radix(std::str::from_utf8(s)?, 16)?))
                    .collect::<Result<_, Box<dyn std::error::Error>>>()?,
            )
        };
        let repeats = if let Some(n) = kind.strip_prefix("repeat") {
            n.parse()?
        } else {
            1usize
        };
        let mut output = None;
        for _ in 0..repeats {
            output = Some(
                if let Some(data) = &data {
                    if kind == "fec" {
                        decoder.decode_fec(data, duration)
                    } else {
                        decoder.decode(data)
                    }
                } else {
                    decoder.conceal_samples(duration)
                }
                .map_err(|e| format!("frame{count} {kind} {hz}Hz C{api}: {e}"))?,
            );
        }
        let actual = output.ok_or("missing output")?;
        let expected: Vec<i16> = r[9..].iter().map(|s| s.parse()).collect::<Result<_, _>>()?;
        assert_eq!(
            actual.samples(),
            expected,
            "PCM frame{count} {kind} {hz}Hz C{api}"
        );
        assert_eq!(actual.final_range(), r[7].parse()?);
        assert_eq!(decoder.final_range(), actual.final_range());
        assert_eq!(actual.samples_per_channel(), r[8].parse::<usize>()?);
        assert_eq!(actual.channels(), api);
        assert_eq!(actual.sample_rate(), hz);
        for c in 0..2 {
            for (k, values) in [
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
                    &state[42 * k + 21 * c..42 * k + 21 * (c + 1)],
                    "energy frame{count} {kind} history{k} channel{c}"
                );
            }
        }
        let backend = decoder.celt().synthesis();
        assert_eq!(
            backend.deemphasis_memory(),
            &state[168..170],
            "deemphasis frame{count}"
        );
        for c in 0..api {
            let post = backend.postfilter(c).ok_or("postfilter")?;
            for (k, p) in post.parameters().iter().enumerate() {
                assert_eq!(
                    [
                        i32::from(p.period()),
                        i32::from(p.gain()),
                        p.tapset() as i32
                    ],
                    state[170 + 3 * k..173 + 3 * k],
                    "filter params frame{count} {kind}"
                );
            }
            assert_eq!(
                backend.overlap(c).ok_or("overlap")?.as_slice(),
                &state[176 + c * 60..176 + (c + 1) * 60],
                "overlap frame{count} {kind}"
            );
            assert_eq!(
                post.history().as_slice(),
                &state[176 + api * 60 + c * 1026..176 + api * 60 + (c + 1) * 1026],
                "filter history frame{count} {kind}"
            );
        }
        let loss = decoder.celt().concealment();
        let mode = match loss.mode() {
            ConcealmentMode::None => 0,
            ConcealmentMode::Normal => 1,
            ConcealmentMode::Noise => 2,
            ConcealmentMode::Periodic => 3,
        };
        assert_eq!(
            [
                i64::from(decoder.celt().rng_state()),
                i64::from(loss.pitch_period()),
                i64::from(loss.loss_duration()),
                i64::from(loss.plc_duration()),
                i64::from(loss.skips_periodic()),
                mode,
                i64::from(loss.prefilter_pending()),
                loss.end_band() as i64
            ],
            plc[..8],
            "PLC control frame{count} {kind}"
        );
        for c in 0..api {
            let expected: Vec<i16> = plc[8 + 24 * c..8 + 24 * (c + 1)]
                .iter()
                .map(|&v| v as i16)
                .collect();
            assert_eq!(
                loss.coefficients(c).ok_or("LPC channel")?.as_slice(),
                expected,
                "LPC frame{count}"
            );
            let expected: Vec<i32> = plc[8 + api * 24 + 2048 * c..8 + api * 24 + 2048 * (c + 1)]
                .iter()
                .map(|&v| v as i32)
                .collect();
            assert_eq!(
                loss.history(c).ok_or("signal channel")?.as_slice(),
                expected,
                "full history frame{count} {kind}"
            );
            let expected: Vec<i32> = plc
                [8 + api * (24 + 2048) + 120 * c..8 + api * (24 + 2048) + 120 * (c + 1)]
                .iter()
                .map(|&v| v as i32)
                .collect();
            assert_eq!(
                loss.future(c).ok_or("future channel")?.as_slice(),
                expected,
                "future frame{count} {kind}"
            );
        }
        if let Some(context) = context {
            let previous = match context[1] {
                0 => None,
                1000 => Some(opura::Mode::Silk),
                1001 => Some(opura::Mode::Hybrid),
                1002 => Some(opura::Mode::Celt),
                _ => panic!("mode"),
            };
            assert_eq!(
                decoder.previous_mode(),
                previous,
                "previous mode frame{count}"
            );
            assert_eq!(
                decoder.previous_redundancy(),
                context[2] != 0,
                "redundancy frame{count}"
            );
            assert_eq!(
                decoder.last_packet_duration(),
                context[4] as u16,
                "duration frame{count}"
            );
            assert_eq!(
                decoder.celt().concealment().start_band(),
                context[7] as usize,
                "start band frame{count}"
            );
            transition_flags |= context[8];
        }
        modes[mode as usize] += 1;
        capped += usize::from(loss.loss_duration() == 10000);
        count += 1;
    }
    if with_context {
        assert_eq!(transition_flags, 15);
    }
    assert_eq!((count, modes, capped), expected);
    Ok(())
}

#[test]
fn prior_hybrid_loss_and_fec_evidence_matches() -> Result<(), Box<dyn std::error::Error>> {
    reference_stream(
        include_str!("hybrid-loss-fixtures.txt"),
        (2130, [120, 840, 1170, 0], 0),
    )
}
#[test]
fn prior_celt_loss_evidence_matches() -> Result<(), Box<dyn std::error::Error>> {
    reference_stream(
        include_str!("celt-loss-fixtures.txt"),
        (872, [120, 331, 121, 300], 1),
    )
}

#[test]
fn mixed_modes_transitions_loss_and_fec_match() -> Result<(), Box<dyn std::error::Error>> {
    reference_stream(
        include_str!("opus-packet-fixtures.txt"),
        (2440, [320, 1350, 390, 380], 0),
    )
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
    use opura::decoder::DecoderError;
    let rows: Vec<_> = include_str!("hybrid-packet-fixtures.txt")
        .lines()
        .filter(|s| s.starts_with("packet "))
        .collect();
    let data = unhex(rows[1].split_whitespace().nth(4).ok_or("warm hex")?)?;
    let mut decoder = OpusDecoder::new(OutputRate::Khz48, PcmChannels::Stereo)?;
    let celt = include_str!("celt-packet-fixtures.txt")
        .lines()
        .filter(|s| s.starts_with("packet "))
        .nth(1)
        .ok_or("CELT warm")?;
    let celt = unhex(celt.split_whitespace().nth(4).ok_or("CELT hex")?)?;
    decoder.decode(&celt)?;
    assert_eq!(decoder.previous_mode(), Some(opura::Mode::Celt));
    let before = decoder.clone();
    for bad in [&[][..], &[0x02][..], &[0x03, 0][..], &[0x63, 0][..]] {
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
    let mut hybrid = before.clone();
    hybrid.decode(&data)?;
    let saved = hybrid.clone();
    assert!(hybrid.decode_fec(&[0x64, 0xff, 0xff], 1200).is_err());
    assert_eq!(hybrid, saved, "FEC prefix PLC must roll back too");
    let mut reset = decoder.clone();
    reset.reset();
    let mut fresh = OpusDecoder::new(OutputRate::Khz48, PcmChannels::Stereo)?;
    assert_eq!(reset.decode(&data)?, fresh.decode(&data)?);
    assert_eq!(reset, fresh);
    Ok(())
}

#[test]
fn malformed_packets_and_valid_loss_sequences_are_bounded_and_atomic()
-> Result<(), Box<dyn std::error::Error>> {
    let mut decoder = OpusDecoder::new(OutputRate::Khz24, PcmChannels::Stereo)?;
    let mut seed = 0x4f505553u32;
    let mut errors = 0;
    for i in 0..512 {
        let mut data = vec![0u8; 3 + (i * 37) % 384];
        data[0] = ((i % 32) as u8) << 3 | (((i / 32) % 2) as u8) << 2;
        for b in &mut data[1..] {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            *b = seed as u8;
        }
        let before = decoder.clone();
        let result = if i % 3 == 0 {
            decoder.decode_fec(&data, [120, 960, 5760][(i / 3) % 3])
        } else {
            decoder.decode(&data)
        };
        match result {
            Ok(pcm) => {
                assert!(pcm.samples().len() <= 11520);
                assert_eq!(pcm.channels(), 2);
            }
            Err(_) => {
                errors += 1;
                assert_eq!(decoder, before);
            }
        }
        if i % 4 == 0 {
            let duration = [120, 240, 360, 480, 600, 840, 960, 1440, 5760][i % 9];
            let pcm = decoder.conceal_samples(duration)?;
            assert_eq!(pcm.samples_per_channel(), usize::from(duration) / 2);
            assert_eq!(pcm.final_range(), 0);
        }
    }
    assert!(errors > 0);
    Ok(())
}
#[test]
fn fec_fallback_and_invalid_duration_preserve_native_context()
-> Result<(), Box<dyn std::error::Error>> {
    use opura::decoder::DecoderError;
    let mut decoder = OpusDecoder::new(OutputRate::Khz48, PcmChannels::Stereo)?;
    for duration in [0, 1, 119, 121, 5761, 5880, u16::MAX] {
        let before = decoder.clone();
        assert_eq!(
            decoder.conceal_samples(duration),
            Err(DecoderError::InvalidDuration)
        );
        assert_eq!(
            decoder.decode_fec(&[0xf8], duration),
            Err(DecoderError::InvalidDuration)
        );
        assert_eq!(decoder, before);
    }
    let data = include_str!("hybrid-packet-fixtures.txt")
        .lines()
        .find(|s| s.starts_with("packet "))
        .ok_or("Hybrid warm")?;
    let data = unhex(data.split_whitespace().nth(4).ok_or("Hybrid hex")?)?;
    for warm in [false, true] {
        if warm {
            decoder.decode(&data)?;
        }
        for duration in [120, 360, 600, 5760] {
            let mut plc = decoder.clone();
            let mut fec = decoder.clone();
            assert_eq!(
                plc.conceal_samples(duration)?,
                fec.decode_fec(&[0xf8, 0xff, 0xff], duration)?
            );
            assert_eq!(plc, fec, "CELT packets cannot supply FEC");
        }
        let mut plc = decoder.clone();
        let mut fec = decoder.clone();
        assert_eq!(plc.conceal_samples(120)?, fec.decode_fec(&data, 120)?);
        assert_eq!(plc, fec, "requested interval is shorter than packet FEC");
    }
    Ok(())
}
