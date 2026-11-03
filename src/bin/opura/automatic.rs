//! File I/O through the actual common packet owner; no independent mode banks.
use super::{FileRate, Result, create, invalid, ogg, wav};
use opura::{
    Bandwidth,
    celt::{FrameSize, input::InputRate},
    encoder::{
        EncodeOptions, OpusEncoder,
        control::{Application, Controls, Fec, Signal},
    },
};
use std::{
    fs::File,
    io::{BufReader, BufWriter},
    path::Path,
    time::Instant,
};

pub struct Options {
    pub size: FrameSize,
    pub application: Application,
    pub signal: Signal,
    pub complexity: u8,
    pub channels: Option<usize>,
    pub bandwidth: Option<Bandwidth>,
    pub dtx: bool,
    pub loss: u8,
}
pub fn encode(input: &Path, output: &Path, rate_control: FileRate, options: Options) -> Result<()> {
    if rate_control
        .packet_samples
        .is_some_and(|n| usize::from(n) != options.size.samples())
    {
        return Err(invalid(
            "long automatic packets require native transition scheduling and are not supported yet",
        )
        .into());
    }
    if options.application == Application::RestrictedSilk && options.size.samples() < 480 {
        return Err(invalid("restricted-silk requires10/20ms frames").into());
    }
    let cap = rate_control.maximum.unwrap_or(1276);
    if !(3..=1276).contains(&cap) {
        return Err(invalid("automatic single-frame cap must be3..1276").into());
    }
    let bitrate = rate_control.bitrate.unwrap_or(64000);
    let fps = 48000 / options.size.samples() as u32;
    if !(24 * fps..=1500000).contains(&bitrate)
        || (!rate_control.vbr && (bitrate + 4 * fps) / (8 * fps) > cap as u32)
    {
        return Err(invalid("invalid bitrate/frame/cap combination for automatic encoding").into());
    }
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
    if options.channels.is_some_and(|n| n > source.channels) {
        return Err(invalid("encoded channels exceed WAV input channels").into());
    }
    let mut encoder = OpusEncoder::with_application(rate, source.channels, options.application)?;
    let skip = encoder.lookahead_48k();
    let end = (source.frames * rate.factor() as u64)
        .checked_add(u64::from(skip))
        .ok_or_else(|| invalid("audio duration overflow"))?;
    let frames = end.div_ceil(options.size.samples() as u64);
    let mut writer = ogg::Writer::with_pre_skip(
        BufWriter::new(create(output)?),
        source.channels,
        source.rate,
        skip as u16,
    )?;
    let mut pcm = [0_i16; 1920];
    let length = options.size.samples() / rate.factor() * source.channels;
    let controls = Controls {
        application: options.application,
        signal: options.signal,
        complexity: options.complexity,
        channels: options.channels,
        bandwidth: options.bandwidth,
        variable: rate_control.vbr,
        loss_percent: options.loss,
        fec: if options.loss > 0 {
            Fec::Enabled
        } else {
            Fec::Disabled
        },
        dtx: options.dtx,
        ..Controls::default()
    };
    let mut bytes = 0;
    for i in 0..frames {
        pcm.fill(0);
        source.read_samples(&mut pcm[..length])?;
        let packet = encoder.encode(
            options.size,
            &pcm[..length],
            cap,
            EncodeOptions {
                bitrate,
                controls,
                constrained: rate_control.constrained,
            },
        )?;
        let final_frame = i + 1 == frames;
        let granule = if final_frame {
            end
        } else {
            (i + 1) * options.size.samples() as u64
        };
        writer.packet(packet.as_bytes(), granule, final_frame)?;
        bytes += packet.as_bytes().len();
    }
    eprintln!(
        "encoded automatic {:?} {} samples/channel at{}Hz,{}channels;{frames} packets,{bytes} packet bytes;{:.3}s",
        options.application,
        source.frames,
        source.rate,
        source.channels,
        start.elapsed().as_secs_f64()
    );
    Ok(())
}
