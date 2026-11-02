//! Optional native cross-decoder corpus; ordinary builds/tests need no native codec.
use opura::{
    decoder::OpusDecoder,
    silk::{
        decoder::PcmChannels,
        encoder::mono::{MonoEncoder, MonoOptions, PacketDuration},
        pitch::SilkRate,
        resampler::OutputRate,
    },
};
use std::{
    env,
    fs::OpenOptions,
    io::{BufWriter, Write},
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = env::args_os().nth(1).ok_or("expected new output path")?;
    let mut out = BufWriter::new(OpenOptions::new().write(true).create_new(true).open(path)?);
    out.write_all(b"OPURAM01")?;
    let mut random = 0x4d4f4e4fu32;
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
                    let mut encoder = MonoEncoder::new(rate, bandwidth);
                    let mut decoder = OpusDecoder::new(OutputRate::Khz48, PcmChannels::Mono)?;
                    let n = rate.hz() as usize * duration.milliseconds() as usize / 1000;
                    for frame in 0..12 {
                        let pcm: Vec<i16> = (0..n)
                            .map(|i| {
                                random = random.wrapping_mul(1664525).wrapping_add(1013904223);
                                match frame {
                                    0 | 1 => 0,
                                    2..=7 => {
                                        let phase = (frame * n + i) % 80;
                                        ((phase.min(80 - phase) as i32 - 20) * 300
                                            + (random >> 24) as i32
                                            - 128) as i16
                                    }
                                    _ => ((random >> 16) as i32 - 32768) as i16,
                                }
                            })
                            .collect();
                        let packet = encoder.encode(
                            duration,
                            &pcm,
                            duration.milliseconds() as usize * 4,
                            MonoOptions {
                                complexity: match bandwidth {
                                    SilkRate::Khz8 => 0,
                                    SilkRate::Khz12 => 6,
                                    SilkRate::Khz16 => 10,
                                },
                                variable_bitrate: variable,
                                packet_loss_percent: if frame < 6 { 0 } else { 20 },
                            },
                        )?;
                        let decoded = decoder.decode(packet.as_bytes())?;
                        let hash = decoded
                            .samples()
                            .iter()
                            .flat_map(|v| v.to_le_bytes())
                            .fold(14695981039346656037u64, |h, b| {
                                (h ^ u64::from(b)).wrapping_mul(1099511628211)
                            });
                        out.write_all(&[u8::from(frame == 0)])?;
                        out.write_all(&(packet.as_bytes().len() as u16).to_le_bytes())?;
                        out.write_all(&(decoded.samples().len() as u16).to_le_bytes())?;
                        out.write_all(&packet.final_range().to_le_bytes())?;
                        out.write_all(&hash.to_le_bytes())?;
                        out.write_all(packet.as_bytes())?;
                    }
                }
            }
        }
    }
    out.flush()?;
    Ok(())
}
