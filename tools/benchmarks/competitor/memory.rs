//! Independent live-decoder residency probe. Built only by memory.py.
use std::error::Error;
use std::fs;
use std::hint::black_box;

fn private_dirty_kib() -> Result<usize, Box<dyn Error>> {
    let stats = fs::read_to_string("/proc/self/smaps_rollup")?;
    Ok(stats
        .lines()
        .find_map(|line| line.strip_prefix("Private_Dirty:"))
        .ok_or("missing Private_Dirty")?
        .split_whitespace()
        .next()
        .ok_or("missing value")?
        .parse()?)
}

fn measure<T>(
    mut new_warmed: impl FnMut() -> Result<T, Box<dyn Error>>,
) -> Result<(), Box<dyn Error>> {
    let mut states = Vec::with_capacity(65);
    let _ = private_dirty_kib()?;
    for target in [1, 33, 65] {
        while states.len() < target {
            states.push(new_warmed()?);
        }
        black_box(&states);
        println!("{target} {}", private_dirty_kib()?);
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().collect();
    let rate: usize = args[2].parse()?;
    let channels: usize = args[3].parse()?;
    let bytes = fs::read(&args[4])?;
    let mut input = bytes.as_slice();
    let mut packets = Vec::new();
    while !input.is_empty() {
        let (header, tail) = input.split_at_checked(12).ok_or("missing header")?;
        let size = u32::from_be_bytes(header[..4].try_into()?) as usize;
        let samples = u32::from_be_bytes(header[4..8].try_into()?) as usize;
        let (packet, next) = tail.split_at_checked(size).ok_or("missing payload")?;
        packets.push((samples, packet));
        input = next;
    }
    match args[1].as_str() {
        "opura" => {
            use opura::decoder::OpusDecoder;
            use opura::silk::{decoder::PcmChannels, resampler::OutputRate};
            let hz = match rate {
                16000 => OutputRate::Khz16,
                48000 => OutputRate::Khz48,
                _ => return Err("bad output rate".into()),
            };
            let layout = match channels {
                1 => PcmChannels::Mono,
                2 => PcmChannels::Stereo,
                _ => return Err("bad channels".into()),
            };
            measure(|| {
                let mut decoder = OpusDecoder::new(hz, layout)?;
                for &(samples, packet) in &packets {
                    let out = decoder.decode(packet)?;
                    if out.samples_per_channel() != samples {
                        return Err("sample count".into());
                    }
                    black_box(out.samples());
                }
                Ok(decoder)
            })
        }
        "opus-rs" => {
            let mut output = vec![0.0; rate * 120 / 1000 * channels];
            measure(|| {
                let mut decoder = opus_rs::OpusDecoder::new(rate as i32, channels)?;
                for &(samples, packet) in &packets {
                    let out = &mut output[..samples * channels];
                    if decoder.decode(packet, samples, out)? != samples {
                        return Err("sample count".into());
                    }
                    black_box(&*out);
                }
                Ok(decoder)
            })
        }
        _ => Err("unknown decoder".into()),
    }
}
