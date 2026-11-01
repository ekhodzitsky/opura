//! Original TF/sign/stereo band reconstruction, folding output and entropy.
use opura::{
    celt::{
        partition::{
            self, BandControls, PartitionControls, PartitionError, PartitionState, StereoControls,
        },
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
fn original_tf_stereo_coefficients_fold_masks_and_state_match()
-> Result<(), Box<dyn std::error::Error>> {
    let mut success = 0;
    let mut corrupt = 0;
    let mut geometry = [[[[[false; 7]; 2]; 2]; 21]; 4];
    for (line_no, line) in include_str!("celt-band-decode-fixtures.txt")
        .lines()
        .enumerate()
    {
        if line.starts_with('#') {
            continue;
        }
        let r: Vec<_> = line.split_whitespace().collect();
        assert_eq!(r[0], "band");
        let band: usize = r[1].parse()?;
        let lm: i8 = r[2].parse()?;
        let blocks: usize = r[3].parse()?;
        let tf: i8 = r[4].parse()?;
        let stereo = r[5] == "1";
        let c = BandControls {
            partition: PartitionControls {
                band,
                lm,
                blocks,
                bits: r[8].parse()?,
                gain: r[9].parse()?,
                spread: SPREADS[r[10].parse::<usize>()?],
                fill: r[11].parse()?,
            },
            tf_change: tf,
            stereo: if stereo {
                Some(StereoControls {
                    intensity: r[6].parse()?,
                    disable_inversion: r[7] == "1",
                })
            } else {
                None
            },
        };
        let mut state = PartitionState::new(r[12].parse()?, r[13].parse()?)?;
        let folded = r[14] == "1";
        let position: usize = r[15].parse()?;
        let data: Vec<u8> = r[16]
            .as_bytes()
            .chunks_exact(2)
            .map(|s| Ok(u8::from_str_radix(std::str::from_utf8(s)?, 16)?))
            .collect::<Result<_, Box<dyn std::error::Error>>>()?;
        let n = (EDGES[band + 1] - EDGES[band]) << lm;
        let mut index = 17;
        let fold: Vec<i32> = if folded {
            let values = r[index..index + n]
                .iter()
                .map(|s| s.parse())
                .collect::<Result<_, _>>()?;
            index += n;
            values
        } else {
            vec![]
        };
        let mask: u8 = r[index].parse()?;
        let remaining: i32 = r[index + 1].parse()?;
        let seed: u32 = r[index + 2].parse()?;
        let range: u32 = r[index + 3].parse()?;
        let tell: u32 = r[index + 4].parse()?;
        let frac: u32 = r[index + 5].parse()?;
        let error: i32 = r[index + 6].parse()?;
        let mut entropy = RangeDecoder::new(&data);
        for j in 0..position {
            entropy.decode_bit_logp(2 + (j % 5) as u32)?;
        }
        if position >= 4 {
            entropy.decode_bits(3)?;
        }
        let previous = (entropy.clone(), state.clone());
        let result = partition::decode_band(
            &mut entropy,
            &mut state,
            c,
            if folded { Some(&fold) } else { None },
        );
        if error != 0 {
            assert!(result.is_err(), "corrupt uniform line {line_no}");
            assert_eq!((entropy, state), previous);
            corrupt += 1;
            continue;
        }
        let result = result.map_err(|e| format!("line {line_no} {c:?}: {e}"))?;
        let expected: Vec<i32> = r[index + 7..]
            .iter()
            .map(|s| s.parse())
            .collect::<Result<_, _>>()?;
        assert_eq!(result.left(), &expected[..n], "left line {line_no} {c:?}");
        assert_eq!(
            result.right(),
            if stereo {
                Some(&expected[n..2 * n])
            } else {
                None
            },
            "right line {line_no} {c:?}"
        );
        assert_eq!(
            result.folding(),
            &expected[2 * n..],
            "folding line {line_no} {c:?}"
        );
        assert_eq!(result.collapse_mask(), mask, "mask line {line_no}");
        assert_eq!(
            (state.remaining_bits(), state.seed()),
            (remaining, seed),
            "state line {line_no}"
        );
        assert_eq!(
            (entropy.range(), entropy.tell(), entropy.tell_frac()),
            (range, tell, frac),
            "entropy line {line_no}"
        );
        geometry[lm as usize][band][usize::from(blocks > 1)][usize::from(stereo)]
            [(tf + 3) as usize] = true;
        success += 1;
    }
    assert_eq!((success, corrupt), (15680, 28));
    let mappings = [
        [0, -1, 0, -1, 0, -1, 0, -1],
        [0, -1, 0, -2, 1, 0, 1, -1],
        [0, -2, 0, -3, 2, 0, 1, -1],
        [0, -2, 0, -3, 3, 0, 1, -1],
    ];
    for lm in 0..4 {
        for band in 0..21 {
            for transient in 0..2 {
                if lm == 0 && transient == 1 {
                    continue;
                }
                for stereo in 0..2 {
                    for tf in &mappings[lm][4 * transient..4 * transient + 4] {
                        assert!(geometry[lm][band][transient][stereo][(tf + 3) as usize]);
                    }
                }
            }
        }
    }
    Ok(())
}
#[test]
fn malformed_band_domains_preserve_entropy_and_shared_state()
-> Result<(), Box<dyn std::error::Error>> {
    let base = BandControls {
        partition: PartitionControls {
            band: 20,
            lm: 3,
            blocks: 8,
            bits: 200,
            gain: i32::MAX,
            spread: Spread::Normal,
            fill: 255,
        },
        tf_change: 0,
        stereo: Some(StereoControls {
            intensity: 21,
            disable_inversion: false,
        }),
    };
    let bad = [
        BandControls {
            partition: PartitionControls {
                lm: -1,
                ..base.partition
            },
            ..base
        },
        BandControls {
            partition: PartitionControls {
                lm: 4,
                ..base.partition
            },
            ..base
        },
        BandControls {
            partition: PartitionControls {
                blocks: 2,
                ..base.partition
            },
            ..base
        },
        BandControls {
            partition: PartitionControls {
                fill: 256,
                ..base.partition
            },
            ..base
        },
        BandControls {
            partition: PartitionControls {
                gain: 1 << 30,
                ..base.partition
            },
            ..base
        },
        BandControls {
            tf_change: 4,
            ..base
        },
        BandControls {
            tf_change: -2,
            ..base
        },
        BandControls {
            stereo: Some(StereoControls {
                intensity: 22,
                disable_inversion: false,
            }),
            ..base
        },
        BandControls {
            partition: PartitionControls {
                bits: -1,
                ..base.partition
            },
            ..base
        },
    ];
    for c in bad {
        let data = [0u8; 128];
        let mut entropy = RangeDecoder::new(&data);
        let mut state = PartitionState::new(8191, 0)?;
        let saved = (entropy.clone(), state.clone());
        assert!(partition::decode_band(&mut entropy, &mut state, c, None).is_err());
        assert_eq!((entropy, state), saved);
    }
    for fold in [vec![1; 175], vec![i32::MAX; 176], vec![]] {
        let data = [0u8; 128];
        let mut entropy = RangeDecoder::new(&data);
        let mut state = PartitionState::new(8191, 0)?;
        let saved = (entropy.clone(), state.clone());
        assert_eq!(
            partition::decode_band(&mut entropy, &mut state, base, Some(&fold)),
            Err(PartitionError::InvalidFold)
        );
        assert_eq!((entropy, state), saved);
    }
    Ok(())
}
