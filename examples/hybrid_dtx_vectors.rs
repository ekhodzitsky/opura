//! Optional Hybrid DTX normal/loss/resume corpus; no native ordinary dependency.
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
    for loss in [false, true] {
        let mut encoder = HybridEncoder::new(InputRate::Khz48, 1, Bandwidth::Fullband)?;
        let mut decoder = OpusDecoder::new(OutputRate::Khz48, PcmChannels::Mono)?;
        for line in include_str!("../tests/hybrid-encoder-dtx-fixtures.txt")
            .lines()
            .filter(|l| !l.starts_with('#'))
        {
            let v = line
                .split_whitespace()
                .map(str::parse)
                .collect::<Result<Vec<usize>, _>>()?;
            let [
                rate,
                band,
                channels,
                ms,
                complexity,
                _,
                frame,
                enabled,
                suppressed,
                _,
                _,
            ]: [usize; 11] = v.try_into().map_err(|_| "bad row")?;
            if frame == 0 {
                encoder = HybridEncoder::new(
                    if rate == 24000 {
                        InputRate::Khz24
                    } else {
                        InputRate::Khz48
                    },
                    channels,
                    if band == 1104 {
                        Bandwidth::SuperWideband
                    } else {
                        Bandwidth::Fullband
                    },
                )?;
                decoder = OpusDecoder::new(
                    OutputRate::Khz48,
                    if channels == 1 {
                        PcmChannels::Mono
                    } else {
                        PcmChannels::Stereo
                    },
                )?;
            }
            encoder.set_dtx(enabled != 0);
            let n = rate * ms / 1000;
            let input: Vec<i16> = (0..n * channels)
                .map(|i| {
                    let phase = (frame * n + i / channels) % 240;
                    if !(5..110).contains(&frame) {
                        ((phase.min(240 - phase) as i32 - 60) * 200) as i16
                    } else if frame < 55 {
                        0
                    } else {
                        ((i / channels + i % channels) % 2) as i16
                    }
                })
                .collect();
            let packet = encoder.encode(
                if ms == 10 {
                    FrameDuration::Ms10
                } else {
                    FrameDuration::Ms20
                },
                &input,
                ms * 4 * channels,
                Options {
                    complexity: complexity as u8,
                    packet_loss_percent: 0,
                },
            )?;
            if (packet.as_bytes().len() == 1) != (suppressed != 0) {
                return Err("native DTX mismatch".into());
            }
            let lost = loss && matches!(frame, 5 | 12 | 13 | 149);
            let bytes = if lost { &[][..] } else { packet.as_bytes() };
            let decoded = if lost {
                decoder.conceal_samples((ms * 48) as u16)?
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
    out.flush()?;
    Ok(())
}
