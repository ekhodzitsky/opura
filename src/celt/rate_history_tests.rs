use super::*;

#[test]
fn corrected_q24_native_rate_switches_and_feedback_match() -> Result<(), Box<dyn std::error::Error>>
{
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
    for line in include_str!("../../tests/celt-rate-history-corrected-q24-fixtures.txt")
        .lines()
        .filter(|line| !line.starts_with('#'))
    {
        let fields: Vec<_> = line.split_whitespace().collect();
        assert_eq!(fields.len(), 27);
        let v = fields[..26]
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
            loss_percent: 0,
            prediction: 2,
        })?;
        encoder.set_encoded_channels(v[11] as usize)?;
        let mut rng = v[9] as u32;
        let pcm: Vec<_> = (0..size.samples() / rate.factor() * channels)
            .map(|_| {
                rng ^= rng << 13;
                rng ^= rng >> 17;
                rng ^= rng << 5;
                ((((rng & 65535) as i32 - 32768) * v[10] as i32) >> 15) as i16
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
        assert_eq!(histories, v[14..24], "histories: {line}");
        assert_eq!(
            packet.as_bytes().len() - 1,
            fields[26].len() / 2,
            "size: {line}"
        );
        assert_eq!(packet.final_range(), v[24] as u32, "range: {line}");
        let decoded = decoder.decode(packet.as_bytes())?;
        let hash = decoded
            .samples()
            .iter()
            .flat_map(|x| x.to_le_bytes())
            .fold(14695981039346656037u64, |h, b| {
                (h ^ u64::from(b)).wrapping_mul(1099511628211)
            });
        assert_eq!(hash, v[25] as u64, "PCM: {line}");
        let hex: String = packet.as_bytes()[1..]
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        identical_bytes += usize::from(hex == fields[26]);
        maximum_count = maximum_count.max(encoder.vbr.count);
        modes[v[7] as usize] += 1;
        count += 1;
    }
    eprintln!("rate-history byte-identical packets:{identical_bytes}/{count}");
    assert_eq!(count, 5132);
    assert_eq!(identical_bytes, count);
    assert_eq!(maximum_count, 970);
    assert!(modes.iter().all(|&count| count > 0));
    Ok(())
}
