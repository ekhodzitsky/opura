use super::*;
#[test]
fn corrected_q24_spectral_patch_decisions_match() -> Result<(), Box<dyn std::error::Error>> {
    let mut count = 0;
    let mut selected = 0;
    for line in include_str!("../../tests/celt-patch-corrected-q24-fixtures.txt")
        .lines()
        .filter(|l| !l.starts_with('#'))
    {
        let v = line
            .split_whitespace()
            .map(str::parse::<i32>)
            .collect::<Result<Vec<_>, _>>()?;
        assert_eq!(v.len(), 89);
        let mut current = [[0; 21]; 2];
        let mut previous = current;
        current[0].copy_from_slice(&v[3..24]);
        current[1].copy_from_slice(&v[24..45]);
        previous[0].copy_from_slice(&v[45..66]);
        previous[1].copy_from_slice(&v[66..87]);
        let patched = analysis::patch_missed(
            &current,
            &previous,
            v[0] as usize,
            v[1] as usize,
            v[2] as usize,
        );
        assert_eq!(v[87], 0, "pristine fixed-point narrowing: {line}");
        assert_eq!(patched, v[88] != 0, "{line}");
        count += 1;
        selected += usize::from(patched);
    }
    assert_eq!(count, 768);
    assert!(selected > 0);
    Ok(())
}
#[test]
fn corrected_q24_native_complexity_transient_and_spreading_histories_match()
-> Result<(), Box<dyn std::error::Error>> {
    let mut encoder = CeltEncoder::new(InputRate::Khz8, 1)?;
    let mut count = 0;
    let mut equal_bytes = 0;
    let mut decoder = crate::decoder::OpusDecoder::new(
        crate::silk::resampler::OutputRate::Khz48,
        crate::silk::decoder::PcmChannels::Mono,
    )?;
    let mut patched = 0;
    let mut stress_stream = false;
    for line in include_str!("../../tests/celt-quality-corrected-q24-fixtures.txt")
        .lines()
        .filter(|l| !l.starts_with('#'))
    {
        let fields: Vec<_> = line.split_whitespace().collect();
        let v = fields[..17]
            .iter()
            .map(|s| s.parse::<i128>())
            .collect::<Result<Vec<_>, _>>()?;
        assert_eq!(fields.len(), 18);
        assert_eq!(v.len(), 17);
        let rate = match v[0] {
            8000 => InputRate::Khz8,
            12000 => InputRate::Khz12,
            16000 => InputRate::Khz16,
            24000 => InputRate::Khz24,
            _ => InputRate::Khz48,
        };
        let channels = v[1] as usize;
        let size = match v[2] {
            0 => FrameSize::Ms2_5,
            1 => FrameSize::Ms5,
            2 => FrameSize::Ms10,
            _ => FrameSize::Ms20,
        };
        if v[5] == 0 {
            encoder = CeltEncoder::new(rate, channels)?;
            decoder = crate::decoder::OpusDecoder::new(
                crate::silk::resampler::OutputRate::Khz48,
                if channels == 1 {
                    crate::silk::decoder::PcmChannels::Mono
                } else {
                    crate::silk::decoder::PcmChannels::Stereo
                },
            )?;
            stress_stream = v[4] == 2;
        }
        encoder.set_native_controls(NativeControls {
            complexity: v[3] as u8,
            loss_percent: 0,
            prediction: 2,
        })?;
        let mut rng = v[6] as u32;
        // Third stream has alternating tiny and normal capacities and full-scale
        // stationary noise; the other streams use ramp and square-wave attacks.
        let stress =
            (v[5] % 2 == 0 && v[4] == 2) || (v[5] % 2 == 1 && v[4] == 160 && stress_stream);
        let scale = if stress {
            32767
        } else {
            [0, 128, 512, 2048, 8192, 30000, 32, 24000][v[5] as usize]
        };
        let pcm: Vec<_> = (0..size.samples() / rate.factor() * channels)
            .map(|i| {
                rng ^= rng << 13;
                rng ^= rng >> 17;
                rng ^= rng << 5;
                let mut x = if stress {
                    if rng & 1 != 0 { 32767 } else { -32768 }
                } else {
                    (rng & 65535) as i32 - 32768
                };
                if v[5] == 7 && !stress {
                    x = if (i / channels) % 80 < 40 {
                        32767
                    } else {
                        -32768
                    };
                }
                ((x * scale) >> 15) as i16
            })
            .collect();
        let packet = encoder
            .encode_stream(size, &pcm, v[4] as usize, None)
            .map_err(|e| format!("{e}: {line}"))?;
        let mut entropy = crate::range::RangeDecoder::new(&packet.as_bytes()[1..]);
        let prefix = control::decode_prefix(&mut entropy, size, 0)?;
        assert_eq!(prefix.transient(), v[9] != 0, "transient: {line}");
        assert_eq!(
            encoder.spreading.decision() as i128,
            v[10],
            "spread: {line}"
        );
        assert_eq!(
            i128::from(encoder.spreading.tonal_average()),
            v[11],
            "tonal average: {line}"
        );
        assert_eq!(
            i128::from(encoder.spreading.hf_average()),
            v[12],
            "HF average: {line}"
        );
        assert_eq!(
            i128::from(encoder.spreading.tapset()),
            v[13],
            "tapset: {line}"
        );
        assert_eq!(
            i128::from(encoder.vbr.spectral_average),
            v[14],
            "temporal VBR history: {line}"
        );
        let hex: String = packet.as_bytes()[1..]
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        equal_bytes += usize::from(hex == fields[17]);
        assert_eq!(
            packet.final_range(),
            v[15] as u32,
            "corrected native range: {line}"
        );
        let decoded = decoder.decode(packet.as_bytes())?;
        let hash = decoded
            .samples()
            .iter()
            .flat_map(|x| x.to_le_bytes())
            .fold(14695981039346656037u64, |h, b| {
                (h ^ u64::from(b)).wrapping_mul(1099511628211)
            });
        assert_eq!(hash, v[16] as u64, "corrected native PCM: {line}");
        assert_eq!(
            decoded.final_range(),
            packet.final_range(),
            "own packet range: {line}"
        );
        patched += usize::from(v[8] == 0 && v[9] != 0);
        count += 1;
    }
    eprintln!("corrected Q24 byte-identical packets:{equal_bytes}/{count}; patched:{patched}");
    assert_eq!(count, 10560);
    assert_eq!(patched, 966);
    assert_eq!(equal_bytes, 10557);
    Ok(())
}
