use std::{
    error::Error,
    fs,
    hint::black_box,
    io::{BufWriter, Write},
    path::Path,
    thread,
    time::{Duration, Instant},
};
const SAMPLES: usize = 320;
const PERIOD: u64 = 20_000_000;
type R<T> = Result<T, Box<dyn Error>>;
#[cfg(feature = "opura-build")]
use opura::decoder::OpusDecoder as Decoder;
#[cfg(feature = "opus-rs-build")]
use opus_rs::OpusDecoder as Decoder;
fn new_decoder() -> R<Decoder> {
    #[cfg(feature = "opura-build")]
    {
        Ok(Decoder::new(
            opura::silk::resampler::OutputRate::Khz16,
            opura::silk::decoder::PcmChannels::Mono,
        )?)
    }
    #[cfg(feature = "opus-rs-build")]
    {
        Ok(Decoder::new(16000, 1)?)
    }
}
fn decode(d: &mut Decoder, p: &[u8], out: &mut [f32; SAMPLES]) -> R<u32> {
    #[cfg(feature = "opura-build")]
    {
        let x = d.decode(p)?;
        if x.samples().len() != SAMPLES {
            return Err("duration".into());
        }
        for (dest, &s) in out.iter_mut().zip(x.samples()) {
            *dest = f32::from(s) / 32768.0
        }
        Ok(x.final_range())
    }
    #[cfg(feature = "opus-rs-build")]
    {
        if d.decode(p, SAMPLES, out)? != SAMPLES {
            return Err("duration".into());
        }
        Ok(d.final_range())
    }
}
fn corpus() -> R<Vec<Vec<Vec<u8>>>> {
    (0..15)
        .map(|i| {
            let buf = fs::read(format!("packets/golos_{i:02}"))?;
            let mut data = buf.as_slice();
            let mut packets = vec![];
            while !data.is_empty() {
                let n = u32::from_be_bytes(data[..4].try_into()?) as usize;
                packets.push(data[4..4 + n].to_vec());
                data = &data[4 + n..];
            }
            Ok(packets)
        })
        .collect()
}
fn qualify(out: &Path) -> R<()> {
    let clips = corpus()?;
    for (i, c) in clips.iter().enumerate() {
        let mut d = new_decoder()?;
        let mut frame = [0.0; SAMPLES];
        let mut pcm = BufWriter::new(fs::File::create(out.join(format!("{i:02}.f32")))?);
        let mut range = BufWriter::new(fs::File::create(out.join(format!("{i:02}.range")))?);
        for p in c {
            range.write_all(&decode(&mut d, p, &mut frame)?.to_le_bytes())?;
            for s in frame {
                if !s.is_finite() {
                    return Err("nonfinite output".into());
                }
                pcm.write_all(&s.to_le_bytes())?
            }
        }
    }
    Ok(())
}
struct Stream {
    d: Decoder,
    pcm: [f32; SAMPLES],
    clip: usize,
    offset: usize,
}
fn ticks() -> R<u64> {
    let stat = fs::read_to_string("/proc/self/stat")?;
    let fields: Vec<_> = stat
        .rsplit_once(')')
        .ok_or("stat comm")?
        .1
        .split_whitespace()
        .collect();
    Ok(fields[11].parse::<u64>()? + fields[12].parse::<u64>()?)
}
fn kib(key: &str) -> R<u64> {
    let stat = fs::read_to_string("/proc/self/status")?;
    Ok(stat
        .lines()
        .find_map(|l| l.strip_prefix(key))
        .ok_or("missing RSS")?
        .split_whitespace()
        .next()
        .ok_or("RSS unit")?
        .parse()?)
}
fn q(x: &[u64], p: usize, denom: usize) -> u64 {
    x[(x.len() * p).div_ceil(denom) - 1]
}
fn workload(n: usize, frames: usize, seed: usize) -> R<()> {
    let clips = corpus()?;
    let mut streams = Vec::with_capacity(n);
    for i in 0..n {
        let clip = (i + seed) % clips.len();
        let packets = &clips[clip];
        // Warm from the beginning of this real recording, then consume a contiguous excerpt.
        let offset = 12 + (i * 37 + seed * 53) % (packets.len() - frames - 12 + 1);
        let mut s = Stream {
            d: new_decoder()?,
            pcm: [0.0; SAMPLES],
            clip,
            offset,
        };
        for p in &packets[..offset] {
            decode(&mut s.d, p, &mut s.pcm)?;
        }
        streams.push(s);
    }
    // One retained output buffer for every stream; reserve a fixed measurement array up front.
    let mut times = vec![0u64; n * frames];
    let rss_start = kib("VmRSS:")?;
    let ticks_before = ticks()?;
    let start = Instant::now();
    let mut miss = 0usize;
    for f in 0..frames {
        for (i, s) in streams.iter_mut().enumerate() {
            // Uniform independent phase, deadlines at arrival + 20 ms. An overload drains all queued work.
            let due_ns = f as u64 * PERIOD + (i as u64 * PERIOD) / n as u64;
            let due = start + Duration::from_nanos(due_ns);
            if let Some(wait) = due.checked_duration_since(Instant::now()) {
                thread::sleep(wait)
            }
            let range = decode(&mut s.d, &clips[s.clip][s.offset + f], &mut s.pcm)?;
            black_box((&s.pcm, range));
            let delay = due.elapsed().as_nanos() as u64;
            times[f * n + i] = delay;
            miss += usize::from(delay > PERIOD);
        }
    }
    let wall_ns = start.elapsed().as_nanos();
    let ticks_spent = ticks()? - ticks_before;
    let high = kib("VmHWM:")?;
    let rss = kib("VmRSS:")?;
    times.sort_unstable();
    println!(
        "{n},{frames},{seed},{},{},{},{},{},{},{},{miss},{},{}",
        n * frames,
        wall_ns,
        ticks_spent,
        rss_start,
        rss,
        high,
        q(&times, 50, 100),
        q(&times, 99, 100),
        q(&times, 999, 1000)
    );
    Ok(())
}
fn main() -> R<()> {
    let args: Vec<String> = std::env::args().collect();
    if args[1] == "qualify" {
        return qualify(Path::new(&args[2]));
    }
    workload(args[1].parse()?, args[2].parse()?, args[3].parse()?)
}
