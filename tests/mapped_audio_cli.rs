//! File contracts beyond a single mono/stereo stream. No external tools needed.
use std::{error::Error, fs, path::PathBuf, process::Command};
type Result<T> = std::result::Result<T, Box<dyn Error>>;
struct Temporary(PathBuf);
impl Temporary {
    fn new(name: &str) -> Result<Self> {
        let path = std::env::temp_dir().join(format!("opura-mapped-{}-{name}", std::process::id()));
        fs::create_dir(&path)?;
        Ok(Self(path))
    }
}
impl Drop for Temporary {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.0) {
            eprintln!("mapped test cleanup: {error}");
        }
    }
}
fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_opura"))
}
fn success(command: &mut Command) -> Result<()> {
    let output = command.output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}
const MASKS: [u32; 8] = [4, 3, 7, 0x33, 0x37, 0x3f, 0x70f, 0x63f];
fn wave(rate: u32, channels: usize, frames: usize, positioned: bool) -> Vec<u8> {
    let bytes = (frames * channels * 2) as u32;
    let mut out = Vec::from(&b"RIFF"[..]);
    out.extend((60 + bytes).to_le_bytes());
    out.extend(b"WAVEfmt ");
    out.extend(40u32.to_le_bytes());
    out.extend(0xfffeu16.to_le_bytes());
    out.extend((channels as u16).to_le_bytes());
    out.extend(rate.to_le_bytes());
    out.extend((rate * channels as u32 * 2).to_le_bytes());
    out.extend((channels as u16 * 2).to_le_bytes());
    out.extend(16u16.to_le_bytes());
    out.extend(22u16.to_le_bytes());
    out.extend(16u16.to_le_bytes());
    out.extend(if positioned { MASKS[channels - 1] } else { 0 }.to_le_bytes());
    out.extend([1, 0, 0, 0, 0, 0, 16, 0, 128, 0, 0, 170, 0, 56, 155, 113]);
    out.extend(b"data");
    out.extend(bytes.to_le_bytes());
    for frame in 0..frames {
        for channel in 0..channels {
            out.extend((((frame * (73 + channel * 49)) % 6000) as i16 - 3000).to_le_bytes());
        }
    }
    out
}
fn output_pcm(bytes: &[u8], channels: usize, frames: usize, family: usize) -> Result<&[u8]> {
    let extended = channels > 2 || family == 255;
    let offset = if extended { 68 } else { 44 };
    assert!(bytes.len() >= offset);
    assert_eq!(&bytes[..4], b"RIFF");
    assert_eq!(
        u32::from_le_bytes(bytes[4..8].try_into()?) as usize,
        bytes.len() - 8
    );
    assert_eq!(
        u16::from_le_bytes(bytes[22..24].try_into()?) as usize,
        channels
    );
    assert_eq!(u32::from_le_bytes(bytes[24..28].try_into()?), 48000);
    assert_eq!(&bytes[offset - 8..offset - 4], b"data");
    assert_eq!(
        u32::from_le_bytes(bytes[offset - 4..offset].try_into()?) as usize,
        frames * channels * 2
    );
    assert_eq!(bytes.len(), offset + frames * channels * 2);
    if extended {
        assert_eq!(u16::from_le_bytes(bytes[20..22].try_into()?), 0xfffe);
        assert_eq!(
            u32::from_le_bytes(bytes[40..44].try_into()?),
            if family == 255 {
                0
            } else {
                MASKS[channels - 1]
            }
        );
    }
    Ok(&bytes[offset..])
}
fn fingerprint(bytes: &[u8]) -> u64 {
    bytes.iter().fold(14695981039346656037, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(1099511628211)
    })
}

fn repair_first_page(bytes: &mut [u8]) -> Result<()> {
    let laces = usize::from(bytes[26]);
    let length: usize = bytes[27..27 + laces].iter().map(|&n| usize::from(n)).sum();
    let page = &mut bytes[..27 + laces + length];
    page[22..26].fill(0);
    let crc = page.iter().fold(0u32, |mut sum, byte| {
        sum ^= u32::from(*byte) << 24;
        for _ in 0..8 {
            sum = (sum << 1) ^ if sum & 0x80000000 != 0 { 0x04c11db7 } else { 0 };
        }
        sum
    });
    page[22..26].copy_from_slice(&crc.to_le_bytes());
    Ok(())
}

