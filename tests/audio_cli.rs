//! Actual audio command execution: duration, reference PCM and file safety.
use std::{
    error::Error,
    fs,
    path::PathBuf,
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};
type Result<T> = std::result::Result<T, Box<dyn Error>>;
struct Temporary(PathBuf);
impl Temporary {
    fn new() -> Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let name = format!(
            "opura-audio-{}-{}-{}",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let path = std::env::temp_dir().join(name);
        fs::create_dir(&path)?;
        Ok(Self(path))
    }
}
impl Drop for Temporary {
    fn drop(&mut self) {
        if let Err(e) = fs::remove_dir_all(&self.0) {
            eprintln!("test temporary cleanup: {e}");
        }
    }
}
fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_opura"))
}
fn success(command: &mut Command) -> Result<Output> {
    let result = command.output()?;
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    Ok(result)
}
fn wave(rate: u32, channels: u16, frames: usize) -> Vec<u8> {
    let bytes = (frames * usize::from(channels) * 2) as u32;
    let mut out = Vec::from(&b"RIFF"[..]);
    out.extend_from_slice(&(36 + bytes).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * u32::from(channels) * 2).to_le_bytes());
    out.extend_from_slice(&(channels * 2).to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&bytes.to_le_bytes());
    for i in 0..frames * usize::from(channels) {
        out.extend_from_slice(&((i as i16).wrapping_mul(239)).to_le_bytes());
    }
    out
}
fn output_pcm(bytes: &[u8], channels: u16, frames: usize) -> Result<&[u8]> {
    assert!(bytes.len() >= 44);
    assert_eq!(&bytes[..4], b"RIFF");
    assert_eq!(&bytes[8..16], b"WAVEfmt ");
    assert_eq!(u16::from_le_bytes(bytes[22..24].try_into()?), channels);
    assert_eq!(u32::from_le_bytes(bytes[24..28].try_into()?), 48000);
    assert_eq!(
        u32::from_le_bytes(bytes[40..44].try_into()?) as usize,
        frames * usize::from(channels) * 2
    );
    assert_eq!(bytes.len(), 44 + frames * usize::from(channels) * 2);
    Ok(&bytes[44..])
}

#[test]
fn automatic_applications_preserve_empty_partial_duration_and_lookahead() -> Result<()> {
    let temp = Temporary::new()?;
    let mut cases = 0;
    for rate in [8000, 12000, 16000, 24000, 48000] {
        for channels in [1u16, 2] {
            for (app, skip) in [
                ("audio", 312),
                ("voip", 312),
                ("lowdelay", 120),
                ("restricted-silk", 312),
                ("restricted-celt", 120),
            ] {
                for variable in [false, true] {
                    let frame = rate as usize / 50;
                    for frames in [0, 1, frame + 1] {
                        let stem = format!("auto-{cases}");
                        cases += 1;
                        let input = temp.0.join(stem.clone() + ".wav");
                        let encoded = temp.0.join(stem.clone() + ".opus");
                        let output = temp.0.join(stem + "-out.wav");
                        fs::write(&input, wave(rate, channels, frames))?;
                        let mut command = cli();
                        command.arg("encode").arg(&input).arg(&encoded).args([
                            "--mode",
                            "auto",
                            "--application",
                            app,
                            "--complexity",
                            "0",
                        ]);
                        if variable {
                            command.arg("--vbr");
                        }
                        success(&mut command)?;
                        success(cli().arg("decode").arg(&encoded).arg(&output))?;
                        output_pcm(
                            &fs::read(output)?,
                            channels,
                            frames * (48000 / rate) as usize,
                        )?;
                        let bytes = fs::read(encoded)?;
                        let head = bytes
                            .windows(8)
                            .position(|v| v == b"OpusHead")
                            .ok_or("missing head")?;
                        assert_eq!(
                            u16::from_le_bytes(bytes[head + 10..head + 12].try_into()?),
                            skip
                        );
                    }
                }
            }
        }
    }
    assert_eq!(cases, 300);
    Ok(())
}

