//! Independent actual CELT packet, decoded PCM and persistent mask/LFE state.
use super::*;
use crate::{
    decoder::OpusDecoder,
    encoder::SurroundContext,
    silk::{decoder::PcmChannels, resampler::OutputRate},
};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn signal(rate: InputRate, channels: usize, duration: usize, step: usize) -> Vec<i16> {
    let count = duration / rate.factor();
    let mut seed = 6716u32.wrapping_add(37 * step as u32);
    let mut result = Vec::new();
    for frame in 0..count {
        for channel in 0..channels {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let phase = (frame + step * 13 + channel * 37) % (31 + 11 * channel);
            let triangle = (phase as i32 - (15 + 5 * channel as i32)) * 173;
            let value = match step % 8 {
                0 => 0,
                1 => {
                    if frame == count / 2 + channel {
                        30000
                    } else {
                        0
                    }
                }
                2 => (seed >> 16) as i32 - 32768,
                3 => {
                    if phase % 9 < 4 {
                        20000
                    } else {
                        -20000
                    }
                }
                4 => {
                    if channel == 1 {
                        triangle
                    } else {
                        0
                    }
                }
                5 => {
                    if (frame + channel) % 2 == 0 {
                        -32768
                    } else {
                        32767
                    }
                }
                6 => triangle >> 10,
                _ => triangle + ((seed >> 24) as i32 - 128) * 3,
            };
            result.push(value as i16);
        }
    }
    result
}
fn hash(values: &[i16]) -> u64 {
    values
        .iter()
        .flat_map(|x| x.to_le_bytes())
        .fold(14695981039346656037, |h, b| {
            (h ^ u64::from(b)).wrapping_mul(1099511628211)
        })
}
#[test]
fn actual_native_surround_masks_and_lfe_packets_pcm_and_histories_match() -> Result<()> {
    let mut owner = None;
    let mut decoder = None;
    let mut lfe = false;
    let mut rows = 0;
    for line in include_str!("../../tests/surround-celt-fixtures.txt")
        .lines()
        .filter(|l| !l.starts_with('#'))
    {
        let fields: Vec<_> = line.split_whitespace().collect();
        match fields[0] {
            "case" => {
                let rate = match fields[1] {
                    "8000" => InputRate::Khz8,
                    "12000" => InputRate::Khz12,
                    "16000" => InputRate::Khz16,
                    "24000" => InputRate::Khz24,
                    "48000" => InputRate::Khz48,
                    _ => return Err("unexpected input rate".into()),
                };
                let channels = fields[2].parse()?;
                lfe = fields[3] == "1";
                owner = Some(CeltEncoder::new(rate, channels)?);
                decoder = Some(OpusDecoder::new(
                    OutputRate::Khz48,
                    if channels == 1 {
                        PcmChannels::Mono
                    } else {
                        PcmChannels::Stereo
                    },
                )?);
            }
            "frame" => {
                assert_eq!(fields.len(), 27);
                let owner = owner.as_mut().ok_or("missing owner")?;
                let decoder = decoder.as_mut().ok_or("missing decoder")?;
                let step: usize = fields[1].parse()?;
                if fields[2] == "1" {
                    owner.reset();
                    decoder.reset();
                }
                let size = [
                    FrameSize::Ms2_5,
                    FrameSize::Ms5,
                    FrameSize::Ms10,
                    FrameSize::Ms20,
                ][fields[3].parse::<usize>()?];
                let complexity = fields[4].parse()?;
                let prediction = fields[5].parse()?;
                let mode: u8 = fields[6].parse()?;
                let bitrate = fields[7].parse()?;
                let cap = fields[8].parse()?;
                let kind: u8 = fields[9].parse()?;
                let mut masks = [[0; 21]; 2];
                for (channel, values) in masks[..owner.channels()].iter_mut().enumerate() {
                    for (band, value) in values.iter_mut().enumerate() {
                        *value = if kind == 1 {
                            0
                        } else {
                            ((step as i32 * 719 + band as i32 * 317 + channel as i32 * 151)
                                % (4 * 4096)
                                - 3 * 4096)
                                * 4096
                        };
                    }
                }
                owner.set_surround_context(SurroundContext::new(
                    lfe,
                    if kind == 0 {
                        None
                    } else {
                        Some(&masks[..owner.channels()])
                    },
                    owner.channels(),
                )?);
                if lfe {
                    owner.set_bandwidth(crate::Bandwidth::Narrowband)?;
                }
                owner.set_native_controls(NativeControls {
                    complexity,
                    prediction,
                    loss_percent: 0,
                })?;
                let input = signal(owner.input_rate(), owner.channels(), size.samples(), step);
                assert_eq!(hash(&input), fields[10].parse()?, "input row{rows}");
                let hex = fields[11];
                let expected: Vec<u8> = (0..hex.len())
                    .step_by(2)
                    .map(|i| u8::from_str_radix(&hex[i..i + 2], 16))
                    .collect::<std::result::Result<_, _>>()?;
                let packet = owner.encode_stream(
                    size,
                    &input,
                    cap,
                    if mode == 0 {
                        None
                    } else {
                        Some((bitrate, mode == 2))
                    },
                )?;
                assert_eq!(
                    packet.as_bytes(),
                    expected,
                    "packet row{rows} step{step} lfe{lfe}"
                );
                let output = decoder.decode(packet.as_bytes())?;
                assert_eq!(hash(output.samples()), fields[12].parse()?, "PCM row{rows}");
                let expected: Vec<i64> = fields[13..]
                    .iter()
                    .map(|v| v.parse())
                    .collect::<std::result::Result<_, _>>()?;
                let actual = [
                    owner.previous_coded as i64,
                    owner.intensity.band() as i64,
                    owner.spreading.decision() as i64,
                    owner.spreading.tapset() as i64,
                    owner.spreading.tonal_average() as i64,
                    owner.spreading.hf_average() as i64,
                    owner.vbr.spectral_average as i64,
                    owner.vbr.reservoir as i64,
                    owner.vbr.drift as i64,
                    owner.vbr.offset as i64,
                    owner.vbr.count as i64,
                    owner.prefilter.parameters().period() as i64,
                    owner.prefilter.parameters().gain() as i64,
                    i64::from(packet.final_range() as i32),
                ];
                assert_eq!(
                    actual.as_slice(),
                    expected,
                    "state row{rows} step{step} lfe{lfe}"
                );
                assert_eq!(packet.final_range(), output.final_range());
                rows += 1;
            }
            _ => return Err("unexpected native record".into()),
        }
    }
    assert_eq!(rows, 1920);
    Ok(())
}
