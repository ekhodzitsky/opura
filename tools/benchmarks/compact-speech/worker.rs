#[cfg(feature = "compact")]
use opura::decoder::CompactOpusDecoder as Decoder;
#[cfg(not(feature = "compact"))]
use opura::decoder::OpusDecoder as Decoder;
use opura::silk::{decoder::PcmChannels, resampler::OutputRate};
use std::{
    error::Error,
    fs,
    hint::black_box,
    io::{self, Write},
    time::Instant,
};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

#[repr(C)]
struct Timespec {
    seconds: i64,
    nanos: i64,
}
extern "C" {
    fn clock_gettime(clock: i32, result: *mut Timespec) -> i32;
}
fn cpu_ns() -> u64 {
    let mut ts = Timespec {
        seconds: 0,
        nanos: 0,
    };
    // On Linux x86-64 CLOCK_THREAD_CPUTIME_ID is 3 and timespec has two i64 fields.
    // The pointer is valid for the duration of this synchronous libc call.
    assert_eq!(unsafe { clock_gettime(3, &mut ts) }, 0);
    (ts.seconds as u64) * 1_000_000_000 + ts.nanos as u64
}

fn metric(text: &str, key: &str) -> Result<u64> {
    let line = text
        .lines()
        .find(|l| l.starts_with(key))
        .ok_or("missing proc key")?;
    Ok(line
        .split_ascii_whitespace()
        .nth(1)
        .ok_or("missing value")?
        .parse()?)
}
fn memory() -> Result<[u64; 4]> {
    let status = fs::read_to_string("/proc/self/status")?;
    let rollup = fs::read_to_string("/proc/self/smaps_rollup")?;
    Ok([
        metric(&status, "VmRSS:")?,
        metric(&status, "VmHWM:")?,
        metric(&rollup, "Private_Dirty:")?,
        metric(&rollup, "Rss:")?,
    ])
}
fn packets(bytes: &[u8]) -> Vec<&[u8]> {
    let mut parts = vec![];
    let mut i = 0;
    while i < bytes.len() {
        let n = u32::from_be_bytes(bytes[i..i + 4].try_into().unwrap()) as usize;
        let packet = &bytes[i + 4..i + 4 + n];
        assert!(packet[0] >> 3 < 12);
        parts.push(packet);
        i += n + 4;
    }
    assert_eq!(parts.len(), 6001);
    parts
}

struct Stream {
    decoder: Decoder,
    pcm: [i16; 320],
    index: usize,
    clip: usize,
}
fn send<const VERIFY: bool>(
    stream: &mut Stream,
    packet: &[u8],
    dump: &mut impl Write,
) -> Result<()> {
    let n = stream
        .decoder
        .decode_into(black_box(packet), &mut stream.pcm)?;
    assert_eq!(n, 320);
    black_box(&stream.pcm[..n]);
    let range = black_box(stream.decoder.final_range());
    if VERIFY {
        dump.write_all(&(n as u32).to_le_bytes())?;
        for v in &stream.pcm[..n] {
            dump.write_all(&v.to_le_bytes())?;
        }
        dump.write_all(&range.to_le_bytes())?;
    }
    Ok(())
}
fn run<const VERIFY: bool>(count: usize) -> Result<()> {
    let rounds = match count {
        1 => 4096,
        128 => 128,
        768 => 64,
        _ => return Err("unknown count".into()),
    };
    let raw = ["ES2002a", "IS1000a", "TS3003a"]
        .map(|name| fs::read(format!("inputs/{name}.packets")).unwrap());
    let corpus = raw.each_ref().map(|bytes| packets(bytes));
    let switch = fs::read("inputs/switch.packet")?;
    assert!(switch[0] >> 3 >= 16);
    let mut dump = io::BufWriter::new(io::stdout().lock());
    let initial = memory()?;
    let mut streams = Vec::with_capacity(count);
    for i in 0..count {
        streams.push(Stream {
            decoder: Decoder::new(OutputRate::Khz16, PcmChannels::Mono)?,
            pcm: [0i16; 320],
            index: 89 + i * 193 % (6001 - 4096 - 16 - 89),
            clip: i % 3,
        });
    }
    for _ in 0..16 {
        for stream in &mut streams {
            send::<VERIFY>(stream, corpus[stream.clip][stream.index], &mut dump)?;
            stream.index += 1;
        }
    }
    let warm = memory()?;
    let start = cpu_ns();
    for _ in 0..rounds {
        for stream in &mut streams {
            send::<VERIFY>(stream, corpus[stream.clip][stream.index], &mut dump)?;
            stream.index += 1;
        }
    }
    let cpu = cpu_ns() - start;
    let steady = memory()?;
    let mut times = Vec::with_capacity(count);
    for stream in &mut streams {
        let t = Instant::now();
        send::<VERIFY>(stream, &switch, &mut dump)?;
        times.push(t.elapsed().as_nanos());
    }
    let switched = memory()?;
    black_box(&streams);
    if !VERIFY {
        writeln!(dump,"{{\"initial\":{:?},\"warm\":{:?},\"steady\":{:?},\"switched\":{:?},\"cpu_ns\":{},\"timed_packets\":{},\"transition_ns\":{:?},\"decoder_bytes\":{}}}",initial,warm,steady,switched,cpu,count*rounds,times,std::mem::size_of::<Decoder>())?;
    }
    Ok(())
}
fn main() -> Result<()> {
    let args = std::env::args().collect::<Vec<_>>();
    let n = args[1].parse()?;
    if args[2] == "verify" {
        run::<true>(n)
    } else {
        run::<false>(n)
    }
}
