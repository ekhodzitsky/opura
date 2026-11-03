//! Mapped input selection, complete packet budgets and encoder transactions.
use opura::{
    celt::input::InputRate,
    encoder::{
        PacketSize,
        control::{Application, Controls},
    },
    multistream::{
        ChannelMapping, MultistreamDecoder, MultistreamEncodeOptions, MultistreamEncoder,
        MultistreamError, MultistreamPacket,
    },
    silk::resampler::OutputRate,
};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
fn output_rate(rate: InputRate) -> OutputRate {
    match rate {
        InputRate::Khz8 => OutputRate::Khz8,
        InputRate::Khz12 => OutputRate::Khz12,
        InputRate::Khz16 => OutputRate::Khz16,
        InputRate::Khz24 => OutputRate::Khz24,
        InputRate::Khz48 => OutputRate::Khz48,
    }
}
fn inputs(samples: usize, step: usize) -> (Vec<i16>, Vec<i16>) {
    let mut mapped = Vec::with_capacity(samples * 5);
    let mut compact = Vec::with_capacity(samples * 3);
    for i in 0..samples {
        let m = ((i * 119 + step * 887) as i16).wrapping_mul(3);
        let l = ((i * 71 + step * 333) as i16).wrapping_mul(7);
        let r = ((i * 191 + step * 11) as i16).wrapping_mul(5);
        mapped.extend([m, l, r, 30000, l.wrapping_neg() / 2]);
        compact.extend([m, l, r]);
    }
    (mapped, compact)
}
#[test]
fn mapped_ignored_inputs_and_all_applications_match_selected_pcm() -> Result<()> {
    for rate in [
        InputRate::Khz8,
        InputRate::Khz12,
        InputRate::Khz16,
        InputRate::Khz24,
        InputRate::Khz48,
    ] {
        for application in [
            Application::Audio,
            Application::Voip,
            Application::RestrictedLowDelay,
            Application::RestrictedSilk,
            Application::RestrictedCelt,
        ] {
            let layout = ChannelMapping::new(2, 1, &[2, 0, 1, 255, 0])?;
            let mut mapped =
                MultistreamEncoder::with_application(rate, layout.clone(), application)?;
            let mut compact = MultistreamEncoder::with_application(
                rate,
                ChannelMapping::new(2, 1, &[2, 0, 1])?,
                application,
            )?;
            let mut decoder = MultistreamDecoder::new(output_rate(rate), layout)?;
            assert_eq!(mapped.application(), application);
            assert_eq!(mapped.input_rate(), rate);
            assert_eq!(
                mapped.lookahead_48k(),
                if matches!(
                    application,
                    Application::RestrictedCelt | Application::RestrictedLowDelay
                ) {
                    120
                } else {
                    312
                }
            );
            for step in 0..6 {
                let mode = step % 3;
                let options = MultistreamEncodeOptions {
                    bitrate: 96000,
                    controls: Controls {
                        application,
                        complexity: 1,
                        variable: mode != 0,
                        ..Controls::default()
                    },
                    constrained: mode == 2,
                };
                let (a, b) = inputs(960 / rate.factor(), step);
                let actual = mapped.encode_packet(PacketSize::Ms20, &a, 800, options)?;
                let expected = compact.encode_packet(PacketSize::Ms20, &b, 800, options)?;
                assert_eq!(
                    actual, expected,
                    "ignored/repeated input slots changed encoding"
                );
                assert!(actual.as_bytes().len() <= 800);
                if mode == 0 {
                    assert_eq!(actual.as_bytes().len(), 240);
                }
                if mode != 0 {
                    assert_eq!(actual.stream_bitrates(), &[59000, 37000]);
                }
                let pcm = decoder.decode(actual.as_bytes())?;
                assert_eq!(pcm.final_range(), actual.final_range());
                assert_eq!(pcm.samples_per_channel(), 960 / rate.factor());
                for frame in pcm.samples().chunks_exact(5) {
                    assert_eq!(frame[3], 0);
                    assert_eq!(frame[1], frame[4]);
                }
            }
            mapped.reset()?;
            assert_eq!(
                mapped,
                MultistreamEncoder::with_application(
                    rate,
                    mapped.channel_mapping().clone(),
                    application
                )?
            );
        }
    }
    Ok(())
}
#[test]
fn all_packet_durations_minimum_framing_and_large_cbr_padding() -> Result<()> {
    let layout = ChannelMapping::new(2, 1, &[0, 1, 2])?;
    let mut encoder = MultistreamEncoder::with_application(
        InputRate::Khz48,
        layout.clone(),
        Application::RestrictedCelt,
    )?;
    let mut decoder = MultistreamDecoder::new(OutputRate::Khz48, layout)?;
    for size in [
        PacketSize::Ms2_5,
        PacketSize::Ms5,
        PacketSize::Ms10,
        PacketSize::Ms20,
        PacketSize::Ms40,
        PacketSize::Ms60,
        PacketSize::Ms80,
        PacketSize::Ms100,
        PacketSize::Ms120,
    ] {
        let pcm: Vec<i16> = (0..size.samples() * 3)
            .map(|i| (i as i16).wrapping_mul(31))
            .collect();
        let minimum = if size == PacketSize::Ms100 { 5 } else { 3 };
        let options = MultistreamEncodeOptions {
            bitrate: 191250000,
            controls: Controls {
                application: Application::RestrictedCelt,
                complexity: 1,
                variable: false,
                ..Controls::default()
            },
            constrained: false,
        };
        for cap in [minimum, 20000] {
            let packet = encoder.encode_packet(size, &pcm, cap, options)?;
            let expected = cap
                .min(((2250000u64 * size.samples() as u64 / 48000 + 4) / 8) as usize)
                .max(minimum);
            assert_eq!(packet.as_bytes().len(), expected);
            assert_eq!(
                usize::from(MultistreamPacket::parse(packet.as_bytes(), 2)?.duration_samples()),
                size.samples()
            );
            let decoded = decoder.decode(packet.as_bytes())?;
            assert_eq!(decoded.final_range(), packet.final_range());
            assert_eq!(decoded.samples_per_channel(), size.samples());
        }
    }
    Ok(())
}
#[test]
fn late_stream_control_failure_and_resume_preserve_whole_owner() -> Result<()> {
    let layout = ChannelMapping::new(2, 1, &[0, 1, 2])?;
    let mut encoder = MultistreamEncoder::new(InputRate::Khz48, layout)?;
    let input: Vec<i16> = (0..2880).map(|i| (i as i16).wrapping_mul(179)).collect();
    let options = MultistreamEncodeOptions {
        bitrate: 96000,
        controls: Controls {
            complexity: 1,
            ..Controls::default()
        },
        constrained: false,
    };
    encoder.encode_packet(PacketSize::Ms20, &input, 800, options)?;
    let before = encoder.clone();
    let bad = MultistreamEncodeOptions {
        controls: Controls {
            channels: Some(2),
            ..options.controls
        },
        ..options
    };
    // The first coupled stream accepts stereo; the later mono stream rejects
    // it after the first stream has encoded. Whole-owner rollback is observable.
    assert!(matches!(
        encoder.encode_packet(PacketSize::Ms20, &input, 800, bad),
        Err(MultistreamError::Encode { stream: 1, .. })
    ));
    assert_eq!(encoder, before);
    for cap in [0, 1, 2, 16 * 1024 * 1024 + 1] {
        assert_eq!(
            encoder.encode_packet(PacketSize::Ms20, &input, cap, options),
            Err(MultistreamError::InvalidBudget)
        );
        assert_eq!(encoder, before);
    }
    assert_eq!(
        encoder.encode_packet(PacketSize::Ms20, &input[..1], 800, options),
        Err(MultistreamError::InvalidInput)
    );
    assert_eq!(encoder, before);
    let mut pristine = before;
    assert_eq!(
        encoder.encode_packet(PacketSize::Ms20, &input, 800, options)?,
        pristine.encode_packet(PacketSize::Ms20, &input, 800, options)?
    );
    assert_eq!(encoder, pristine);
    assert!(
        MultistreamEncoder::new(InputRate::Khz48, ChannelMapping::new(2, 1, &[0, 1])?).is_err()
    );
    Ok(())
}
