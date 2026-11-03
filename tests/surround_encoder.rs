//! Whole-owner surround packets, including actual LFE and later-stream rollback.
use opura::{
    celt::input::InputRate,
    encoder::{
        PacketSize,
        control::{Application, Controls},
    },
    multistream::{
        MultistreamDecoder, MultistreamEncodeOptions, MultistreamEncoder, MultistreamPacket,
    },
    silk::resampler::OutputRate,
};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn pcm(channels: usize, samples: usize) -> Vec<i16> {
    (0..samples)
        .flat_map(|i| (0..channels).map(move |c| (((i * (73 + 19 * c)) % 8000) as i16 - 4000)))
        .collect()
}
#[test]
fn surround_audio_packets_include_real_lfe_and_preserve_state_on_late_failure() -> Result<()> {
    let mut encoder =
        MultistreamEncoder::with_surround_application(InputRate::Khz48, 6, Application::Audio)?;
    let mut decoder =
        MultistreamDecoder::new(OutputRate::Khz48, encoder.channel_mapping().clone())?;
    for variable in [false, true] {
        let input = pcm(6, 960);
        let options = MultistreamEncodeOptions {
            bitrate: 192000,
            controls: Controls {
                variable,
                complexity: 5,
                ..Controls::default()
            },
            constrained: variable,
        };
        let packet = encoder.encode_packet(PacketSize::Ms20, &input, 8192, options)?;
        assert_eq!(
            MultistreamPacket::parse(packet.as_bytes(), 4)?.duration_samples(),
            960
        );
        let decoded = decoder.decode(packet.as_bytes())?;
        assert_eq!(decoded.samples().len(), 960 * 6);
        assert_eq!(decoded.final_range(), packet.final_range());
        let before = encoder.clone();
        let failed_input: Vec<i16> = input.iter().map(|value| value.wrapping_neg()).collect();
        assert!(
            encoder
                .encode_packet(
                    PacketSize::Ms20,
                    &failed_input,
                    8192,
                    MultistreamEncodeOptions {
                        controls: Controls {
                            channels: Some(2),
                            ..options.controls
                        },
                        ..options
                    }
                )
                .is_err()
        );
        assert_eq!(encoder, before);
        let mut reference = before;
        assert_eq!(
            encoder.encode_packet(PacketSize::Ms20, &input, 8192, options)?,
            reference.encode_packet(PacketSize::Ms20, &input, 8192, options)?
        );
    }
    encoder.reset()?;
    assert_eq!(
        encoder,
        MultistreamEncoder::with_surround_application(InputRate::Khz48, 6, Application::Audio)?
    );
    Ok(())
}

#[test]
fn original_native_coupled_and_lfe_wire_policies_match_at_all_rates_and_durations() -> Result<()> {
    let mut owner = None;
    let mut application = Application::Audio;
    let mut rows = 0;
    for line in include_str!("surround-policy-fixtures.txt")
        .lines()
        .filter(|l| !l.starts_with('#'))
    {
        let fields: Vec<_> = line.split_whitespace().collect();
        match fields[0] {
            "case" => {
                let rate = match fields[1] {
                    "8000" => InputRate::Khz8,
                    "12000" => InputRate::Khz12,
                    "16000" => InputRate::Khz16,
                    "24000" => InputRate::Khz24,
                    "48000" => InputRate::Khz48,
                    _ => return Err("invalid input rate".into()),
                };
                application = match fields[3] {
                    "2049" => Application::Audio,
                    "2048" => Application::Voip,
                    "2051" => Application::RestrictedLowDelay,
                    "2052" => Application::RestrictedSilk,
                    "2053" => Application::RestrictedCelt,
                    _ => return Err("invalid application".into()),
                };
                let encoder = MultistreamEncoder::with_surround_application(
                    rate,
                    fields[2].parse()?,
                    application,
                )?;
                assert_eq!(encoder.channel_mapping().streams(), fields[4].parse()?);
                assert_eq!(
                    encoder.channel_mapping().coupled_streams(),
                    fields[5].parse()?
                );
                owner = Some(encoder);
            }
            "frame" => {
                let owner = owner.as_mut().ok_or("missing owner")?;
                let size = PacketSize::try_from(fields[1].parse::<usize>()?)?;
                let channels = owner.channel_mapping().channels();
                let coupled = owner.channel_mapping().coupled_streams();
                let streams = owner.channel_mapping().streams();
                let input = pcm(channels, size.samples() / owner.input_rate().factor());
                let packet = owner.encode_packet(
                    size,
                    &input,
                    8192,
                    MultistreamEncodeOptions {
                        bitrate: 32000 * channels as u32,
                        controls: Controls {
                            application,
                            channels: Some(1),
                            complexity: 0,
                            variable: false,
                            ..Controls::default()
                        },
                        constrained: false,
                    },
                )?;
                let parsed = MultistreamPacket::parse(packet.as_bytes(), streams)?;
                assert_eq!(fields.len(), 2 + streams);
                for (index, stream) in parsed.streams().iter().enumerate() {
                    if index < coupled
                        || (channels >= 6 && index + 1 == streams)
                        || application == Application::RestrictedSilk
                    {
                        let toc =
                            stream.configuration() * 8 + if stream.channels() == 2 { 4 } else { 0 };
                        assert_eq!(
                            toc,
                            fields[2 + index].parse::<u8>()?,
                            "native policy row{rows} stream{index}"
                        );
                    }
                }
                rows += 1;
            }
            _ => return Err("invalid native policy record".into()),
        }
    }
    assert_eq!(rows, 1290);
    for channels in [0, 9, usize::MAX] {
        assert!(
            MultistreamEncoder::with_surround_application(
                InputRate::Khz48,
                channels,
                Application::Audio
            )
            .is_err()
        );
    }
    Ok(())
}
