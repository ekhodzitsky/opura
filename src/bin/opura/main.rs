//! Native zero-dependency WAV/Ogg Opus command-line conversion.
//! Existing output files are never overwritten. I/O/decoding errors can leave
//! a newly created partial output, which must not be used.
mod automatic;
mod grouping;
mod layout;
mod ogg;
mod wav;
use opura::{
    Bandwidth, Packet,
    celt::{
        FrameSize,
        encoder::{CeltEncoder, VbrOptions},
        input::InputRate,
    },
    decoder::OpusDecoder,
    encoder::control::{Application, Signal},
    hybrid::encoder::{HybridEncoder, Options as HybridOptions},
    multistream::{MultistreamDecoder, MultistreamPacket, MultistreamPcm},
    silk::{
        decoder::PcmChannels,
        encoder::{
            mono::{MonoEncoder, MonoOptions, PacketDuration},
            stereo::StereoEncoder,
        },
        gains::FrameDuration,
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
const HELP: &str = "opura encode INPUT.wav OUTPUT.opus [OPTIONS]
opura decode INPUT.opus OUTPUT.wav
opura --version

Encode PCM16 WAV at 8, 12, 16, 24 or 48 kHz. Choose a mode with --mode:
  celt     Positioned mono/stereo, 2.5/5/10/20 ms frames (default mode).
  silk     Positioned mono/stereo, 10/20/40/60 ms frames.
  hybrid   Positioned mono/stereo at 24 or 48 kHz, 10/20 ms frames.
           24 kHz supports SWB. 48 kHz supports SWB and FB.
  auto     1–8 channels, automatic mode selection, packets from 2.5 to 120 ms.
           Canonical WAVE speaker masks select surround encoding with LFE.
           Unpositioned PCM uses mapping family 255. Positioned multichannel
           input preserves coupled stereo and rejects --channels 1.

Encoding options (availability depends on mode):
  --application audio|voip|lowdelay|restricted-silk|restricted-celt
  --signal auto|voice|music     --complexity 0..10      --channels 1|2
  --bitrate BPS                 --frame-ms MS           --packet-ms MS
  --bandwidth nb|mb|wb|auto|swb|fb
  --vbr | --cvbr                --max-packet-bytes BYTES
  --dtx                         --fec LOSS_PERCENT

Frames default to 20 ms. CELT defaults to 64000 bits/s. SILK defaults to
24000/48000 bits/s for mono/stereo and Hybrid to 32000/64000. SILK bandwidth
defaults to NB/MB/WB by input rate. In SILK mode, --bandwidth auto also uses
bitrate and expected loss. DTX suppresses silence. FEC requests SILK-band
protection with a 1–100% loss estimate (0 disables it).

CBR is the default. --vbr selects variable bitrate and --cvbr constrains CELT
rate variation. Bitrate includes Opus packet framing and excludes Ogg overhead.
For fixed modes, --packet-ms groups compatible frames up to 120 ms. The group
can end early at EOF or a bandwidth change. The default variable-rate cap is
1276 bytes per component frame. In auto mode, --packet-ms schedules one
complete packet. A very small cap can signal loss. A 100 ms packet needs at
least two bytes. Mapped packet caps cover all streams together.

Decode one Ogg Opus logical stream (1–8 channels, mapping families 0/1/255)
to 48 kHz PCM16 WAV with header gain and speaker metadata.
Use new output paths. A failed conversion can leave a partial output file.";
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
            let mut application = None;
            let mut signal = None;
            let mut complexity = None;
            let mut wire_channels = None;
            let mut dtx = false;
            let mut fec = None;
            let mut vbr = false;
            let mut constrained = false;
            let mut max_packet_bytes = None;
            let mut packet_samples = None;
            let mut i = 3;
            while i < args.len() {
                if args[i] == "--cvbr" && !vbr {
                    vbr = true;
                    constrained = true;
                    i += 1;
                    continue;
                }
                if args[i] == "--vbr" && !vbr {
                    vbr = true;
                    i += 1;
                    continue;
                }
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
                    Some("--application") if application.is_none() => {
                        application = Some(match value {
                            "audio" => Application::Audio,
                            "voip" => Application::Voip,
                            "lowdelay" => Application::RestrictedLowDelay,
                            "restricted-silk" => Application::RestrictedSilk,
                            "restricted-celt" => Application::RestrictedCelt,
                            _ => return Err(invalid("invalid application").into()),
                        });
                    }
                    Some("--signal") if signal.is_none() => {
                        signal = Some(match value {
                            "auto" => Signal::Auto,
                            "voice" => Signal::Voice,
                            "music" => Signal::Music,
                            _ => return Err(invalid("signal must be auto/voice/music").into()),
                        });
                    }
                    Some("--complexity") if complexity.is_none() => {
                        let parsed = value
                            .parse::<u8>()
                            .map_err(|_| invalid("complexity must be0..10"))?;
                        if parsed > 10 {
                            return Err(invalid("complexity must be0..10").into());
                        }
                        complexity = Some(parsed);
                    }
                    Some("--channels") if wire_channels.is_none() => {
                        let parsed = value
                            .parse::<usize>()
                            .map_err(|_| invalid("encoded channels must be1 or2"))?;
                        if !(1..=2).contains(&parsed) {
                            return Err(invalid("encoded channels must be1 or2").into());
                        }
                        wire_channels = Some(parsed);
                    }
                    Some("--packet-ms") if packet_samples.is_none() => {
                        packet_samples = Some(grouping::duration(value)?);
                    }
                    Some("--max-packet-bytes") if max_packet_bytes.is_none() => {
                        let bytes = value
                            .parse::<usize>()
                            .map_err(|_| invalid("max-packet-bytes must be1..16777216"))?;
                        if !(1..=opura::multistream::MAX_PACKET_BYTES).contains(&bytes) {
                            return Err(invalid("max-packet-bytes must be1..16777216").into());
                        }
                        max_packet_bytes = Some(bytes);
                    }
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
                        if !["2.5", "5", "10", "20", "40", "60", "80", "100", "120"]
                            .contains(&value)
                        {
                            return Err(invalid("invalid frame-ms").into());
                        }
                        size = Some(value);
                    }
                    Some("--mode") if mode.is_none() => {
                        if !["celt", "silk", "hybrid", "auto"].contains(&value) {
                            return Err(invalid("mode must be celt/silk/hybrid/auto").into());
                        }
                        mode = Some(value);
                    }
                    Some("--bandwidth") if bandwidth.is_none() => {
                        bandwidth = Some(match value {
                            "nb" => EncodingBandwidth::Fixed(SilkRate::Khz8),
                            "mb" => EncodingBandwidth::Fixed(SilkRate::Khz12),
                            "wb" => EncodingBandwidth::Fixed(SilkRate::Khz16),
                            "auto" => EncodingBandwidth::Automatic,
                            "swb" => EncodingBandwidth::Hybrid(Bandwidth::SuperWideband),
                            "fb" => EncodingBandwidth::Hybrid(Bandwidth::Fullband),
                            _ => {
                                return Err(
                                    invalid("bandwidth must be nb/mb/wb/auto/swb/fb").into()
                                );
                            }
                        });
                    }
                    _ => return Err(invalid("unknown or repeated encode option").into()),
                }
                i += 2;
            }
            if mode != Some("auto") && max_packet_bytes.is_some_and(|n| n < 3) {
                return Err(invalid("max-packet-bytes must be3..61248").into());
            }
            if mode != Some("auto")
                && (application.is_some()
                    || signal.is_some()
                    || complexity.is_some()
                    || wire_channels.is_some())
            {
                return Err(
                    invalid("application/signal/complexity/channels require mode auto").into(),
                );
            }
            if (vbr || max_packet_bytes.is_some()) && mode == Some("silk") {
                return Err(
                    invalid("vbr/cvbr/max-packet-bytes require mode celt or hybrid").into(),
                );
            }
            if constrained && mode == Some("hybrid") {
                return Err(invalid("cvbr requires mode celt").into());
            }
            if max_packet_bytes.is_some() && !vbr && mode != Some("auto") {
                return Err(invalid("max-packet-bytes requires vbr").into());
            }
            let rate_control = FileRate {
                bitrate,
                vbr,
                constrained,
                maximum: max_packet_bytes,
                packet_samples,
            };
            if mode == Some("auto") {
                let frame = match size.unwrap_or("20") {
                    "2.5" => opura::encoder::PacketSize::Ms2_5,
                    "5" => opura::encoder::PacketSize::Ms5,
                    "10" => opura::encoder::PacketSize::Ms10,
                    "20" => opura::encoder::PacketSize::Ms20,
                    "40" => opura::encoder::PacketSize::Ms40,
                    "60" => opura::encoder::PacketSize::Ms60,
                    "80" => opura::encoder::PacketSize::Ms80,
                    "100" => opura::encoder::PacketSize::Ms100,
                    "120" => opura::encoder::PacketSize::Ms120,
                    _ => {
                        return Err(
                            invalid("auto frame-ms must be2.5/5/10/20/40/60/80/100/120").into()
                        );
                    }
                };
                let bandwidth = bandwidth.and_then(|b| match b {
                    EncodingBandwidth::Automatic => None,
                    EncodingBandwidth::Fixed(SilkRate::Khz8) => Some(Bandwidth::Narrowband),
                    EncodingBandwidth::Fixed(SilkRate::Khz12) => Some(Bandwidth::Mediumband),
                    EncodingBandwidth::Fixed(SilkRate::Khz16) => Some(Bandwidth::Wideband),
                    EncodingBandwidth::Hybrid(b) => Some(b),
                });
                automatic::encode(
                    Path::new(&args[1]),
                    Path::new(&args[2]),
                    rate_control,
                    automatic::Options {
                        size: frame,
                        application: application.unwrap_or(Application::Audio),
                        signal: signal.unwrap_or(Signal::Auto),
                        complexity: complexity.unwrap_or(9),
                        channels: wire_channels,
                        bandwidth,
                        dtx,
                        loss: fec.unwrap_or(0),
                    },
                )
            } else if mode == Some("silk") {
                if matches!(bandwidth, Some(EncodingBandwidth::Hybrid(_))) {
                    return Err(invalid("SILK bandwidth must be nb/mb/wb/auto").into());
                }
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
                    rate_control,
                    duration,
                    bandwidth,
                    dtx,
                    fec.unwrap_or(0),
                )
            } else if mode == Some("hybrid") {
                let bandwidth = match bandwidth {
                    Some(EncodingBandwidth::Hybrid(bandwidth)) => Some(bandwidth),
                    None => None,
                    _ => return Err(invalid("Hybrid bandwidth must be swb/fb").into()),
                };
                let duration = match size.unwrap_or("20") {
                    "10" => FrameDuration::Ms10,
                    "20" => FrameDuration::Ms20,
                    _ => return Err(invalid("Hybrid frame-ms must be 10/20").into()),
                };
                encode_hybrid(
                    Path::new(&args[1]),
                    Path::new(&args[2]),
                    rate_control,
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
                    rate_control,
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
fn encode(input: &Path, output: &Path, rate_control: FileRate, size: FrameSize) -> Result<()> {
    let bitrate = rate_control.bitrate.unwrap_or(64000);
    let start = Instant::now();
    let mut source = wav::Reader::new(BufReader::new(File::open(input)?))?;
    if source.channels > 2 || !source.positioned {
        return Err(invalid("mapped or unpositioned WAVE input requires mode auto").into());
    }
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
    if if rate_control.vbr {
        bitrate < (24 * 48000 / size.samples()) as u32 || bitrate > 1500000
    } else {
        numerator / denominator < 3 || numerator.div_ceil(denominator) > 1276
    } {
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
    let plan = grouping::Plan::new(
        size.samples() as u16,
        rate_control.packet_samples,
        bitrate,
        rate_control.vbr,
        rate_control.maximum,
    )?;
    let mut writer = grouping::Writer::new(
        ogg::Writer::new(
            BufWriter::new(create(output)?),
            source.channels,
            source.rate,
        )?,
        plan,
    );
    let mut pcm = [0i16; 1920];
    let length = size.samples() / rate.factor() * source.channels;
    let mut credit = denominator / 2;
    for index in 0..frames {
        pcm.fill(0);
        source.read_samples(&mut pcm[..length])?;
        credit += numerator;
        let bytes = credit / denominator;
        credit %= denominator;
        let bytes = writer.component_bytes(bytes as usize);
        let packet = if rate_control.vbr {
            encoder.encode_vbr(
                size,
                &pcm[..length],
                VbrOptions {
                    bitrate,
                    constrained: rate_control.constrained,
                    payload_bytes: writer.component_bytes(0) - 1,
                },
            )?
        } else {
            encoder.encode(size, &pcm[..length], bytes - 1)?
        };
        let final_frame = index + 1 == frames;
        let granule = if final_frame {
            end
        } else {
            (index + 1) * size.samples() as u64
        };
        writer.packet(packet.as_bytes(), granule, final_frame)?;
    }
    let packet_bytes = writer.packet_bytes();
    let packets = writer.packet_count();
    eprintln!(
        "encoded {} samples/channel at {} Hz, {} channels; {packets} packets, {packet_bytes} packet bytes; {:.3} s",
        source.frames,
        source.rate,
        source.channels,
        start.elapsed().as_secs_f64()
    );
    Ok(())
}
#[derive(Clone, Copy)]
enum EncodingBandwidth {
    Fixed(SilkRate),
    Automatic,
    Hybrid(Bandwidth),
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
    rate_control: FileRate,
    duration: PacketDuration,
    bandwidth: Option<EncodingBandwidth>,
    dtx: bool,
    fec_loss: u8,
) -> Result<()> {
    let start = Instant::now();
    let mut source = wav::Reader::new(BufReader::new(File::open(input)?))?;
    if source.channels > 2 || !source.positioned {
        return Err(invalid("mapped or unpositioned WAVE input requires mode auto").into());
    }
    let bitrate = rate_control
        .bitrate
        .unwrap_or(if source.channels == 2 { 48000 } else { 24000 });
    let rate = match source.rate {
        8000 => OutputRate::Khz8,
        12000 => OutputRate::Khz12,
        16000 => OutputRate::Khz16,
        24000 => OutputRate::Khz24,
        48000 => OutputRate::Khz48,
        _ => return Err(invalid("unsupported input rate").into()),
    };
    let automatic = matches!(bandwidth, Some(EncodingBandwidth::Automatic));
    let bandwidth = match bandwidth {
        Some(EncodingBandwidth::Fixed(rate)) => rate,
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
    let plan = grouping::Plan::new(
        frame_samples as u16,
        rate_control.packet_samples,
        bitrate,
        rate_control.vbr,
        rate_control.maximum,
    )?;
    let mut writer = grouping::Writer::new(
        ogg::Writer::with_pre_skip(
            BufWriter::new(create(output)?),
            source.channels,
            source.rate,
            lookahead as u16,
        )?,
        plan,
    );
    let mut pcm = [0i16; 5760];
    let length = source.rate as usize / 1000 * duration.milliseconds() as usize * source.channels;
    let mut credit = denominator / 2;
    let mut plc_packets = 0u64;
    for index in 0..frames {
        pcm.fill(0);
        source.read_samples(&mut pcm[..length])?;
        credit += numerator;
        let bytes = credit / denominator;
        credit %= denominator;
        let bytes = writer.component_bytes(bytes as usize);
        let final_frame = index + 1 == frames;
        let granule = if final_frame {
            end
        } else {
            (index + 1) * frame_samples
        };
        match &mut encoder {
            SilkFileEncoder::Mono(encoder) => {
                let packet = encoder.encode(
                    duration,
                    &pcm[..length],
                    bytes,
                    MonoOptions {
                        packet_loss_percent: fec_loss,
                        ..MonoOptions::default()
                    },
                )?;
                plc_packets += u64::from(packet.used_plc());
                writer.packet(packet.as_bytes(), granule, final_frame)?;
            }
            SilkFileEncoder::Stereo(encoder) => {
                let packet = encoder.encode(
                    duration,
                    &pcm[..length],
                    bytes,
                    MonoOptions {
                        packet_loss_percent: fec_loss,
                        ..MonoOptions::default()
                    },
                )?;
                plc_packets += u64::from(packet.used_plc());
                writer.packet(packet.as_bytes(), granule, final_frame)?;
            }
        };
    }
    let packet_bytes = writer.packet_bytes();
    let packets = writer.packet_count();
    let plc_unit = if rate_control
        .packet_samples
        .is_some_and(|s| u64::from(s) > frame_samples)
    {
        "frames"
    } else {
        "packets"
    };
    eprintln!(
        "encoded SILK {} samples/channel at {} Hz, {} channels; {packets} packets, {packet_bytes} packet bytes, {plc_packets} PLC fallback {plc_unit}; {:.3} s",
        source.frames,
        source.rate,
        source.channels,
        start.elapsed().as_secs_f64()
    );
    Ok(())
}
struct FileRate {
    bitrate: Option<u32>,
    vbr: bool,
    constrained: bool,
    maximum: Option<usize>,
    packet_samples: Option<u16>,
}
fn encode_hybrid(
    input: &Path,
    output: &Path,
    rate_control: FileRate,
    duration: FrameDuration,
    bandwidth: Option<Bandwidth>,
    dtx: bool,
    fec_loss: u8,
) -> Result<()> {
    let start = Instant::now();
    let mut source = wav::Reader::new(BufReader::new(File::open(input)?))?;
    if source.channels > 2 || !source.positioned {
        return Err(invalid("mapped or unpositioned WAVE input requires mode auto").into());
    }
    let rate = match source.rate {
        24000 => InputRate::Khz24,
        48000 => InputRate::Khz48,
        _ => return Err(invalid("Hybrid input rate must be 24000 or 48000").into()),
    };
    let bandwidth = bandwidth.unwrap_or(if rate == InputRate::Khz24 {
        Bandwidth::SuperWideband
    } else {
        Bandwidth::Fullband
    });
    let mut encoder = HybridEncoder::new(rate, source.channels, bandwidth)?;
    encoder.set_dtx(dtx);
    encoder.set_inband_fec(fec_loss > 0);
    let bitrate = rate_control
        .bitrate
        .unwrap_or(if source.channels == 1 { 32000 } else { 64000 });
    let ms = if duration == FrameDuration::Ms10 {
        10u64
    } else {
        20
    };
    let numerator = u64::from(bitrate) * ms;
    let denominator = 8000;
    if if rate_control.vbr {
        bitrate < (24000 / ms) as u32 || bitrate > 1024000
    } else {
        numerator / denominator < 3 || numerator.div_ceil(denominator) > 1276
    } {
        return Err(invalid(
            "bitrate/frame combination requires packet size outside 3..1276 bytes",
        )
        .into());
    }
    let lookahead = u64::from(encoder.lookahead_48k());
    let original_samples = source.frames * u64::from(48000 / source.rate);
    let end = original_samples
        .checked_add(lookahead)
        .ok_or_else(|| invalid("audio duration overflow"))?;
    let frame_samples = ms * 48;
    let frames = end.div_ceil(frame_samples);
    let plan = grouping::Plan::new(
        frame_samples as u16,
        rate_control.packet_samples,
        bitrate,
        rate_control.vbr,
        rate_control.maximum,
    )?;
    let mut writer = grouping::Writer::new(
        ogg::Writer::with_pre_skip(
            BufWriter::new(create(output)?),
            source.channels,
            source.rate,
            lookahead as u16,
        )?,
        plan,
    );
    let mut pcm = [0i16; 1920];
    let length = source.rate as usize / 1000 * ms as usize * source.channels;
    let mut credit = denominator / 2;
    for index in 0..frames {
        pcm.fill(0);
        source.read_samples(&mut pcm[..length])?;
        credit += numerator;
        let bytes = credit / denominator;
        credit %= denominator;
        let bytes = writer.component_bytes(bytes as usize);
        let options = HybridOptions {
            packet_loss_percent: fec_loss,
            ..HybridOptions::default()
        };
        let packet = if rate_control.vbr {
            encoder.encode_vbr(
                duration,
                &pcm[..length],
                bitrate,
                writer.component_bytes(0),
                options,
            )?
        } else {
            encoder.encode(duration, &pcm[..length], bytes, options)?
        };
        let final_frame = index + 1 == frames;
        let granule = if final_frame {
            end
        } else {
            (index + 1) * frame_samples
        };
        writer.packet(packet.as_bytes(), granule, final_frame)?;
    }
    let packet_bytes = writer.packet_bytes();
    let packets = writer.packet_count();
    eprintln!(
        "encoded Hybrid {} samples/channel at {} Hz, {} channels; {packets} packets, {packet_bytes} packet bytes; {:.3} s",
        source.frames,
        source.rate,
        source.channels,
        start.elapsed().as_secs_f64()
    );
    Ok(())
}
fn head(packet: &[u8]) -> io::Result<layout::Header> {
    layout::parse(packet)
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
    if header.layout.family() == 0 {
        let channels = if header.layout.channels() == 1 {
            PcmChannels::Mono
        } else {
            PcmChannels::Stereo
        };
        let mut decoder = OpusDecoder::new(OutputRate::Khz48, channels)?;
        decoder.set_gain_q8(header.gain_q8);
        decode_audio(
            source,
            output,
            header,
            start,
            |data| Ok(decoder.decode(data)?),
            opura::decoder::DecodedPcm::samples,
        )
    } else {
        let mut decoder = MultistreamDecoder::new(OutputRate::Khz48, header.layout.wave().clone())?;
        decoder.set_gain_q8(header.gain_q8);
        decode_audio(
            source,
            output,
            header,
            start,
            |data| Ok(decoder.decode(data)?),
            MultistreamPcm::samples,
        )
    }
}
fn decode_audio<P>(
    mut source: ogg::Reader<BufReader<File>>,
    output: &Path,
    header: layout::Header,
    start: Instant,
    mut decode_packet: impl FnMut(&[u8]) -> Result<P>,
    get_samples: fn(&P) -> &[i16],
) -> Result<()> {
    let mut writer = wav::Writer::with_channel_order(
        BufWriter::new(create(output)?),
        48000,
        header.layout.channels(),
        header.layout.positioned_order(),
    )?;
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
            let duration = if header.layout.opus().streams() == 1 {
                Packet::parse(packet)?.duration_samples()
            } else {
                MultistreamPacket::parse(packet, header.layout.opus().streams())?.duration_samples()
            };
            samples += u64::from(duration);
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
            let pcm =
                decode_packet(packet).map_err(|e| invalid(format!("packet {packets}: {e}")))?;
            let values = get_samples(&pcm);
            if values.len() % header.layout.channels() != 0 {
                return Err(invalid("partial decoded PCM channel frame").into());
            }
            let count = values.len() / header.layout.channels();
            let after = decoded
                .checked_add(count as u64)
                .ok_or_else(|| invalid("decoded duration overflow"))?;
            let begin = header.skip.max(decoded);
            let stop = if page.eos { end.min(after) } else { after };
            if begin < stop {
                let offset = ((begin - decoded) as usize) * header.layout.channels();
                let length = ((stop - begin) as usize) * header.layout.channels();
                writer.samples(&values[offset..offset + length])?;
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
        header.layout.channels(),
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
        for (index, value) in [(8, 16), (9, 0), (9, 3), (18, 1)] {
            let mut invalid = packet.clone();
            invalid[index] = value;
            assert!(head(&invalid).is_err());
        }
        packet[8] = 15;
        assert!(head(&packet).is_ok());
        for gain in [i16::MIN, -1536, 1, 1536, i16::MAX] {
            packet[16..18].copy_from_slice(&gain.to_le_bytes());
            assert!(head(&packet).is_ok_and(|h| h.gain_q8 == gain));
        }
        assert!(tags(b"OpusTags\0\0\0\0\0\0\0\0").is_ok());
        assert!(tags(b"OpusTags\xff\xff\xff\xff\0\0\0\0").is_err());
        assert!(tags(b"OpusTags\0\0\0\0\xff\xff\xff\xff").is_err());
        assert!(tags(b"OpusTags\0\0\0\0\x01\0\0\0\x05\0\0\0a").is_err());
    }
}
