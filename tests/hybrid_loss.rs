//! Public Hybrid libopus PLC/DTX/FEC/recovery PCM and independent state snapshots.
use opura::{
    celt::decoder::ConcealmentMode,
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
#[test]
fn public_loss_recovery_pcm_and_complete_state_match() -> Result<(), Box<dyn std::error::Error>> {
    let mut decoder = HybridDecoder::new(OutputRate::Khz48, PcmChannels::Mono)?;
    let mut lines = include_str!("hybrid-loss-fixtures.txt")
        .lines()
        .filter(|s| !s.starts_with('#'));
    let mut count = 0;
    let mut modes = [0; 4];
    let mut capped = 0;
    while let Some(line) = lines.next() {
        let r: Vec<_> = line.split_whitespace().collect();
        assert_eq!(r[0], "frame");
        let kind = r[1];
        let reset: u8 = r[2].parse()?;
        let hz: u32 = r[3].parse()?;
        let api: usize = r[4].parse()?;
        let duration: u16 = r[5].parse()?;
        if decoder.output_rate().hz() != hz || decoder.output_channels().count() != api {
            decoder = HybridDecoder::new(rate(hz), channels(api))?;
        }
        if reset == 2 {
            decoder = HybridDecoder::new(rate(hz), channels(api))?;
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
                        decoder.decode_fec(data)
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
        modes[mode as usize] += 1;
        capped += usize::from(loss.loss_duration() == 10000);
        count += 1;
    }
    assert_eq!((count, modes, capped), (2130, [120, 840, 1170, 0], 0));
    Ok(())
}

#[test]
fn invalid_loss_durations_and_resets_preserve_context() -> Result<(), Box<dyn std::error::Error>> {
    use opura::hybrid::DecoderError;
    for hz in [8000, 12000, 16000, 24000, 48000] {
        for api in [1, 2] {
            let mut decoder = HybridDecoder::new(rate(hz), channels(api))?;
            let initial = decoder.clone();
            for duration in [120, 360, 600, 840, 5760] {
                let pcm = decoder.conceal_samples(duration)?;
                assert_eq!(
                    pcm.samples_per_channel(),
                    usize::from(duration) * hz as usize / 48000
                );
                assert!(pcm.samples().iter().all(|&v| v == 0));
                assert_eq!(decoder, initial);
            }
            for duration in [0, 1, 119, 121, 5761, 5880, u16::MAX] {
                assert_eq!(
                    decoder.conceal_samples(duration),
                    Err(DecoderError::InvalidLossDuration)
                );
                assert_eq!(decoder, initial);
            }
            let pcm = decoder.decode(&[0x78])?;
            assert_eq!(pcm.samples_per_channel(), hz as usize / 50);
            assert_eq!(decoder.celt(), initial.celt());
            decoder.reset();
            assert_eq!(decoder, initial);
        }
    }
    Ok(())
}
#[test]
fn corrupted_normal_fec_loss_recovery_is_bounded_and_atomic()
-> Result<(), Box<dyn std::error::Error>> {
    let mut decoder = HybridDecoder::new(OutputRate::Khz24, PcmChannels::Stereo)?;
    let mut seed = 0x48594645u32;
    for i in 0..128 {
        let mut data = vec![0u8; 3 + (i * 37) % 384];
        data[0] = [0x60, 0x6c, 0x70, 0x7c][i % 4];
        for b in &mut data[1..] {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            *b = seed as u8;
        }
        let before = decoder.clone();
        let result = if i % 3 == 0 {
            decoder.decode_fec(&data)
        } else {
            decoder.decode(&data)
        };
        if result.is_err() {
            assert_eq!(decoder, before);
        }
        let duration = [120, 240, 360, 480, 600, 840, 960, 1440, 5760][i % 9];
        let pcm = decoder.conceal_samples(duration)?;
        assert_eq!(pcm.samples_per_channel(), usize::from(duration) / 2);
        assert_eq!(pcm.final_range(), 0);
        assert_eq!(decoder.final_range(), 0);
        let before = decoder.clone();
        if decoder.decode(&data).is_err() {
            assert_eq!(decoder, before);
        }
    }
    Ok(())
}

#[test]
fn fec_uses_only_first_compressed_frame() -> Result<(), Box<dyn std::error::Error>> {
    let row = include_str!("hybrid-loss-fixtures.txt")
        .lines()
        .find(|s| s.starts_with("frame fec "))
        .ok_or("FEC fixture")?;
    let hex = row.split_whitespace().nth(6).ok_or("FEC hex")?;
    let data: Vec<u8> = hex
        .as_bytes()
        .chunks_exact(2)
        .map(|s| Ok(u8::from_str_radix(std::str::from_utf8(s)?, 16)?))
        .collect::<Result<_, Box<dyn std::error::Error>>>()?;
    let packet = opura::Packet::parse(&data)?;
    assert_eq!(packet.frames().len(), 1);
    let first = packet.frames()[0];
    let mut multi = vec![(data[0] & 0xfc) | 2];
    if first.len() < 252 {
        multi.push(first.len() as u8);
    } else {
        multi.push((252 + first.len() % 4) as u8);
        multi.push(((first.len() - 252) / 4) as u8);
    }
    multi.extend_from_slice(first);
    multi.push(0);
    let mut single = HybridDecoder::new(OutputRate::Khz48, PcmChannels::Stereo)?;
    let mut combined = single.clone();
    assert_eq!(single.decode_fec(&data)?, combined.decode_fec(&multi)?);
    assert_eq!(single, combined);
    Ok(())
}
