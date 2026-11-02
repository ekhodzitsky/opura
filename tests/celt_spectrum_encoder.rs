//! Pinned full-spectrum entropy, reconstruction and stereo candidate evidence.
use opura::{
    celt::{
        FrameSize, allocation, control,
        energy::{BandLayout, EnergyState},
        partition::{self, SpectrumEncoding, SpectrumInput},
        pvq::Spread,
    },
    range::{RangeDecoder, RangeEncoder},
};
const SIZES: [FrameSize; 4] = [
    FrameSize::Ms2_5,
    FrameSize::Ms5,
    FrameSize::Ms10,
    FrameSize::Ms20,
];
const SPREADS: [Spread; 4] = [
    Spread::None,
    Spread::Light,
    Spread::Normal,
    Spread::Aggressive,
];
fn numbers(line: &str) -> Result<Vec<i64>, Box<dyn std::error::Error>> {
    Ok(line
        .split_whitespace()
        .skip(1)
        .map(|s| s.parse())
        .collect::<Result<_, _>>()?)
}
fn replay(writer: &mut RangeEncoder, symbols: &[i64]) -> Result<(), Box<dyn std::error::Error>> {
    let mut at = 0;
    while at < symbols.len() {
        let kind = symbols[at];
        let value = symbols[at + 1] as u32;
        let arg = symbols[at + 2] as u32;
        let len = symbols[at + 3] as usize;
        at += 4;
        match kind {
            0 => writer.encode_bit_logp(value != 0, arg)?,
            1 => writer.encode_bits(value, arg)?,
            2 => writer.encode_uint(value, arg)?,
            3 => {
                let icdf: Vec<u8> = symbols[at..at + len].iter().map(|&x| x as u8).collect();
                writer.encode_icdf(value as usize, &icdf, arg)?;
            }
            _ => return Err("unknown prefix symbol".into()),
        }
        at += len;
    }
    Ok(())
}
#[test]
fn original_full_spectrum_bytes_channels_masks_seed_and_counters_match()
-> Result<(), Box<dyn std::error::Error>> {
    let mut lines = include_str!("celt-spectrum-encoder-fixtures.txt")
        .lines()
        .filter(|l| !l.starts_with('#'));
    let mut rows = 0;
    let mut dual = 0;
    let mut rdo = 0;
    while let Some(line) = lines.next() {
        let f: Vec<_> = line.split_whitespace().collect();
        assert_eq!(f[0], "frame");
        let lm = f[1].parse::<usize>()?;
        let channels = f[2].parse::<usize>()?;
        let start = f[3].parse::<usize>()?;
        let end = f[4].parse::<usize>()?;
        let transient = f[5] == "1";
        let spread = SPREADS[f[6].parse::<usize>()?];
        let seed = f[7].parse::<u32>()?;
        let disable = f[8] == "1";
        let theta_rdo = f[9] == "1";
        let data: Vec<u8> = (0..f[10].len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&f[10][i..i + 2], 16))
            .collect::<Result<_, _>>()?;
        let symbols = numbers(lines.next().ok_or("missing symbols")?)?;
        let values: Vec<i32> = numbers(lines.next().ok_or("missing input")?)?
            .into_iter()
            .map(|x| x as i32)
            .collect();
        let a = numbers(lines.next().ok_or("missing amplitudes")?)?;
        assert_eq!(a.len(), 42);
        let amplitudes = std::array::from_fn(|c| std::array::from_fn(|i| a[c * 21 + i] as i32));
        let expected = numbers(lines.next().ok_or("missing spectrum")?)?;
        let size = SIZES[lm];
        let n = size.samples();
        let layout = BandLayout::new(channels, start, end)?;
        assert_eq!(values.len(), channels * n);
        assert_eq!(expected.len(), 119 + channels * n);
        assert_eq!(expected[75], 0);
        let mut decoder = RangeDecoder::new(&data);
        let tf = control::decode_tf(&mut decoder, size, start..end, transient)?;
        let allocation = allocation::decode_allocation(&mut decoder, size, layout, transient)?;
        EnergyState::default().fine(&mut decoder, layout, allocation.fine_bits())?;
        assert_eq!(
            [
                i64::from(decoder.range()),
                i64::from(decoder.tell()),
                i64::from(decoder.tell_frac())
            ],
            expected[..3],
            "prefix frame{rows}"
        );
        assert_eq!(
            [
                allocation.intensity() as i64,
                i64::from(allocation.dual_stereo()),
                allocation.coded_bands() as i64,
                i64::from(allocation.balance()),
                i64::from(allocation.anti_collapse_reserved()) * 8
            ],
            expected[3..8]
        );
        for i in 0..21 {
            assert_eq!(i64::from(tf.values()[i]), expected[8 + i]);
            assert_eq!(i64::from(allocation.pvq_bits()[i]), expected[29 + i]);
            assert_eq!(i64::from(allocation.fine_bits()[i]), expected[50 + i]);
        }
        let mut writer = RangeEncoder::new(data.len())?;
        replay(&mut writer, &symbols)?;
        assert_eq!(
            [writer.range(), writer.tell(), writer.tell_frac()],
            [decoder.range(), decoder.tell(), decoder.tell_frac()]
        );
        let before = values.clone();
        let result = partition::encode_spectrum(
            &mut writer,
            &allocation,
            &tf,
            spread,
            SpectrumInput {
                left: &values[..n],
                right: if channels == 2 {
                    Some(&values[n..])
                } else {
                    None
                },
                amplitudes,
            },
            SpectrumEncoding {
                seed,
                disable_inversion: disable,
                theta_rdo,
            },
        )
        .map_err(|e| {
            format!(
                "frame{rows} LM{lm} C{channels} {start}..{end} len{}: {e}",
                data.len()
            )
        })?;
        assert_eq!(values, before);
        assert_eq!(
            [
                i64::from(result.seed()),
                i64::from(writer.range()),
                i64::from(writer.tell()),
                i64::from(writer.tell_frac())
            ],
            expected[71..75],
            "state frame{rows}"
        );
        for c in 0..channels {
            let actual = result.channel(c).ok_or("missing channel")?;
            let native: Vec<i32> = expected[119 + c * n..119 + (c + 1) * n]
                .iter()
                .map(|&x| x as i32)
                .collect();
            assert_eq!(actual, native, "output frame{rows} channel{c}");
            let masks = result.collapse_masks(c).ok_or("missing masks")?;
            for i in 0..21 {
                assert_eq!(
                    i64::from(masks[i]),
                    expected[76 + i * channels + c],
                    "mask frame{rows}"
                );
            }
        }
        assert_eq!(writer.finish()?, data, "bytes frame{rows}");
        let decoded =
            partition::decode_spectrum(&mut decoder, &allocation, &tf, spread, seed, disable)?;
        assert_eq!(decoded, result, "roundtrip frame{rows}");
        assert_eq!(decoder.range(), writer.range());
        assert_eq!(decoder.tell(), writer.tell());
        assert_eq!(decoder.tell_frac(), writer.tell_frac());
        dual += usize::from(allocation.dual_stereo());
        rdo += usize::from(theta_rdo && channels == 2 && !allocation.dual_stereo());
        rows += 1;
    }
    assert_eq!((rows, dual, rdo), (3456, 629, 751));
    eprintln!("matched {rows} full spectra, dual{dual}, RDO{rdo}");
    Ok(())
}