#[test]
fn malformed_mapped_headers_reject_before_creating_output() -> Result<()> {
    let temp = Temporary::new("headers")?;
    let original = fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/audio/mapped/family1-6ch-gain0.opus"),
    )?;
    let input = temp.0.join("bad.opus");
    let output = temp.0.join("bad.wav");
    for (offset, value) in [
        (8, 16),
        (9, 0),
        (9, 9),
        (18, 0),
        (18, 2),
        (19, 0),
        (20, 5),
        (21, 254),
    ] {
        let mut bytes = original.clone();
        bytes[28 + offset] = value;
        repair_first_page(&mut bytes)?;
        fs::write(&input, bytes)?;
        let result = cli().arg("decode").arg(&input).arg(&output).output()?;
        assert!(!result.status.success(), "header offset{offset}");
        assert!(!output.exists());
    }
    Ok(())
}

#[test]
fn mapped_native_files_match_independent_gain_trim_and_wave_order() -> Result<()> {
    let temp = Temporary::new("native")?;
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/audio/mapped");
    let manifest = fs::read_to_string(fixtures.join("manifest.txt"))?;
    let mut cases = 0;
    for line in manifest
        .lines()
        .filter(|s| !s.starts_with('#') && !s.is_empty())
    {
        let fields: Vec<_> = line.split_whitespace().collect();
        assert_eq!(fields.len(), 5);
        let channels = fields[1].parse()?;
        let family = fields[2].parse()?;
        let frames = fields[3].parse()?;
        let hash: u64 = fields[4].parse()?;
        let output = temp.0.join(format!("{cases}.wav"));
        success(
            cli()
                .arg("decode")
                .arg(fixtures.join(fields[0]))
                .arg(&output),
        )?;
        let bytes = fs::read(output)?;
        assert_eq!(
            fingerprint(output_pcm(&bytes, channels, frames, family)?),
            hash,
            "{}",
            fields[0]
        );
        cases += 1;
    }
    assert_eq!(cases, 60);
    Ok(())
}

#[test]
fn mapped_encode_preserves_layout_duration_and_empty_tail_at_all_rates() -> Result<()> {
    let temp = Temporary::new("duration")?;
    let mut cases = 0;
    for rate in [8000, 12000, 16000, 24000, 48000] {
        for channels in 1..=8 {
            for positioned in [true, false] {
                for frames in [0, 1, rate as usize / 50 + 1] {
                    let input = temp.0.join(format!("{cases}.wav"));
                    let encoded = temp.0.join(format!("{cases}.opus"));
                    let output = temp.0.join(format!("{cases}-out.wav"));
                    fs::write(&input, wave(rate, channels, frames, positioned))?;
                    let mut command = cli();
                    command.arg("encode").arg(&input).arg(&encoded).args([
                        "--mode",
                        "auto",
                        "--complexity",
                        "0",
                    ]);
                    let (app, skip) = [
                        ("audio", 312),
                        ("voip", 312),
                        ("lowdelay", 120),
                        ("restricted-silk", 312),
                        ("restricted-celt", 120),
                    ][cases % 5];
                    command.args(["--application", app]);
                    if cases % 3 == 1 {
                        command.arg("--vbr");
                    }
                    if cases % 3 == 2 {
                        command.arg("--cvbr");
                    }
                    if cases % 4 == 1 {
                        command.args(["--packet-ms", "100"]);
                    }
                    if cases % 4 == 2 {
                        command.args(["--packet-ms", "120"]);
                    }
                    success(&mut command)?;
                    let bytes = fs::read(&encoded)?;
                    // The first OpusHead has exactly one lacing entry.
                    assert_eq!(&bytes[28..36], b"OpusHead");
                    assert_eq!(bytes[37] as usize, channels);
                    assert_eq!(u16::from_le_bytes(bytes[38..40].try_into()?), skip);
                    let family = if !positioned {
                        255
                    } else if channels > 2 {
                        1
                    } else {
                        0
                    };
                    assert_eq!(bytes[46] as usize, family);
                    success(cli().arg("decode").arg(&encoded).arg(&output))?;
                    output_pcm(
                        &fs::read(output)?,
                        channels,
                        frames * (48000 / rate) as usize,
                        family,
                    )?;
                    cases += 1;
                }
            }
        }
    }
    assert_eq!(cases, 240);
    Ok(())
}

