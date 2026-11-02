use super::*;
use crate::{
    decoder::OpusDecoder,
    silk::{decoder::PcmChannels, resampler::OutputRate},
};

fn native_analysis(encoder: &mut CeltEncoder) {
    encoder.set_transient_detection(true);
    encoder.set_tf_analysis(true);
    encoder.set_dynamic_allocation(true);
    encoder.set_spreading_analysis(true);
    encoder.set_allocation_analysis(true);
    encoder.set_pitch_prefilter(true);
}

#[test]
fn silence_keeps_the_previous_raw_overlap_before_suppression()
-> Result<(), Box<dyn std::error::Error>> {
    for rate in [InputRate::Khz8, InputRate::Khz48] {
        let mut encoder = CeltEncoder::new(rate, 1)?;
        let n = FrameSize::Ms20.samples() / rate.factor();
        let mut pcm = vec![0; n];
        pcm[n - 1] = 1;
        encoder.encode(FrameSize::Ms20, &pcm, 159)?;
        pcm.fill(0);
        let options = VbrOptions {
            bitrate: 32000,
            constrained: true,
            payload_bytes: 319,
        };
        let first = encoder.encode_vbr(FrameSize::Ms20, &pcm, options)?;
        assert_eq!(first.as_bytes()[1] & 128, 0);
        let silent = encoder.encode_vbr(FrameSize::Ms20, &pcm, options)?;
        assert_eq!(silent.as_bytes().len(), 3);
        assert_ne!(silent.as_bytes()[1] & 128, 0);
    }
    Ok(())
}

#[test]
fn vbr_and_cvbr_geometry_silence_and_toggles_decode() -> Result<(), Box<dyn std::error::Error>> {
    for rate in [
        InputRate::Khz8,
        InputRate::Khz12,
        InputRate::Khz16,
        InputRate::Khz24,
        InputRate::Khz48,
    ] {
        for channels in 1..=2 {
            for constrained in [false, true] {
                let mut encoder = CeltEncoder::new(rate, channels)?;
                native_analysis(&mut encoder);
                let mut decoder = OpusDecoder::new(
                    OutputRate::Khz48,
                    if channels == 1 {
                        PcmChannels::Mono
                    } else {
                        PcmChannels::Stereo
                    },
                )?;
                let mut lengths = std::collections::BTreeSet::new();
                let mut silent = 0;
                for frame in 0..48 {
                    let size = [
                        FrameSize::Ms2_5,
                        FrameSize::Ms5,
                        FrameSize::Ms10,
                        FrameSize::Ms20,
                    ][frame / 12];
                    let n = size.samples() / rate.factor();
                    let pcm: Vec<i16> = (0..n * channels)
                        .map(|i| {
                            if frame % 12 < 4 {
                                0
                            } else {
                                ((((frame * n + i / channels + i % channels * 17) % 80).min(40)
                                    as i32
                                    - 20)
                                    * 400) as i16
                            }
                        })
                        .collect();
                    let options = VbrOptions {
                        bitrate: 64001 * channels as u32,
                        constrained,
                        payload_bytes: 319,
                    };
                    let packet = if frame % 12 == 11 {
                        encoder.encode(size, &pcm, 159)?
                    } else {
                        encoder.encode_vbr(size, &pcm, options)?
                    };
                    assert!((3..=320).contains(&packet.as_bytes().len()));
                    if frame % 12 != 11 {
                        assert!(packet.as_bytes().len() <= 1 + (1275 >> (3 - size.lm())));
                    }
                    lengths.insert(packet.as_bytes().len());
                    let decoded = decoder.decode(packet.as_bytes())?;
                    assert_eq!(decoded.samples().len(), size.samples() * channels);
                    assert_eq!(decoded.final_range(), packet.final_range());
                    silent += usize::from(packet.as_bytes().len() == 3);
                }
                assert!(lengths.len() > 4);
                assert!(silent >= 4);
                encoder.reset();
                let mut fresh = CeltEncoder::new(rate, channels)?;
                native_analysis(&mut fresh);
                assert_eq!(encoder, fresh);
            }
        }
    }
    Ok(())
}

#[test]
fn vbr_failures_after_resize_roll_back_both_rate_and_audio_histories()
-> Result<(), Box<dyn std::error::Error>> {
    let mut encoder = CeltEncoder::new(InputRate::Khz48, 2)?;
    native_analysis(&mut encoder);
    let pcm: Vec<i16> = (0..1920)
        .map(|i| ((i * 31) % 16000 - 8000) as i16)
        .collect();
    let options = VbrOptions {
        bitrate: 96000,
        constrained: true,
        payload_bytes: 319,
    };
    for _ in 0..3 {
        encoder.encode_vbr(FrameSize::Ms20, &pcm, options)?;
    }
    let before = encoder.clone();
    let mut calls = 0;
    let result = encoder.encode_with_rate(
        FrameSize::Ms20,
        &pcm,
        options.payload_bytes,
        Some(options),
        |writer| {
            calls += 1;
            assert!(writer.capacity() < options.payload_bytes);
            writer.finish()?;
            Err(EntropyError::CapacityExhausted)
        },
    );
    assert_eq!(calls, 1);
    assert_eq!(
        result,
        Err(EncoderError::Entropy(EntropyError::CapacityExhausted))
    );
    assert_eq!(encoder, before);
    for bitrate in [0, 1, 1199, 1500001, u32::MAX] {
        assert_eq!(
            encoder.encode_vbr(FrameSize::Ms20, &pcm, VbrOptions { bitrate, ..options }),
            Err(EncoderError::InvalidRate)
        );
        assert_eq!(encoder, before);
    }
    for capacity in [2, 3, 8, 16, 32, 1275] {
        let mut trial = before.clone();
        match trial.encode_vbr(
            FrameSize::Ms20,
            &pcm,
            VbrOptions {
                payload_bytes: capacity,
                ..options
            },
        ) {
            Ok(packet) => assert!(packet.as_bytes().len() <= capacity + 1),
            Err(_) => assert_eq!(trial, before),
        }
    }
    Ok(())
}
