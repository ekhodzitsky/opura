//! Public libopus PLC/DTX/recovery PCM plus complete independent state snapshots.
use opura::{
    Packet,
    celt::{
        FrameSize,
        decoder::{CeltDecoder, ConcealmentError, ConcealmentMode, DecoderError, FrameConfig},
        output::Decimation,
    },
};
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
fn public_loss_recovery_pcm_and_complete_state_match() -> Result<(), Box<dyn std::error::Error>> {
    let mut decoder = CeltDecoder::new(Decimation::By1, 1)?;
    let mut lines = include_str!("celt-loss-fixtures.txt")
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
        if decoder.sample_rate() != hz || decoder.channels() != api {
            decoder = CeltDecoder::new(rate(hz), api)?;
        }
        if reset == 2 {
            decoder = CeltDecoder::new(rate(hz), api)?;
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
        let mut separate = decoder.clone();
        let mut frame_pcm = Vec::new();
        if let Some(data) = &data {
            let packet = Packet::parse(data)?;
            let size = match packet.frame_samples() {
                120 => FrameSize::Ms2_5,
                240 => FrameSize::Ms5,
                480 => FrameSize::Ms10,
                960 => FrameSize::Ms20,
                _ => panic!("CELT frame size"),
            };
            let config = FrameConfig {
                size,
                channels: usize::from(packet.channels()),
                bandwidth: packet.bandwidth(),
            };
            for payload in packet.frames() {
                frame_pcm.extend_from_slice(separate.decode_frame(payload, config)?.samples());
            }
        }
        let repeats = if let Some(n) = kind.strip_prefix("repeat") {
            n.parse()?
        } else {
            1usize
        };
        let mut output = None;
        for _ in 0..repeats {
            output = Some(
                if let Some(data) = &data {
                    decoder.decode(data)
                } else {
                    decoder.conceal_samples(duration)
                }
                .map_err(|e| format!("frame{count} {kind} {hz}Hz C{api}: {e}"))?,
            );
        }
        let actual = output.ok_or("missing output")?;
        if data.is_some() {
            assert_eq!(actual.samples(), frame_pcm);
            assert_eq!(decoder, separate, "packet/frame state {count} {kind}");
        }
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
                    &state[42 * k + 21 * c..42 * k + 21 * (c + 1)],
                    "energy frame{count} {kind} history{k} channel{c}"
                );
            }
        }
        let backend = decoder.synthesis();
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
        let loss = decoder.concealment();
        let mode = match loss.mode() {
            ConcealmentMode::None => 0,
            ConcealmentMode::Normal => 1,
            ConcealmentMode::Noise => 2,
            ConcealmentMode::Periodic => 3,
        };
        assert_eq!(
            [
                i64::from(decoder.rng_state()),
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
    assert_eq!((count, modes, capped), (872, [120, 331, 121, 300], 1));
    Ok(())
}
#[test]
fn loss_duration_errors_and_reset_are_atomic() -> Result<(), Box<dyn std::error::Error>> {
    let mut decoder = CeltDecoder::new(Decimation::By1, 2)?;
    decoder.decode(&[0xf8, 0xff, 0xff])?;
    decoder.conceal_samples(5760)?;
    let saved = decoder.clone();
    for duration in [0, 1, 119, 121, 5761, 5880, u16::MAX] {
        assert_eq!(
            decoder.conceal_samples(duration),
            Err(DecoderError::Concealment(ConcealmentError::InvalidDuration))
        );
        assert_eq!(decoder, saved);
    }
    decoder.reset();
    assert_eq!(decoder, CeltDecoder::new(Decimation::By1, 2)?);
    let before = decoder.clone();
    let initial = decoder.conceal_samples(5760)?;
    assert!(initial.samples().iter().all(|&v| v == 0));
    assert_eq!(initial.samples_per_channel(), 5760);
    assert_eq!(decoder, before);
    Ok(())
}

#[test]
fn adversarial_normal_loss_recovery_sequences_remain_bounded_and_atomic()
-> Result<(), Box<dyn std::error::Error>> {
    let mut decoder = CeltDecoder::new(Decimation::By3, 2)?;
    let mut seed = 0x5245434fu32;
    let sizes = [
        FrameSize::Ms2_5,
        FrameSize::Ms5,
        FrameSize::Ms10,
        FrameSize::Ms20,
    ];
    for index in 0..256 {
        let mut data = vec![0u8; [2, 3, 8, 16, 64, 255, 1275][index % 7]];
        for byte in &mut data {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            *byte = seed as u8;
        }
        let config = FrameConfig {
            size: sizes[index % 4],
            channels: 1 + index % 2,
            bandwidth: [
                opura::Bandwidth::Narrowband,
                opura::Bandwidth::Wideband,
                opura::Bandwidth::SuperWideband,
                opura::Bandwidth::Fullband,
            ][index % 4],
        };
        let saved = decoder.clone();
        if decoder.decode_frame(&data, config).is_err() {
            assert_eq!(decoder, saved);
        }
        let duration = [120, 360, 600, 840, 5760][index % 5];
        let pcm = decoder
            .conceal_samples(duration)
            .map_err(|e| format!("adversarial PLC index{index}: {e}"))?;
        assert_eq!(pcm.samples_per_channel(), usize::from(duration) / 3);
        assert_eq!(pcm.final_range(), 0);
        assert!(decoder.concealment().loss_duration() <= 10000);
        assert!(decoder.concealment().plc_duration() <= 10000);
        for c in 0..2 {
            assert!(
                decoder
                    .concealment()
                    .history(c)
                    .ok_or("history")?
                    .iter()
                    .all(|v| (-536870911..=536870911).contains(v))
            );
            assert!(
                4096 + decoder
                    .concealment()
                    .coefficients(c)
                    .ok_or("coefficients")?
                    .iter()
                    .map(|&v| i32::from(v).abs())
                    .sum::<i32>()
                    < 65535
            );
        }
    }
    Ok(())
}
