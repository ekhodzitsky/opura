use opura::{
    multistream::{ChannelMapping, MultistreamDecoder},
    silk::resampler::OutputRate,
};
use std::{env, error::Error, fs, hint::black_box, io::Write};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

struct Clip {
    layout: ChannelMapping,
    packets: Vec<Vec<u8>>,
}

impl Clip {
    fn from_ogg(file: &[u8]) -> Result<Self> {
        let mut reader = file;
        let mut packet = Vec::new();
        let mut packets = Vec::new();
        while !reader.is_empty() {
            let header = reader.get(..27).ok_or("short Ogg page")?;
            if &header[..4] != b"OggS" {
                return Err("Ogg signature".into());
            }
            let count = usize::from(header[26]);
            let lacing = reader.get(27..27 + count).ok_or("lacing")?;
            reader = &reader[27 + count..];
            for &n in lacing {
                let n = usize::from(n);
                packet.extend_from_slice(reader.get(..n).ok_or("segment")?);
                reader = &reader[n..];
                if n < 255 {
                    packets.push(std::mem::take(&mut packet));
                }
            }
        }
        let head = packets.first().ok_or("missing header")?;
        if !head.starts_with(b"OpusHead")
            || !packets.get(1).is_some_and(|p| p.starts_with(b"OpusTags"))
            || !packet.is_empty()
        {
            return Err("unexpected fixture".into());
        }
        let channels = usize::from(*head.get(9).ok_or("channels")?);
        let fields = head.get(19..21 + channels).ok_or("channel mapping")?;
        let layout =
            ChannelMapping::new(usize::from(fields[0]), usize::from(fields[1]), &fields[2..])?;
        Ok(Self {
            layout,
            packets: packets.split_off(2),
        })
    }

    fn decode(&self, mut dump: Option<&mut fs::File>) -> Result<()> {
        let mut decoder = MultistreamDecoder::new(OutputRate::Khz48, self.layout.clone())?;
        for packet in &self.packets {
            let pcm = decoder.decode(black_box(packet))?;
            black_box((pcm.samples(), pcm.final_range()));
            if let Some(out) = dump.as_deref_mut() {
                out.write_all(&pcm.final_range().to_le_bytes())?;
                out.write_all(&u32::try_from(pcm.samples().len())?.to_le_bytes())?;
                for sample in pcm.samples() {
                    out.write_all(&sample.to_le_bytes())?;
                }
            }
        }
        Ok(())
    }
}

fn thread_cpu_ns() -> Result<u64> {
    Ok(fs::read_to_string("/proc/thread-self/schedstat")?
        .split_whitespace()
        .next()
        .ok_or("CPU time missing")?
        .parse()?)
}

fn main() -> Result<()> {
    let args: Vec<_> = env::args().collect();
    let clip = Clip::from_ogg(&fs::read(args.get(1).ok_or("fixture path missing")?)?)?;
    if let Some(dump) = args.get(2) {
        return clip.decode(Some(&mut fs::File::create(dump)?));
    }
    for _ in 0..8 {
        clip.decode(None)?;
    }
    let start = thread_cpu_ns()?;
    for _ in 0..128 {
        clip.decode(None)?;
    }
    println!("{}", thread_cpu_ns()? - start);
    Ok(())
}
