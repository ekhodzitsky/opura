//! Optional mono/stereo gradual bandwidth-target, FEC/DTX and loss cross-decoder corpus; no native Cargo dependency.
use opura::{
    decoder::OpusDecoder,
    silk::{
        decoder::PcmChannels,
        encoder::{
            mono::{MonoEncoder, MonoOptions, PacketDuration},
            stereo::StereoEncoder,
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
    let channels: usize = env::args()
        .nth(2)
        .ok_or("expected channels 1 or 2")?
        .parse()?;
    if !(1..=2).contains(&channels) {
        return Err("expected channels 1 or 2".into());
    }
    let mode = env::args().nth(3).unwrap_or_else(|| "loss".to_owned());
    if !["normal", "loss"].contains(&mode.as_str()) {
        return Err("expected normal or loss".into());
    }
    let mut protected = 0;
    let mut changes = [[0usize; 3]; 3];
    let mut fec_calls = 0;
    let mut out = BufWriter::new(OpenOptions::new().write(true).create_new(true).open(path)?);
    out.write_all(if channels == 1 {
        b"OPURAA01"
    } else {
        b"OPURAA02"
    })?;
    let mut suppressed = 0;
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
                    let mut mono = MonoEncoder::new(rate, bandwidth);
                    let mut stereo = StereoEncoder::new(rate, bandwidth);
                    let mut decoder = OpusDecoder::new(
                        OutputRate::Khz48,
                        if channels == 1 {
                            PcmChannels::Mono
                        } else {
                            PcmChannels::Stereo
                        },
                    )?;
                    mono.set_dtx(true);
                    stereo.set_dtx(true);
                    let mut previous_duration = duration;
                    for frame in 0..900 {
                        let band_index = match bandwidth {
                            SilkRate::Khz8 => 0,
                            SilkRate::Khz12 => 1,
                            SilkRate::Khz16 => 2,
                        };
                        let band = [SilkRate::Khz8, SilkRate::Khz12, SilkRate::Khz16]
                            [(band_index + [0, 2, 0][frame / 300]) % 3];
                        mono.set_bandwidth_target(Some(band));
                        stereo.set_bandwidth_target(Some(band));
                        let event = frame % 100;
                        let enabled = !(55..60).contains(&event);
                        mono.set_inband_fec(enabled);
                        stereo.set_inband_fec(enabled);
                        let current_duration = if (20..24).contains(&event) {
                            [
                                PacketDuration::Ms10,
                                PacketDuration::Ms20,
                                PacketDuration::Ms40,
                                PacketDuration::Ms60,
                            ][event - 20]
                        } else {
                            duration
                        };
                        let n =
                            rate.hz() as usize * current_duration.milliseconds() as usize / 1000;
                        let pcm: Vec<i16> = (0..n)
                            .flat_map(|i| {
                                (0..channels).map(move |ch| {
                                    if !(6..48).contains(&event) {
                                        let phase = (frame * n
                                            + i
                                            + if (80..90).contains(&event) {
                                                0
                                            } else {
                                                ch * 17
                                            })
                                            % 80;
                                        ((phase.min(80 - phase) as i32 - 20)
                                            * if ch == 1 && (80..90).contains(&event) {
                                                -600
                                            } else {
                                                600
                                            }) as i16
                                    } else if event < 34 {
                                        0
                                    } else {
                                        ((i + ch) % 2) as i16
                                    }
                                })
                            })
                            .collect();
                        let options = MonoOptions {
                            complexity: match bandwidth {
                                SilkRate::Khz8 => 0,
                                SilkRate::Khz12 => 6,
                                SilkRate::Khz16 => 10,
                            },
                            variable_bitrate: variable,
                            packet_loss_percent: if (60..65).contains(&event) { 0 } else { 20 },
                        };
                        let budget = current_duration.milliseconds() as usize
                            * if (70..76).contains(&event) { 2 } else { 4 }
                            * channels;
                        let previous_band = if channels == 1 {
                            mono.bandwidth()
                        } else {
                            stereo.bandwidth()
                        };
                        let (bytes, range, dtx) = if channels == 1 {
                            let packet = mono.encode(current_duration, &pcm, budget, options).map_err(|e| format!("mono {rate:?}/{bandwidth:?}/{duration:?}/variable={variable}/frame{frame}: {e}"))?;
                            (
                                packet.as_bytes().to_vec(),
                                packet.final_range(),
                                mono.in_dtx(),
                            )
                        } else {
                            let packet = stereo.encode(current_duration, &pcm, budget, options).map_err(|e| format!("stereo {rate:?}/{bandwidth:?}/{duration:?}/variable={variable}/frame{frame}: {e}"))?;
                            (
                                packet.as_bytes().to_vec(),
                                packet.final_range(),
                                stereo.in_dtx(),
                            )
                        };
                        let actual_band = if channels == 1 {
                            mono.bandwidth()
                        } else {
                            stereo.bandwidth()
                        };
                        let index = |band| match band {
                            SilkRate::Khz8 => 0,
                            SilkRate::Khz12 => 1,
                            SilkRate::Khz16 => 2,
                        };
                        if frame > 0 && actual_band != previous_band {
                            changes[index(previous_band)][index(actual_band)] += 1;
                        }
                        if dtx != (bytes.len() == 1) {
                            return Err("DTX diagnostic mismatch".into());
                        }
                        suppressed += usize::from(dtx);
                        protected += usize::from(if channels == 1 {
                            mono.has_fec()
                        } else {
                            stereo.has_fec()
                        });
                        if mode == "normal" || frame % 3 != 1 {
                            if mode == "loss" && frame % 3 == 2 {
                                let decoded = decoder.decode_fec(
                                    &bytes,
                                    previous_duration.milliseconds() as u16 * 48,
                                )?;
                                write_record(&mut out, &bytes, &decoded, 2)?;
                                fec_calls += 1;
                            }
                            let decoded = decoder.decode(&bytes)?;
                            if decoded.final_range() != range {
                                return Err("primary final range mismatch".into());
                            }
                            write_record(&mut out, &bytes, &decoded, u8::from(frame == 0))?;
                        }
                        previous_duration = current_duration;
                    }
                }
            }
        }
    }
    if protected < 1000 {
        return Err("insufficient FEC coverage".into());
    }
    if suppressed < 100 {
        return Err("insufficient DTX coverage".into());
    }
    for (from, to) in [(0, 1), (1, 0), (1, 2), (2, 1)] {
        if changes[from][to] < 10 {
            return Err(format!(
                "insufficient actual bandwidth transitions {from}->{to}: {}",
                changes[from][to]
            )
            .into());
        }
    }
    out.flush()?;
    println!(
        "Generated120 {mode} gradual-bandwidth/FEC streams/108000 records, {channels} channels; {protected} protected, {suppressed} suppressed, {fec_calls} recovery calls; actual changes {changes:?}"
    );
    Ok(())
}

fn write_record(
    out: &mut impl Write,
    bytes: &[u8],
    decoded: &opura::decoder::DecodedPcm,
    flags: u8,
) -> Result<(), Box<dyn std::error::Error>> {
    let hash = decoded
        .samples()
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .fold(14695981039346656037u64, |h, b| {
            (h ^ u64::from(b)).wrapping_mul(1099511628211)
        });
    out.write_all(&[flags])?;
    out.write_all(&(bytes.len() as u16).to_le_bytes())?;
    out.write_all(&(decoded.samples().len() as u16).to_le_bytes())?;
    out.write_all(&decoded.final_range().to_le_bytes())?;
    out.write_all(&hash.to_le_bytes())?;
    out.write_all(bytes)?;
    Ok(())
}
