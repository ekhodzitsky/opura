//! Optional actual unified-encoder mode/channel/band/DTX/FEC/loss corpus.
use opura::{
    Mode,
    celt::{FrameSize, input::InputRate},
    decoder::{DecodedPcm, OpusDecoder},
    encoder::{
        EncodeOptions, OpusEncoder,
        control::{Application, Controls, Fec, Signal},
    },
    silk::{decoder::PcmChannels, resampler::OutputRate},
};
use std::{
    env,
    fs::OpenOptions,
    io::{BufWriter, Write},
};

fn record(
    out: &mut impl Write,
    bytes: &[u8],
    pcm: &DecodedPcm,
    flags: u8,
) -> Result<(), Box<dyn std::error::Error>> {
    let hash = pcm
        .samples()
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .fold(14695981039346656037u64, |h, b| {
            (h ^ u64::from(b)).wrapping_mul(1099511628211)
        });
    out.write_all(&[flags])?;
    out.write_all(&(bytes.len() as u16).to_le_bytes())?;
    out.write_all(&(pcm.samples().len() as u16).to_le_bytes())?;
    out.write_all(&pcm.final_range().to_le_bytes())?;
    out.write_all(&hash.to_le_bytes())?;
    out.write_all(bytes)?;
    Ok(())
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = env::args_os().nth(1).ok_or("expected new output path")?;
    let mut out = BufWriter::new(OpenOptions::new().write(true).create_new(true).open(path)?);
    out.write_all(b"OPURAP02")?;
    let mut counts = [0usize; 8]; // calls, SILK, Hybrid, CELT, wire changes, DTX, PLC, FEC
    for rate in [
        InputRate::Khz8,
        InputRate::Khz12,
        InputRate::Khz16,
        InputRate::Khz24,
        InputRate::Khz48,
    ] {
        for channels in 1..=2 {
            for application in [
                Application::Audio,
                Application::Voip,
                Application::RestrictedLowDelay,
                Application::RestrictedSilk,
                Application::RestrictedCelt,
            ] {
                for variable in [false, true] {
                    let mut encoder = OpusEncoder::with_application(rate, channels, application)?;
                    let mut decoder = OpusDecoder::new(OutputRate::Khz48, PcmChannels::Stereo)?;
                    let mut previous_size = FrameSize::Ms20;
                    let mut previous_wire = channels;
                    for frame in 0..96 {
                        let size = if application != Application::RestrictedSilk && frame % 8 < 2 {
                            [FrameSize::Ms2_5, FrameSize::Ms5][frame % 8]
                        } else if frame % 5 == 0 {
                            FrameSize::Ms10
                        } else {
                            FrameSize::Ms20
                        };
                        let n = size.samples() / rate.factor();
                        let input: Vec<i16> = (0..n * channels)
                            .map(|i| {
                                if (48..84).contains(&frame) {
                                    return if frame < 48 { 0 } else { (i % 2) as i16 };
                                }
                                let p = (frame * n + i / channels + i % channels * 17) % 80;
                                ((p.min(80 - p) as i32 - 20) * 500) as i16
                            })
                            .collect();
                        let mode = if frame % 32 < 16 {
                            Some([Mode::Celt, Mode::Silk, Mode::Hybrid, Mode::Celt][frame / 4 % 4])
                        } else {
                            None
                        };
                        let options = EncodeOptions {
                            bitrate: [24000, 48000, 96000][frame / 16 % 3] * channels as u32,
                            controls: Controls {
                                mode,
                                application,
                                signal: if frame >= 48 {
                                    Signal::Voice
                                } else {
                                    Signal::Auto
                                },
                                variable,
                                channels: if frame % 16 < 8 {
                                    None
                                } else {
                                    Some(1 + (channels - 1) * (frame / 7 % 2))
                                },
                                bandwidth: None,
                                complexity: [0, 6, 10][frame / 32 % 3],
                                fec: if frame >= 32 {
                                    Fec::Enabled
                                } else {
                                    Fec::Disabled
                                },
                                loss_percent: if frame >= 32 { 20 } else { 0 },
                                dtx: true,
                                ..Controls::default()
                            },
                            constrained: application == Application::RestrictedLowDelay,
                        };
                        let packet = encoder.encode(size, &input, 640, options)
                            .map_err(|e| format!("{rate:?}/{channels}/{application:?}/{variable}/frame{frame}/{size:?}: {e:?}"))?;
                        counts[match packet.decision().mode {
                            Mode::Silk => 1,
                            Mode::Hybrid => 2,
                            Mode::Celt => 3,
                        }] += 1;
                        counts[4] += usize::from(previous_wire != packet.decision().channels);
                        previous_wire = packet.decision().channels;
                        counts[5] += usize::from(packet.as_bytes().len() == 1);
                        if frame % 12 == 5 {
                            let pcm = decoder.conceal_samples(size.samples() as u16)?;
                            record(&mut out, &[], &pcm, 0)?;
                            counts[0] += 1;
                            counts[6] += 1;
                        } else if frame % 12 != 9 {
                            if frame % 12 == 10 {
                                let pcm = decoder.decode_fec(
                                    packet.as_bytes(),
                                    previous_size.samples() as u16,
                                )?;
                                record(&mut out, packet.as_bytes(), &pcm, 2)?;
                                counts[0] += 1;
                                counts[7] += 1;
                            }
                            let pcm = decoder.decode(packet.as_bytes())?;
                            if pcm.final_range() != packet.final_range() {
                                return Err(format!("final range mismatch at {rate:?}/{channels}/{application:?}/{variable}/frame{frame}").into());
                            }
                            record(&mut out, packet.as_bytes(), &pcm, u8::from(frame == 0))?;
                            counts[0] += 1;
                        }
                        previous_size = size;
                    }
                }
            }
        }
    }
    if counts[0] != 9600 || counts[1..6].contains(&0) || counts[6] != 800 || counts[7] != 800 {
        return Err(format!("insufficient coverage: {counts:?}").into());
    }
    out.flush()?;
    println!("PASS:100 streams; calls/SILK/Hybrid/CELT/wire changes/DTX/PLC/FEC={counts:?}");
    Ok(())
}
