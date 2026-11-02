//! Native recursive mono partition encoding, atan arithmetic and rollback.
use opura::{
    celt::{
        math::{self, MathError},
        partition::{self, PartitionControls, PartitionError, PartitionState},
        pulses::PulseError,
        pvq::{PvqError, Spread},
    },
    range::{EncoderError, RangeDecoder, RangeEncoder},
};
const EDGES: [usize; 22] = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 10, 12, 14, 16, 20, 24, 28, 34, 40, 48, 60, 78, 100,
];
fn angle(op: u8, x: i32, y: i32) -> Result<i32, MathError> {
    if op == 0 {
        math::atan_norm(x)
    } else {
        math::atan2_norm(y, x)
    }
}
#[test]
fn original_dense_normalized_angles_and_wide_positive_ratios_match()
-> Result<(), Box<dyn std::error::Error>> {
    let mut coverage = [0; 2];
    for line in include_str!("celt-angle-fixtures.txt").lines() {
        if line.starts_with('#') {
            continue;
        }
        let c: Vec<_> = line.split_whitespace().collect();
        let op = c[1].parse::<u8>()?;
        if c[0] == "block" {
            let start = c[2].parse::<i32>()?;
            let step = c[3].parse::<i32>()?;
            let count = c[4].parse::<usize>()?;
            let mut hash = 14695981039346656037u64;
            for i in 0..count {
                for byte in angle(
                    op,
                    (i64::from(start) + i as i64 * i64::from(step)) as i32,
                    0,
                )?
                .to_le_bytes()
                {
                    hash = (hash ^ u64::from(byte)).wrapping_mul(1099511628211);
                }
            }
            assert_eq!(hash, c[5].parse::<u64>()?);
            coverage[op as usize] += count;
        } else {
            assert_eq!(
                angle(op, c[2].parse()?, c[3].parse()?)?,
                c[4].parse::<i32>()?
            );
            coverage[op as usize] += 1;
        }
    }
    assert_eq!(coverage, [69833, 4321]);
    for x in [i32::MIN, -(1 << 30) - 1, (1 << 30) + 1, i32::MAX] {
        assert_eq!(math::atan_norm(x), Err(MathError::NotNormalized));
    }
    assert_eq!(math::atan_norm(1 << 30)?, 1 << 29);
    assert_eq!(math::atan_norm(-(1 << 30))?, -(1 << 29));
    assert_eq!(math::atan2_norm(0, 0)?, 0);
    assert_eq!(math::atan2_norm(0, 123)?, 0);
    assert_eq!(math::atan2_norm(123, 0)?, 1 << 30);
    assert_eq!(math::atan2_norm(-1, 0), Err(MathError::NegativeInput));
    assert_eq!(math::atan2_norm(1, -1), Err(MathError::NegativeInput));
    Ok(())
}
#[test]
fn original_recursive_bytes_reconstructions_masks_and_shared_states_match()
-> Result<(), Box<dyn std::error::Error>> {
    let mut rows = 0;
    let mut geometry = [[[false; 5]; 5]; 21];
    let mut negative_budget = false;
    let mut wide_mask = false;
    for (line_index, line) in include_str!("celt-partition-encoder-fixtures.txt")
        .lines()
        .enumerate()
    {
        if line.starts_with('#') {
            continue;
        }
        let c: Vec<_> = line.split_whitespace().collect();
        assert_eq!(c[0], "partition");
        let band = c[1].parse::<usize>()?;
        let lm = c[2].parse::<i8>()?;
        let blocks = c[3].parse::<usize>()?;
        let controls = PartitionControls {
            band,
            lm,
            blocks,
            bits: c[4].parse()?,
            gain: c[5].parse()?,
            spread: [
                Spread::None,
                Spread::Light,
                Spread::Normal,
                Spread::Aggressive,
            ][c[6].parse::<usize>()?],
            fill: c[7].parse()?,
        };
        let remaining = c[8].parse::<i32>()?;
        let seed = c[9].parse::<u32>()?;
        let folded = c[10] == "1";
        let avoid = c[11] == "1";
        let prefix_bits = c[12].parse::<u32>()?;
        let prefix_value = c[13].parse::<u32>()?;
        let data: Vec<u8> = (0..c[14].len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&c[14][i..i + 2], 16))
            .collect::<Result<_, _>>()?;
        let width = EDGES[band + 1] - EDGES[band];
        let n = if lm < 0 { width / 2 } else { width << lm };
        let input: Vec<i32> = c[15..15 + n]
            .iter()
            .map(|s| s.parse())
            .collect::<Result<_, _>>()?;
        let offset = 15 + n;
        let fold: Option<Vec<i32>> = if folded {
            Some(
                c[offset..offset + n]
                    .iter()
                    .map(|s| s.parse())
                    .collect::<Result<_, _>>()?,
            )
        } else {
            None
        };
        let expected: Vec<i64> = c[offset + if folded { n } else { 0 }..]
            .iter()
            .map(|s| s.parse())
            .collect::<Result<_, _>>()?;
        assert_eq!(expected.len(), n + 7);
        assert_eq!(expected[6], 0);
        let before_input = input.clone();
        let before_fold = fold.clone();
        let mut writer = RangeEncoder::new(1275)?;
        if prefix_bits > 0 {
            writer.encode_uint(prefix_value, 1 << prefix_bits)?;
        }
        let mut state = PartitionState::new(remaining, seed)?;
        let result = partition::encode_partition(
            &mut writer,
            &mut state,
            controls,
            &input,
            fold.as_deref(),
            avoid,
        )?;
        assert_eq!(input, before_input);
        assert_eq!(fold, before_fold);
        assert_eq!(
            [
                i64::from(result.collapse_mask()),
                i64::from(state.remaining_bits()),
                i64::from(state.seed()),
                i64::from(writer.range()),
                i64::from(writer.tell()),
                i64::from(writer.tell_frac())
            ],
            expected[..6],
            "metadata line{line_index}"
        );
        for (i, &value) in result.values().iter().enumerate() {
            assert_eq!(
                i64::from(value),
                expected[7 + i],
                "output line{line_index} coordinate{i}"
            );
        }
        writer.shrink(data.len())?;
        assert_eq!(writer.finish()?, data, "bytes line{line_index}");
        let mut decoder = RangeDecoder::new(&data);
        if prefix_bits > 0 {
            assert_eq!(decoder.decode_uint(1 << prefix_bits)?, prefix_value);
        }
        let mut decoded_state = PartitionState::new(remaining, seed)?;
        let decoded = partition::decode_partition(
            &mut decoder,
            &mut decoded_state,
            controls,
            fold.as_deref(),
        )?;
        assert_eq!(decoded, result);
        assert_eq!(decoded_state, state);
        assert_eq!(
            [
                i64::from(decoder.range()),
                i64::from(decoder.tell()),
                i64::from(decoder.tell_frac())
            ],
            expected[3..6]
        );
        negative_budget |= state.remaining_bits() < 0;
        wide_mask |= result.collapse_mask() > 255;
        geometry[band][(lm + 1) as usize][blocks.trailing_zeros() as usize] = true;
        rows += 1;
    }
    assert_eq!(rows, 9440);
    assert_eq!(
        geometry.iter().flatten().flatten().filter(|&&v| v).count(),
        286
    );
    assert!(negative_budget && wide_mask);
    Ok(())
}

