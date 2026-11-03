//! Actual native common-owner targets, fades and CELT calls with masks/LFE.
use super::*;
use crate::encoder::SurroundContext;
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
#[test]
fn actual_native_common_surround_targets_gains_width_and_celt_calls_match() -> Result<()> {
    check_common(
        include_str!("../../tests/surround-common-fixtures.txt"),
        false,
    )
}
#[test]
fn actual_native_long_surround_prefills_and_shared_mask_lifetimes_match() -> Result<()> {
    check_common(
        include_str!("../../tests/surround-common-long-fixtures.txt"),
        true,
    )
}
fn check_common(fixture: &str, long: bool) -> Result<()> {
    let mut owner = None;
    let mut lfe = false;
    let mut rate_mode = 0;
    let mut rows = 0;
    for line in fixture.lines().filter(|l| !l.starts_with('#')) {
        let mut fields: Vec<_> = line.split_whitespace().collect();
        let duration = if long && fields[0] == "frame" {
            fields.remove(2).parse::<usize>()?
        } else {
            960
        };
        match fields[0] {
            "case" => {
                let rate = match fields[1] {
                    "16000" => InputRate::Khz16,
                    "24000" => InputRate::Khz24,
                    "48000" => InputRate::Khz48,
                    _ => return Err("invalid native rate".into()),
                };
                lfe = fields[3] == "1";
                let application = match fields[4] {
                    "2049" => control::Application::Audio,
                    "2048" => control::Application::Voip,
                    "2051" => control::Application::RestrictedLowDelay,
                    "2052" => control::Application::RestrictedSilk,
                    "2053" => control::Application::RestrictedCelt,
                    _ => return Err("invalid application".into()),
                };
                rate_mode = fields[5].parse::<u8>()?;
                owner = Some(OpusEncoder::with_application(
                    rate,
                    fields[2].parse()?,
                    application,
                )?);
            }
            "frame" => {
                let owner = owner.as_mut().ok_or("missing common owner")?;
                let step = fields[1].parse::<usize>()?;
                let mode = match fields[2] {
                    "1000" => Mode::Silk,
                    "1001" => Mode::Hybrid,
                    "1002" => Mode::Celt,
                    _ => return Err("invalid mode".into()),
                };
                let band = match fields[3] {
                    "1101" => Bandwidth::Narrowband,
                    "1103" => Bandwidth::Wideband,
                    "1105" => Bandwidth::Fullband,
                    _ => return Err("invalid bandwidth".into()),
                };
                let bitrate = fields[4].parse()?;
                let kind = fields[5].parse::<u8>()?;
                let mut masks = [[0; 21]; 2];
                for (channel, values) in masks[..owner.channels].iter_mut().enumerate() {
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
                        Some(&masks[..owner.channels])
                    },
                    owner.channels,
                )?);
                let mut input = Vec::new();
                let mut seed = 6716u32.wrapping_add(37 * step as u32);
                for frame in 0..duration / owner.rate.factor() {
                    for channel in 0..owner.channels {
                        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                        let phase = (frame + step * 13 + channel * 37) % (31 + 11 * channel);
                        input.push(
                            ((phase as i32 - (15 + 5 * channel as i32)) * 173
                                + ((seed >> 24) as i32 - 128) * 3)
                                as i16,
                        );
                    }
                }
                let input_hash = input
                    .iter()
                    .flat_map(|x| x.to_le_bytes())
                    .fold(14695981039346656037u64, |h, b| {
                        (h ^ u64::from(b)).wrapping_mul(1099511628211)
                    });
                assert_eq!(input_hash, fields[6].parse()?);
                let packet = owner.encode_packet(
                    PacketSize::try_from(duration)?,
                    &input,
                    640,
                    EncodeOptions {
                        bitrate,
                        controls: control::Controls {
                            application: owner.application,
                            complexity: 0,
                            channels: Some(owner.channels),
                            mode: Some(mode),
                            bandwidth: Some(band),
                            variable: rate_mode != 0,
                            ..control::Controls::default()
                        },
                        constrained: rate_mode == 2,
                    },
                )?;
                assert_eq!(
                    owner.surround_silk_rate,
                    fields[7].parse()?,
                    "SILK target row{rows}"
                );
                assert_eq!(
                    owner.high_gain,
                    fields[8].parse::<i16>()?,
                    "HB gain row{rows}"
                );
                assert_eq!(owner.width, fields[9].parse::<i16>()?, "width row{rows}");
                let actual_mode = match packet.decision().mode {
                    Mode::Silk => 1000,
                    Mode::Hybrid => 1001,
                    Mode::Celt => 1002,
                };
                assert_eq!(actual_mode, fields[10].parse::<i32>()?, "mode row{rows}");
                let actual_band = match packet.decision().bandwidth {
                    Bandwidth::Narrowband => 1101,
                    Bandwidth::Mediumband => 1102,
                    Bandwidth::Wideband => 1103,
                    Bandwidth::SuperWideband => 1104,
                    Bandwidth::Fullband => 1105,
                };
                assert_eq!(actual_band, fields[11].parse::<i32>()?, "band row{rows}");
                let expected: Vec<u64> = fields[12..]
                    .iter()
                    .map(|n| n.parse())
                    .collect::<std::result::Result<_, _>>()?;
                let count = expected[0] as usize;
                assert_eq!(expected.len(), 1 + 7 * count);
                assert_eq!(owner.celt.call_trace.len(), count, "CELT count row{rows}");
                assert_eq!(owner.celt.surround_calls.len(), count);
                for (index, (actual, context)) in owner
                    .celt
                    .call_trace
                    .iter()
                    .zip(&owner.celt.surround_calls)
                    .enumerate()
                {
                    let expected = &expected[1 + 7 * index..1 + 7 * (index + 1)];
                    assert_eq!(
                        actual.as_slice(),
                        &expected[..5],
                        "CELT call row{rows} call{index}"
                    );
                    assert_eq!(
                        context.as_slice(),
                        &expected[5..],
                        "CELT mask/LFE row{rows} call{index}"
                    );
                }
                rows += 1;
            }
            _ => return Err("invalid native common record".into()),
        }
    }
    assert_eq!(rows, 3240);
    Ok(())
}