#[test]
fn automatic_long_packets_preserve_duration_and_whole_cbr_budget() -> Result<()> {
    let temp = Temporary::new()?;
    let mut cases = 0;
    for rate in [8000, 12000, 16000, 24000, 48000] {
        for channels in [1u16, 2] {
            for app in [
                "audio",
                "voip",
                "lowdelay",
                "restricted-silk",
                "restricted-celt",
            ] {
                for ms in [40, 60, 80, 100, 120] {
                    let variable = cases % 2 == 0;
                    let input = temp.0.join(format!("long-{cases}.wav"));
                    let encoded = temp.0.join(format!("long-{cases}.opus"));
                    let output = temp.0.join(format!("long-{cases}-out.wav"));
                    let frames = rate as usize * ms / 1000 + 1;
                    fs::write(&input, wave(rate, channels, frames))?;
                    let mut command = cli();
                    command.arg("encode").arg(&input).arg(&encoded).args([
                        "--mode",
                        "auto",
                        "--application",
                        app,
                        "--complexity",
                        "0",
                    ]);
                    if variable {
                        command.args(["--frame-ms", "20", "--packet-ms", &ms.to_string(), "--vbr"]);
                    } else {
                        command.args(["--frame-ms", &ms.to_string()]);
                    }
                    success(&mut command)?;
                    success(cli().arg("decode").arg(&encoded).arg(&output))?;
                    output_pcm(
                        &fs::read(output)?,
                        channels,
                        frames * (48000 / rate) as usize,
                    )?;
                    for packet in audio_packets(&fs::read(encoded)?)? {
                        let parsed = opura::Packet::parse(&packet)?;
                        assert_eq!(usize::from(parsed.duration_samples()), ms * 48);
                        if !variable {
                            assert_eq!(packet.len(), (64000 * ms / 1000 + 4) / 8);
                        }
                    }
                    cases += 1;
                }
            }
        }
    }
    assert_eq!(cases, 250);
    Ok(())
}
#[test]
fn automatic_tiny_budgets_emit_valid_plc_and_keep_file_timing() -> Result<()> {
    let temp = Temporary::new()?;
    let input = temp.0.join("budget.wav");
    fs::write(&input, wave(48000, 2, 9601))?;
    let mut cases = 0;
    for app in [
        "audio",
        "voip",
        "lowdelay",
        "restricted-silk",
        "restricted-celt",
    ] {
        for ms in [20, 40, 60, 80, 100, 120] {
            for cap in [1, 2, 3] {
                let encoded = temp.0.join(format!("budget-{cases}.opus"));
                let output = temp.0.join(format!("budget-{cases}.wav"));
                let mut cmd = cli();
                cmd.arg("encode").arg(&input).arg(&encoded).args([
                    "--mode",
                    "auto",
                    "--application",
                    app,
                    "--frame-ms",
                    &ms.to_string(),
                    "--bitrate",
                    "100",
                    "--max-packet-bytes",
                    &cap.to_string(),
                    "--complexity",
                    "0",
                ]);
                if ms == 100 && cap == 1 {
                    assert!(!cmd.output()?.status.success());
                    assert!(!encoded.exists());
                    continue;
                }
                if cases % 2 == 0 {
                    cmd.arg("--vbr");
                }
                success(&mut cmd)?;
                success(cli().arg("decode").arg(&encoded).arg(&output))?;
                output_pcm(&fs::read(output)?, 2, 9601)?;
                for bytes in audio_packets(&fs::read(encoded)?)? {
                    let packet = opura::Packet::parse(&bytes)?;
                    assert!(bytes.len() <= cap);
                    assert_eq!(usize::from(packet.duration_samples()), ms * 48);
                    assert!(packet.frames().iter().all(|f| f.is_empty()));
                }
                cases += 1;
            }
        }
    }
    assert_eq!(cases, 85);
    Ok(())
}
#[test]
fn automatic_controls_and_invalid_packets_are_explicit() -> Result<()> {
    let temp = Temporary::new()?;
    let input = temp.0.join("source.wav");
    let mut data = wave(48000, 2, 48000 / 2);
    data[44 + 4800 * 4..44 + 18000 * 4].fill(0);
    fs::write(&input, data)?;
    for (i, app) in [
        "audio",
        "voip",
        "lowdelay",
        "restricted-silk",
        "restricted-celt",
    ]
    .into_iter()
    .enumerate()
    {
        let encoded = temp.0.join(format!("configured-{i}.opus"));
        let output = temp.0.join(format!("configured-{i}.wav"));
        success(cli().arg("encode").arg(&input).arg(&encoded).args([
            "--mode",
            "auto",
            "--application",
            app,
            "--signal",
            "voice",
            "--channels",
            "1",
            "--complexity",
            "6",
            "--fec",
            "20",
            "--dtx",
            "--vbr",
            "--bitrate",
            "32000",
            "--max-packet-bytes",
            "160",
        ]))?;
        success(cli().arg("decode").arg(&encoded).arg(&output))?;
        output_pcm(&fs::read(output)?, 2, 24000)?;
        for packet in audio_packets(&fs::read(encoded)?)? {
            assert!(packet.len() <= 160);
        }
    }
    for (i, args) in [
        vec!["--mode", "auto", "--packet-ms", "10"],
        vec![
            "--mode",
            "auto",
            "--application",
            "restricted-silk",
            "--frame-ms",
            "5",
        ],
        vec!["--mode", "auto", "--complexity", "11"],
        vec!["--mode", "celt", "--application", "voip"],
    ]
    .into_iter()
    .enumerate()
    {
        let out = temp.0.join(format!("invalid-auto-{i}.opus"));
        let result = cli()
            .arg("encode")
            .arg(&input)
            .arg(&out)
            .args(args)
            .output()?;
        assert!(!result.status.success());
        assert!(!out.exists());
    }
    Ok(())
}
#[test]
fn command_roundtrip_preserves_exact_duration_at_all_geometries() -> Result<()> {
    let temp = Temporary::new()?;
    for (r, rate) in [8000, 12000, 16000, 24000, 48000].into_iter().enumerate() {
        for channels in [1u16, 2] {
            for (size, n) in [("2.5", 120), ("5", 240), ("10", 480), ("20", 960)] {
                let count = n / (48000 / rate) as usize;
                let frames = match (r + usize::from(channels)) % 5 {
                    0 => 0,
                    1 => 1,
                    2 => count - 1,
                    3 => count,
                    _ => count + 1,
                };
                let stem = format!("{rate}-{channels}-{size}");
                let input = temp.0.join(stem.clone() + ".wav");
                let opus = temp.0.join(stem.clone() + ".opus");
                let output = temp.0.join(stem + "-decoded.wav");
                fs::write(&input, wave(rate, channels, frames))?;
                success(cli().arg("encode").arg(&input).arg(&opus).args([
                    "--bitrate",
                    "64000",
                    "--frame-ms",
                    size,
                ]))?;
                success(cli().arg("decode").arg(&opus).arg(&output))?;
                output_pcm(
                    &fs::read(&output)?,
                    channels,
                    frames * (48000 / rate) as usize,
                )?;
            }
        }
    }
    Ok(())
}
#[test]
fn command_decodes_original_silk_hybrid_celt_to_exact_native_pcm() -> Result<()> {
    let temp = Temporary::new()?;
    for (name, channels) in [("silk-mono", 1), ("hybrid-stereo", 2), ("celt-stereo", 2)] {
        let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/audio")
            .join(name.to_owned() + ".opus");
        let expected = fs::read(source.with_extension("pcm"))?;
        let output = temp.0.join(name.to_owned() + ".wav");
        success(cli().arg("decode").arg(&source).arg(&output))?;
        let decoded = fs::read(output)?;
        assert_eq!(output_pcm(&decoded, channels, 4663)?, expected, "{name}");
    }
    Ok(())
}
#[test]
fn command_refuses_existing_output_and_bad_arguments() -> Result<()> {
    let temp = Temporary::new()?;
    let input = temp.0.join("input.wav");
    let existing = temp.0.join("existing.opus");
    fs::write(&input, wave(48000, 1, 960))?;
    fs::write(&existing, b"keep existing user data")?;
    assert!(
        !cli()
            .arg("encode")
            .arg(&input)
            .arg(&existing)
            .output()?
            .status
            .success()
    );
    assert_eq!(fs::read(&existing)?, b"keep existing user data");
    for arguments in [
        vec!["--frame-ms", "40"],
        vec!["--dtx"],
        vec!["--fec", "20"],
        vec!["--mode", "silk", "--fec", "101"],
        vec!["--mode", "silk", "--fec", "-1"],
        vec!["--mode", "silk", "--fec", "bad"],
        vec!["--mode", "silk", "--fec", "20", "--fec", "20"],
        vec!["--mode", "silk", "--fec"],
        vec!["--mode", "silk", "--dtx", "--dtx"],
        vec!["--bitrate", "0"],
        vec!["--bitrate", "64000", "--bitrate", "64000"],
        vec!["--frame-ms"],
    ] {
        let output = temp.0.join("must-not-create.opus");
        let result = cli()
            .arg("encode")
            .arg(&input)
            .arg(&output)
            .args(arguments)
            .output()?;
        assert!(!result.status.success());
        assert!(!output.exists());
    }
    let version = success(cli().arg("--version"))?;
    assert_eq!(
        String::from_utf8(version.stdout)?.trim(),
        concat!("opura ", env!("CARGO_PKG_VERSION"))
    );
    Ok(())
}
#[test]
fn malformed_audio_fails_without_overwriting_input() -> Result<()> {
    let temp = Temporary::new()?;
    let source = temp.0.join("broken.opus");
    let output = temp.0.join("broken.wav");
    let original =
        fs::read(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/audio/celt-stereo.opus"))?;
    for bytes in [
        vec![],
        original[..26].to_vec(),
        {
            let mut b = original.clone();
            let n = b.len();
            b[n - 1] ^= 1;
            b
        },
        original[..original.len() - 1].to_vec(),
    ] {
        fs::write(&source, &bytes)?;
        let result = cli().arg("decode").arg(&source).arg(&output).output()?;
        assert!(!result.status.success());
        assert_eq!(fs::read(&source)?, bytes);
        if output.exists() {
            fs::remove_file(&output)?;
        }
    }
    // PCM geometry must be validated before an output is created.
    let input = temp.0.join("bad.wav");
    let encoded = temp.0.join("bad.opus");
    let mut bad = wave(48000, 1, 1);
    bad[24..28].copy_from_slice(&44100u32.to_le_bytes());
    fs::write(&input, bad)?;
    assert!(
        !cli()
            .arg("encode")
            .arg(input)
            .arg(&encoded)
            .output()?
            .status
            .success()
    );
    assert!(!encoded.exists());
    Ok(())
}

fn rewrite_granules(bytes: &mut [u8], change: impl Fn(usize, u64) -> u64) -> Result<()> {
    let mut position = 0;
    let mut index = 0;
    while position < bytes.len() {
        let count = usize::from(bytes[position + 26]);
        let header = position + 27 + count;
        let body: usize = bytes[position + 27..header]
            .iter()
            .map(|&v| usize::from(v))
            .sum();
        let end = header + body;
        let granule = u64::from_le_bytes(bytes[position + 6..position + 14].try_into()?);
        bytes[position + 6..position + 14].copy_from_slice(&change(index, granule).to_le_bytes());
        bytes[position + 22..position + 26].fill(0);
        let mut crc = 0u32;
        for &byte in &bytes[position..end] {
            crc ^= u32::from(byte) << 24;
            for _ in 0..8 {
                crc = if crc & 0x80000000 != 0 {
                    (crc << 1) ^ 0x04c11db7
                } else {
                    crc << 1
                };
            }
        }
        bytes[position + 22..position + 26].copy_from_slice(&crc.to_le_bytes());
        position = end;
        index += 1;
    }
    Ok(())
}
#[test]
fn timestamp_offsets_and_invalid_granules_use_real_audio() -> Result<()> {
    let temp = Temporary::new()?;
    let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/audio/silk-mono.opus");
    let original = fs::read(&base)?;
    let source = temp.0.join("shifted.opus");
    let output = temp.0.join("shifted.wav");
    let mut shifted = original.clone();
    rewrite_granules(&mut shifted, |page, g| if page >= 2 { g + 4800 } else { g })?;
    fs::write(&source, shifted)?;
    success(cli().arg("decode").arg(&source).arg(&output))?;
    let decoded = fs::read(&output)?;
    assert_eq!(
        output_pcm(&decoded, 1, 4663)?,
        fs::read(base.with_extension("pcm"))?
    );
    fs::remove_file(&output)?;
    for mutation in 0..4 {
        let mut invalid = original.clone();
        rewrite_granules(&mut invalid, |page, g| match (mutation, page) {
            (0, 2) => 959,
            (1, 7) => 0,
            (2, 1) => u64::MAX,
            (3, 2) => u64::MAX,
            _ => g,
        })?;
        fs::write(&source, invalid)?;
        assert!(
            !cli()
                .arg("decode")
                .arg(&source)
                .arg(&output)
                .output()?
                .status
                .success()
        );
        if output.exists() {
            fs::remove_file(&output)?;
        }
    }
    // Decode refuses pre-existing destinations just like encode.
    fs::write(&output, b"existing output")?;
    assert!(
        !cli()
            .arg("decode")
            .arg(&base)
            .arg(&output)
            .output()?
            .status
            .success()
    );
    assert_eq!(fs::read(&output)?, b"existing output");
    Ok(())
}
#[test]
fn encoder_delay_is_removed_from_an_impulse_recording() -> Result<()> {
    let temp = Temporary::new()?;
    let input = temp.0.join("impulse.wav");
    let encoded = temp.0.join("impulse.opus");
    let output = temp.0.join("impulse-decoded.wav");
    let mut bytes = wave(48000, 1, 1923);
    bytes[44..].fill(0);
    bytes[44 + 480 * 2..44 + 482 * 2].copy_from_slice(&[0x20, 0x4e, 0, 0]);
    fs::write(&input, bytes)?;
    success(
        cli()
            .arg("encode")
            .arg(&input)
            .arg(&encoded)
            .args(["--bitrate", "128000"]),
    )?;
    success(cli().arg("decode").arg(&encoded).arg(&output))?;
    let decoded = fs::read(output)?;
    let samples = output_pcm(&decoded, 1, 1923)?;
    let peak = samples
        .chunks_exact(2)
        .enumerate()
        .max_by_key(|(_, v)| i32::from(i16::from_le_bytes([v[0], v[1]])).abs())
        .map(|(i, _)| i)
        .ok_or("empty decoded impulse")?;
    assert_eq!(peak, 480);
    Ok(())
}

#[test]
fn silk_command_preserves_duration_for_all_rates_bandwidths_and_packet_sizes() -> Result<()> {
    let temp = Temporary::new()?;
    let mut case = 0;
    for rate in [8000, 12000, 16000, 24000, 48000] {
        for bandwidth in ["nb", "mb", "wb", "auto"] {
            for channels in [1u16, 2] {
                for ms in [10, 20, 40, 60] {
                    let frame = rate as usize / 1000 * ms;
                    let frames = match case % 5 {
                        0 => 0,
                        1 => 1,
                        2 => frame - 1,
                        3 => frame,
                        _ => frame + 1,
                    };
                    case += 1;
                    let stem = format!("silk-{rate}-{bandwidth}-{channels}-{ms}");
                    let input = temp.0.join(stem.clone() + ".wav");
                    let encoded = temp.0.join(stem.clone() + ".opus");
                    let output = temp.0.join(stem + "-decoded.wav");
                    fs::write(&input, wave(rate, channels, frames))?;
                    success(cli().arg("encode").arg(&input).arg(&encoded).args([
                        "--mode",
                        "silk",
                        "--bandwidth",
                        bandwidth,
                        "--bitrate",
                        "32101",
                        "--frame-ms",
                        &ms.to_string(),
                    ]))?;
                    let container = fs::read(&encoded)?;
                    let head = container
                        .windows(8)
                        .position(|w| w == b"OpusHead")
                        .ok_or("missing OpusHead")?;
                    assert_eq!(
                        u16::from_le_bytes(container[head + 10..head + 12].try_into()?),
                        312
                    );
                    success(cli().arg("decode").arg(&encoded).arg(&output))?;
                    output_pcm(
                        &fs::read(output)?,
                        channels,
                        frames * (48000 / rate) as usize,
                    )?;
                }
            }
        }
    }
    assert_eq!(case, 160);
    Ok(())
}
#[test]
fn silk_pre_skip_centers_impulse_and_rejects_unsupported_cli_geometry() -> Result<()> {
    let temp = Temporary::new()?;
    let input = temp.0.join("impulse.wav");
    let encoded = temp.0.join("impulse.opus");
    let output = temp.0.join("decoded.wav");
    let mut bytes = wave(48000, 1, 1923);
    bytes[44..].fill(0);
    bytes[44 + 480 * 2..44 + 481 * 2].copy_from_slice(&20000i16.to_le_bytes());
    fs::write(&input, bytes)?;
    success(cli().arg("encode").arg(&input).arg(&encoded).args([
        "--mode",
        "silk",
        "--bitrate",
        "128000",
    ]))?;
    success(cli().arg("decode").arg(&encoded).arg(&output))?;
    let decoded = fs::read(output)?;
    let samples = output_pcm(&decoded, 1, 1923)?;
    let peak = samples
        .chunks_exact(2)
        .enumerate()
        .max_by_key(|(_, v)| i32::from(i16::from_le_bytes([v[0], v[1]])).abs())
        .map(|(i, _)| i)
        .ok_or("missing impulse")?;
    // Native SILK resampling is not linear phase. Guard nominal6.5ms trimming
    // within its short impulse lobe, rather than assuming a CELT-exact peak.
    assert!(peak.abs_diff(480) <= 8, "SILK impulse at {peak}");
    let rejected = temp.0.join("rejected.opus");
    for arguments in [
        vec!["--mode", "silk", "--frame-ms", "5"],
        vec!["--mode", "silk", "--mode", "silk"],
        vec!["--mode", "unknown"],
        vec!["--mode", "celt", "--bandwidth", "wb"],
        vec!["--mode", "silk", "--bandwidth", "bad"],
    ] {
        assert!(
            !cli()
                .arg("encode")
                .arg(&input)
                .arg(&rejected)
                .args(arguments)
                .output()?
                .status
                .success()
        );
        assert!(!rejected.exists());
    }
    let before = fs::read(&encoded)?;
    assert!(
        !cli()
            .arg("encode")
            .arg(&input)
            .arg(&encoded)
            .args(["--mode", "silk"])
            .output()?
            .status
            .success()
    );
    assert_eq!(fs::read(encoded)?, before);
    Ok(())
}

#[test]
fn silk_dtx_saves_bytes_and_preserves_duration_in_both_channel_layouts() -> Result<()> {
    let temp = Temporary::new()?;
    for rate in [8000, 48000] {
        for channels in [1u16, 2] {
            for ms in [10, 20, 40, 60] {
                let stem = format!("dtx-{rate}-{channels}-{ms}");
                let input = temp.0.join(stem.clone() + ".wav");
                let plain = temp.0.join(stem.clone() + "-plain.opus");
                let dtx = temp.0.join(stem.clone() + ".opus");
                let output = temp.0.join(stem + "-decoded.wav");
                let frames = rate as usize + 17;
                let mut wave = wave(rate, channels, frames);
                wave[44..].fill(0);
                fs::write(&input, wave)?;
                for (path, enabled) in [(&plain, false), (&dtx, true)] {
                    let mut command = cli();
                    command.arg("encode").arg(&input).arg(path).args([
                        "--mode",
                        "silk",
                        "--frame-ms",
                        &ms.to_string(),
                        "--bitrate",
                        "32101",
                    ]);
                    if enabled {
                        command.arg("--dtx");
                    }
                    success(&mut command)?;
                }
                assert!(fs::metadata(&dtx)?.len() * 2 < fs::metadata(&plain)?.len());
                success(cli().arg("decode").arg(&dtx).arg(&output))?;
                output_pcm(
                    &fs::read(output)?,
                    channels,
                    frames * (48000 / rate) as usize,
                )?;
            }
        }
    }
    Ok(())
}

fn audio_packets(bytes: &[u8]) -> Result<Vec<Vec<u8>>> {
    let mut offset = 0;
    let mut pending = Vec::new();
    let mut packets = Vec::new();
    while offset < bytes.len() {
        let header = bytes.get(offset..offset + 27).ok_or("truncated Ogg page")?;
        assert_eq!(&header[..4], b"OggS");
        let laces = bytes
            .get(offset + 27..offset + 27 + usize::from(header[26]))
            .ok_or("truncated laces")?;
        offset += 27 + laces.len();
        for &length in laces {
            let end = offset + usize::from(length);
            pending.extend_from_slice(bytes.get(offset..end).ok_or("truncated packet")?);
            offset = end;
            if length < 255 {
                packets.push(std::mem::take(&mut pending));
            }
        }
    }
    assert!(pending.is_empty() && packets.len() >= 3);
    Ok(packets.into_iter().skip(2).collect())
}

#[test]
fn hybrid_cli_preserves_pre_skip_duration_and_fractional_packet_budgets() -> Result<()> {
    let temp = Temporary::new()?;
    let mut cases = 0;
    for (rate, band) in [(24000, "swb"), (48000, "swb"), (48000, "fb")] {
        for channels in [1u16, 2] {
            for ms in [10, 20] {
                let frame = rate as usize / 1000 * ms;
                for frames in [0, 1, frame - 1, frame, frame + 1] {
                    let stem = format!("hybrid-{cases}");
                    cases += 1;
                    let input = temp.0.join(stem.clone() + ".wav");
                    let encoded = temp.0.join(stem.clone() + ".opus");
                    let output = temp.0.join(stem + "-out.wav");
                    fs::write(&input, wave(rate, channels, frames))?;
                    let bitrate = if channels == 1 { "32001" } else { "64001" };
                    success(cli().arg("encode").arg(&input).arg(&encoded).args([
                        "--mode",
                        "hybrid",
                        "--bandwidth",
                        band,
                        "--bitrate",
                        bitrate,
                        "--frame-ms",
                        &ms.to_string(),
                    ]))?;
                    success(cli().arg("decode").arg(&encoded).arg(&output))?;
                    output_pcm(
                        &fs::read(&output)?,
                        channels,
                        frames * (48000 / rate) as usize,
                    )?;
                    let ogg = fs::read(encoded)?;
                    let head = ogg
                        .windows(8)
                        .position(|v| v == b"OpusHead")
                        .ok_or("missing head")?;
                    assert_eq!(
                        u16::from_le_bytes(ogg[head + 10..head + 12].try_into()?),
                        312
                    );
                    let mut credit = 4000;
                    for bytes in audio_packets(&ogg)? {
                        credit += bitrate.parse::<usize>()? * ms;
                        assert_eq!(bytes.len(), credit / 8000);
                        credit %= 8000;
                        let packet = opura::Packet::parse(&bytes)?;
                        assert_eq!(packet.mode(), opura::Mode::Hybrid);
                        assert_eq!(packet.frame_samples(), ms as u16 * 48);
                    }
                }
            }
        }
    }
    assert_eq!(cases, 60);
    let input = temp.0.join("bad.wav");
    fs::write(&input, wave(48000, 1, 960))?;
    for args in [
        vec!["--mode", "hybrid", "--frame-ms", "5"],
        vec!["--mode", "hybrid", "--frame-ms", "40"],
        vec!["--mode", "hybrid", "--bandwidth", "auto"],
        vec!["--mode", "hybrid", "--bandwidth", "wb"],
        vec!["--mode", "silk", "--bandwidth", "fb"],
    ] {
        let output = temp.0.join("must-not-create.opus");
        assert!(
            !cli()
                .arg("encode")
                .arg(&input)
                .arg(&output)
                .args(args)
                .output()?
                .status
                .success()
        );
        assert!(!output.exists());
    }
    for rate in [8000, 12000, 16000, 24000] {
        fs::write(&input, wave(rate, 1, 960))?;
        let output = temp.0.join("bad-rate.opus");
        assert!(
            !cli()
                .arg("encode")
                .arg(&input)
                .arg(&output)
                .args(["--mode", "hybrid", "--bandwidth", "fb"])
                .output()?
                .status
                .success()
        );
        assert!(!output.exists());
    }
    Ok(())
}

#[test]
fn hybrid_dtx_saves_packet_bytes_and_preserves_silence_resume_duration() -> Result<()> {
    let temp = Temporary::new()?;
    for rate in [24000, 48000] {
        for channels in [1u16, 2] {
            for ms in ["10", "20"] {
                let count = rate as usize * 2;
                let mut input = wave(rate, channels, count);
                let start = 44 + rate as usize / 4 * usize::from(channels) * 2;
                let end = 44 + rate as usize * 3 / 2 * usize::from(channels) * 2;
                input[start..end].fill(0);
                let stem = format!("hybrid-dtx-{rate}-{channels}-{ms}");
                let source = temp.0.join(stem.clone() + ".wav");
                let encoded = temp.0.join(stem.clone() + ".opus");
                let normal = temp.0.join(stem.clone() + "-normal.opus");
                let decoded = temp.0.join(stem + "-decoded.wav");
                fs::write(&source, input)?;
                success(cli().arg("encode").arg(&source).arg(&encoded).args([
                    "--mode",
                    "hybrid",
                    "--dtx",
                    "--frame-ms",
                    ms,
                ]))?;
                success(cli().arg("encode").arg(&source).arg(&normal).args([
                    "--mode",
                    "hybrid",
                    "--frame-ms",
                    ms,
                ]))?;
                success(cli().arg("decode").arg(&encoded).arg(&decoded))?;
                output_pcm(
                    &fs::read(&decoded)?,
                    channels,
                    count * (48000 / rate) as usize,
                )?;
                let packets = audio_packets(&fs::read(&encoded)?)?;
                assert!(packets.iter().filter(|p| p.len() == 1).count() > 10);
                assert!(packets.iter().filter(|p| p.len() > 1).count() > 10);
                assert!(fs::metadata(&encoded)?.len() < fs::metadata(&normal)?.len());
                assert!(packets.last().ok_or("missing last packet")?.len() > 1);
            }
        }
    }
    Ok(())
}

#[test]
fn hybrid_fec_cli_protects_the_silk_band_and_combines_with_dtx() -> Result<()> {
    let temp = Temporary::new()?;
    for rate in [24000, 48000] {
        for channels in [1u16, 2] {
            for ms in ["10", "20"] {
                let count = rate as usize * 2;
                let mut input = wave(rate, channels, count);
                let start = 44 + rate as usize / 4 * usize::from(channels) * 2;
                let end = 44 + rate as usize * 3 / 2 * usize::from(channels) * 2;
                input[start..end].fill(0);
                let stem = format!("hybrid-fec-{rate}-{channels}-{ms}");
                let source = temp.0.join(stem.clone() + ".wav");
                let encoded = temp.0.join(stem.clone() + ".opus");
                let decoded = temp.0.join(stem + "-decoded.wav");
                fs::write(&source, input)?;
                success(cli().arg("encode").arg(&source).arg(&encoded).args([
                    "--mode",
                    "hybrid",
                    "--fec",
                    "20",
                    "--dtx",
                    "--bitrate",
                    if channels == 1 { "64000" } else { "128000" },
                    "--frame-ms",
                    ms,
                ]))?;
                success(cli().arg("decode").arg(&encoded).arg(&decoded))?;
                output_pcm(
                    &fs::read(&decoded)?,
                    channels,
                    count * (48000 / rate) as usize,
                )?;
                let packets = audio_packets(&fs::read(&encoded)?)?;
                let mut protected = 0;
                let mut dtx = 0;
                for bytes in packets {
                    let packet = opura::Packet::parse(&bytes)?;
                    assert_eq!(packet.mode(), opura::Mode::Hybrid);
                    if bytes.len() == 1 {
                        dtx += 1;
                        continue;
                    }
                    let (header, _) = opura::silk::decode_header(&packet, 0)?;
                    protected += usize::from((0..usize::from(channels)).any(|c| {
                        header
                            .redundancy(c)
                            .is_some_and(|flags| flags.iter().any(|&v| v))
                    }));
                }
                assert!(protected > 5 && dtx > 5);
            }
        }
    }
    Ok(())
}

#[test]
fn hybrid_vbr_cli_uses_variable_lengths_with_explicit_caps() -> Result<()> {
    let temp = Temporary::new()?;
    for rate in [24000, 48000] {
        for channels in [1u16, 2] {
            for ms in ["10", "20"] {
                let count = rate as usize / 2 + 1;
                let source = temp.0.join(format!("vbr-{rate}-{channels}-{ms}.wav"));
                let opus = source.with_extension("opus");
                let output = source.with_extension("decoded.wav");
                fs::write(&source, wave(rate, channels, count))?;
                success(cli().arg("encode").arg(&source).arg(&opus).args([
                    "--mode",
                    "hybrid",
                    "--vbr",
                    "--bitrate",
                    if channels == 1 { "32001" } else { "64001" },
                    "--frame-ms",
                    ms,
                    "--max-packet-bytes",
                    "200",
                    "--fec",
                    "20",
                    "--dtx",
                ]))?;
                success(cli().arg("decode").arg(&opus).arg(&output))?;
                output_pcm(
                    &fs::read(&output)?,
                    channels,
                    count * (48000 / rate) as usize,
                )?;
                let packets = audio_packets(&fs::read(&opus)?)?;
                let sizes: std::collections::BTreeSet<_> = packets.iter().map(Vec::len).collect();
                assert!(sizes.len() > 2);
                assert!(sizes.iter().all(|&n| (1..=200).contains(&n)));
            }
        }
    }
    let source = temp.0.join("invalid-vbr.wav");
    fs::write(&source, wave(48000, 1, 960))?;
    for args in [
        vec!["--mode", "silk", "--vbr"],
        vec!["--mode", "hybrid", "--max-packet-bytes", "200"],
        vec!["--mode", "hybrid", "--vbr", "--max-packet-bytes", "2"],
        vec!["--mode", "hybrid", "--vbr", "--max-packet-bytes", "1277"],
        vec!["--mode", "hybrid", "--vbr", "--bitrate", "0"],
        vec!["--mode", "hybrid", "--vbr", "--bitrate", "1024001"],
        vec!["--mode", "hybrid", "--vbr", "--vbr"],
    ] {
        let output = temp.0.join("must-not-create.opus");
        assert!(
            !cli()
                .arg("encode")
                .arg(&source)
                .arg(&output)
                .args(args)
                .output()?
                .status
                .success()
        );
        assert!(!output.exists());
    }
    Ok(())
}
#[test]
fn celt_vbr_cvbr_cli_caps_silence_duration_and_rejects_conflicts() -> Result<()> {
    let temp = Temporary::new()?;
    for rate in [8000, 12000, 16000, 24000, 48000] {
        for channels in [1u16, 2] {
            for ms in ["2.5", "5", "10", "20"] {
                for mode in ["--vbr", "--cvbr"] {
                    let count = rate as usize / 10 + 1;
                    let source = temp
                        .0
                        .join(format!("celt-{rate}-{channels}-{ms}{mode}.wav"));
                    let opus = source.with_extension("opus");
                    let output = source.with_extension("decoded.wav");
                    let mut data = wave(rate, channels, count);
                    data[44..44 + count * usize::from(channels)].fill(0);
                    fs::write(&source, data)?;
                    success(cli().arg("encode").arg(&source).arg(&opus).args([
                        mode,
                        "--frame-ms",
                        ms,
                        "--bitrate",
                        "64001",
                        "--max-packet-bytes",
                        "200",
                    ]))?;
                    success(cli().arg("decode").arg(&opus).arg(&output))?;
                    output_pcm(
                        &fs::read(&output)?,
                        channels,
                        count * (48000 / rate) as usize,
                    )?;
                    let packets = audio_packets(&fs::read(&opus)?)?;
                    let sizes: std::collections::BTreeSet<_> =
                        packets.iter().map(Vec::len).collect();
                    assert!(sizes.len() > 1);
                    assert!(sizes.iter().all(|&n| (3..=200).contains(&n)));
                    assert!(sizes.contains(&3));
                }
            }
        }
    }
    let input = temp.0.join("bad-cvbr.wav");
    fs::write(&input, wave(48000, 1, 960))?;
    for args in [
        vec!["--cvbr", "--cvbr"],
        vec!["--vbr", "--cvbr"],
        vec!["--cvbr", "--vbr"],
        vec!["--cvbr", "--mode", "hybrid"],
        vec!["--cvbr", "--mode", "silk"],
        vec!["--max-packet-bytes", "200"],
        vec!["--cvbr", "--bitrate", "1500001"],
        vec!["--cvbr", "--bitrate", "0"],
        vec!["--cvbr", "--frame-ms", "2.5", "--bitrate", "9599"],
    ] {
        let output = temp.0.join("bad-cvbr.opus");
        assert!(
            !cli()
                .arg("encode")
                .arg(&input)
                .arg(&output)
                .args(args)
                .output()?
                .status
                .success()
        );
        assert!(!output.exists());
    }
    Ok(())
}

#[test]
fn grouped_audio_packets_preserve_duration_rate_caps_and_compatible_frames() -> Result<()> {
    let temp = Temporary::new()?;
    for rate in [8000, 12000, 16000, 24000, 48000] {
        for channels in [1u16, 2] {
            let input = temp.0.join(format!("group-{rate}-{channels}.wav"));
            let count = rate as usize / 4 + 1;
            fs::write(&input, wave(rate, channels, count))?;
            let mut profiles: Vec<(&str, &str, &str, Vec<&str>)> = Vec::new();
            for (ms, packet_ms) in [("2.5", "7.5"), ("5", "20"), ("10", "60"), ("20", "120")] {
                for flags in [vec![], vec!["--vbr"], vec!["--cvbr"]] {
                    profiles.push(("celt", ms, packet_ms, flags));
                }
            }
            for (ms, packet_ms) in [("10", "120"), ("20", "100"), ("40", "80"), ("60", "120")] {
                profiles.push(("silk", ms, packet_ms, vec!["--fec", "20", "--dtx"]));
                profiles.push((
                    "silk",
                    ms,
                    packet_ms,
                    vec!["--bandwidth", "auto", "--fec", "20", "--dtx"],
                ));
            }
            if rate >= 24000 {
                for (ms, packet_ms) in [("10", "60"), ("20", "120")] {
                    profiles.push(("hybrid", ms, packet_ms, vec!["--fec", "20", "--dtx"]));
                    profiles.push((
                        "hybrid",
                        ms,
                        packet_ms,
                        vec!["--vbr", "--fec", "20", "--dtx"],
                    ));
                }
            }
            for (case, (mode, ms, packet_ms, flags)) in profiles.iter().enumerate() {
                let opus = temp.0.join(format!("group-{rate}-{channels}-{case}.opus"));
                let output = opus.with_extension("wav");
                let bitrate = if channels == 1 { 32001u64 } else { 64001 };
                let variable = flags.contains(&"--vbr") || flags.contains(&"--cvbr");
                let mut command = cli();
                command
                    .arg("encode")
                    .arg(&input)
                    .arg(&opus)
                    .args([
                        "--mode",
                        mode,
                        "--frame-ms",
                        ms,
                        "--packet-ms",
                        packet_ms,
                        "--bitrate",
                        &bitrate.to_string(),
                    ])
                    .args(flags);
                if variable {
                    command.args(["--max-packet-bytes", "1200"]);
                }
                success(&mut command)?;
                success(cli().arg("decode").arg(&opus).arg(&output))?;
                output_pcm(
                    &fs::read(&output)?,
                    channels,
                    count * (48000 / rate) as usize,
                )?;
                let packets = audio_packets(&fs::read(&opus)?)?;
                let maximum_samples = (packet_ms.parse::<f64>()? * 48.0) as u16;
                let mut credit = 384000u64 / 2;
                let mut multiple = 0;
                for bytes in &packets {
                    let packet = opura::Packet::parse(bytes)?;
                    assert!(packet.duration_samples() <= maximum_samples);
                    multiple += usize::from(packet.frames().len() > 1);
                    credit += bitrate * u64::from(packet.duration_samples());
                    let target = credit / 384000;
                    credit %= 384000;
                    if variable {
                        assert!(bytes.len() <= 1200);
                    } else if !packet.frames().iter().all(|f| f.is_empty()) {
                        assert_eq!(
                            bytes.len() as u64,
                            target,
                            "{rate}/{channels}/{mode}/{ms}/{packet_ms}"
                        );
                    }
                }
                assert!(
                    multiple > 0,
                    "missing grouping {rate}/{channels}/{mode}/{ms}/{packet_ms}"
                );
            }
        }
    }
    Ok(())
}

#[test]
fn grouped_cli_rejects_bad_durations_and_impossible_caps_before_output() -> Result<()> {
    let temp = Temporary::new()?;
    let input = temp.0.join("group-invalid.wav");
    fs::write(&input, wave(48000, 1, 960))?;
    for args in [
        vec!["--packet-ms", "0"],
        vec!["--packet-ms", "120.5"],
        vec!["--packet-ms", "NaN"],
        vec!["--packet-ms", "10"],
        vec!["--packet-ms", "30", "--frame-ms", "20"],
        vec!["--packet-ms", "120", "--packet-ms", "120"],
        vec!["--packet-ms", "120", "--vbr", "--max-packet-bytes", "14"],
        vec!["--packet-ms", "120", "--vbr", "--max-packet-bytes", "7657"],
        vec!["--packet-ms", "120", "--bitrate", "1200"],
    ] {
        let output = temp.0.join("group-invalid.opus");
        assert!(
            !cli()
                .arg("encode")
                .arg(&input)
                .arg(&output)
                .args(args)
                .output()?
                .status
                .success()
        );
        assert!(!output.exists());
    }
    Ok(())
}

#[test]
fn grouped_empty_partial_and_silence_resume_files_preserve_timing() -> Result<()> {
    let temp = Temporary::new()?;
    for mode in ["celt", "silk", "hybrid"] {
        for channels in [1u16, 2] {
            for (case, count) in [0, 1, 48000 * 3 + 13].into_iter().enumerate() {
                let source = temp
                    .0
                    .join(format!("group-tail-{mode}-{channels}-{case}.wav"));
                let opus = source.with_extension("opus");
                let output = source.with_extension("decoded.wav");
                let mut data = wave(48000, channels, count);
                if case == 2 {
                    data[44 + 12000 * usize::from(channels) * 2
                        ..44 + 132000 * usize::from(channels) * 2]
                        .fill(0);
                }
                fs::write(&source, data)?;
                let mut command = cli();
                command.arg("encode").arg(&source).arg(&opus).args([
                    "--mode",
                    mode,
                    "--frame-ms",
                    "20",
                    "--packet-ms",
                    "120",
                    "--bitrate",
                    if channels == 1 { "32001" } else { "64001" },
                ]);
                if mode == "celt" {
                    command.arg("--cvbr");
                } else {
                    command.args(["--fec", "20", "--dtx"]);
                }
                success(&mut command)?;
                success(cli().arg("decode").arg(&opus).arg(&output))?;
                output_pcm(&fs::read(output)?, channels, count)?;
                if case == 2 && mode != "celt" {
                    let packets = audio_packets(&fs::read(opus)?)?;
                    let mut dtx = 0;
                    let mut primary = 0;
                    for bytes in packets {
                        let p = opura::Packet::parse(&bytes)?;
                        if p.frames().iter().all(|f| f.is_empty()) {
                            dtx += 1;
                            assert!(bytes.len() <= 2);
                        } else {
                            primary += 1;
                        }
                    }
                    assert!(dtx > 2 && primary > 2);
                }
            }
        }
    }
    Ok(())
}
#[test]
fn silk_cli_fec_emits_redundancy_and_combines_with_dtx() -> Result<()> {
    let temp = Temporary::new()?;
    for rate in [8000, 48000] {
        for channels in [1u16, 2] {
            for ms in [10, 20, 40, 60] {
                let stem = format!("fec-{rate}-{channels}-{ms}");
                let input = temp.0.join(stem.clone() + ".wav");
                let frames = rate as usize * 2 + 17;
                let mut wave = wave(rate, channels, frames);
                for (index, sample) in wave[44..].chunks_exact_mut(2).enumerate() {
                    let i = index / usize::from(channels);
                    let value = if (rate as usize / 3..rate as usize * 3 / 2).contains(&i) {
                        0
                    } else if index % usize::from(channels) == 0 {
                        ((i % 80) as i16 - 40) * 300
                    } else {
                        ((i % 53) as i16 - 26) * 400
                    };
                    sample.copy_from_slice(&value.to_le_bytes());
                }
                fs::write(&input, wave)?;
                for dtx in [false, true] {
                    let encoded = temp.0.join(format!("{stem}-{dtx}.opus"));
                    let output = temp.0.join(format!("{stem}-{dtx}-decoded.wav"));
                    let mut command = cli();
                    command.arg("encode").arg(&input).arg(&encoded).args([
                        "--mode",
                        "silk",
                        "--fec",
                        "20",
                        "--bitrate",
                        "64000",
                        "--frame-ms",
                        &ms.to_string(),
                    ]);
                    if dtx {
                        command.arg("--dtx");
                    }
                    success(&mut command)?;
                    let mut protected = 0;
                    let mut suppressed = 0;
                    for bytes in audio_packets(&fs::read(&encoded)?)? {
                        let packet = opura::Packet::parse(&bytes)?;
                        if packet.frames()[0].is_empty() {
                            suppressed += 1;
                            continue;
                        }
                        let (header, _) = opura::silk::decode_header(&packet, 0)?;
                        protected +=
                            usize::from((0..header.channel_count()).any(|ch| {
                                header.redundancy(ch).is_some_and(|f| f.iter().any(|&v| v))
                            }));
                    }
                    assert!(protected > 0, "no FEC: {stem}/{dtx}");
                    assert_eq!(suppressed > 0, dtx);
                    success(cli().arg("decode").arg(&encoded).arg(&output))?;
                    output_pcm(
                        &fs::read(output)?,
                        channels,
                        frames * (48000 / rate) as usize,
                    )?;
                }
            }
        }
    }
    Ok(())
}

#[test]
fn signed_header_gain_reaches_frame_decoder_before_exact_file_trimming() -> Result<()> {
    use opura::{
        decoder::OpusDecoder,
        silk::{decoder::PcmChannels, resampler::OutputRate},
    };
    let temp = Temporary::new()?;
    for name in ["silk-mono", "celt-stereo", "hybrid-stereo"] {
        let base = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/audio")
            .join(name);
        let original = fs::read(base.with_extension("opus"))?;
        let baseline = fs::read(base.with_extension("pcm"))?;
        let header = original
            .windows(8)
            .position(|s| s == b"OpusHead")
            .ok_or("missing OpusHead")?;
        let channels = usize::from(original[header + 9]);
        let skip = usize::from(u16::from_le_bytes([
            original[header + 10],
            original[header + 11],
        ]));
        let frames = baseline.len() / 2 / channels;
        let packets = audio_packets(&original)?;
        for gain in [0, i16::MIN, -1536, 1, 1536, i16::MAX] {
            let mut bytes = original.clone();
            bytes[header + 16..header + 18].copy_from_slice(&gain.to_le_bytes());
            rewrite_granules(&mut bytes, |_, g| g)?;
            let source = temp.0.join(format!("{name}-gain{gain}.opus"));
            let output = temp.0.join(format!("{name}-gain{gain}.wav"));
            fs::write(&source, &bytes)?;
            success(cli().arg("decode").arg(&source).arg(&output))?;
            let mut decoder = OpusDecoder::new(
                OutputRate::Khz48,
                if channels == 1 {
                    PcmChannels::Mono
                } else {
                    PcmChannels::Stereo
                },
            )?;
            decoder.set_gain_q8(gain);
            let mut expected = Vec::new();
            for packet in &packets {
                expected.extend_from_slice(decoder.decode(packet)?.samples());
            }
            let expected: Vec<u8> = expected[skip * channels..(skip + frames) * channels]
                .iter()
                .flat_map(|n| n.to_le_bytes())
                .collect();
            let wave = fs::read(output)?;
            let actual = output_pcm(&wave, channels as u16, frames)?;
            assert_eq!(actual, expected, "{name},gain{gain}");
            if gain == 0 {
                assert_eq!(actual, baseline);
            }
            if gain == i16::MIN {
                assert!(actual.iter().all(|&n| n == 0));
            }
            if gain == 1536 || gain == -1536 {
                assert_ne!(actual, baseline, "gain was ignored");
            }
            assert_eq!(fs::read(source)?, bytes, "decoding modified the source");
        }
    }
    Ok(())
}
