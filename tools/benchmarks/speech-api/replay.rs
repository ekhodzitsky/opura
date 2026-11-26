use std::{
    env,
    error::Error,
    fs,
    hint::black_box,
    io::{BufWriter, Write},
    path::Path,
};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

fn cpu_ns() -> Result<u64> {
    Ok(fs::read_to_string("/proc/thread-self/schedstat")?
        .split_whitespace()
        .next()
        .ok_or("missing scheduler CPU time")?
        .parse()?)
}

fn corpus(dir: &Path) -> Result<Vec<Vec<Vec<u8>>>> {
    (0..15)
        .map(|i| {
            let data = fs::read(dir.join(format!("golos_{i:02}")))?;
            let mut remaining = &data[..];
            let mut packets = Vec::new();
            while !remaining.is_empty() {
                let size = u32::from_be_bytes(remaining[..4].try_into()?) as usize;
                packets.push(remaining[4..4 + size].to_vec());
                remaining = &remaining[4 + size..];
            }
            Ok(packets)
        })
        .collect()
}

fn pass(
    clips: &[Vec<Vec<u8>>],
    hz: usize,
    mut dump: Option<&mut BufWriter<fs::File>>,
) -> Result<()> {
    let count = hz / 50;
    let mut out = vec![0.0f32; count];
    #[cfg(feature = "caller")]
    let mut pcm = vec![0i16; count];
    for clip in clips {
        #[cfg(any(feature = "owned", feature = "caller"))]
        let mut decoder = {
            use opura::silk::{decoder::PcmChannels, resampler::OutputRate};
            let rate = match hz {
                16000 => OutputRate::Khz16,
                48000 => OutputRate::Khz48,
                _ => return Err("rate".into()),
            };
            opura::decoder::OpusDecoder::new(rate, PcmChannels::Mono)?
        };
        #[cfg(feature = "reference")]
        let mut decoder = opus_rs::OpusDecoder::new(hz as i32, 1)?;
        for packet in clip {
            #[cfg(feature = "owned")]
            let owned = decoder.decode(packet)?;
            #[cfg(feature = "owned")]
            let data = owned.samples();
            #[cfg(feature = "caller")]
            let data = {
                let n = decoder.decode_into(packet, &mut pcm)?;
                &pcm[..n]
            };
            #[cfg(any(feature = "owned", feature = "caller"))]
            {
                if data.len() != count {
                    return Err("packet duration".into());
                }
                for (x, &v) in out.iter_mut().zip(data) {
                    *x = f32::from(v) / 32768.0;
                }
            }
            #[cfg(feature = "reference")]
            if decoder.decode(packet, count, &mut out)? != count {
                return Err("packet duration".into());
            }
            let range = decoder.final_range();
            black_box((&out, range));
            if let Some(writer) = dump.as_deref_mut() {
                writer.write_all(&range.to_le_bytes())?;
                for x in &out {
                    writer.write_all(&x.to_le_bytes())?;
                }
            }
        }
    }
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<_> = env::args().collect();
    let clips = corpus(Path::new(&args[1]))?;
    let hz = args[2].parse()?;
    let repeats: usize = args[3].parse()?;
    let mut dump = if let Some(path) = args.get(4) {
        Some(BufWriter::new(fs::File::create(path)?))
    } else {
        None
    };
    pass(&clips, hz, dump.as_mut())?;
    if let Some(f) = dump.as_mut() {
        f.flush()?
    }
    let before = cpu_ns()?;
    for _ in 0..repeats {
        pass(&clips, hz, None)?;
    }
    println!("{}", cpu_ns()? - before);
    Ok(())
}
