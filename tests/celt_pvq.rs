//! Original alg_unquant normalized band/entropy evidence.
use opura::{
    celt::{
        math,
        pulses::PulseCodebook,
        pvq::{self, PvqError, Spread},
    },
    range::RangeDecoder,
};
fn bytes(s: &str) -> Result<Vec<u8>, std::num::ParseIntError> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16))
        .collect()
}
#[test]
fn normalized_bands_masks_entropy_and_refined_roots_match_original()
-> Result<(), Box<dyn std::error::Error>> {
    let mut lines = include_str!("celt-pvq-fixtures.txt")
        .lines()
        .filter(|l| !l.starts_with('#'));
    let mut count = 0;
    let mut roots = 0;
    let mut modes = [[false; 4]; 4];
    while let Some(line) = lines.next() {
        let row: Vec<_> = line.split_whitespace().collect();
        if row[0] == "rootblock" {
            let first = row[1].parse::<i32>()?;
            let n = row[2].parse::<i32>()?;
            let step = row[3].parse::<i32>()?;
            let offset = row[4].parse::<i32>()?;
            let mut hash = 14695981039346656037u64;
            for i in 0..n {
                let root = math::rsqrt_normalized_q31(first + i * step + offset)?;
                for byte in root.to_le_bytes() {
                    hash = (hash ^ u64::from(byte)).wrapping_mul(1099511628211);
                }
                roots += 1;
            }
            assert_eq!(hash, row[5].parse::<u64>()?);
            continue;
        }
        assert_eq!(row[0], "stream");
        let n = row[1].parse()?;
        let k = row[2].parse()?;
        let spread_id = row[3].parse::<usize>()?;
        let spread = [
            Spread::None,
            Spread::Light,
            Spread::Normal,
            Spread::Aggressive,
        ][spread_id];
        let blocks = row[4].parse::<usize>()?;
        let gain = row[5].parse()?;
        let data = bytes(row[6])?;
        let mut entropy = RangeDecoder::new(&data);
        let book = PulseCodebook::new(n, k)?;
        modes[spread_id][blocks.trailing_zeros() as usize] = true;
        for _ in 0..row[7].parse::<usize>()? {
            let step: Vec<_> = lines
                .next()
                .ok_or("missing band row")?
                .split_whitespace()
                .collect();
            assert_eq!(step[0], "band");
            let band = pvq::decode_band(&mut entropy, book, spread, blocks, gain)?;
            assert_eq!(
                band.collapse_mask(),
                step[1].parse::<u8>()?,
                "mask case {count}"
            );
            assert_eq!(entropy.range(), step[2].parse::<u32>()?);
            assert_eq!(entropy.tell(), step[3].parse::<u32>()?);
            assert_eq!(entropy.tell_frac(), step[4].parse::<u32>()?);
            let expected: Vec<_> = step[5..]
                .iter()
                .map(|s| s.parse::<i32>())
                .collect::<Result<_, _>>()?;
            assert_eq!(
                band.values(),
                expected,
                "case {count} n{n} k{k} spread{spread_id} b{blocks} gain{gain}"
            );
            assert_eq!(band.values().len(), n);
            assert_ne!(band.collapse_mask(), 0);
            assert!(u16::from(band.collapse_mask()) < (1u16 << blocks));
            if gain == 0 {
                assert!(band.values().iter().all(|&x| x == 0));
            }
            count += 1;
        }
    }
    assert_eq!(roots, 196608);
    assert_eq!(modes, [[true; 4]; 4]);
    assert_eq!(count, 28464);
    Ok(())
}
#[test]
fn invalid_geometry_gains_and_missing_raw_bits_preserve_entropy()
-> Result<(), Box<dyn std::error::Error>> {
    let book = PulseCodebook::new(8, 1)?;
    let mut entropy = RangeDecoder::new(&[0xff; 4]);
    let before = entropy.clone();
    for blocks in [0, 3, 5, 6, 9, usize::MAX] {
        assert_eq!(
            pvq::decode_band(&mut entropy, book, Spread::Normal, blocks, i32::MAX),
            Err(PvqError::InvalidBlocks)
        );
        assert_eq!(entropy, before);
    }
    assert_eq!(
        pvq::decode_band(&mut entropy, PulseCodebook::new(6, 1)?, Spread::Light, 4, 1),
        Err(PvqError::InvalidBlocks)
    );
    assert_eq!(
        pvq::decode_band(&mut entropy, book, Spread::Light, 1, -1),
        Err(PvqError::InvalidGain)
    );
    assert_eq!(
        pvq::decode_band(&mut entropy, PulseCodebook::new(8, 0)?, Spread::Light, 1, 1),
        Err(PvqError::NoPulses)
    );
    assert_eq!(entropy, before);
    let mut empty = RangeDecoder::new(&[]);
    empty.decode_bit_logp(1)?;
    let before = empty.clone();
    assert!(
        pvq::decode_band(
            &mut empty,
            PulseCodebook::new(6, 95)?,
            Spread::Light,
            2,
            i32::MAX
        )
        .is_err()
    );
    assert_eq!(empty, before);
    for invalid in [i32::MIN, -1, 0, (1 << 29) - 1] {
        assert_eq!(
            math::rsqrt_normalized_q31(invalid),
            Err(math::MathError::NotNormalized)
        );
    }
    let mut control = entropy.clone();
    assert_eq!(
        pvq::decode_band(&mut entropy, book, Spread::Aggressive, 4, i32::MAX)?,
        pvq::decode_band(&mut control, book, Spread::Aggressive, 4, i32::MAX)?
    );
    assert_eq!(entropy, control);
    Ok(())
}