fn controls() -> PartitionControls {
    PartitionControls {
        band: 20,
        lm: 3,
        blocks: 8,
        bits: 81600,
        gain: i32::MAX,
        spread: Spread::Normal,
        fill: 255,
    }
}
#[test]
fn invalid_models_and_late_capacity_failures_preserve_writer_and_shared_state()
-> Result<(), Box<dyn std::error::Error>> {
    let mut writer = RangeEncoder::new(128)?;
    writer.encode_uint(173, 257)?;
    let mut state = PartitionState::new(8191, 123)?;
    let original = writer.clone();
    let before = state.clone();
    let input = [0; 176];
    for c in [
        PartitionControls {
            band: 21,
            ..controls()
        },
        PartitionControls {
            band: usize::MAX,
            ..controls()
        },
        PartitionControls {
            lm: -2,
            ..controls()
        },
        PartitionControls {
            lm: 4,
            ..controls()
        },
        PartitionControls {
            blocks: 0,
            ..controls()
        },
        PartitionControls {
            blocks: 3,
            ..controls()
        },
    ] {
        assert_eq!(
            partition::encode_partition(&mut writer, &mut state, c, &input, None, false),
            Err(PartitionError::InvalidGeometry)
        );
        assert_eq!(writer, original);
        assert_eq!(state, before);
    }
    for c in [
        PartitionControls {
            bits: -1,
            ..controls()
        },
        PartitionControls {
            bits: 81601,
            ..controls()
        },
    ] {
        assert_eq!(
            partition::encode_partition(&mut writer, &mut state, c, &input, None, false),
            Err(PartitionError::InvalidBudget)
        );
    }
    assert_eq!(
        partition::encode_partition(
            &mut writer,
            &mut state,
            PartitionControls {
                gain: -1,
                ..controls()
            },
            &input,
            None,
            false
        ),
        Err(PartitionError::InvalidGain)
    );
    for vector in [
        &[][..],
        &input[..175],
        &[i32::MIN; 176][..],
        &[i32::MAX; 176][..],
    ] {
        assert_eq!(
            partition::encode_partition(&mut writer, &mut state, controls(), vector, None, false),
            Err(PartitionError::InvalidInput)
        );
    }
    for fold in [&[0; 175][..], &[i32::MIN; 176][..], &[i32::MAX; 176][..]] {
        assert_eq!(
            partition::encode_partition(
                &mut writer,
                &mut state,
                controls(),
                &input,
                Some(fold),
                false
            ),
            Err(PartitionError::InvalidFold)
        );
    }
    assert_eq!(writer, original);
    assert_eq!(state, before);
    let mut boundary = [0; 176];
    boundary[0] = 1 << 24;
    boundary[1] = 1 << 24;
    partition::encode_partition(
        &mut writer,
        &mut state,
        PartitionControls {
            bits: 0,
            fill: 0,
            ..controls()
        },
        &boundary,
        None,
        false,
    )?;
    assert_eq!(writer, original);
    assert_eq!(state, before);
    boundary[1] += 1;
    assert_eq!(
        partition::encode_partition(
            &mut writer,
            &mut state,
            PartitionControls {
                bits: 0,
                fill: 0,
                ..controls()
            },
            &boundary,
            None,
            false
        ),
        Err(PartitionError::InvalidInput)
    );
    assert_eq!(writer, original);
    assert_eq!(state, before);
    let mut tight = RangeEncoder::new(2)?;
    let original = tight.clone();
    let error = partition::encode_partition(
        &mut tight,
        &mut state,
        controls(),
        &[1 << 20; 176],
        None,
        false,
    );
    assert!(matches!(
        error,
        Err(PartitionError::Encoding(EncoderError::CapacityExhausted))
            | Err(PartitionError::Pvq(PvqError::Pulses(PulseError::Encoding(
                EncoderError::CapacityExhausted
            ))))
    ));
    assert_eq!(tight, original);
    assert_eq!(state, before);
    writer.finish()?;
    let original = writer.clone();
    assert_eq!(
        partition::encode_partition(&mut writer, &mut state, controls(), &input, None, false),
        Err(PartitionError::Encoding(EncoderError::Finished))
    );
    assert_eq!(writer, original);
    assert_eq!(state, before);
    Ok(())
}
