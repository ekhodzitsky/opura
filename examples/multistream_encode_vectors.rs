//! Optional synthetic mapped-encoder streams for independent native decoding.
use opura::{
    celt::input::InputRate,
    encoder::{
        PacketSize,
        control::{Application, Controls, Fec, Signal},
    },
    multistream::{
        ChannelMapping, MultistreamDecoder, MultistreamEncodeOptions, MultistreamEncoder,
        MultistreamPcm,
    },
    silk::resampler::OutputRate,
};
use std::{
    env,
    error::Error,
    fs::OpenOptions,
    io::{BufWriter, Write},
};
type Result<T> = std::result::Result<T, Box<dyn Error>>;
fn rate(rate: InputRate) -> OutputRate {
    match rate {
        InputRate::Khz8 => OutputRate::Khz8,
        InputRate::Khz12 => OutputRate::Khz12,
        InputRate::Khz16 => OutputRate::Khz16,
        InputRate::Khz24 => OutputRate::Khz24,
        InputRate::Khz48 => OutputRate::Khz48,
    }
}
fn fingerprint(pcm: &[i16]) -> u64 {
    pcm.iter()
        .flat_map(|n| n.to_le_bytes())
        .fold(14695981039346656037u64, |h, b| {
            (h ^ u64::from(b)).wrapping_mul(1099511628211)
        })
}
fn mapping(channels: usize) -> Result<ChannelMapping> {
    let (streams, coupled, map): (usize, usize, &[u8]) = match channels {
        1 => (1, 0, &[0]),
        2 => (1, 1, &[0, 1]),
        3 => (2, 1, &[0, 2, 1]),
        4 => (2, 2, &[0, 1, 2, 3]),
        5 => (3, 2, &[0, 4, 1, 2, 3]),
        6 => (4, 2, &[0, 4, 1, 2, 3, 5]),
        7 => (4, 3, &[0, 4, 1, 2, 3, 5, 6]),
        8 => (5, 3, &[0, 6, 1, 2, 3, 4, 5, 7]),
        9 => (2, 1, &[2, 0, 1, 255, 0]),
        _ => return Err("invalid synthetic layout".into()),
    };
    Ok(ChannelMapping::new(streams, coupled, map)?)
}
fn input(samples: usize, channels: usize, step: usize, quiet: bool) -> Vec<i16> {
    let mut pcm = Vec::with_capacity(samples * channels);
    for frame in 0..samples {
        for channel in 0..channels {
            let phase = (frame + step * samples + 37 * channel) % 240;
            let triangle = if phase < 120 { phase } else { 240 - phase };
            pcm.push(if quiet {
                0
            } else {
                (triangle as i16 - 60) * (110 - channel as i16 * 3)
            });
        }
    }
    pcm
}
fn head(
    out: &mut impl Write,
    rate: InputRate,
    layout: &ChannelMapping,
    application: usize,
) -> Result<()> {
    write!(
        out,
        "case {} {} {} {} ",
        rate.hz(),
        layout.streams(),
        layout.coupled_streams(),
        application
    )?;
    for (i, n) in layout.mapping().iter().enumerate() {
        if i != 0 {
            write!(out, ",")?;
        }
        write!(out, "{n}")?;
    }
    writeln!(out)?;
    Ok(())
}
fn record(
    out: &mut impl Write,
    kind: &str,
    duration: usize,
    cap: usize,
    options: MultistreamEncodeOptions,
    packet: Option<&[u8]>,
    pcm: &MultistreamPcm,
) -> Result<()> {
    write!(
        out,
        "frame {kind} {duration} {cap} {} {} {} {} {} {} {} {} ",
        options.bitrate,
        u8::from(options.controls.variable),
        u8::from(options.constrained),
        u8::from(options.controls.fec != Fec::Disabled),
        u8::from(options.controls.dtx),
        pcm.final_range(),
        pcm.samples_per_channel(),
        fingerprint(pcm.samples())
    )?;
    if let Some(packet) = packet {
        for byte in packet {
            write!(out, "{byte:02x}")?;
        }
    } else {
        write!(out, "-")?;
    }
    writeln!(out)?;
    Ok(())
}
fn main() -> Result<()> {
    let mut args = env::args_os().skip(1);
    let path = args.next().ok_or("output path required")?;
    let surround = match args.next() {
        None => false,
        Some(arg) if arg == "--surround" => true,
        _ => return Err("expected optional --surround".into()),
    };
    if args.next().is_some() {
        return Err("unexpected export argument".into());
    }
    let mut out = BufWriter::new(OpenOptions::new().write(true).create_new(true).open(path)?);
    writeln!(
        out,
        "# {} {}",
        if surround {
            "OPURA-SURROUND-ENCODE01"
        } else {
            "OPURA-MULTISTREAM-ENCODE01"
        },
        env!("CARGO_PKG_VERSION")
    )?;
    let applications = [
        Application::Audio,
        Application::Voip,
        Application::RestrictedLowDelay,
        Application::RestrictedSilk,
        Application::RestrictedCelt,
    ];
    let sizes = [
        PacketSize::Ms2_5,
        PacketSize::Ms5,
        PacketSize::Ms10,
        PacketSize::Ms20,
        PacketSize::Ms40,
        PacketSize::Ms60,
        PacketSize::Ms80,
        PacketSize::Ms100,
        PacketSize::Ms120,
    ];
    let mut cases = 0usize;
    let mut rows = 0usize;
    let mut encoded = 0usize;
    let mut dropped = 0usize;
    for hz in [
        InputRate::Khz8,
        InputRate::Khz12,
        InputRate::Khz16,
        InputRate::Khz24,
        InputRate::Khz48,
    ] {
        for layout_index in 1..=if surround { 8 } else { 9 } {
            let layout = mapping(layout_index)?;
            for (app, application) in applications.iter().copied().enumerate() {
                head(&mut out, hz, &layout, app)?;
                cases += 1;
                let mut encoder = if surround {
                    MultistreamEncoder::with_surround_application(
                        hz,
                        layout.channels(),
                        application,
                    )?
                } else {
                    MultistreamEncoder::with_application(hz, layout.clone(), application)?
                };
                let mut decoder = MultistreamDecoder::new(rate(hz), layout.clone())?;
                let mut step = 0;
                for size in sizes {
                    if application == Application::RestrictedSilk && size.samples() < 480 {
                        continue;
                    }
                    for mode in 0..3 {
                        let cap = if mode == 0 {
                            7662 * layout.streams() + 2 * layout.streams()
                        } else if mode == 1 {
                            120 * layout.streams()
                        } else {
                            2 * layout.streams() - 1
                                + usize::from(size == PacketSize::Ms100) * layout.streams()
                        };
                        let options = MultistreamEncodeOptions {
                            bitrate: 32000 * layout.coded_channels() as u32,
                            controls: Controls {
                                application,
                                complexity: 3,
                                variable: mode != 0,
                                ..Controls::default()
                            },
                            constrained: mode == 2,
                        };
                        let pcm =
                            input(size.samples() / hz.factor(), layout.channels(), step, false);
                        let packet = encoder.encode_packet(size, &pcm, cap, options)?;
                        encoded += 1;
                        let decoded = decoder.decode(packet.as_bytes())?;
                        if packet.final_range() != decoded.final_range() {
                            return Err("Rust encoder/decoder range mismatch".into());
                        }
                        record(
                            &mut out,
                            "normal",
                            size.samples(),
                            cap,
                            options,
                            Some(packet.as_bytes()),
                            &decoded,
                        )?;
                        rows += 1;
                        step += 1;
                    }
                }
            }
        }
    }
    for layout_index in [3, 8] {
        let layout = mapping(layout_index)?;
        for fec in [false, true] {
            head(&mut out, InputRate::Khz48, &layout, 1)?;
            cases += 1;
            let mut encoder = if surround {
                MultistreamEncoder::with_surround_application(
                    InputRate::Khz48,
                    layout.channels(),
                    Application::Voip,
                )?
            } else {
                MultistreamEncoder::with_application(
                    InputRate::Khz48,
                    layout.clone(),
                    Application::Voip,
                )?
            };
            let mut decoder = MultistreamDecoder::new(OutputRate::Khz48, layout.clone())?;
            let options = MultistreamEncodeOptions {
                bitrate: 32000 * layout.coded_channels() as u32,
                controls: Controls {
                    application: Application::Voip,
                    signal: Signal::Voice,
                    complexity: 10,
                    variable: true,
                    loss_percent: if fec { 20 } else { 0 },
                    fec: if fec { Fec::Enabled } else { Fec::Disabled },
                    dtx: true,
                    ..Controls::default()
                },
                constrained: false,
            };
            let mut pending = false;
            for step in 0..400 {
                let pcm = input(
                    960,
                    layout.channels(),
                    step,
                    (100..250).contains(&step) || step >= 350,
                );
                let packet = encoder.encode_packet(PacketSize::Ms20, &pcm, 8192, options)?;
                encoded += 1;
                if step % 17 == 3 {
                    dropped += 1;
                    if fec {
                        pending = true;
                    } else {
                        let loss = decoder.conceal_samples(960)?;
                        record(&mut out, "plc", 960, 8192, options, None, &loss)?;
                        rows += 1;
                    }
                    continue;
                }
                if pending {
                    let recovered = decoder.decode_fec(packet.as_bytes(), 960)?;
                    record(
                        &mut out,
                        "fec",
                        960,
                        8192,
                        options,
                        Some(packet.as_bytes()),
                        &recovered,
                    )?;
                    rows += 1;
                    pending = false;
                }
                let decoded = decoder.decode(packet.as_bytes())?;
                if packet.final_range() != decoded.final_range() {
                    return Err("Rust recovery stream range mismatch".into());
                }
                record(
                    &mut out,
                    "normal",
                    960,
                    8192,
                    options,
                    Some(packet.as_bytes()),
                    &decoded,
                )?;
                rows += 1;
            }
            if pending {
                return Err("unrecovered final dropped packet".into());
            }
        }
    }
    writeln!(
        out,
        "# cases={cases} rows={rows} encoded={encoded} dropped={dropped}"
    )?;
    out.flush()?;
    println!("exported {cases} cases/{rows} operations/{encoded} encoder calls/{dropped} drops");
    Ok(())
}
