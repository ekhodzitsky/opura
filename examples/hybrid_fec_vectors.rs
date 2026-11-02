//! Optional shared Hybrid LBRR/DTX/normal/recovery corpus for original libopus.
use opura::{
    Bandwidth,
    celt::input::InputRate,
    decoder::{DecodedPcm, OpusDecoder},
    hybrid::encoder::{HybridEncoder, Options},
    silk::{decoder::PcmChannels, gains::FrameDuration, resampler::OutputRate},
};
use std::{
    env,
    fs::OpenOptions,
    io::{BufWriter, Write},
};
fn record(
    out: &mut impl Write,
    bytes: &[u8],
    decoded: &DecodedPcm,
    flags: u8,
    channels: usize,
) -> std::io::Result<()> {
    let hash = decoded
        .samples()
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .fold(14695981039346656037u64, |h, b| {
            (h ^ u64::from(b)).wrapping_mul(1099511628211)
        });
    out.write_all(&[flags, channels as u8])?;
    out.write_all(&(bytes.len() as u16).to_le_bytes())?;
    out.write_all(&(decoded.samples().len() as u16).to_le_bytes())?;
    out.write_all(&decoded.final_range().to_le_bytes())?;
    out.write_all(&hash.to_le_bytes())?;
    out.write_all(bytes)
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = env::args_os().nth(1).ok_or("expected new output path")?;
    let mut out = BufWriter::new(OpenOptions::new().write(true).create_new(true).open(path)?);
    out.write_all(b"OPURAH02")?;
    let mut protected = 0;
    let mut suppressed = 0;
    let mut recoveries = 0;
    for rate in [InputRate::Khz24, InputRate::Khz48] {
        for bandwidth in [Bandwidth::SuperWideband, Bandwidth::Fullband] {
            if rate == InputRate::Khz24 && bandwidth == Bandwidth::Fullband {
                continue;
            }
            for channels in 1..=2 {
                for default_ms in [10, 20] {
                    for complexity in [0, 6, 10] {
                        for rate_kbps in [32, 48] {
                            for loss_percent in [0, 5, 20, 100] {
                                for dtx in [false, true] {
                                    for loss in [false, true] {
                                        let mut encoder =
                                            HybridEncoder::new(rate, channels, bandwidth)?;
                                        encoder.set_dtx(dtx);
                                        let mut decoder = OpusDecoder::new(
                                            OutputRate::Khz48,
                                            if channels == 1 {
                                                PcmChannels::Mono
                                            } else {
                                                PcmChannels::Stereo
                                            },
                                        )?;
                                        let mut previous_samples = default_ms * 48;
                                        for frame in 0..160 {
                                            let ms = if (100..110).contains(&frame) {
                                                30 - default_ms
                                            } else {
                                                default_ms
                                            };
                                            let n = rate.hz() as usize * ms / 1000;
                                            let input: Vec<i16> = (0..n * channels)
                                                .map(|i| {
                                                    let c = i % channels;
                                                    let phase = (frame * n
                                                        + i / channels
                                                        + if (125..140).contains(&frame) {
                                                            0
                                                        } else {
                                                            17 * c
                                                        })
                                                        % 80;
                                                    if !(30..100).contains(&frame) {
                                                        ((phase.min(80 - phase) as i32 - 20)
                                                            * if c == 1
                                                                && (125..140).contains(&frame)
                                                            {
                                                                -600
                                                            } else {
                                                                600
                                                            })
                                                            as i16
                                                    } else if frame < 70 {
                                                        0
                                                    } else {
                                                        ((i / channels + c) % 2) as i16
                                                    }
                                                })
                                                .collect();
                                            encoder.set_inband_fec(!(85..95).contains(&frame));
                                            let packet = encoder.encode(if ms == 10 { FrameDuration::Ms10 } else { FrameDuration::Ms20 }, &input,
                                                ms * if (112..116).contains(&frame) { 3 } else { rate_kbps / 8 } * channels,
                                                Options { complexity, packet_loss_percent: loss_percent })
                                                .map_err(|e| format!("{rate:?}/{bandwidth:?}/{channels}/{default_ms}/{complexity}/{rate_kbps}/{loss_percent}/dtx={dtx}/f{frame}: {e}"))?;
                                            protected += usize::from(encoder.has_fec());
                                            suppressed += usize::from(encoder.in_dtx());
                                            if !loss || frame % 3 != 1 {
                                                if loss && frame % 3 == 2 {
                                                    let decoded = decoder.decode_fec(
                                                        packet.as_bytes(),
                                                        previous_samples as u16,
                                                    )?;
                                                    record(
                                                        &mut out,
                                                        packet.as_bytes(),
                                                        &decoded,
                                                        2,
                                                        channels,
                                                    )?;
                                                    recoveries += 1;
                                                }
                                                let decoded = decoder.decode(packet.as_bytes())?;
                                                if decoded.final_range() != packet.final_range() {
                                                    return Err("primary range mismatch".into());
                                                }
                                                record(
                                                    &mut out,
                                                    packet.as_bytes(),
                                                    &decoded,
                                                    u8::from(frame == 0),
                                                    channels,
                                                )?;
                                            }
                                            previous_samples = ms * 48;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    if protected == 0 || suppressed == 0 || recoveries == 0 {
        return Err("insufficient coverage".into());
    }
    out.flush()?;
    eprintln!(
        "{protected} protected primary frames, {suppressed} DTX frames, {recoveries} FEC calls"
    );
    Ok(())
}
