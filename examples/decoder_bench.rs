//! Optional in-process public decoder benchmark. File I/O is outside timing.
//! Usage: decoder_bench VERIFIED_VECTOR_DIR RATE CHANNELS PASSES WARMUP [MINIMUM_MS] [EXPECTED_FINGERPRINT]
use opura::{
    decoder::OpusDecoder,
    silk::{decoder::PcmChannels, resampler::OutputRate},
};
use std::{
    env, error::Error, fs::File, hint::black_box, io::Read, mem::size_of, path::PathBuf,
    time::Instant,
};
struct Record {
    data: Vec<u8>,
    range: u32,
}
#[derive(Default, Eq, PartialEq)]
struct Pass {
    packets: u64,
    samples: u64,
    fingerprint: u64,
}
fn fold(hash: &mut u64, bytes: &[u8]) {
    for &b in bytes {
        *hash = (*hash ^ u64::from(b)).wrapping_mul(1099511628211);
    }
}
fn load(directory: &std::path::Path) -> Result<Vec<Vec<Record>>, Box<dyn Error>> {
    let mut streams = Vec::new();
    for number in 1..=12 {
        let mut data = Vec::new();
        File::open(directory.join(format!("testvector{number:02}.bit")))?
            .take(256 * 1024 * 1024 + 1)
            .read_to_end(&mut data)?;
        if data.len() > 256 * 1024 * 1024 {
            return Err("vector exceeds256MiB".into());
        }
        let mut offset = 0;
        let mut records = Vec::new();
        while offset < data.len() {
            let header = data
                .get(offset..offset + 8)
                .ok_or("truncated vector header")?;
            let length = u32::from_be_bytes(header[..4].try_into()?) as usize;
            let range = u32::from_be_bytes(header[4..].try_into()?);
            offset += 8;
            if length == 0 || length > 65536 {
                return Err("invalid benchmark packet length".into());
            }
            let end = offset.checked_add(length).ok_or("packet offset overflow")?;
            records.push(Record {
                data: data.get(offset..end).ok_or("truncated packet")?.to_vec(),
                range,
            });
            offset = end;
            if records.len() > 1_000_000 {
                return Err("too many records".into());
            }
        }
        if records.is_empty() {
            return Err("empty vector".into());
        }
        streams.push(records);
    }
    Ok(streams)
}
fn replay(
    decoder: &mut OpusDecoder,
    streams: &[Vec<Record>],
    check: bool,
    mut latency: Option<&mut Vec<u64>>,
) -> Result<Pass, Box<dyn Error>> {
    let mut pass = Pass {
        fingerprint: 14695981039346656037,
        ..Pass::default()
    };
    for (stream, records) in streams.iter().enumerate() {
        decoder.reset();
        for (index, record) in records.iter().enumerate() {
            let begin = latency.as_ref().map(|_| Instant::now());
            let pcm = decoder.decode(&record.data)?;
            black_box(pcm.samples());
            if let (Some(begin), Some(times)) = (begin, latency.as_deref_mut()) {
                times.push(begin.elapsed().as_nanos().try_into()?);
            }
            if check {
                if record.range != 0 && record.range != pcm.final_range() {
                    return Err(format!("vector{} packet{index} range mismatch", stream + 1).into());
                }
                for &sample in pcm.samples() {
                    fold(&mut pass.fingerprint, &sample.to_le_bytes());
                }
                fold(
                    &mut pass.fingerprint,
                    &(pcm.samples_per_channel() as u32).to_le_bytes(),
                );
                fold(&mut pass.fingerprint, &pcm.final_range().to_le_bytes());
            }
            pass.packets += 1;
            pass.samples += pcm.samples_per_channel() as u64;
        }
    }
    Ok(pass)
}
fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    if !(5..=7).contains(&args.len()) {
        return Err("usage: decoder_bench VECTOR_DIR RATE CHANNELS PASSES WARMUP [MINIMUM_MS] [EXPECTED_FINGERPRINT]".into());
    }
    let directory = PathBuf::from(&args[0]);
    let rate = match args[1].to_str() {
        Some("8000") => OutputRate::Khz8,
        Some("12000") => OutputRate::Khz12,
        Some("16000") => OutputRate::Khz16,
        Some("24000") => OutputRate::Khz24,
        Some("48000") => OutputRate::Khz48,
        _ => return Err("unsupported rate".into()),
    };
    let channels = match args[2].to_str() {
        Some("1") => PcmChannels::Mono,
        Some("2") => PcmChannels::Stereo,
        _ => return Err("unsupported channels".into()),
    };
    let passes: usize = args[3].to_str().ok_or("passes")?.parse()?;
    let warmup: usize = args[4].to_str().ok_or("warmup")?.parse()?;
    if !(1..=1000).contains(&passes) || !(1..=100).contains(&warmup) {
        return Err("passes/warmup out of bounds".into());
    }
    let minimum_ms: u64 = args
        .get(5)
        .map(|v| v.to_str().ok_or("minimum time"))
        .transpose()?
        .map(str::parse)
        .transpose()?
        .map_or(0, |v| v);
    if minimum_ms > 2000 {
        return Err("minimum time must be0..2000ms".into());
    }
    let expected = args
        .get(6)
        .map(|v| {
            let value = v.to_str().ok_or("fingerprint")?;
            if value.len() != 16 {
                return Err("fingerprint requires16hex digits".into());
            }
            Ok::<u64, Box<dyn Error>>(u64::from_str_radix(value, 16)?)
        })
        .transpose()?;
    let streams = load(&directory)?;
    let init = Instant::now();
    let mut decoder = OpusDecoder::new(rate, channels)?;
    let init_ns = init.elapsed().as_nanos();
    let validation = replay(&mut decoder, &streams, true, None)?;
    if expected.is_some_and(|expected| expected != validation.fingerprint) {
        return Err("expected PCM/range fingerprint mismatch".into());
    }
    for _ in 0..warmup {
        black_box(replay(&mut decoder, &streams, false, None)?);
    }
    let mut completed = 0;
    let mut elapsed = 0u128;
    while completed < passes || elapsed < u128::from(minimum_ms) * 1_000_000 {
        if completed == 1000 {
            return Err("minimum time exceeds1000pass bound".into());
        }
        let begin = Instant::now();
        let observed = black_box(replay(&mut decoder, &streams, false, None)?);
        elapsed += begin.elapsed().as_nanos();
        completed += 1;
        if (observed.packets, observed.samples) != (validation.packets, validation.samples) {
            return Err("timed decode geometry changed".into());
        }
    }
    let mut latency = Vec::with_capacity(validation.packets as usize);
    replay(&mut decoder, &streams, false, Some(&mut latency))?;
    let mut mode_times = [0u64; 3];
    let mut mode_counts = [0u64; 3];
    for (record, &elapsed) in streams.iter().flatten().zip(&latency) {
        let mode = if record.data[0] & 0x80 != 0 {
            2
        } else if record.data[0] & 0x60 == 0x60 {
            1
        } else {
            0
        };
        mode_times[mode] += elapsed;
        mode_counts[mode] += 1;
    }
    let final_validation = replay(&mut decoder, &streams, true, None)?;
    if final_validation != validation {
        return Err("output changed after timing".into());
    }
    latency.sort_unstable();
    let percentile = |p: usize| latency[(latency.len() - 1) * p / 100];
    println!(
        "{{\"packets\":{},\"samples_per_pass\":{},\"fingerprint\":\"{:016x}\",\"decoder_bytes\":{},\"passes\":{},\"elapsed_ns\":{},\"init_ns\":{},\"latency_ns\":{{\"p50\":{},\"p95\":{},\"p99\":{},\"max\":{}}},\"mode_packets\":[{},{},{}],\"mode_latency_total_ns\":[{},{},{}]}}",
        validation.packets,
        validation.samples,
        validation.fingerprint,
        size_of::<OpusDecoder>(),
        completed,
        elapsed,
        init_ns,
        percentile(50),
        percentile(95),
        percentile(99),
        percentile(100),
        mode_counts[0],
        mode_counts[1],
        mode_counts[2],
        mode_times[0],
        mode_times[1],
        mode_times[2]
    );
    Ok(())
}
