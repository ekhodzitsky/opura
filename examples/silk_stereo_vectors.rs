//! Optional native cross-decoder corpus; ordinary builds/tests need no native codec.
use opura::{
    decoder::OpusDecoder,
    silk::{
        decoder::PcmChannels,
        encoder::{
            mono::PacketDuration,
            stereo::{StereoEncoder, StereoOptions},
        },
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
    out.write_all(b"OPURAS01")?;
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
                    let mut encoder = StereoEncoder::new(rate, bandwidth);
                    let mut decoder = OpusDecoder::new(OutputRate::Khz48, PcmChannels::Stereo)?;
                    let n = rate.hz() as usize * duration.milliseconds() as usize / 1000;
                    for frame in 0..24 {
                        let mut pcm = vec![0i16; 2 * n];
                        for (i, pair) in pcm.chunks_exact_mut(2).enumerate() {
                            random = random.wrapping_mul(1664525).wrapping_add(1013904223);
                            let value = (((frame * n + i) % 80) as i32 - 40) * 300;
                            pair[0] = if frame < 2 {
                                0
                            } else if frame < 14 {
                                value as i16
                            } else {
                                ((random >> 16) as i32 - 32768) as i16
                            };
                            random = random.wrapping_mul(1664525).wrapping_add(1013904223);
                            pair[1] = if frame < 14 {
                                pair[0]
                            } else {
                                ((random >> 16) as i32 - 32768) as i16
                            };
                        }
                        let packet = encoder.encode(
                            duration,
                            &pcm,
                            duration.milliseconds() as usize * 8,
                            StereoOptions {
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