type StereoContext = (
    allocation::Allocation,
    control::TfResolution,
    RangeEncoder,
    Vec<i32>,
    [[i32; 21]; 2],
);
fn stereo_context() -> Result<StereoContext, Box<dyn std::error::Error>> {
    let mut lines = include_str!("celt-spectrum-encoder-fixtures.txt")
        .lines()
        .filter(|l| !l.starts_with('#'));
    while let Some(line) = lines.next() {
        let f: Vec<_> = line.split_whitespace().collect();
        let symbols = numbers(lines.next().ok_or("symbols")?)?;
        let values = numbers(lines.next().ok_or("input")?)?;
        let amp = numbers(lines.next().ok_or("amplitudes")?)?;
        let _ = lines.next().ok_or("spectrum")?;
        if f[1] != "3"
            || f[2] != "2"
            || f[3] != "0"
            || f[4] != "21"
            || f[5] != "0"
            || f[9] != "1"
            || f[10].len() != 240
        {
            continue;
        }
        let data: Vec<u8> = (0..f[10].len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&f[10][i..i + 2], 16))
            .collect::<Result<_, _>>()?;
        let mut dec = RangeDecoder::new(&data);
        let tf = control::decode_tf(&mut dec, FrameSize::Ms20, 0..21, false)?;
        let a = allocation::decode_allocation(
            &mut dec,
            FrameSize::Ms20,
            BandLayout::new(2, 0, 21)?,
            false,
        )?;
        if a.dual_stereo() {
            continue;
        }
        let mut writer = RangeEncoder::new(data.len())?;
        replay(&mut writer, &symbols)?;
        return Ok((
            a,
            tf,
            writer,
            values.into_iter().map(|x| x as i32).collect(),
            std::array::from_fn(|c| std::array::from_fn(|i| amp[c * 21 + i] as i32)),
        ));
    }
    Err("missing stereo RDO context".into())
}
#[test]
fn invalid_input_amplitude_lifecycle_and_tf_domains_preserve_writer()
-> Result<(), Box<dyn std::error::Error>> {
    use partition::PartitionError;
    let (allocation, tf, mut writer, values, amplitudes) = stereo_context()?;
    let original = writer.clone();
    let n = 960;
    let options = SpectrumEncoding {
        seed: 123,
        disable_inversion: false,
        theta_rdo: true,
    };
    let input = SpectrumInput {
        left: &values[..n],
        right: Some(&values[n..]),
        amplitudes,
    };
    for bad in [
        SpectrumInput {
            left: &values[..n - 1],
            ..input
        },
        SpectrumInput {
            right: None,
            ..input
        },
        SpectrumInput {
            right: Some(&values[n..2 * n - 1]),
            ..input
        },
    ] {
        assert_eq!(
            partition::encode_spectrum(&mut writer, &allocation, &tf, Spread::Normal, bad, options),
            Err(PartitionError::InvalidInput)
        );
        assert_eq!(writer, original);
    }
    for channel in 0..2 {
        for value in [-1, i32::MIN] {
            let mut bad = input;
            bad.amplitudes[channel][0] = value;
            assert_eq!(
                partition::encode_spectrum(
                    &mut writer,
                    &allocation,
                    &tf,
                    Spread::Normal,
                    bad,
                    options
                ),
                Err(PartitionError::InvalidInput)
            );
        }
    }
    let mut bad = input;
    bad.amplitudes[0][0] = i32::MAX;
    bad.amplitudes[1][0] = i32::MAX;
    assert_eq!(
        partition::encode_spectrum(&mut writer, &allocation, &tf, Spread::Normal, bad, options),
        Err(PartitionError::InvalidInput)
    );
    for value in [i32::MIN, i32::MAX, 1 << 25] {
        let mut bad_values = values.clone();
        bad_values[0] = value;
        assert_eq!(
            partition::encode_spectrum(
                &mut writer,
                &allocation,
                &tf,
                Spread::Normal,
                SpectrumInput {
                    left: &bad_values[..n],
                    right: Some(&bad_values[n..]),
                    amplitudes
                },
                options
            ),
            Err(PartitionError::InvalidInput)
        );
        assert_eq!(writer, original);
    }
    let mut boundary = values.clone();
    boundary[..8].fill(0);
    boundary[0] = 1 << 24;
    boundary[1] = 1 << 23;
    let mut boundary_writer = writer.clone();
    partition::encode_spectrum(
        &mut boundary_writer,
        &allocation,
        &tf,
        Spread::Normal,
        SpectrumInput {
            left: &boundary[..n],
            right: Some(&boundary[n..]),
            amplitudes,
        },
        options,
    )?;
    boundary[1] += 1;
    assert_eq!(
        partition::encode_spectrum(
            &mut writer,
            &allocation,
            &tf,
            Spread::Normal,
            SpectrumInput {
                left: &boundary[..n],
                right: Some(&boundary[n..]),
                amplitudes
            },
            options
        ),
        Err(PartitionError::InvalidInput)
    );
    assert_eq!(writer, original);
    for cap in [1, 2] {
        let mut tight = RangeEncoder::new(cap)?;
        if cap == 2 {
            tight.encode_bits(0, 16)?;
        }
        let before = tight.clone();
        assert_eq!(
            partition::encode_spectrum(
                &mut tight,
                &allocation,
                &tf,
                Spread::Normal,
                input,
                options
            ),
            Err(PartitionError::InvalidBudget)
        );
        assert_eq!(tight, before);
    }
    let bytes = [0; 128];
    let mut dec = RangeDecoder::new(&bytes);
    let wrong_tf = control::decode_tf(&mut dec, FrameSize::Ms20, 0..21, true)?;
    assert_eq!(
        partition::encode_spectrum(
            &mut writer,
            &allocation,
            &wrong_tf,
            Spread::Normal,
            input,
            options
        ),
        Err(PartitionError::InvalidGeometry)
    );
    assert_eq!(writer, original);
    // Inactive spectral bins are not input to shape coding and return zero.
    let mut valid = writer.clone();
    let expected =
        partition::encode_spectrum(&mut valid, &allocation, &tf, Spread::Normal, input, options)?;
    let mut poisoned = values.clone();
    poisoned[800..960].fill(i32::MIN);
    poisoned[1760..1920].fill(i32::MAX);
    assert_eq!(
        partition::encode_spectrum(
            &mut writer,
            &allocation,
            &tf,
            Spread::Normal,
            SpectrumInput {
                left: &poisoned[..n],
                right: Some(&poisoned[n..]),
                amplitudes
            },
            options
        )?,
        expected
    );
    assert_eq!(writer, valid);
    writer.finish()?;
    let finished = writer.clone();
    assert_eq!(
        partition::encode_spectrum(
            &mut writer,
            &allocation,
            &tf,
            Spread::Normal,
            input,
            options
        ),
        Err(PartitionError::Encoding(
            opura::range::EncoderError::Finished
        ))
    );
    assert_eq!(writer, finished);
    Ok(())
}