#[test]
fn mapped_preflight_rejects_invalid_masks_controls_budgets_without_output() -> Result<()> {
    let temp = Temporary::new("errors")?;
    let input = temp.0.join("input.wav");
    let output = temp.0.join("output.opus");
    for positioned in [true, false] {
        fs::write(&input, wave(48000, 6, 961, positioned))?;
        for args in [
            vec!["--mode", "celt"],
            vec!["--mode", "silk"],
            vec!["--mode", "hybrid"],
            vec!["--mode", "auto", "--channels", "2"],
            vec!["--mode", "auto", "--max-packet-bytes", "1"],
            vec!["--mode", "auto", "--max-packet-bytes", "16777217"],
            vec!["--mode", "auto", "--bitrate", "191250001"],
        ] {
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
    }
    let mut bad = wave(48000, 6, 0, true);
    bad[40..44].copy_from_slice(&0x63fu32.to_le_bytes());
    fs::write(&input, bad)?;
    assert!(
        !cli()
            .arg("encode")
            .arg(&input)
            .arg(&output)
            .args(["--mode", "auto"])
            .output()?
            .status
            .success()
    );
    assert!(!output.exists());
    // Unpositioned mono/stereo must not silently become family0 in fixed modes.
    for channels in [1, 2] {
        fs::write(&input, wave(48000, channels, 0, false))?;
        assert!(
            !cli()
                .arg("encode")
                .arg(&input)
                .arg(&output)
                .output()?
                .status
                .success()
        );
        assert!(!output.exists());
    }
    Ok(())
}

#[test]
fn mapped_encoder_uses_wave_input_order_and_whole_packet_controls() -> Result<()> {
    use opura::{
        celt::input::InputRate,
        encoder::{
            PacketSize,
            control::{Application, Controls},
        },
        multistream::{ChannelMapping, MultistreamEncodeOptions, MultistreamEncoder},
    };
    let temp = Temporary::new("input-order")?;
    // Captured native OpusHead provides coded order; these permutations were
    // observed independently with FFmpeg, as recorded by the fixture manifest.
    let orders: [&[usize]; 8] = [
        &[0],
        &[0, 1],
        &[0, 2, 1],
        &[0, 1, 2, 3],
        &[0, 2, 1, 3, 4],
        &[0, 2, 1, 5, 3, 4],
        &[0, 2, 1, 6, 5, 3, 4],
        &[0, 2, 1, 7, 5, 6, 3, 4],
    ];
    for channels in 1..=8 {
        for positioned in [false, true] {
            if positioned && channels <= 2 {
                continue;
            }
            let input = temp.0.join(format!("{channels}-{positioned}.wav"));
            let output = input.with_extension("opus");
            let source = wave(48000, channels, 961, positioned);
            fs::write(&input, &source)?;
            success(cli().arg("encode").arg(&input).arg(&output).args([
                "--mode",
                "auto",
                "--application",
                "restricted-celt",
                "--complexity",
                "0",
            ]))?;
            let (streams, coupled, mapping) = if positioned {
                let native = fs::read(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!(
                    "tests/audio/mapped/family1-{channels}ch-gain0.opus"
                )))?;
                let head = &native[28..];
                (
                    usize::from(head[19]),
                    usize::from(head[20]),
                    orders[channels - 1]
                        .iter()
                        .map(|&i| head[21 + i])
                        .collect::<Vec<_>>(),
                )
            } else {
                (channels, 0, (0..channels as u8).collect())
            };
            let map = ChannelMapping::new(streams, coupled, &mapping)?;
            let mut owner = MultistreamEncoder::with_application(
                InputRate::Khz48,
                map,
                Application::RestrictedCelt,
            )?;
            let mut expected = Vec::new();
            let original: Vec<i16> = source[68..]
                .chunks_exact(2)
                .map(|b| i16::from_le_bytes([b[0], b[1]]))
                .collect();
            for index in 0..2 {
                let mut pcm = vec![0; 960 * channels];
                let offset = index * pcm.len();
                let n = pcm.len().min(original.len().saturating_sub(offset));
                pcm[..n].copy_from_slice(&original[offset..offset + n]);
                expected.push(
                    owner
                        .encode_packet(
                            PacketSize::Ms20,
                            &pcm,
                            1276 * streams + 2 * (streams - 1),
                            MultistreamEncodeOptions {
                                bitrate: 64000 * streams as u32,
                                controls: Controls {
                                    application: Application::RestrictedCelt,
                                    variable: false,
                                    complexity: 0,
                                    ..Controls::default()
                                },
                                constrained: false,
                            },
                        )?
                        .as_bytes()
                        .to_vec(),
                );
            }
            let encoded = fs::read(output)?;
            let mut offset = 0;
            let mut pending = Vec::new();
            let mut packets = Vec::new();
            while offset < encoded.len() {
                let n = usize::from(encoded[offset + 26]);
                let laces = &encoded[offset + 27..offset + 27 + n];
                offset += 27 + n;
                for &length in laces {
                    let end = offset + usize::from(length);
                    pending.extend_from_slice(&encoded[offset..end]);
                    offset = end;
                    if length < 255 {
                        packets.push(std::mem::take(&mut pending));
                    }
                }
            }
            assert!(pending.is_empty());
            assert_eq!(
                packets[2..],
                expected,
                "channels{channels} positioned{positioned}"
            );
        }
    }
    Ok(())
}
