//! Optional matched public-API encoder benchmark; input and verification I/O are untimed.
use opura::{
    celt::input::InputRate,
    decoder::OpusDecoder,
    encoder::{
        EncodeOptions, OpusEncoder, PacketSize,
        control::{Application, Controls, Fec},
    },
    silk::{decoder::PcmChannels, resampler::OutputRate},
};
use std::{
    env,
    error::Error,
    fs::{File, OpenOptions},
    hint::black_box,
    io::{Read, Write},
    mem::size_of,
    path::Path,
    time::Instant,
};
type Result<T> = std::result::Result<T, Box<dyn Error>>;
const LIMIT: u64 = 64 * 1024 * 1024;
struct Input {
    header: [u8; 32],
    rate: InputRate,
    output: OutputRate,
    channels: usize,
    samples: usize,
    size: PacketSize,
    frames: usize,
    cap: usize,
    options: EncodeOptions,
    pcm: Vec<i16>,
}
fn u16_at(bytes: &[u8], i: usize) -> Result<u16> {
    Ok(u16::from_le_bytes(
        bytes.get(i..i + 2).ok_or("short integer")?.try_into()?,
    ))
}
fn u32_at(bytes: &[u8], i: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(
        bytes.get(i..i + 4).ok_or("short integer")?.try_into()?,
    ))
}
fn load(path: &Path) -> Result<Input> {
    let mut data = Vec::new();
    File::open(path)?.take(LIMIT + 1).read_to_end(&mut data)?;
    if data.len() < 32 || data.len() as u64 > LIMIT || &data[..8] != b"OPURAB01" {
        return Err("invalid bounded encoder corpus".into());
    }
    let header = data[..32].try_into()?;
    let (rate, output) = match u32_at(&data, 8)? {
        8000 => (InputRate::Khz8, OutputRate::Khz8),
        12000 => (InputRate::Khz12, OutputRate::Khz12),
        16000 => (InputRate::Khz16, OutputRate::Khz16),
        24000 => (InputRate::Khz24, OutputRate::Khz24),
        48000 => (InputRate::Khz48, OutputRate::Khz48),
        _ => return Err("invalid rate".into()),
    };
    let channels = usize::from(u16_at(&data, 12)?);
    let samples = usize::from(u16_at(&data, 14)?);
    let size = PacketSize::try_from(samples * rate.factor())?;
    let cap = usize::from(u16_at(&data, 20)?);
    let application = match data[22] {
        0 => Application::Audio,
        1 => Application::Voip,
        2 => Application::RestrictedLowDelay,
        3 => Application::RestrictedSilk,
        4 => Application::RestrictedCelt,
        _ => return Err("invalid application".into()),
    };
    let bitrate = u32_at(&data, 16)?;
    let frames = u32_at(&data, 28)? as usize;
    if !(1..=2).contains(&channels)
        || !(1..=100000).contains(&frames)
        || !(1..=1500000).contains(&bitrate)
        || !(1..=if size.samples() > 960 { 7656 } else { 1276 }).contains(&cap)
        || data[23] > 10
        || data[24] > 2
        || data[25] > 100
        || data[26] > 1
        || data[27] > 1
    {
        return Err("invalid benchmark configuration".into());
    }
    if data.len() != 32 + frames * samples * channels * 2 {
        return Err("PCM count mismatch".into());
    }
    let options = EncodeOptions {
        bitrate,
        constrained: data[24] == 2,
        controls: Controls {
            application,
            complexity: data[23],
            variable: data[24] != 0,
            loss_percent: data[25],
            fec: if data[26] == 0 {
                Fec::Disabled
            } else {
                Fec::Enabled
            },
            dtx: data[27] != 0,
            ..Controls::default()
        },
    };
    let pcm = data[32..]
        .chunks_exact(2)
        .map(|b| i16::from_le_bytes([b[0], b[1]]))
        .collect();
    Ok(Input {
        header,
        rate,
        output,
        channels,
        samples,
        size,
        frames,
        cap,
        options,
        pcm,
    })
}
fn fold(hash: &mut u64, bytes: &[u8]) {
    for &b in bytes {
        *hash = (*hash ^ u64::from(b)).wrapping_mul(1099511628211);
    }
}
fn pcm_hash(pcm: &[i16]) -> u64 {
    let mut hash = 14695981039346656037;
    for x in pcm {
        fold(&mut hash, &x.to_le_bytes());
    }
    hash
}
fn decoder(input: &Input) -> Result<OpusDecoder> {
    Ok(OpusDecoder::new(
        input.output,
        if input.channels == 1 {
            PcmChannels::Mono
        } else {
            PcmChannels::Stereo
        },
    )?)
}
fn record_header(input: &Input, lookahead: u32) -> Vec<u8> {
    let mut bytes = b"OPURAE01".to_vec();
    bytes.extend_from_slice(&input.header[8..16]);
    bytes.extend_from_slice(&(input.frames as u32).to_le_bytes());
    bytes.extend_from_slice(&lookahead.to_le_bytes());
    bytes
}
#[derive(Debug, Default, PartialEq, Eq)]
struct Stats {
    packets: u64,
    bytes: u64,
    fingerprint: u64,
    modes: [u64; 3],
}
fn replay(
    input: &Input,
    encoder: &mut OpusEncoder,
    mut record: Option<&mut File>,
    check: bool,
    mut times: Option<&mut Vec<u64>>,
) -> Result<Stats> {
    let mut stats = Stats {
        fingerprint: 14695981039346656037,
        ..Stats::default()
    };
    let mut decoded = if check { Some(decoder(input)?) } else { None };
    for pcm in input.pcm.chunks_exact(input.samples * input.channels) {
        let begin = times.as_ref().map(|_| Instant::now());
        let packet = encoder.encode_packet(input.size, black_box(pcm), input.cap, input.options)?;
        black_box(packet.as_bytes());
        let length = packet.as_bytes().len();
        if check {
            let decoded = decoded
                .as_mut()
                .ok_or("missing decoder")?
                .decode(packet.as_bytes())?;
            if decoded.samples_per_channel() != input.samples
                || decoded.final_range() != packet.final_range()
            {
                return Err("encoder/decoder range or duration mismatch".into());
            }
            let hash = pcm_hash(decoded.samples());
            let mut header = Vec::with_capacity(14);
            header.extend_from_slice(&(length as u16).to_le_bytes());
            header.extend_from_slice(&packet.final_range().to_le_bytes());
            header.extend_from_slice(&hash.to_le_bytes());
            fold(&mut stats.fingerprint, &header);
            fold(&mut stats.fingerprint, packet.as_bytes());
            if let Some(record) = record.as_deref_mut() {
                record.write_all(&header)?;
                record.write_all(packet.as_bytes())?;
            }
            let toc = packet.as_bytes()[0];
            stats.modes[if toc & 128 != 0 {
                2
            } else if toc & 96 == 96 {
                1
            } else {
                0
            }] += 1;
        }
        drop(packet);
        if let (Some(begin), Some(times)) = (begin, times.as_deref_mut()) {
            times.push(begin.elapsed().as_nanos().try_into()?);
        }
        stats.packets += 1;
        stats.bytes += length as u64;
    }
    Ok(stats)
}
fn verify(input: &Input, path: &Path) -> Result<()> {
    let mut file = File::open(path)?;
    let mut header = [0; 24];
    file.read_exact(&mut header)?;
    let encoder = OpusEncoder::with_application(
        input.rate,
        input.channels,
        input.options.controls.application,
    )?;
    let skip = encoder.lookahead_48k() / input.rate.factor() as u32;
    if header != record_header(input, skip).as_slice() {
        return Err("validation geometry/lookahead mismatch".into());
    }
    let mut decoder = decoder(input)?;
    for _ in 0..input.frames {
        let mut record = [0; 14];
        file.read_exact(&mut record)?;
        let length = usize::from(u16_at(&record, 0)?);
        if length == 0 || length > input.cap {
            return Err("invalid validation length".into());
        }
        let mut packet = vec![0; length];
        file.read_exact(&mut packet)?;
        let decoded = decoder.decode(&packet)?;
        let expected = u64::from_le_bytes(record[6..14].try_into()?);
        if decoded.samples_per_channel() != input.samples
            || decoded.final_range() != u32_at(&record, 2)?
            || pcm_hash(decoded.samples()) != expected
        {
            return Err("cross-decoder verification failed".into());
        }
    }
    if file.read(&mut [0])? != 0 {
        return Err("trailing validation data".into());
    }
    println!("{{\"verified_packets\":{}}}", input.frames);
    Ok(())
}
fn main() -> Result<()> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    if args.len() == 3 && args[0] == "verify" {
        let input = load(Path::new(&args[1]))?;
        return verify(&input, Path::new(&args[2]));
    }
    if args.len() != 4 {
        return Err(
            "usage: encoder_bench CORPUS RECORD_OR_DASH PASSES WARMUP; or verify CORPUS RECORD"
                .into(),
        );
    }
    let input = load(Path::new(&args[0]))?;
    let passes: usize = args[2].to_str().ok_or("passes")?.parse()?;
    let warmup: usize = args[3].to_str().ok_or("warmup")?.parse()?;
    if !(1..=1000).contains(&passes) || !(1..=100).contains(&warmup) {
        return Err("invalid pass/warmup count".into());
    }
    let begin = Instant::now();
    let mut encoder = OpusEncoder::with_application(
        input.rate,
        input.channels,
        input.options.controls.application,
    )?;
    let init_ns = begin.elapsed().as_nanos();
    let skip = encoder.lookahead_48k() / input.rate.factor() as u32;
    let mut record = if args[1] == "-" {
        None
    } else {
        Some(
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&args[1])?,
        )
    };
    if let Some(record) = record.as_mut() {
        record.write_all(&record_header(&input, skip))?;
    }
    let validation = replay(&input, &mut encoder, record.as_mut(), true, None)?;
    for _ in 0..warmup {
        encoder.reset()?;
        black_box(replay(&input, &mut encoder, None, false, None)?);
    }
    let mut elapsed = 0u128;
    for _ in 0..passes {
        encoder.reset()?;
        let begin = Instant::now();
        let stats = replay(&input, &mut encoder, None, false, None)?;
        elapsed += begin.elapsed().as_nanos();
        if stats.packets != validation.packets || stats.bytes != validation.bytes {
            return Err("timed pass changed packet/byte counts".into());
        }
    }
    let mut latency = Vec::with_capacity(input.frames);
    encoder.reset()?;
    replay(&input, &mut encoder, None, false, Some(&mut latency))?;
    encoder.reset()?;
    if replay(&input, &mut encoder, None, true, None)? != validation {
        return Err("post-timing validation changed output".into());
    }
    latency.sort_unstable();
    let percentile = |p: usize| latency[(latency.len() - 1) * p / 100];
    println!(
        "{{\"packets\":{},\"samples_per_pass\":{},\"packet_bytes\":{},\"fingerprint\":\"{:016x}\",\"encoder_object_bytes\":{},\"lookahead\":{},\"passes\":{},\"elapsed_ns\":{},\"init_ns\":{},\"latency_ns\":{{\"p50\":{},\"p95\":{},\"p99\":{},\"max\":{}}},\"mode_packets\":[{},{},{}]}}",
        validation.packets,
        input.frames * input.samples,
        validation.bytes,
        validation.fingerprint,
        size_of::<OpusEncoder>(),
        skip,
        passes,
        elapsed,
        init_ns,
        percentile(50),
        percentile(95),
        percentile(99),
        percentile(100),
        validation.modes[0],
        validation.modes[1],
        validation.modes[2]
    );
    Ok(())
}
