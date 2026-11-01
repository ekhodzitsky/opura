//! Original complete spectral pipeline through all-band assembly.
use opura::{
    celt::{
        FrameSize, allocation, control,
        energy::{BandLayout, EnergyState},
        partition::{self, AntiCollapseError, PartitionError},
        pvq::Spread,
    },
    range::RangeDecoder,
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
#[derive(Debug)]
struct PipelineStats {
    success: usize,
    corrupt: usize,
    rejected: usize,
    prefix: usize,
    dual: usize,
    anti: usize,
    changed: usize,
}
#[test]
fn full_spectral_pipeline_matches_original_channels_masks_seed_and_entropy()
-> Result<(), Box<dyn std::error::Error>> {
    let s = verify_pipeline(include_str!("celt-spectrum-fixtures.txt"), false)?;
    assert_eq!((s.success, s.corrupt, s.rejected), (3779, 253, 0));
    assert_eq!(s.prefix, 1008);
    assert_eq!(s.dual, 408);
    Ok(())
}
#[test]
fn final_energy_and_anti_collapse_match_original_histories_and_noise()
-> Result<(), Box<dyn std::error::Error>> {
    let s = verify_pipeline(include_str!("celt-anti-collapse-fixtures.txt"), true)?;
    assert_eq!(
        (
            s.success, s.corrupt, s.rejected, s.prefix, s.dual, s.anti, s.changed
        ),
        (1850, 166, 0, 504, 178, 395, 135)
    );
    Ok(())
}
fn verify_pipeline(text: &str, finish: bool) -> Result<PipelineStats, Box<dyn std::error::Error>> {
    let mut lines = text.lines().filter(|s| !s.starts_with('#'));
    let mut anti_count = 0;
    let mut changed = 0;
    let mut count = 0;
    let mut corrupt = 0;
    let mut rejected = 0;
    let mut full_prefix = 0;
    let mut dual = 0;
    while let Some(line) = lines.next() {
        let r: Vec<_> = line.split_whitespace().collect();
        assert_eq!(r[0], "frame");
        let lm: usize = r[1].parse()?;
        let channels: usize = r[2].parse()?;
        let start: usize = r[3].parse()?;
        let end: usize = r[4].parse()?;
        let mut transient = r[5] == "1";
        let mut intra = r[6] == "1";
        let prefix = r[7] == "1";
        let position: usize = r[8].parse()?;
        let seed: u32 = r[9].parse()?;
        let disable = r[10] == "1";
        let data: Vec<u8> = r[11]
            .as_bytes()
            .chunks_exact(2)
            .map(|s| Ok(u8::from_str_radix(std::str::from_utf8(s)?, 16)?))
            .collect::<Result<_, Box<dyn std::error::Error>>>()?;
        let mut histories = [[[0i32; 21]; 2]; 2];
        if finish {
            let h: Vec<i32> = lines
                .next()
                .ok_or("missing history")?
                .split_whitespace()
                .skip(1)
                .map(|s| s.parse())
                .collect::<Result<_, _>>()?;
            for k in 0..2 {
                for c in 0..2 {
                    histories[k][c].copy_from_slice(&h[42 * k + 21 * c..42 * k + 21 * (c + 1)]);
                }
            }
        }
        let expected: Vec<i64> = lines
            .next()
            .ok_or("missing spectrum")?
            .split_whitespace()
            .skip(1)
            .map(|s| s.parse())
            .collect::<Result<_, _>>()?;
        let mut ec = RangeDecoder::new(&data);
        let size = SIZES[lm];
        let layout = BandLayout::new(channels, start, end)?;
        match position {
            1 => {
                for _ in 0..3 {
                    ec.decode_bit_logp(3)?;
                }
            }
            2 => {
                ec.decode_bits(5)?;
            }
            3 => {
                ec.decode_uint(17)?;
                ec.decode_bits(3)?;
            }
            _ => {}
        }
        let pipeline = (|| -> Result<_, Box<dyn std::error::Error>> {
            if prefix {
                if start == 17 {
                    for _ in 0..3 {
                        ec.decode_bit_logp(3)?;
                    }
                }
                let p = control::decode_prefix(&mut ec, size, start)?;
                transient = p.transient();
                intra = p.intra();
                full_prefix += 1;
            }
            let mut energy = EnergyState::default();
            energy.coarse(&mut ec, layout, size, intra)?;
            let tf = control::decode_tf(&mut ec, size, start..end, transient)?;
            let spread = control::decode_spread(&mut ec)?;
            let a = allocation::decode_allocation(&mut ec, size, layout, transient)?;
            energy.fine(&mut ec, layout, a.fine_bits())?;
            Ok((energy, tf, spread, a))
        })();
        let invalid = expected[11] != 0 || expected[16] != 0 || expected[9] > data.len() as i64 * 8;
        let (mut energy, tf, spread, a) = match pipeline {
            Ok(p) => p,
            Err(e) => {
                assert!(
                    invalid,
                    "pipeline rejected frame{}: {e}",
                    count + corrupt + rejected
                );
                rejected += 1;
                continue;
            }
        };
        assert_eq!(
            (i64::from(transient), i64::from(intra), spread),
            (expected[0], expected[1], SPREADS[expected[2] as usize])
        );
        assert_eq!(
            (
                i64::from(a.anti_collapse_reserved()) * 8,
                a.coded_bands() as i64,
                a.intensity() as i64,
                i64::from(a.dual_stereo()),
                i64::from(a.balance())
            ),
            (
                expected[3],
                expected[4],
                expected[5],
                expected[6],
                expected[7]
            )
        );
        assert_eq!(
            (
                i64::from(ec.range()),
                i64::from(ec.tell()),
                i64::from(ec.tell_frac())
            ),
            (expected[8], expected[9], expected[10])
        );
        if !finish {
            for c in 0..2 {
                let actual = energy.channel(c).ok_or("energy channel")?;
                for i in 0..21 {
                    assert_eq!(i64::from(actual[i]), expected[17 + c * 21 + i]);
                }
            }
        }
        let before = ec.clone();
        let result = partition::decode_spectrum(&mut ec, &a, &tf, spread, seed, disable);
        if invalid {
            assert!(
                result.is_err(),
                "corrupt spectrum frame{}",
                count + corrupt + rejected
            );
            assert_eq!(ec, before);
            corrupt += 1;
            continue;
        }
        let mut result = result.map_err(|e| {
            format!(
                "frame{} LM{lm} C{channels} {start}..{end} len{}: {e}",
                count + corrupt + rejected,
                data.len()
            )
        })?;
        if finish {
            let on = if a.anti_collapse_reserved() {
                ec.decode_bits(1)? != 0
            } else {
                false
            };
            assert_eq!(i64::from(on), *expected.last().ok_or("anti flag")?);
            let bits_left = (data.len() as u32 * 8).saturating_sub(ec.tell());
            energy.finalise(&mut ec, layout, a.fine_bits(), a.fine_priority(), bits_left)?;
            let current = [
                *energy.channel(0).ok_or("current left")?,
                *energy.channel(1).ok_or("current right")?,
            ];
            let saved = result.clone();
            if a.anti_collapse_reserved() {
                if anti_count == 0 {
                    let bytes = [0u8; 128];
                    let mut wrong_cursor = RangeDecoder::new(&bytes);
                    let other = allocation::decode_allocation(
                        &mut wrong_cursor,
                        size,
                        BandLayout::new(channels, if start == 0 { 17 } else { 0 }, 21)?,
                        true,
                    )?;
                    assert_eq!(
                        result.anti_collapse(&other, &current, &histories[0], &histories[1]),
                        Err(AntiCollapseError::InvalidContext)
                    );
                    assert_eq!(result, saved);
                }
                let mut invalid = current;
                invalid[0][0] = i32::MAX;
                assert_eq!(
                    result.anti_collapse(&a, &invalid, &histories[0], &histories[1]),
                    Err(AntiCollapseError::InvalidEnergy)
                );
                assert_eq!(result, saved);
            } else {
                assert_eq!(
                    result.anti_collapse(&a, &current, &histories[0], &histories[1]),
                    Err(AntiCollapseError::InvalidContext)
                );
                assert_eq!(result, saved);
            }
            if on {
                result.anti_collapse(&a, &current, &histories[0], &histories[1])?;
                anti_count += 1;
                changed += usize::from(result != saved);
            }
            for c in 0..2 {
                for i in 0..21 {
                    assert_eq!(
                        i64::from(current[c][i]),
                        expected[17 + c * 21 + i],
                        "final energy"
                    );
                }
            }
        }
        assert_eq!(result.layout(), layout);
        assert_eq!(result.size(), size);
        assert_eq!(i64::from(result.seed()), expected[12]);
        assert_eq!(
            (
                i64::from(ec.range()),
                i64::from(ec.tell()),
                i64::from(ec.tell_frac())
            ),
            (expected[13], expected[14], expected[15]),
            "entropy frame{}",
            count + corrupt + rejected
        );
        for c in 0..channels {
            let masks = result.collapse_masks(c).ok_or("mask channel")?;
            for i in 0..21 {
                assert_eq!(
                    i64::from(masks[i]),
                    expected[59 + i * channels + c],
                    "mask frame{} band{i}",
                    count + corrupt + rejected
                );
            }
            let actual = result.channel(c).ok_or("spectrum channel")?;
            let n = size.samples();
            let values: Vec<i32> = expected[101 + c * n..101 + (c + 1) * n]
                .iter()
                .map(|&v| v as i32)
                .collect();
            let edges = [
                0, 1, 2, 3, 4, 5, 6, 7, 8, 10, 12, 14, 16, 20, 24, 28, 34, 40, 48, 60, 78, 100,
            ];
            let first = edges[start] << lm;
            let last = edges[end] << lm;
            // Original quant_all_bands borrows the highest band as internal fold
            // scratch, even when it is outside the decoded interval. Only
            // selected bins are signal; our public output zeros all others.
            assert_eq!(
                &actual[first..last],
                &values[first..last],
                "coefficients frame{} LM{lm} C{channels} {start}..{end} len{}",
                count + corrupt + rejected,
                data.len()
            );
            assert!(
                actual[..first]
                    .iter()
                    .chain(&actual[last..])
                    .all(|&v| v == 0)
            );
        }
        assert!(result.channel(channels).is_none());
        assert!(result.collapse_masks(channels).is_none());
        dual += usize::from(a.dual_stereo());
        count += 1;
    }
    Ok(PipelineStats {
        success: count,
        corrupt,
        rejected,
        prefix: full_prefix,
        dual,
        anti: anti_count,
        changed,
    })
}
#[test]
fn invalid_spectrum_payload_and_context_preserve_entropy() -> Result<(), Box<dyn std::error::Error>>
{
    let data = [0u8; 128];
    let mut source = RangeDecoder::new(&data);
    let size = FrameSize::Ms20;
    let tf = control::decode_tf(&mut source, size, 0..21, false)?;
    let a = allocation::decode_allocation(&mut source, size, BandLayout::new(1, 0, 21)?, false)?;
    for n in [0, 1, 1276] {
        let data = vec![0; n];
        let mut ec = RangeDecoder::new(&data);
        let before = ec.clone();
        assert_eq!(
            partition::decode_spectrum(&mut ec, &a, &tf, Spread::Normal, 0, false),
            Err(PartitionError::InvalidBudget)
        );
        assert_eq!(ec, before);
    }
    let mut ec = RangeDecoder::new(&data);
    let bad = allocation::decode_allocation(&mut ec, size, BandLayout::new(1, 1, 21)?, false)?;
    let before = ec.clone();
    assert_eq!(
        partition::decode_spectrum(&mut ec, &bad, &tf, Spread::Normal, 0, false),
        Err(PartitionError::InvalidGeometry)
    );
    assert_eq!(ec, before);
    let mut found = false;
    for byte in 0..=255u8 {
        let bytes = [byte; 128];
        let mut cursor = RangeDecoder::new(&bytes);
        let wrong_tf = control::decode_tf(&mut cursor, FrameSize::Ms2_5, 0..21, false)?;
        if wrong_tf.values().contains(&-1) {
            let mut target = RangeDecoder::new(&data);
            let saved = target.clone();
            assert_eq!(
                partition::decode_spectrum(&mut target, &a, &wrong_tf, Spread::Normal, 0, false),
                Err(PartitionError::InvalidGeometry)
            );
            assert_eq!(target, saved);
            found = true;
            break;
        }
    }
    assert!(found);
    let mut over = RangeDecoder::new(&data);
    for _ in 0..1100 {
        if over.tell() > 1024 {
            break;
        }
        over.decode_bit_logp(1)?;
    }
    assert!(over.tell() > 1024);
    let saved = over.clone();
    assert_eq!(
        partition::decode_spectrum(&mut over, &a, &tf, Spread::Normal, 0, false),
        Err(PartitionError::InvalidBudget)
    );
    assert_eq!(over, saved);
    Ok(())
}
