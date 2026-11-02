//! Optional connected Hybrid packet/PCM/loss corpus for original libopus checks.
use opura::{
    Bandwidth,
    celt::input::InputRate,
    decoder::OpusDecoder,
    hybrid::encoder::{HybridEncoder, Options},
    silk::{decoder::PcmChannels, gains::FrameDuration, resampler::OutputRate},
};
use std::{
    env,
    fs::OpenOptions,
    io::{BufWriter, Write},
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = env::args_os().nth(1).ok_or("expected new output path")?;
    let mut out = BufWriter::new(OpenOptions::new().write(true).create_new(true).open(path)?);
    out.write_all(b"OPURAH01")?;
    let mut random = 0x48594252u32;
    for rate in [InputRate::Khz24, InputRate::Khz48] {
        for bandwidth in [Bandwidth::SuperWideband, Bandwidth::Fullband] {
            if rate == InputRate::Khz24 && bandwidth == Bandwidth::Fullband {
                continue;
            }
            for channels in 1..=2 {
                for complexity in [0, 1, 2, 6, 10] {
                    for budget in [40, 80, 160, 320] {
                        for loss in [false, true] {
                            let mut encoder = HybridEncoder::new(rate, channels, bandwidth)?;
                            let mut decoder = OpusDecoder::new(
                                OutputRate::Khz48,
                                if channels == 1 {
                                    PcmChannels::Mono
                                } else {
                                    PcmChannels::Stereo
                                },
                            )?;
                            for frame in 0..24 {
                                let short = frame % 3 != 2;
                                let duration = if short {
                                    FrameDuration::Ms10
                                } else {
                                    FrameDuration::Ms20
                                };
                                let n = rate.hz() as usize / if short { 100 } else { 50 };
                                let input: Vec<i16> = (0..n * channels)
                                    .map(|i| {
                                        random =
                                            random.wrapping_mul(1664525).wrapping_add(1013904223);
                                        let phase =
                                            (frame * n + i / channels + 17 * (i % channels)) % 160;
                                        match frame {
                                            0..=2 => 0,
                                            3..=10 | 16..=20 => {
                                                ((phase.min(160 - phase) as i32 - 40) * 240
                                                    + (random >> 24) as i32
                                                    - 128)
                                                    as i16
                                            }
                                            11..=15 => ((random >> 16) as i32 - 32768) as i16,
                                            _ if i % channels == 0 => {
                                                ((random >> 24) as i32 - 128) as i16
                                            }
                                            _ => 0,
                                        }
                                    })
                                    .collect();
                                let packet = encoder.encode(
                                    duration,
                                    &input,
                                    if short { budget } else { budget * 2 },
                                    Options {
                                        complexity,
                                        packet_loss_percent: if frame < 12 { 0 } else { 20 },
                                    },
                                )?;
                                let lost = loss && matches!(frame, 5 | 12 | 13 | 23);
                                let bytes = if lost { &[][..] } else { packet.as_bytes() };
                                let decoded = if lost {
                                    decoder.conceal_samples(if short { 480 } else { 960 })?
                                } else {
                                    decoder.decode(bytes)?
                                };
                                if !lost && decoded.final_range() != packet.final_range() {
                                    return Err("self range mismatch".into());
                                }
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
                            }
                        }
                    }
                }
            }
        }
    }
    out.flush()?;
    Ok(())
}
