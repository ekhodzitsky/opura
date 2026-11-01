//! Original recursive partition state/entropy/vector evidence.
use opura::{
    celt::{
        partition::{self, PartitionControls, PartitionError, PartitionState},
        pvq::Spread,
    },
    range::RangeDecoder,
};
const EDGES: [usize; 22] = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 10, 12, 14, 16, 20, 24, 28, 34, 40, 48, 60, 78, 100,
];
const SPREADS: [Spread; 4] = [
    Spread::None,
    Spread::Light,
    Spread::Normal,
    Spread::Aggressive,
];

#[test]
fn original_recursive_outputs_and_following_entropy_match() -> Result<(), Box<dyn std::error::Error>>
{
    let mut count = 0;
    let mut rejected = 0;
    let mut seen = [[[false; 5]; 21]; 5];
    for (line_no, line) in include_str!("celt-partition-fixtures.txt")
        .lines()
        .enumerate()
    {
        if line.starts_with('#') {
            continue;
        }
        let row: Vec<_> = line.split_whitespace().collect();
        assert_eq!(row[0], "partition");
        let band: usize = row[1].parse()?;
        let lm: i8 = row[2].parse()?;
        let blocks: usize = row[3].parse()?;
        let c = PartitionControls {
            band,
            lm,
            blocks,
            bits: row[4].parse()?,
            gain: row[5].parse()?,
            spread: SPREADS[row[6].parse::<usize>()?],
            fill: row[7].parse()?,
        };
        let mut state = PartitionState::new(row[8].parse()?, row[9].parse()?)?;
        let folded = row[10] == "1";
        let data: Vec<u8> = row[12]
            .as_bytes()
            .chunks_exact(2)
            .map(|s| Ok(u8::from_str_radix(std::str::from_utf8(s)?, 16)?))
            .collect::<Result<_, Box<dyn std::error::Error>>>()?;
        let width = EDGES[band + 1] - EDGES[band];
        let n = if lm < 0 { width / 2 } else { width << lm };
        let mut index = 13;
        let fold: Vec<i32> = if folded {
            let values = row[index..index + n]
                .iter()
                .map(|s| s.parse())
                .collect::<Result<_, _>>()?;
            index += n;
            values
        } else {
            vec![]
        };
        let expected_mask: u16 = row[index].parse()?;
        let remaining: i32 = row[index + 1].parse()?;
        let seed: u32 = row[index + 2].parse()?;
        let range: u32 = row[index + 3].parse()?;
        let tell: u32 = row[index + 4].parse()?;
        let frac: u32 = row[index + 5].parse()?;
        let error: i32 = row[index + 6].parse()?;
        let mut entropy = RangeDecoder::new(&data);
        let position: usize = row[11].parse()?;
        for j in 0..position {
            entropy.decode_bit_logp(2 + (j % 5) as u32)?;
        }
        if position >= 4 {
            entropy.decode_bits(3)?;
        }
        let before = entropy.clone();
        let before_state = state.clone();
        let result = partition::decode_partition(
            &mut entropy,
            &mut state,
            c,
            if folded { Some(&fold) } else { None },
        );
        if error != 0 {
            assert!(result.is_err(), "oracle corrupt uniform line {line_no}");
            assert_eq!(entropy, before);
            assert_eq!(state, before_state);
            rejected += 1;
            continue;
        }
        let result = result.map_err(|e| {
            format!(
                "line {line_no}, band{band} lm{lm} blocks{blocks} bits{}: {e}",
                c.bits
            )
        })?;
        let expected: Vec<i32> = row[index + 7..]
            .iter()
            .map(|s| s.parse())
            .collect::<Result<_, _>>()?;
        assert_eq!(
            result.values(),
            expected,
            "coefficients line {line_no} {c:?}"
        );
        assert_eq!(result.collapse_mask(), expected_mask, "mask line {line_no}");
        assert_eq!(
            (state.remaining_bits(), state.seed()),
            (remaining, seed),
            "budget/seed line {line_no}"
        );
        assert_eq!(
            (entropy.range(), entropy.tell(), entropy.tell_frac()),
            (range, tell, frac),
            "entropy line {line_no}"
        );
        assert!(u32::from(result.collapse_mask()) < (1u32 << blocks));
        if c.gain == 0 {
            assert!(result.values().iter().all(|&v| v == 0));
        }
        if remaining < 0 {
            let saved = (entropy.clone(), state.clone());
            assert_eq!(
                partition::decode_partition(&mut entropy, &mut state, c, None),
                Err(PartitionError::InvalidBudget)
            );
            assert_eq!((entropy, state), saved);
        }
        seen[(lm + 1) as usize][band][blocks.trailing_zeros() as usize] = true;
        count += 1;
    }
    for lm in -1i8..=3 {
        for band in 0..21 {
            let width = EDGES[band + 1] - EDGES[band];
            let n = if lm < 0 { width / 2 } else { width << lm };
            for k in 0..5 {
                assert_eq!(
                    seen[(lm + 1) as usize][band][k],
                    n >= 2 && n % (1 << k) == 0
                );
            }
        }
    }
    assert_eq!((count, rejected), (9409, 29));
    Ok(())
}

