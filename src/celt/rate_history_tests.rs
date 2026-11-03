use super::*;

#[test]
fn corrected_q24_native_rate_switches_and_feedback_match() -> Result<(), Box<dyn std::error::Error>>
{
    check_histories(
        include_str!("../../tests/celt-rate-history-corrected-q24-fixtures.txt"),
        false,
    )
}

#[test]
fn corrected_q24_native_prediction_loss_band_and_wire_controls_match()
-> Result<(), Box<dyn std::error::Error>> {
    check_histories(
        include_str!("../../tests/celt-control-history-corrected-q24-fixtures.txt"),
        true,
    )
}

fn check_histories(fixture: &str, controlled: bool) -> Result<(), Box<dyn std::error::Error>> {
    use crate::{
        decoder::OpusDecoder,
        silk::{decoder::PcmChannels, resampler::OutputRate},
    };
    let mut encoder = CeltEncoder::new(InputRate::Khz48, 1)?;
    let mut decoder = OpusDecoder::new(OutputRate::Khz48, PcmChannels::Mono)?;
    let mut count = 0;
    let mut identical_bytes = 0;
    let mut maximum_count = 0;
    let mut modes = [0usize; 3];
    let mut predictions = [false; 3];
    let mut losses = [false; 101];
    let mut active_prefilters = 0;
    let extra = if controlled { 4 } else { 0 };
    for line in fixture.lines().filter(|line| !line.starts_with('#')) {
        let fields: Vec<_> = line.split_whitespace().collect();
        assert_eq!(fields.len(), 27 + extra);
        let v = fields[..26 + extra]
            .iter()
            .map(|field| field.parse::<i128>())
            .collect::<Result<Vec<_>, _>>()?;
        let rate = match v[0] {
            8000 => InputRate::Khz8,
            12000 => InputRate::Khz12,
            16000 => InputRate::Khz16,
            24000 => InputRate::Khz24,
            48000 => InputRate::Khz48,
            _ => return Err("invalid fixture rate".into()),
        };
        let channels = v[1] as usize;
        let size = match v[4] {
            0 => FrameSize::Ms2_5,
            1 => FrameSize::Ms5,
            2 => FrameSize::Ms10,
            3 => FrameSize::Ms20,
            _ => return Err("invalid fixture duration".into()),
        };
        if v[3] == 0 {
            encoder = CeltEncoder::new(rate, channels)?;
            decoder = OpusDecoder::new(
                OutputRate::Khz48,
                if channels == 1 {
                    PcmChannels::Mono
                } else {
                    PcmChannels::Stereo
                },
            )?;
        }
        encoder.set_native_controls(NativeControls {
            complexity: v[5] as u8,
            loss_percent: if controlled { v[14] as u8 } else { 0 },
            prediction: if controlled { v[13] as u8 } else { 2 },
        })?;
        encoder.set_encoded_channels(v[11] as usize)?;
        if controlled {
            encoder.set_bandwidth(match v[12] {
                13 => crate::Bandwidth::Narrowband,
                17 => crate::Bandwidth::Wideband,
                19 => crate::Bandwidth::SuperWideband,
                21 => crate::Bandwidth::Fullband,
                _ => return Err("invalid fixture end band".into()),
            })?;
        }
        let mut rng = v[9] as u32;
        let pcm: Vec<_> = (0..size.samples() / rate.factor() * channels)
            .map(|i| {
                rng ^= rng << 13;
                rng ^= rng >> 17;
                rng ^= rng << 5;
                let mut x = (rng & 65535) as i32 - 32768;
                if controlled && v[15] == 1 {
                    let period = v[0] as usize / 200;
                    let n = size.samples() / rate.factor();
                    let phase =
                        (v[3] as usize * n + i / channels + (i % channels) * (period / 4)) % period;
                    x = (if phase < period / 2 { 16384 } else { -16384 }) + (x >> 4);
                } else if controlled && v[15] == 2 {
                    let n = size.samples() / rate.factor();
                    let phase = v[3] as usize * n + i / channels;
                    x = if phase % (v[0] as usize / 100) == 0 {
                        32767
                    } else {
                        0
                    };
                }
                ((x * v[10] as i32) >> 15) as i16
            })
            .collect();
        let packet = encoder
            .encode_stream(
                size,
                &pcm,
                v[6] as usize,
                (v[7] != 0).then_some((v[8] as u32, v[7] == 2)),
            )
            .map_err(|error| format!("{error}: {line}"))?;
        let histories = [
            i128::from(encoder.spreading.decision() as u8),
            i128::from(encoder.spreading.tonal_average()),
            i128::from(encoder.spreading.hf_average()),
            i128::from(encoder.spreading.tapset()),
            i128::from(encoder.vbr.spectral_average),
            i128::from(encoder.vbr.reservoir),
            i128::from(encoder.vbr.drift),
            i128::from(encoder.vbr.offset),
            i128::from(encoder.vbr.count),
            encoder.intensity.band() as i128,
        ];
        assert_eq!(histories, v[14 + extra..24 + extra], "histories: {line}");
        assert_eq!(
            packet.as_bytes().len() - 1,
            fields[26 + extra].len() / 2,
            "size: {line}"
        );
        assert_eq!(packet.final_range(), v[24 + extra] as u32, "range: {line}");
        let decoded = decoder.decode(packet.as_bytes())?;
        let hash = decoded
            .samples()
            .iter()
            .flat_map(|x| x.to_le_bytes())
            .fold(14695981039346656037u64, |h, b| {
                (h ^ u64::from(b)).wrapping_mul(1099511628211)
            });
        assert_eq!(hash, v[25 + extra] as u64, "PCM: {line}");
        let hex: String = packet.as_bytes()[1..]
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        identical_bytes += usize::from(hex == fields[26 + extra]);
        if controlled {
            predictions[v[13] as usize] = true;
            losses[v[14] as usize] = true;
            active_prefilters += usize::from(encoder.prefilter.parameters().gain() != 0);
        }
        maximum_count = maximum_count.max(encoder.vbr.count);
        modes[v[7] as usize] += 1;
        count += 1;
    }
    eprintln!("rate-history byte-identical packets:{identical_bytes}/{count}");
    assert_eq!(count, if controlled { 6144 } else { 5132 });
    assert_eq!(identical_bytes, count);
    if !controlled {
        assert_eq!(maximum_count, 970);
    }
    assert!(modes.iter().all(|&count| count > 0));
    if controlled {
        assert!(predictions.into_iter().all(|seen| seen));
        assert!(
            [0, 1, 5, 10, 20, 50, 100]
                .into_iter()
                .all(|loss| losses[loss])
        );
        assert!(active_prefilters >= 50);
        eprintln!("control-history active prefilters:{active_prefilters}");
    }
    Ok(())
}
