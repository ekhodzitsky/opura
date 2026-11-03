use super::*;
use crate::silk::resampler::OutputRate;
type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

#[test]
fn capacity_two_mono_entry_retains_the_same_live_speech_history_as_stereo_entry() -> Result {
    let mut mono_frames = 0;
    let mut stereo_frames = 0;
    for variable in [false, true] {
        let mut left = SpeechState::<2>::new(OutputRate::Khz48, SilkRate::Khz16);
        let mut right = left.clone();
        let mut left_band = PacketBandwidth::<2>::default();
        let mut right_band = left_band.clone();
        left.dtx.enabled = true;
        right.dtx.enabled = true;
        left.fec.enabled = true;
        right.fec.enabled = true;
        let mut previous = 1;
        for frame in 0..64 {
            let duration = [
                PacketDuration::Ms10,
                PacketDuration::Ms20,
                PacketDuration::Ms40,
                PacketDuration::Ms60,
            ][frame / 16];
            let wire = 1 + (frame / 8) % 2;
            let silence = (3..8).contains(&(frame % 16));
            let options = MonoOptions {
                complexity: 0,
                variable_bitrate: variable,
                packet_loss_percent: 20,
            };
            let bytes = duration.milliseconds() as usize * 8;
            left.begin_channels(previous, wire, SilkRate::Khz16);
            right.begin_channels(previous, wire, SilkRate::Khz16);
            for band in [&mut left_band, &mut right_band] {
                band.prepare_channels(
                    (OutputRate::Khz48, wire),
                    SilkRate::Khz16,
                    duration,
                    bytes,
                    options,
                    true,
                )?;
            }
            left.dtx.begin(silence);
            right.dtx.begin(silence);
            let length = 16 * duration.milliseconds() as usize;
            let mut pcm = [[0; 960]; 2];
            for (ch, samples) in pcm.iter_mut().enumerate() {
                for (i, sample) in samples[..length].iter_mut().enumerate() {
                    *sample = if silence {
                        0
                    } else {
                        (((frame * length + i + ch * 17) % 80) as i32 - 40) as i16 * 300
                    };
                }
            }
            let controls = PacketControls {
                duration,
                packet_bytes: bytes,
                options,
                digital_silence: silence,
                include_fec: true,
                channels: wire,
                to_mono: wire == 2 && frame % 8 == 7,
            };
            let a = if wire == 1 {
                mono_frames += 1;
                mono(
                    &mut left,
                    &mut left_band,
                    SilkRate::Khz16,
                    &pcm[0][..length],
                    controls,
                )?
            } else {
                stereo_frames += 1;
                stereo(&mut left, &mut left_band, SilkRate::Khz16, &pcm, controls)?
            };
            let b = stereo(&mut right, &mut right_band, SilkRate::Khz16, &pcm, controls)?;
            assert_eq!(a, b, "packet frame{frame}/variable{variable}");
            assert_eq!(
                left, right,
                "whole speech history frame{frame}/variable{variable}"
            );
            assert_eq!(left_band, right_band);
            previous = wire;
        }
    }
    assert_eq!((mono_frames, stereo_frames), (64, 64));
    Ok(())
}

#[test]
fn invalid_borrowed_packet_geometry_is_rejected_before_history_changes() -> Result {
    let mut speech = SpeechState::<2>::new(OutputRate::Khz48, SilkRate::Khz16);
    let mut band = PacketBandwidth::<2>::default();
    let options = MonoOptions::default();
    band.prepare_channels(
        (OutputRate::Khz48, 1),
        SilkRate::Khz16,
        PacketDuration::Ms20,
        160,
        options,
        false,
    )?;
    let before = speech.clone();
    let before_band = band.clone();
    let controls = PacketControls {
        duration: PacketDuration::Ms20,
        packet_bytes: 160,
        options,
        digital_silence: false,
        include_fec: true,
        channels: 1,
        to_mono: false,
    };
    assert!(mono(&mut speech, &mut band, SilkRate::Khz16, &[0; 319], controls).is_err());
    assert_eq!(speech, before);
    assert_eq!(band, before_band);
    for invalid in [
        PacketControls {
            channels: 2,
            ..controls
        },
        PacketControls {
            to_mono: true,
            ..controls
        },
        PacketControls {
            packet_bytes: 2,
            ..controls
        },
    ] {
        assert!(mono(&mut speech, &mut band, SilkRate::Khz16, &[0; 320], invalid).is_err());
        assert_eq!(speech, before);
        assert_eq!(band, before_band);
    }
    Ok(())
}

#[test]
fn borrowed_mono_fec_uses_wire_rate_in_capacity_two_state() -> Result {
    let mut speech = SpeechState::<2>::new(OutputRate::Khz48, SilkRate::Khz8);
    speech.fec.enabled = true;
    let mut band = PacketBandwidth::<2>::default();
    let options = MonoOptions {
        complexity: 0,
        packet_loss_percent: 20,
        ..MonoOptions::default()
    };
    band.prepare_channels(
        (OutputRate::Khz48, 1),
        SilkRate::Khz8,
        PacketDuration::Ms20,
        40,
        options,
        true,
    )?;
    speech.dtx.begin(true);
    mono(
        &mut speech,
        &mut band,
        SilkRate::Khz8,
        &[0; 160],
        PacketControls {
            duration: PacketDuration::Ms20,
            packet_bytes: 40,
            options,
            digital_silence: true,
            include_fec: true,
            channels: 1,
            to_mono: false,
        },
    )?;
    // Original fixed-band policy selects FEC at15.6kbps mono/20% loss. Treating
    // state capacity as wire channels would instead test7.8kbps and disable it.
    assert!(speech.fec.selected());
    Ok(())
}
