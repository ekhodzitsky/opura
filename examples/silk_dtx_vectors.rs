//! Optional mono/stereo DTX cross-decoder corpus; no native Cargo dependency.
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
    let mut out = BufWriter::new(OpenOptions::new().write(true).create_new(true).open(path)?);
    out.write_all(if channels == 1 {
        b"OPURAD01"
    } else {
        b"OPURAD02"
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
                    let n = rate.hz() as usize * duration.milliseconds() as usize / 1000;
                    for frame in 0..100 {
                        let enabled = !(55..60).contains(&frame);
                        mono.set_dtx(enabled);
                        stereo.set_dtx(enabled);
                        let pcm: Vec<i16> = (0..n)
                            .flat_map(|i| {
                                (0..channels).map(move |ch| {
                                    if !(5..80).contains(&frame) {
                                        let phase = (frame * n + i + ch * 17) % 80;
                                        ((phase.min(80 - phase) as i32 - 20) * 600) as i16
                                    } else if frame < 50 {
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
                            packet_loss_percent: 20,
                        };
                        let budget = duration.milliseconds() as usize * 4 * channels;
                        let (bytes, range, dtx) = if channels == 1 {
                            let packet = mono.encode(duration, &pcm, budget, options).map_err(|e| format!("mono {rate:?}/{bandwidth:?}/{duration:?}/variable={variable}/frame{frame}: {e}"))?;
                            (
                                packet.as_bytes().to_vec(),
                                packet.final_range(),
                                mono.in_dtx(),
                            )
                        } else {
                            let packet = stereo.encode(duration, &pcm, budget, options).map_err(|e| format!("stereo {rate:?}/{bandwidth:?}/{duration:?}/variable={variable}/frame{frame}: {e}"))?;
                            (
                                packet.as_bytes().to_vec(),
                                packet.final_range(),
                                stereo.in_dtx(),
                            )
                        };
                        if dtx != (bytes.len() == 1) {
                            return Err("DTX diagnostic mismatch".into());
                        }
                        suppressed += usize::from(dtx);
                        let decoded = decoder.decode(&bytes)?;
                        if decoded.final_range() != range {
                            return Err("DTX final range mismatch".into());
                        }
                        let hash = decoded
                            .samples()
                            .iter()
                            .flat_map(|v| v.to_le_bytes())
                            .fold(14695981039346656037u64, |h, b| {
                                (h ^ u64::from(b)).wrapping_mul(1099511628211)
                            });
                        out.write_all(&[u8::from(frame == 0)])?;
                        out.write_all(&(bytes.len() as u16).to_le_bytes())?;
                        out.write_all(&(decoded.samples().len() as u16).to_le_bytes())?;
                        out.write_all(&range.to_le_bytes())?;
                        out.write_all(&hash.to_le_bytes())?;
                        out.write_all(&bytes)?;
                    }
                }
            }
        }
    }
    if suppressed < 1000 {
        return Err("insufficient DTX coverage".into());
    }
    out.flush()?;
    println!(
        "Generated120 DTX streams/12000 packets, {channels} channels; {suppressed} suppressed"
    );
    Ok(())
}
