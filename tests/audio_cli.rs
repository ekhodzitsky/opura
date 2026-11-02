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
        for bandwidth in ["nb", "mb", "wb"] {
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
    assert_eq!(case, 120);
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
