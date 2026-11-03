//! Optional actual speech-layout/loss/FEC cross-decoder corpus.
use opura::{
    decoder::{DecodedPcm, OpusDecoder},
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
    out.write_all(b"OPURAL02")?;
    let durations = [
        PacketDuration::Ms10,
        PacketDuration::Ms20,
        PacketDuration::Ms40,
        PacketDuration::Ms60,
    ];
    let mut counts = [0usize; 8]; // records, wire changes, mono, stereo, DTX, FEC calls, PLC, protected packets
    for rate in [
        OutputRate::Khz8,
        OutputRate::Khz12,
        OutputRate::Khz16,
        OutputRate::Khz24,
        OutputRate::Khz48,
    ] {
        for band in [SilkRate::Khz8, SilkRate::Khz12, SilkRate::Khz16] {
            for duration in durations {
                for variable in [false, true] {
                    let mut encoder = StereoEncoder::new(rate, band);
                    let mut decoder = OpusDecoder::new(OutputRate::Khz48, PcmChannels::Stereo)?;
                    let mut previous_duration = duration;
                    for frame in 0..96 {
                        let current = if (52..56).contains(&frame) {
                            durations[frame - 52]
                        } else {
                            duration
                        };
                        let request = 1 + ((frame / 7 + usize::from(variable)) % 2);
                        let old = encoder.encoded_channels();
                        encoder.set_encoded_channels(request)?;
                        encoder.set_dtx((16..80).contains(&frame));
                        encoder.set_inband_fec(!(68..73).contains(&frame));
                        let n = rate.hz() as usize * current.milliseconds() as usize / 1000;
                        let input: Vec<i16> = (0..n)
                            .flat_map(|i| {
                                (0..2).map(move |ch| {
                                    let phase = (frame * n + i + ch * 17) % 80;
                                    if (12..28).contains(&(frame % 32)) {
                                        0
                                    } else {
                                        ((phase.min(80 - phase) as i32 - 20) * 600) as i16
                                    }
                                })
                            })
                            .collect();
                        let packet = encoder
                            .encode(
                                current,
                                &input,
                                current.milliseconds() as usize * 6,
                                StereoOptions {
                                    complexity: match band {
                                        SilkRate::Khz8 => 0,
                                        SilkRate::Khz12 => 6,
                                        SilkRate::Khz16 => 10,
                                    },
                                    variable_bitrate: variable,
                                    packet_loss_percent: 20,
                                },
                            )
                            .map_err(|e| {
                                format!(
                                    "{rate:?}/{band:?}/{duration:?}/{variable}/frame{frame}: {e}"
                                )
                            })?;
                        counts[1] += usize::from(old != encoder.encoded_channels());
                        counts[if encoder.encoded_channels() == 1 {
                            2
                        } else {
                            3
                        }] += 1;
                        counts[4] += usize::from(encoder.in_dtx());
                        counts[7] += usize::from(encoder.has_fec());
                        if old != encoder.encoded_channels() && encoder.has_fec() {
                            return Err("incompatible FEC crossed wire layout".into());
                        }
                        if encoder.in_dtx() != (packet.as_bytes().len() == 1) {
                            return Err("DTX diagnostic mismatch".into());
                        }
                        if frame % 11 == 5 {
                            let pcm =
                                decoder.conceal_samples(current.milliseconds() as u16 * 48)?;
                            record(&mut out, &[], &pcm, 0)?;
                            counts[0] += 1;
                            counts[6] += 1;
                        } else if frame % 11 != 9 {
                            if frame % 11 == 10 {
                                let pcm = decoder.decode_fec(
                                    packet.as_bytes(),
                                    previous_duration.milliseconds() as u16 * 48,
                                )?;
                                record(&mut out, packet.as_bytes(), &pcm, 2)?;
                                counts[0] += 1;
                                counts[5] += 1;
                            }
                            let pcm = decoder.decode(packet.as_bytes())?;
                            if pcm.final_range() != packet.final_range() {
                                return Err("primary final range mismatch".into());
                            }
                            record(&mut out, packet.as_bytes(), &pcm, u8::from(frame == 0))?;
                            counts[0] += 1;
                        }
                        previous_duration = current;
                    }
                }
            }
        }
    }
    if counts[0] != 11520
        || counts[1] < 1000
        || counts[2] < 1000
        || counts[3] < 1000
        || counts[4] < 100
        || counts[5] != 960
        || counts[6] != 1080
        || counts[7] < 500
    {
        return Err(format!("insufficient coverage: {counts:?}").into());
    }
    out.flush()?;
    println!("PASS:120 streams; records/changes/mono/stereo/DTX/FEC/PLC/protected={counts:?}");
    Ok(())
}
