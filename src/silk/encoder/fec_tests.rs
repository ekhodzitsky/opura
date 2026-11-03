use super::*;
use crate::{
    Packet,
    decoder::OpusDecoder,
    silk::{
        decode_header,
        decoder::PcmChannels,
        encoder::{
            mono::{MonoEncoder, MonoOptions},
            stereo::StereoEncoder,
        },
        resampler::OutputRate,
    },
};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
#[test]
fn original_fixed_bandwidth_thresholds_and_gain_policy_match() -> Result<()> {
    let mut cases = 0;
    for line in include_str!("../../../tests/silk-encoder-fec-policy-fixtures.txt").lines() {
        if line.starts_with('#') {
            continue;
        }
        let v: Vec<i32> = line
            .split_whitespace()
            .map(|s| s.parse())
            .collect::<std::result::Result<_, _>>()?;
        let rate = match v[0] {
            1101 => SilkRate::Khz8,
            1102 => SilkRate::Khz12,
            _ => SilkRate::Khz16,
        };
        let expected = v[4] != 0 && v[5] == v[0];
        let mut mono = FecState::<1> {
            enabled: true,
            previous_enabled: v[1] != 0,
            ..FecState::default()
        };
        mono.begin(PacketDuration::Ms20, rate, v[3], v[2] as u8);
        let mut stereo = FecState::<2> {
            enabled: true,
            previous_enabled: v[1] != 0,
            ..FecState::default()
        };
        stereo.begin(PacketDuration::Ms20, rate, 2 * v[3], v[2] as u8);
        let mut mono_in_stereo_owner = FecState::<2> {
            enabled: true,
            previous_enabled: v[1] != 0,
            ..FecState::default()
        };
        mono_in_stereo_owner.begin_for_channels(
            PacketDuration::Ms20,
            rate,
            v[3],
            v[2] as u8,
            None,
            1,
        )?;
        assert_eq!(mono.generating, expected);
        assert_eq!(stereo.generating, expected);
        assert_eq!(mono_in_stereo_owner.generating, expected);
        if expected {
            assert_eq!(i32::from(mono.gain_increase), v[6]);
            assert_eq!(i32::from(stereo.gain_increase), v[6]);
            assert_eq!(i32::from(mono_in_stereo_owner.gain_increase), v[6]);
        }
        cases += 1;
    }
    assert_eq!(cases, 576);
    Ok(())
}
fn packet_fec(bytes: &[u8]) -> Result<bool> {
    let packet = Packet::parse(bytes)?;
    if packet.frames()[0].len() <= 1 {
        return Ok(false);
    }
    let (header, _) = decode_header(&packet, 0)?;
    Ok((0..header.channel_count())
        .any(|ch| header.redundancy(ch).is_some_and(|f| f.iter().any(|&v| v))))
}
#[test]
fn pcm_fec_packets_cover_all_geometries_and_sparse_stereo_masks() -> Result<()> {
    let mut protected = 0;
    let mut sparse = 0;
    for rate in [
        OutputRate::Khz8,
        OutputRate::Khz12,
        OutputRate::Khz16,
        OutputRate::Khz24,
        OutputRate::Khz48,
    ] {
        for bandwidth in [SilkRate::Khz8, SilkRate::Khz12, SilkRate::Khz16] {
            for duration in [
                PacketDuration::Ms10,
                PacketDuration::Ms20,
                PacketDuration::Ms40,
                PacketDuration::Ms60,
            ] {
                for variable in [false, true] {
                    let mut mono = MonoEncoder::new(rate, bandwidth);
                    let mut stereo = StereoEncoder::new(rate, bandwidth);
                    mono.set_inband_fec(true);
                    stereo.set_inband_fec(true);
                    let mut mono_decoder = OpusDecoder::new(OutputRate::Khz48, PcmChannels::Mono)?;
                    let mut stereo_decoder =
                        OpusDecoder::new(OutputRate::Khz48, PcmChannels::Stereo)?;
                    let n = rate.hz() as usize * duration.milliseconds() as usize / 1000;
                    for frame in 0..16 {
                        let left: Vec<i16> = (0..n)
                            .map(|i| (((frame * n + i) % 80) as i16 - 40) * 300)
                            .collect();
                        let pair: Vec<i16> = left
                            .iter()
                            .enumerate()
                            .flat_map(|(i, &v)| {
                                [
                                    v,
                                    if frame < 5 {
                                        v
                                    } else if frame < 10 {
                                        -v
                                    } else {
                                        ((i * 7919 % 30000) as i16) - 15000
                                    },
                                ]
                            })
                            .collect();
                        let options = MonoOptions {
                            complexity: if bandwidth == SilkRate::Khz8 { 0 } else { 6 },
                            variable_bitrate: variable,
                            packet_loss_percent: 20,
                        };
                        let budget = duration.milliseconds() as usize * 8;
                        let a = mono.encode(duration, &left, budget, options).map_err(|e| {
                            format!("mono {rate:?}/{bandwidth:?}/{duration:?}/frame{frame}: {e}")
                        })?;
                        let b = stereo
                            .encode(duration, &pair, budget, options)
                            .map_err(|e| {
                                format!(
                                    "stereo {rate:?}/{bandwidth:?}/{duration:?}/frame{frame}: {e}"
                                )
                            })?;
                        assert_eq!(packet_fec(a.as_bytes())?, mono.has_fec());
                        assert_eq!(packet_fec(b.as_bytes())?, stereo.has_fec());
                        assert!(a.as_bytes().len() <= budget && b.as_bytes().len() <= budget);
                        if !variable {
                            assert_eq!(a.as_bytes().len(), budget);
                            assert_eq!(b.as_bytes().len(), budget);
                        }
                        assert_eq!(
                            mono_decoder.decode(a.as_bytes())?.samples().len(),
                            duration.milliseconds() as usize * 48
                        );
                        assert_eq!(
                            stereo_decoder.decode(b.as_bytes())?.samples().len(),
                            duration.milliseconds() as usize * 96
                        );
                        assert_eq!(mono_decoder.final_range(), a.final_range());
                        assert_eq!(stereo_decoder.final_range(), b.final_range());
                        protected += usize::from(mono.has_fec()) + usize::from(stereo.has_fec());
                        let (h, _) = decode_header(&Packet::parse(b.as_bytes())?, 0)?;
                        sparse += usize::from(h.redundancy(0) != h.redundancy(1));
                    }
                }
            }
        }
    }
    assert!(protected > 1000);
    assert!(sparse > 100);
    Ok(())
}
#[test]
fn queue_geometry_budget_retry_dtx_and_errors_preserve_stream_contracts() -> Result<()> {
    let mut mono = MonoEncoder::new(OutputRate::Khz48, SilkRate::Khz16);
    let mut stereo = StereoEncoder::new(OutputRate::Khz48, SilkRate::Khz16);
    mono.set_inband_fec(true);
    stereo.set_inband_fec(true);
    mono.set_dtx(true);
    stereo.set_dtx(true);
    let options = MonoOptions {
        packet_loss_percent: 20,
        complexity: 0,
        ..MonoOptions::default()
    };
    for _ in 0..5 {
        let pcm: Vec<i16> = (0..960).map(|i| ((i % 80) as i16 - 40) * 300).collect();
        mono.encode(PacketDuration::Ms20, &pcm, 160, options)?;
        let pair: Vec<i16> = pcm.iter().flat_map(|&v| [v, -v]).collect();
        stereo.encode(PacketDuration::Ms20, &pair, 160, options)?;
    }
    let saved_mono = mono.clone();
    let saved_stereo = stereo.clone();
    assert!(
        mono.encode(PacketDuration::Ms20, &[0; 959], 80, options)
            .is_err()
    );
    assert!(
        stereo
            .encode(PacketDuration::Ms20, &[0; 1919], 80, options)
            .is_err()
    );
    assert!(
        mono.encode(PacketDuration::Ms20, &[i16::MAX; 960], 3, options)
            .is_err()
    );
    assert!(
        stereo
            .encode(PacketDuration::Ms20, &[i16::MAX; 1920], 3, options)
            .is_err()
    );
    assert_eq!(mono, saved_mono);
    assert_eq!(stereo, saved_stereo);
    mono.encode(PacketDuration::Ms10, &[0; 480], 80, options)?;
    stereo.encode(PacketDuration::Ms10, &[0; 960], 80, options)?;
    assert!(!mono.has_fec() && !stereo.has_fec()); // Incompatible queue duration.
    for _ in 0..40 {
        let a = mono.encode(PacketDuration::Ms20, &[0; 960], 80, options)?;
        let b = stereo.encode(PacketDuration::Ms20, &[0; 1920], 160, options)?;
        if mono.in_dtx() {
            assert_eq!(a.as_bytes().len(), 1);
            assert!(!mono.has_fec());
        }
        if stereo.in_dtx() {
            assert_eq!(b.as_bytes().len(), 1);
            assert!(!stereo.has_fec());
        }
    }
    assert!(mono.in_dtx() && stereo.in_dtx());
    mono.reset();
    stereo.reset();
    let mut fresh_mono = MonoEncoder::new(OutputRate::Khz48, SilkRate::Khz16);
    let mut fresh_stereo = StereoEncoder::new(OutputRate::Khz48, SilkRate::Khz16);
    fresh_mono.set_dtx(true);
    fresh_mono.set_inband_fec(true);
    fresh_stereo.set_dtx(true);
    fresh_stereo.set_inband_fec(true);
    assert_eq!(mono, fresh_mono);
    assert_eq!(stereo, fresh_stereo);
    Ok(())
}

#[test]
fn outer_automatic_fec_eligibility_replaces_fixed_per_channel_threshold() {
    let mut fec = FecState::<2> {
        enabled: true,
        ..FecState::default()
    };
    fec.begin_with_policy(PacketDuration::Ms20, SilkRate::Khz16, 18000, 20, Some(true));
    assert!(fec.controls(0, 0).is_some());
    fec.begin_with_policy(
        PacketDuration::Ms20,
        SilkRate::Khz16,
        64000,
        20,
        Some(false),
    );
    assert!(fec.controls(0, 0).is_none());
    fec.begin_with_policy(PacketDuration::Ms20, SilkRate::Khz16, 64000, 0, Some(true));
    assert!(fec.controls(0, 0).is_none());
    fec.enabled = false;
    fec.begin_with_policy(PacketDuration::Ms20, SilkRate::Khz16, 64000, 20, Some(true));
    assert!(fec.controls(0, 0).is_none());
}
