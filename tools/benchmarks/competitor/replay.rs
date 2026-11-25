//! External probe built in an isolated Cargo project with one decoder dependency.
use std::error::Error;
use std::fs::{self, File};
use std::hint::black_box;
use std::io::{BufWriter, Write};
use std::time::Instant;

struct Record<'a> {
    data: &'a [u8],
    samples: usize,
    range: u32,
}

fn records(mut bytes: &[u8]) -> Result<Vec<Record<'_>>, Box<dyn Error>> {
    let mut packets = Vec::new();
    while !bytes.is_empty() {
        let (header, rest) = bytes.split_at_checked(12).ok_or("incomplete header")?;
        let n = u32::from_be_bytes(header[..4].try_into()?) as usize;
        let (data, tail) = rest.split_at_checked(n).ok_or("incomplete packet")?;
        packets.push(Record {
            data,
            samples: u32::from_be_bytes(header[4..8].try_into()?) as usize,
            range: u32::from_be_bytes(header[8..].try_into()?),
        });
        bytes = tail;
    }
    Ok(packets)
}

#[cfg(feature = "opura")]
fn replay(
    rate: usize,
    channels: usize,
    packets: &[Record],
    pcm: &mut Option<BufWriter<File>>,
    times: &mut Vec<u64>,
) -> Result<usize, Box<dyn Error>> {
    use opura::decoder::OpusDecoder;
    use opura::silk::{decoder::PcmChannels, resampler::OutputRate};
    let rate = match rate {
        16000 => OutputRate::Khz16,
        48000 => OutputRate::Khz48,
        _ => return Err("rate".into()),
    };
    let layout = match channels {
        1 => PcmChannels::Mono,
        2 => PcmChannels::Stereo,
        _ => return Err("channels".into()),
    };
    let mut decoder = OpusDecoder::new(rate, layout)?;
    let mut wrong_ranges = 0;
    for r in packets {
        let start = Instant::now();
        let out = decoder.decode(r.data)?;
        black_box(out.samples());
        times.push(start.elapsed().as_nanos() as u64);
        if out.samples().len() != channels * r.samples {
            return Err("frame length".into());
        }
        if let Some(file) = pcm {
            wrong_ranges += usize::from(r.range != 0 && out.final_range() != r.range);
            for s in out.samples() {
                file.write_all(&s.to_le_bytes())?;
            }
        }
    }
    Ok(wrong_ranges)
}

#[cfg(not(feature = "opura"))]
fn replay(
    rate: usize,
    channels: usize,
    packets: &[Record],
    pcm: &mut Option<BufWriter<File>>,
    times: &mut Vec<u64>,
) -> Result<usize, Box<dyn Error>> {
    let mut decoder = opus_rs::OpusDecoder::new(rate as i32, channels)?;
    let mut output = vec![0.0; rate * 120 / 1000 * channels];
    let mut wrong_ranges = 0;
    for r in packets {
        let requested = &mut output[..r.samples * channels];
        let start = Instant::now();
        let n = decoder.decode(r.data, r.samples, requested)?;
        black_box(&*requested);
        times.push(start.elapsed().as_nanos() as u64);
        if n != r.samples {
            return Err("frame length".into());
        }
        if let Some(file) = pcm {
            wrong_ranges += usize::from(r.range != 0 && decoder.final_range() != r.range);
            for s in requested {
                if !s.is_finite() {
                    return Err("non-finite PCM".into());
                }
                file.write_all(&((*s * 32768.0).round() as i16).to_le_bytes())?;
            }
        }
    }
    Ok(wrong_ranges)
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().collect();
    let rate = args[1].parse()?;
    let channels = args[2].parse()?;
    let bytes = fs::read(&args[3])?;
    let packets = records(&bytes)?;
    let mut times = Vec::with_capacity(packets.len());
    let timing = args[4] == "-";
    let mut output = if timing {
        None
    } else {
        Some(BufWriter::new(File::create(&args[4])?))
    };
    if timing {
        replay(rate, channels, &packets, &mut None, &mut times)?;
        times.clear();
    }
    let mismatches = replay(rate, channels, &packets, &mut output, &mut times)?;
    if let Some(mut file) = output {
        file.flush()?;
    }
    times.sort_unstable();
    let p99 = times[(99 * times.len()).div_ceil(100) - 1];
    println!(
        "{} {} {} {}",
        times.len(),
        mismatches,
        times.iter().sum::<u64>(),
        p99
    );
    Ok(())
}
