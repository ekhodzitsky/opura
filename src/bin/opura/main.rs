//! Native zero-dependency WAV/Ogg Opus command-line conversion.
//! Existing output files are never overwritten. I/O/decoding errors can leave
//! a newly created partial output, which must not be used.
mod ogg;
mod wav;
use opura::{
    Packet,
    celt::{FrameSize, encoder::CeltEncoder, input::InputRate},
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
    error::Error,
    fs::{File, OpenOptions},
    io::{self, BufReader, BufWriter},
    path::Path,
    process::ExitCode,
    time::Instant,
};
type Result<T> = std::result::Result<T, Box<dyn Error>>;
fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}
const HELP: &str = "opura encode INPUT.wav OUTPUT.opus [--mode celt|silk] [--bitrate BPS] [--frame-ms MS] [--bandwidth nb|mb|wb|auto] [--dtx] [--fec LOSS_PERCENT]\nopura decode INPUT.opus OUTPUT.wav\nopura --version\n\nEncode: PCM16 WAV at 8/12/16/24/48 kHz.\nCELT: mono/stereo, frames 2.5/5/10/20 ms. SILK: mono/stereo, frames 10/20/40/60 ms.\nDefaults: CELT, 64000 bits/s (SILK: 24000 mono/48000 stereo), 20 ms. SILK bandwidth defaults to input-rate NB/MB/WB; auto selects it from bitrate/loss. --dtx enables SILK silence suppression, including in CBR. --fec 1..100 requests SILK in-band FEC with the supplied loss estimate (0 disables it). Ogg overhead is additional.\nDecode: one mono/stereo Ogg Opus stream, zero header gain; output PCM16/48 kHz.\nExisting outputs are refused. Errors may leave a partial new output.";
fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("opura: {error}");
            ExitCode::FAILURE
        }
    }
}
fn run() -> Result<()> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    if args.is_empty() || args[0] == "--help" || args[0] == "-h" {
        println!("{HELP}");
        return Ok(());
    }
    if args.len() == 1 && args[0] == "--version" {
        println!("opura {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if args.len() < 3 {
        return Err(invalid(HELP).into());
    }
    match args[0].to_str() {
        Some("encode") => {
            let mut bitrate = None;
            let mut size = None;
            let mut mode = None;
            let mut bandwidth = None;
            let mut dtx = false;
            let mut fec = None;
            let mut i = 3;
            while i < args.len() {
                if args[i] == "--dtx" && !dtx {
                    dtx = true;
                    i += 1;
                    continue;
                }
                let value = args
                    .get(i + 1)
                    .and_then(|v| v.to_str())
                    .ok_or_else(|| invalid("option requires a value"))?;
                match args[i].to_str() {
                    Some("--fec") if fec.is_none() => {
                        let loss = value
                            .parse::<u8>()
                            .map_err(|_| invalid("fec loss must be integer0..100"))?;
                        if loss > 100 {
                            return Err(invalid("fec loss must be0..100").into());
                        }
                        fec = Some(loss);
                    }
                    Some("--bitrate") if bitrate.is_none() => {
                        bitrate = Some(
                            value
                                .parse::<u32>()
                                .map_err(|_| invalid("bitrate must be integer bits/s"))?,
                        )
                    }
                    Some("--frame-ms") if size.is_none() => {
                        if !["2.5", "5", "10", "20", "40", "60"].contains(&value) {
                            return Err(invalid("invalid frame-ms").into());
                        }
                        size = Some(value);
                    }
                    Some("--mode") if mode.is_none() => {
                        if !["celt", "silk"].contains(&value) {
                            return Err(invalid("mode must be celt/silk").into());
                        }
                        mode = Some(value);
                    }
                    Some("--bandwidth") if bandwidth.is_none() => {
                        bandwidth = Some(match value {
                            "nb" => SilkBandwidth::Fixed(SilkRate::Khz8),
                            "mb" => SilkBandwidth::Fixed(SilkRate::Khz12),
                            "wb" => SilkBandwidth::Fixed(SilkRate::Khz16),
                            "auto" => SilkBandwidth::Automatic,
                            _ => return Err(invalid("bandwidth must be nb/mb/wb/auto").into()),
                        });
                    }
                    _ => return Err(invalid("unknown or repeated encode option").into()),
                }
                i += 2;
            }
            if mode == Some("silk") {
                let duration = match size.unwrap_or("20") {
                    "10" => PacketDuration::Ms10,
                    "20" => PacketDuration::Ms20,
                    "40" => PacketDuration::Ms40,
                    "60" => PacketDuration::Ms60,
                    _ => return Err(invalid("SILK frame-ms must be 10/20/40/60").into()),
                };
                encode_silk(
                    Path::new(&args[1]),
                    Path::new(&args[2]),
                    bitrate,
                    duration,
                    bandwidth,
                    dtx,
                    fec.unwrap_or(0),
                )
            } else {
                if bandwidth.is_some() || dtx || fec.is_some() {
                    return Err(
                        invalid("bandwidth, dtx and fec are available with mode silk").into(),
                    );
                }
                let frame = match size.unwrap_or("20") {
                    "2.5" => FrameSize::Ms2_5,
                    "5" => FrameSize::Ms5,
                    "10" => FrameSize::Ms10,
                    "20" => FrameSize::Ms20,
                    _ => return Err(invalid("CELT frame-ms must be 2.5/5/10/20").into()),
                };
                encode(
                    Path::new(&args[1]),
                    Path::new(&args[2]),
                    bitrate.unwrap_or(64000),
                    frame,
                )
            }
        }
        Some("decode") if args.len() == 3 => decode(Path::new(&args[1]), Path::new(&args[2])),
        _ => Err(invalid(HELP).into()),
    }
}
fn create(path: &Path) -> io::Result<File> {
    OpenOptions::new().write(true).create_new(true).open(path)
}
fn encode(input: &Path, output: &Path, bitrate: u32, size: FrameSize) -> Result<()> {
    let start = Instant::now();
    let mut source = wav::Reader::new(BufReader::new(File::open(input)?))?;
    let rate = match source.rate {
        8000 => InputRate::Khz8,
        12000 => InputRate::Khz12,
        16000 => InputRate::Khz16,
        24000 => InputRate::Khz24,
        48000 => InputRate::Khz48,
        _ => return Err(invalid("unsupported input rate").into()),
    };
    let numerator = u64::from(bitrate) * size.samples() as u64;
    let denominator = 48000 * 8;
    if numerator / denominator < 3 || numerator.div_ceil(denominator) > 1276 {
        return Err(invalid(
            "bitrate/frame combination requires packet size outside 3..1276 bytes",
        )
        .into());
    }
    let mut encoder = CeltEncoder::new(rate, source.channels)?;
    encoder.set_transient_detection(true);
    encoder.set_tf_analysis(true);
    encoder.set_dynamic_allocation(true);
    encoder.set_spreading_analysis(true);
    encoder.set_allocation_analysis(true);
    encoder.set_pitch_prefilter(true);
    let original_samples = source.frames * rate.factor() as u64;
    let end = original_samples
        .checked_add(120)
        .ok_or_else(|| invalid("audio duration overflow"))?;
    let frames = end.div_ceil(size.samples() as u64);
    let mut writer = ogg::Writer::new(
        BufWriter::new(create(output)?),
        source.channels,
        source.rate,
    )?;
    let mut pcm = [0i16; 1920];
    let length = size.samples() / rate.factor() * source.channels;
    let mut credit = denominator / 2;
    let mut packet_bytes = 0u64;
    for index in 0..frames {
        pcm.fill(0);
        source.read_samples(&mut pcm[..length])?;
        credit += numerator;
        let bytes = credit / denominator;
        credit %= denominator;
        let packet = encoder.encode(size, &pcm[..length], bytes as usize - 1)?;
        let final_frame = index + 1 == frames;
        let granule = if final_frame {
            end
        } else {
            (index + 1) * size.samples() as u64
        };
        writer.packet(packet.as_bytes(), granule, final_frame)?;
        packet_bytes += packet.as_bytes().len() as u64;
    }
    eprintln!(
        "encoded {} samples/channel at {} Hz, {} channels; {frames} packets, {packet_bytes} packet bytes; {:.3} s",
        source.frames,
        source.rate,
        source.channels,
        start.elapsed().as_secs_f64()
    );
    Ok(())
}
#[derive(Clone, Copy)]
enum SilkBandwidth {
    Fixed(SilkRate),
    Automatic,
}
enum SilkFileEncoder {
    Mono(Box<MonoEncoder>),
    Stereo(Box<StereoEncoder>),
}
impl SilkFileEncoder {
    fn lookahead_48k(&self) -> u32 {
        match self {
            Self::Mono(e) => e.lookahead_48k(),
            Self::Stereo(e) => e.lookahead_48k(),
        }
    }
}
fn encode_silk(
    input: &Path,
    output: &Path,
    bitrate: Option<u32>,
    duration: PacketDuration,
    bandwidth: Option<SilkBandwidth>,
    dtx: bool,
    fec_loss: u8,
) -> Result<()> {
    let start = Instant::now();
    let mut source = wav::Reader::new(BufReader::new(File::open(input)?))?;
    let bitrate = bitrate.unwrap_or(if source.channels == 2 { 48000 } else { 24000 });
    let rate = match source.rate {
        8000 => OutputRate::Khz8,
        12000 => OutputRate::Khz12,
        16000 => OutputRate::Khz16,
        24000 => OutputRate::Khz24,
        48000 => OutputRate::Khz48,
        _ => return Err(invalid("unsupported input rate").into()),
    };
    let automatic = matches!(bandwidth, Some(SilkBandwidth::Automatic));
    let bandwidth = match bandwidth {
        Some(SilkBandwidth::Fixed(rate)) => rate,
        _ => match source.rate {
            8000 => SilkRate::Khz8,
            12000 => SilkRate::Khz12,
            _ => SilkRate::Khz16,
        },
    };
    let numerator = u64::from(bitrate) * u64::from(duration.milliseconds());
    let denominator = 8000;
    if numerator / denominator < 3 || numerator.div_ceil(denominator) > 1276 {
        return Err(invalid(
            "bitrate/frame combination requires packet size outside 3..1276 bytes",
        )
        .into());
    }
    let mut encoder = if source.channels == 1 {
        SilkFileEncoder::Mono(Box::new(MonoEncoder::new(rate, bandwidth)))
    } else {
        SilkFileEncoder::Stereo(Box::new(StereoEncoder::new(rate, bandwidth)))
    };
    match &mut encoder {
        SilkFileEncoder::Mono(e) => {
            e.set_automatic_bandwidth(automatic);
            e.set_dtx(dtx);
            e.set_inband_fec(fec_loss > 0);
        }
        SilkFileEncoder::Stereo(e) => {
            e.set_automatic_bandwidth(automatic);
            e.set_dtx(dtx);
            e.set_inband_fec(fec_loss > 0);
        }
    }
    let lookahead = u64::from(encoder.lookahead_48k());
    let original_samples = source.frames * u64::from(48000 / source.rate);
    let end = original_samples
        .checked_add(lookahead)
        .ok_or_else(|| invalid("audio duration overflow"))?;
    let frame_samples = u64::from(duration.milliseconds()) * 48;
    let frames = end.div_ceil(frame_samples);
    let mut writer = ogg::Writer::with_pre_skip(
        BufWriter::new(create(output)?),
        source.channels,
        source.rate,
        lookahead as u16,
    )?;
    let mut pcm = [0i16; 5760];
    let length = source.rate as usize / 1000 * duration.milliseconds() as usize * source.channels;
    let mut credit = denominator / 2;
    let mut packet_bytes = 0u64;
    let mut plc_packets = 0u64;
    for index in 0..frames {
        pcm.fill(0);
        source.read_samples(&mut pcm[..length])?;
        credit += numerator;
        let bytes = credit / denominator;
        credit %= denominator;
        let final_frame = index + 1 == frames;
        let granule = if final_frame {
            end
        } else {
            (index + 1) * frame_samples
        };
        let written = match &mut encoder {
            SilkFileEncoder::Mono(encoder) => {
                let packet = encoder.encode(
                    duration,
                    &pcm[..length],
                    bytes as usize,
                    MonoOptions {
                        packet_loss_percent: fec_loss,
                        ..MonoOptions::default()
                    },
                )?;
                plc_packets += u64::from(packet.used_plc());
                writer.packet(packet.as_bytes(), granule, final_frame)?;
                packet.as_bytes().len()
            }
            SilkFileEncoder::Stereo(encoder) => {
                let packet = encoder.encode(
                    duration,
                    &pcm[..length],
                    bytes as usize,
                    MonoOptions {
                        packet_loss_percent: fec_loss,
                        ..MonoOptions::default()
                    },
                )?;
                plc_packets += u64::from(packet.used_plc());
                writer.packet(packet.as_bytes(), granule, final_frame)?;
                packet.as_bytes().len()
            }
        };
        packet_bytes += written as u64;
    }
    eprintln!(
        "encoded SILK {} samples/channel at {} Hz, {} channels; {frames} packets, {packet_bytes} packet bytes, {plc_packets} PLC fallback packets; {:.3} s",
        source.frames,
        source.rate,
        source.channels,
        start.elapsed().as_secs_f64()
    );
    Ok(())
}
struct Head {
    channels: usize,
    skip: u64,
}
fn head(packet: &[u8]) -> io::Result<Head> {
    if packet.len() < 19 || &packet[..8] != b"OpusHead" || packet[8] > 15 {
        return Err(invalid("invalid/unsupported OpusHead"));
    }
    let channels = usize::from(packet[9]);
    if !(1..=2).contains(&channels) || packet[18] != 0 {
        return Err(invalid(
            "only mono/stereo Opus mapping family zero is supported",
        ));
    }
    if packet[16..18] != [0, 0] {
        return Err(invalid("nonzero OpusHead output gain is not supported"));
    }
    Ok(Head {
        channels,
        skip: u64::from(u16::from_le_bytes([packet[10], packet[11]])),
    })
}
fn tags(packet: &[u8]) -> io::Result<()> {
    if packet.len() < 16 || &packet[..8] != b"OpusTags" {
        return Err(invalid("invalid OpusTags"));
    }
    let mut position = 8;
    fn length(packet: &[u8], position: &mut usize) -> io::Result<usize> {
        let end = position
            .checked_add(4)
            .filter(|&e| e <= packet.len())
            .ok_or_else(|| invalid("truncated OpusTags length"))?;
        let value = u32::from_le_bytes(
            packet[*position..end]
                .try_into()
                .map_err(|_| invalid("OpusTags length"))?,
        );
        *position = end;
        Ok(value as usize)
    }
    let vendor = length(packet, &mut position)?;
    position = position
        .checked_add(vendor)
        .filter(|&p| p <= packet.len())
        .ok_or_else(|| invalid("truncated OpusTags vendor"))?;
    let comments = length(packet, &mut position)?;
    if comments > (packet.len() - position) / 4 {
        return Err(invalid("truncated OpusTags comments"));
    }
    for _ in 0..comments {
        let size = length(packet, &mut position)?;
        position = position
            .checked_add(size)
            .filter(|&p| p <= packet.len())
            .ok_or_else(|| invalid("truncated OpusTags comment"))?;
    }
    Ok(())
}
fn decode(input: &Path, output: &Path) -> Result<()> {
    let start = Instant::now();
    let mut source = ogg::Reader::new(BufReader::new(File::open(input)?));
    let first = source.page()?.ok_or_else(|| invalid("OpusHead absent"))?;
    if first.eos || first.unfinished || first.granule != Some(0) || first.packets.len() != 1 {
        return Err(invalid("OpusHead must occupy its own first page").into());
    }
    let header = head(&first.packets[0])?;
    loop {
        let page = source.page()?.ok_or_else(|| invalid("OpusTags absent"))?;
        if page.eos || page.packets.len() > 1 || page.granule.is_some_and(|g| g != 0) {
            return Err(invalid("invalid OpusTags page").into());
        }
        if let Some(packet) = page.packets.first() {
            if page.unfinished || page.granule != Some(0) {
                return Err(invalid("OpusTags must end on its own granule-zero page").into());
            }
            tags(packet)?;
            break;
        }
    }
    let channels = if header.channels == 1 {
        PcmChannels::Mono
    } else {
        PcmChannels::Stereo
    };
    let mut decoder = OpusDecoder::new(OutputRate::Khz48, channels)?;
    let mut writer = wav::Writer::new(BufWriter::new(create(output)?), 48000, header.channels)?;
    let mut decoded = 0u64;
    let mut written = 0u64;
    let mut packets = 0u64;
    let mut initial = None;
    let mut eos = false;
    while let Some(page) = source.page()? {
        if page.packets.is_empty() {
            continue;
        }
        let granule = page
            .granule
            .ok_or_else(|| invalid("audio page granule absent"))?;
        let mut samples = 0u64;
        for packet in &page.packets {
            let parsed = Packet::parse(packet)?;
            samples += u64::from(parsed.frame_samples()) * parsed.frames().len() as u64;
        }
        let base = match initial {
            Some(base) => base,
            None => {
                let base = if granule >= samples {
                    granule - samples
                } else if page.eos && granule >= header.skip {
                    0
                } else {
                    return Err(invalid("invalid initial Opus granule").into());
                };
                initial = Some(base);
                base
            }
        };
        let end = granule
            .checked_sub(base)
            .ok_or_else(|| invalid("Opus granule precedes stream origin"))?;
        let expected = decoded
            .checked_add(samples)
            .ok_or_else(|| invalid("Opus duration overflow"))?;
        if (!page.eos && end != expected)
            || (page.eos && (end < decoded || end > expected || end < header.skip))
        {
            return Err(invalid("inconsistent Opus granule/sample duration").into());
        }
        for packet in &page.packets {
            let pcm = decoder
                .decode(packet)
                .map_err(|e| invalid(format!("packet {packets}: {e}")))?;
            let after = decoded + pcm.samples_per_channel() as u64;
            let begin = header.skip.max(decoded);
            let stop = if page.eos { end.min(after) } else { after };
            if begin < stop {
                let offset = ((begin - decoded) as usize) * header.channels;
                let length = ((stop - begin) as usize) * header.channels;
                writer.samples(&pcm.samples()[offset..offset + length])?;
                written += stop - begin;
            }
            decoded = after;
            packets += 1;
        }
        if decoded != expected {
            return Err(invalid("decoder returned inconsistent packet duration").into());
        }
        eos = page.eos;
    }
    if !eos || initial.is_none() || decoded < header.skip {
        return Err(invalid("incomplete Opus audio stream").into());
    }
    writer.finish()?;
    eprintln!(
        "decoded {written} samples/channel at 48000 Hz, {} channels; {packets} packets; {:.3} s",
        header.channels,
        start.elapsed().as_secs_f64()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn metadata_bounds_and_unsupported_interpretation_are_explicit() {
        let mut packet = b"OpusHead\x01\x02\x78\x00\x80\xbb\x00\x00\x00\x00\x00".to_vec();
        assert!(head(&packet).is_ok());
        for length in 0..19 {
            assert!(head(&packet[..length]).is_err());
        }
        for (index, value) in [(8, 16), (9, 0), (9, 3), (16, 1), (18, 1)] {
            let mut invalid = packet.clone();
            invalid[index] = value;
            assert!(head(&invalid).is_err());
        }
        packet[8] = 15;
        assert!(head(&packet).is_ok());
        assert!(tags(b"OpusTags\0\0\0\0\0\0\0\0").is_ok());
        assert!(tags(b"OpusTags\xff\xff\xff\xff\0\0\0\0").is_err());
        assert!(tags(b"OpusTags\0\0\0\0\xff\xff\xff\xff").is_err());
        assert!(tags(b"OpusTags\0\0\0\0\x01\0\0\0\x05\0\0\0a").is_err());
    }
}
