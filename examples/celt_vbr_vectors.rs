//! Optional actual CELT VBR/CVBR state/loss corpus for unchanged libopus replay.
use opura::{
    celt::{
        FrameSize,
        encoder::{CeltEncoder, VbrOptions},
        input::InputRate,
    },
    decoder::OpusDecoder,
    silk::{decoder::PcmChannels, resampler::OutputRate},
};
use std::{
    env,
    fs::OpenOptions,
    io::{BufWriter, Write},
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = env::args_os().nth(1).ok_or("expected new output path")?;
    let mut out = BufWriter::new(OpenOptions::new().write(true).create_new(true).open(path)?);
    out.write_all(b"OPURACV1")?;
    let mut records = 0;
    for rate in [
        InputRate::Khz8,
        InputRate::Khz12,
        InputRate::Khz16,
        InputRate::Khz24,
        InputRate::Khz48,
    ] {
        for channels in 1..=2 {
            for mode in 0..4 {
                for loss in [false, true] {
                    let mut encoder = CeltEncoder::new(rate, channels)?;
                    encoder.set_transient_detection(true);
                    encoder.set_tf_analysis(true);
                    encoder.set_dynamic_allocation(true);
                    encoder.set_spreading_analysis(true);
                    encoder.set_allocation_analysis(true);
                    encoder.set_pitch_prefilter(true);
                    let mut decoder = OpusDecoder::new(
                        OutputRate::Khz48,
                        if channels == 1 {
                            PcmChannels::Mono
                        } else {
                            PcmChannels::Stereo
                        },
                    )?;
                    for frame in 0..128 {
                        let size = [
                            FrameSize::Ms2_5,
                            FrameSize::Ms5,
                            FrameSize::Ms10,
                            FrameSize::Ms20,
                        ][(frame / 16 + mode) % 4];
                        let n = size.samples() / rate.factor();
                        let pcm: Vec<i16> = (0..n * channels)
                            .map(|i| {
                                if (24..48).contains(&frame) || (80..88).contains(&frame) {
                                    0
                                } else if (48..64).contains(&frame) {
                                    if i % 3 == 0 { 1 } else { 0 }
                                } else {
                                    let phase =
                                        (frame * n + i / channels + 17 * (i % channels)) % 80;
                                    ((phase.min(80 - phase) as i32 - 20)
                                        * if frame % 17 == 0 { 1000 } else { 300 })
                                        as i16
                                }
                            })
                            .collect();
                        let packet = if (96..104).contains(&frame) {
                            encoder.encode(size, &pcm, 159)?
                        } else {
                            encoder.encode_vbr(
                                size,
                                &pcm,
                                VbrOptions {
                                    bitrate: if (112..120).contains(&frame) {
                                        1500000
                                    } else if mode < 2 {
                                        32001 * channels as u32
                                    } else {
                                        64001 * channels as u32
                                    },
                                    constrained: if mode % 2 == 1 {
                                        (frame / 40) % 2 == 0
                                    } else {
                                        false
                                    },
                                    payload_bytes: if (104..112).contains(&frame) {
                                        63 * channels
                                    } else {
                                        1275
                                    },
                                },
                            )?
                        };
                        let lost = loss && frame % 11 == 7;
                        let decoded = if lost {
                            decoder.conceal_samples(size.samples() as u16)?
                        } else {
                            decoder.decode(packet.as_bytes())?
                        };
                        let bytes = if lost { &[][..] } else { packet.as_bytes() };
                        let hash = decoded
                            .samples()
                            .iter()
                            .flat_map(|v| v.to_le_bytes())
                            .fold(14695981039346656037u64, |h, b| {
                                (h ^ u64::from(b)).wrapping_mul(1099511628211)
                            });
                        out.write_all(&[u8::from(frame == 0), channels as u8])?;
                        out.write_all(&(bytes.len() as u16).to_le_bytes())?;
                        out.write_all(&(decoded.samples().len() as u16).to_le_bytes())?;
                        out.write_all(&decoded.final_range().to_le_bytes())?;
                        out.write_all(&hash.to_le_bytes())?;
                        out.write_all(bytes)?;
                        records += 1;
                    }
                }
            }
        }
    }
    out.flush()?;
    eprintln!("Produced {records} CELT VBR/CVBR records");
    Ok(())
}