#[test]
fn invalid_controls_fold_and_entropy_roll_back() -> Result<(), Box<dyn std::error::Error>> {
    let base = PartitionControls {
        band: 20,
        lm: 3,
        blocks: 8,
        bits: 200,
        gain: i32::MAX,
        spread: Spread::Normal,
        fill: u16::MAX,
    };
    assert_eq!(
        PartitionState::new(-1, 0),
        Err(PartitionError::InvalidBudget)
    );
    assert_eq!(
        PartitionState::new(81601, 0),
        Err(PartitionError::InvalidBudget)
    );
    for invalid in [
        PartitionControls { band: 21, ..base },
        PartitionControls {
            band: usize::MAX,
            ..base
        },
        PartitionControls { lm: -2, ..base },
        PartitionControls { lm: 4, ..base },
        PartitionControls {
            band: 0,
            lm: 0,
            ..base
        },
        PartitionControls {
            band: 0,
            lm: -1,
            ..base
        },
        PartitionControls { blocks: 0, ..base },
        PartitionControls { blocks: 3, ..base },
        PartitionControls { blocks: 32, ..base },
        PartitionControls {
            blocks: usize::MAX,
            ..base
        },
        PartitionControls { bits: -1, ..base },
        PartitionControls {
            bits: 81601,
            ..base
        },
        PartitionControls { gain: -1, ..base },
    ] {
        let data = [0u8; 128];
        let mut ec = RangeDecoder::new(&data);
        let mut state = PartitionState::new(8191, 7)?;
        let previous = (ec.clone(), state.clone());
        assert!(partition::decode_partition(&mut ec, &mut state, invalid, None).is_err());
        assert_eq!((ec, state), previous);
    }
    for values in [
        vec![],
        vec![1; 175],
        vec![1; 177],
        vec![i32::MAX; 176],
        vec![i32::MIN; 176],
        vec![32760 << 10; 176],
    ] {
        let data = [0u8; 128];
        let mut ec = RangeDecoder::new(&data);
        let mut state = PartitionState::new(8191, 7)?;
        let previous = (ec.clone(), state.clone());
        assert_eq!(
            partition::decode_partition(&mut ec, &mut state, base, Some(&values)),
            Err(PartitionError::InvalidFold)
        );
        assert_eq!((ec, state), previous);
    }
    for length in [0, 1, 1276] {
        let data = vec![0u8; length];
        let mut ec = RangeDecoder::new(&data);
        let mut state = PartitionState::new(8191, 7)?;
        let previous = (ec.clone(), state.clone());
        assert_eq!(
            partition::decode_partition(&mut ec, &mut state, base, None),
            Err(PartitionError::InvalidBudget)
        );
        assert_eq!((ec, state), previous);
    }
    Ok(())
}
