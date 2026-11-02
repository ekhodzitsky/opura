use super::*;
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
fn rate(value: i64) -> SilkRate {
    match value {
        8 => SilkRate::Khz8,
        12 => SilkRate::Khz12,
        _ => SilkRate::Khz16,
    }
}
fn optional_rate(value: i64) -> Option<SilkRate> {
    (value != 0).then(|| rate(value))
}
fn input_rate(value: i64) -> OutputRate {
    match value {
        8000 => OutputRate::Khz8,
        12000 => OutputRate::Khz12,
        16000 => OutputRate::Khz16,
        24000 => OutputRate::Khz24,
        _ => OutputRate::Khz48,
    }
}
fn duration(value: i64) -> PacketDuration {
    match value {
        10 => PacketDuration::Ms10,
        20 => PacketDuration::Ms20,
        40 => PacketDuration::Ms40,
        _ => PacketDuration::Ms60,
    }
}
fn state_values(s: &BandwidthTransition) -> [i64; 5] {
    [
        i64::from(s.position),
        i64::from(s.direction),
        s.saved.map_or(0, |r| i64::from(r.khz())),
        i64::from(s.delay[0]),
        i64::from(s.delay[1]),
    ]
}
#[test]
fn unchanged_native_bandwidth_decisions_and_every_lp_taper_step_match() -> Result<()> {
    let mut controls = 0;
    let mut filters = 0;
    let mut ready = 0;
    for line in include_str!("../../../tests/silk-encoder-bandwidth-fixtures.txt").lines() {
        if line.starts_with('#') {
            continue;
        }
        let fields: Vec<_> = line.split_whitespace().collect();
        let end = if fields[0] == "filter" {
            fields.len() - 1
        } else {
            fields.len()
        };
        let v: Vec<i64> = fields[1..end]
            .iter()
            .map(|s| s.parse())
            .collect::<std::result::Result<_, _>>()?;
        if fields[0] == "control" {
            let mut s = BandwidthTransition {
                position: v[9] as u16,
                direction: v[10] as i8,
                saved: optional_rate(v[11]),
                delay: [v[12] as i32, v[13] as i32],
            };
            let result = s.select(BandwidthControls {
                current: optional_rate(v[0]),
                input: input_rate(v[1]),
                minimum: rate(v[2]),
                maximum: rate(v[3]),
                desired: rate(v[4]),
                allow_switch: v[5] != 0,
                can_switch: v[6] != 0,
                duration: duration(v[7]),
                max_bits: v[8] as u32,
            })?;
            assert_eq!(result.rate(), rate(v[14]));
            assert_eq!(result.switch_ready(), v[15] != 0);
            assert_eq!(result.max_bits(), v[16] as u32);
            assert_eq!(state_values(&s), v[17..]);
            controls += 1;
            ready += usize::from(result.switch_ready());
        } else {
            assert_eq!(fields[0], "filter");
            let mut s = BandwidthTransition {
                position: v[2] as u16,
                direction: v[3] as i8,
                saved: optional_rate(v[4]),
                delay: [v[5] as i32, v[6] as i32],
            };
            let mut random = v[7] as u32;
            let length = (v[0] * v[1]) as usize;
            let mut samples: Vec<i16> = (0..length)
                .map(|i| {
                    random = random.wrapping_mul(1664525).wrapping_add(1013904223);
                    match v[8] {
                        0 => 0,
                        1 => i16::MIN,
                        2 => i16::MAX,
                        3 => {
                            if i % 2 == 0 {
                                i16::MAX
                            } else {
                                i16::MIN
                            }
                        }
                        4 => ((i % 64) as i32 * 1024 - 32768) as i16,
                        _ => ((random >> 16) as i32 - 32768) as i16,
                    }
                })
                .collect();
            s.filter(
                rate(v[0]),
                if v[1] == 10 {
                    FrameDuration::Ms10
                } else {
                    FrameDuration::Ms20
                },
                &mut samples,
            )?;
            assert_eq!(state_values(&s), v[9..]);
            let hash = samples
                .iter()
                .flat_map(|s| s.to_le_bytes())
                .fold(14695981039346656037u64, |h, b| {
                    (h ^ u64::from(b)).wrapping_mul(1099511628211)
                });
            assert_eq!(
                hash,
                fields
                    .last()
                    .ok_or("missing PCM fingerprint")?
                    .parse::<u64>()?
            );
            filters += 1;
        }
    }
    assert_eq!((controls, filters), (2400, 4626));
    assert!(ready > 20);
    Ok(())
}
#[test]
fn invalid_controls_filter_geometry_and_reset_preserve_contracts() -> Result<()> {
    let mut s = BandwidthTransition::default();
    let controls = BandwidthControls {
        current: Some(SilkRate::Khz16),
        input: OutputRate::Khz48,
        minimum: SilkRate::Khz8,
        maximum: SilkRate::Khz16,
        desired: SilkRate::Khz8,
        allow_switch: true,
        can_switch: false,
        duration: PacketDuration::Ms20,
        max_bits: 512,
    };
    let choice = s.select(controls)?;
    assert_eq!(choice.rate(), SilkRate::Khz16);
    assert_eq!((s.position(), s.direction()), (256, -2));
    let mut pcm = [i16::MAX; 320];
    s.filter(SilkRate::Khz16, FrameDuration::Ms20, &mut pcm)?;
    let saved = s.clone();
    for invalid in [
        BandwidthControls {
            max_bits: 0,
            ..controls
        },
        BandwidthControls {
            max_bits: 10201,
            ..controls
        },
        BandwidthControls {
            minimum: SilkRate::Khz16,
            ..controls
        },
        BandwidthControls {
            maximum: SilkRate::Khz8,
            desired: SilkRate::Khz16,
            ..controls
        },
    ] {
        assert_eq!(s.select(invalid), Err(BandwidthError::InvalidControls));
        assert_eq!(s, saved);
    }
    let original = pcm;
    assert_eq!(
        s.filter(SilkRate::Khz16, FrameDuration::Ms20, &mut pcm[..319]),
        Err(BandwidthError::InvalidInputLength)
    );
    assert_eq!(s, saved);
    assert_eq!(pcm, original);
    s.reset_filter_delay();
    assert_eq!(s.delay, [0; 2]);
    assert_eq!(s.direction(), saved.direction());
    s.save_rate(SilkRate::Khz12);
    s.reset();
    assert_eq!(s, BandwidthTransition::default());
    Ok(())
}
#[test]
fn public_pcm_band_changes_decode_continuously_at_every_input_rate() -> Result<()> {
    use crate::{
        Bandwidth, Packet,
        decoder::OpusDecoder,
        silk::{
            decoder::PcmChannels,
            encoder::{
                mono::{MonoEncoder, MonoOptions},
                stereo::StereoEncoder,
            },
        },
    };
    for input in [
        OutputRate::Khz8,
        OutputRate::Khz12,
        OutputRate::Khz16,
        OutputRate::Khz24,
        OutputRate::Khz48,
    ] {
        let mut mono = MonoEncoder::new(input, SilkRate::Khz16);
        let mut stereo = StereoEncoder::new(input, SilkRate::Khz16);
        mono.set_inband_fec(true);
        stereo.set_inband_fec(true);
        let mut a = OpusDecoder::new(OutputRate::Khz48, PcmChannels::Mono)?;
        let mut b = OpusDecoder::new(OutputRate::Khz48, PcmChannels::Stereo)?;
        for (step, rate) in [
            SilkRate::Khz16,
            SilkRate::Khz12,
            SilkRate::Khz8,
            SilkRate::Khz16,
            SilkRate::Khz8,
            SilkRate::Khz12,
        ]
        .into_iter()
        .enumerate()
        {
            mono.set_bandwidth(rate)?;
            stereo.set_bandwidth(rate)?;
            let expected = match rate {
                SilkRate::Khz8 => Bandwidth::Narrowband,
                SilkRate::Khz12 => Bandwidth::Mediumband,
                SilkRate::Khz16 => Bandwidth::Wideband,
            };
            let duration = [
                PacketDuration::Ms10,
                PacketDuration::Ms60,
                PacketDuration::Ms20,
                PacketDuration::Ms40,
            ][step % 4];
            let n = input.hz() as usize * duration.milliseconds() as usize / 1000;
            for frame in 0..2 {
                let pcm: Vec<i16> = (0..n)
                    .map(|i| (((step * 2 + frame) * n + i) % 80) as i16 * 300 - 12000)
                    .collect();
                let pairs: Vec<i16> = pcm
                    .iter()
                    .flat_map(|&v| [v, if step % 2 == 0 { v } else { -v }])
                    .collect();
                let options = MonoOptions {
                    complexity: 0,
                    packet_loss_percent: 20,
                    ..MonoOptions::default()
                };
                let bytes = duration.milliseconds() as usize * 8;
                let x = mono.encode(duration, &pcm, bytes, options)?;
                let y = stereo.encode(duration, &pairs, bytes, options)?;
                assert_eq!(Packet::parse(x.as_bytes())?.bandwidth(), expected);
                assert_eq!(Packet::parse(y.as_bytes())?.bandwidth(), expected);
                assert_eq!(
                    a.decode(x.as_bytes())?.samples().len(),
                    duration.milliseconds() as usize * 48
                );
                assert_eq!(
                    b.decode(y.as_bytes())?.samples().len(),
                    duration.milliseconds() as usize * 96
                );
                assert_eq!(a.final_range(), x.final_range());
                assert_eq!(b.final_range(), y.final_range());
                if frame == 0 {
                    assert!(!mono.has_fec() && !stereo.has_fec());
                }
            }
        }
    }
    Ok(())
}
