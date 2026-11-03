//! CWRS/PVQ vectors against original generic reference and independent counts.
use opura::{
    celt::pulses::{PulseCodebook, PulseError, PulseVector},
    range::RangeDecoder,
};
fn bytes(s: &str) -> Result<Vec<u8>, std::num::ParseIntError> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16))
        .collect()
}
fn invariant(vector: &PulseVector, k: usize) {
    assert_eq!(
        vector
            .values()
            .iter()
            .map(|x| usize::from(x.unsigned_abs()))
            .sum::<usize>(),
        k
    );
    assert_eq!(
        vector.squared_norm(),
        vector
            .values()
            .iter()
            .map(|&x| (i32::from(x) * i32::from(x)) as u32)
            .sum()
    );
    assert!(vector.squared_norm() <= 16384);
}
fn compare(vector: &PulseVector, row: &[&str]) -> Result<(), Box<dyn std::error::Error>> {
    assert_eq!(vector.squared_norm(), row[0].parse::<u32>()?);
    let expected: Vec<_> = row[1..]
        .iter()
        .map(|s| s.parse::<i16>())
        .collect::<Result<_, _>>()?;
    assert_eq!(vector.values(), expected);
    Ok(())
}
#[test]
fn counts_all_small_indices_wide_vectors_and_entropy_match_reference()
-> Result<(), Box<dyn std::error::Error>> {
    let mut lines = include_str!("celt-pulse-fixtures.txt")
        .lines()
        .filter(|l| !l.starts_with('#'));
    let mut books = 0;
    let mut fitting = 0;
    let mut exhaustive = 0;
    let mut vectors = 0;
    let mut streams = 0;
    while let Some(line) = lines.next() {
        let row: Vec<_> = line.split_whitespace().collect();
        let n = row[1].parse()?;
        let k = row[2].parse()?;
        if row[0] == "book" {
            books += 1;
            let count = row[3].parse::<u32>()?;
            if count == 0 {
                assert_eq!(
                    PulseCodebook::new(n, k),
                    Err(PulseError::CardinalityOverflow)
                );
            } else {
                let book = PulseCodebook::new(n, k)?;
                assert_eq!(book.cardinality(), count);
                assert_eq!(book.dimensions(), n);
                assert_eq!(book.pulses(), k);
                fitting += 1;
            }
            continue;
        }
        let book = PulseCodebook::new(n, k)?;
        match row[0] {
            "all" => {
                let count = row[3].parse::<u32>()?;
                assert_eq!(book.cardinality(), count);
                let mut hash = 14695981039346656037u64;
                for index in 0..count {
                    let vector = book.decode_index(index)?;
                    invariant(&vector, k);
                    for value in std::iter::once(vector.squared_norm() as i32)
                        .chain(vector.values().iter().map(|&v| i32::from(v)))
                    {
                        for byte in value.to_le_bytes() {
                            hash = (hash ^ u64::from(byte)).wrapping_mul(1099511628211);
                        }
                    }
                    exhaustive += 1;
                }
                assert_eq!(hash, row[4].parse::<u64>()?, "codebook {n},{k}");
            }
            "vector" => {
                let vector = book.decode_index(row[3].parse()?)?;
                invariant(&vector, k);
                compare(&vector, &row[4..])?;
                vectors += 1;
            }
            "stream" => {
                let data = bytes(row[3])?;
                let mut entropy = RangeDecoder::new(&data);
                for _ in 0..row[4].parse::<usize>()? {
                    let step: Vec<_> = lines
                        .next()
                        .ok_or("missing step")?
                        .split_whitespace()
                        .collect();
                    assert_eq!(step[0], "step");
                    let vector = book.decode(&mut entropy)?;
                    invariant(&vector, k);
                    compare(&vector, &step[4..])?;
                    assert_eq!(entropy.range(), step[1].parse::<u32>()?);
                    assert_eq!(entropy.tell(), step[2].parse::<u32>()?);
                    assert_eq!(entropy.tell_frac(), step[3].parse::<u32>()?);
                }
                streams += 1;
            }
            _ => panic!("unexpected fixture row"),
        }
    }
    assert_eq!(books, 22575);
    assert_eq!(fitting, 1834);
    assert!(exhaustive > 50000);
    assert!(vectors > 10000);
    assert!(streams > 50);
    Ok(())
}

#[test]
fn closed_form_cardinalities_validate_every_geometry() -> Result<(), Box<dyn std::error::Error>> {
    fn choose(n: usize, k: usize) -> u128 {
        let mut value = 1u128;
        for i in 1..=k.min(n - k) {
            value = value * (n + 1 - i) as u128 / i as u128;
            if value > u32::MAX as u128 {
                return u32::MAX as u128 + 1;
            }
        }
        value
    }
    for n in 2..=176 {
        assert_eq!(PulseCodebook::new(n, 0)?.cardinality(), 1);
        for k in 1..=128 {
            let mut total = 0u128;
            for occupied in 1..=n.min(k) {
                total += (1u128 << occupied) * choose(n, occupied) * choose(k - 1, occupied - 1);
                if total > u32::MAX as u128 {
                    break;
                }
            }
            if total <= u32::MAX as u128 {
                assert_eq!(PulseCodebook::new(n, k)?.cardinality(), total as u32);
            } else {
                assert_eq!(
                    PulseCodebook::new(n, k),
                    Err(PulseError::CardinalityOverflow)
                );
            }
        }
    }
    Ok(())
}

#[test]
fn invalid_codebooks_indices_and_entropy_reject_without_advancing()
-> Result<(), Box<dyn std::error::Error>> {
    for (n, k) in [(0, 0), (1, 1), (177, 1), (2, 129), (usize::MAX, usize::MAX)] {
        assert_eq!(PulseCodebook::new(n, k), Err(PulseError::InvalidGeometry));
    }
    let book = PulseCodebook::new(6, 95)?;
    assert_eq!(
        book.decode_index(book.cardinality()),
        Err(PulseError::InvalidIndex)
    );
    assert_eq!(book.decode_index(u32::MAX), Err(PulseError::InvalidIndex));
    let mut entropy = RangeDecoder::new(&[]);
    let before = entropy.clone();
    assert!(book.decode(&mut entropy).is_err());
    assert_eq!(entropy, before);
    // The uniform prefix may advance range state before raw suffix exhaustion.
    for byte in 0..=u8::MAX {
        let data = [byte];
        let mut cursor = RangeDecoder::new(&data);
        cursor.decode_bit_logp(1)?;
        let before = cursor.clone();
        assert_eq!(
            book.decode(&mut cursor),
            Err(PulseError::Entropy(
                opura::range::RangeError::RawBitsExhausted
            ))
        );
        assert_eq!(cursor, before);
    }
    let zero = PulseCodebook::new(176, 0)?.decode(&mut entropy)?;
    assert_eq!(zero.values(), [0; 176]);
    assert_eq!(zero.squared_norm(), 0);
    assert_eq!(entropy, before);
    let data = bytes("f38a014754cda841")?;
    let mut entropy = RangeDecoder::new(&data);
    let small = PulseCodebook::new(3, 4)?;
    let mut control = entropy.clone();
    assert_eq!(small.decode(&mut entropy)?, small.decode(&mut control)?);
    assert_eq!(entropy, control);
    Ok(())
}

#[test]
fn specialized_unranking_preserves_every_geometry_and_boundary()
-> Result<(), Box<dyn std::error::Error>> {
    // Independent generic traversal retained only as a test oracle. Native
    // fixture tests above establish its coordinate order and cardinalities.
    let mut table = [[0u32; 177]; 15];
    table[0][0] = 1;
    for n in 1..15 {
        for k in 1..177 {
            table[n][k] = table[n - 1][k]
                .saturating_add(table[n][k - 1])
                .saturating_add(table[n - 1][k - 1]);
        }
    }
    let u = |n: usize, k: usize| {
        if n.min(k) >= 15 {
            u32::MAX
        } else {
            table[n.min(k)][n.max(k)]
        }
    };
    let mut seed = 0x624b6321u32;
    let mut checked = 0;
    for n in 2..=176 {
        for k in 0..=128 {
            let book = match PulseCodebook::new(n, k) {
                Ok(book) => book,
                Err(PulseError::CardinalityOverflow) => continue,
                Err(error) => return Err(error.into()),
            };
            let count = book.cardinality();
            let boundary = u(n, k + 1);
            let mut indices = vec![0, count - 1];
            for index in [
                boundary.saturating_sub(1),
                boundary,
                boundary.saturating_add(1),
            ] {
                if index < count {
                    indices.push(index);
                }
            }
            if count <= 512 {
                indices.extend(0..count);
            } else {
                for _ in 0..64 {
                    seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                    indices.push(seed % count);
                }
            }
            for original in indices {
                let (mut index, mut remaining) = (original, k);
                let mut expected = vec![0; n];
                for (coordinate, dimensions) in (1..=n).rev().enumerate() {
                    let boundary = u(dimensions, remaining + 1);
                    let negative = index >= boundary;
                    if negative {
                        index -= boundary;
                    }
                    let before = remaining;
                    while u(dimensions, remaining) > index {
                        remaining -= 1;
                    }
                    index -= u(dimensions, remaining);
                    let magnitude = (before - remaining) as i16;
                    expected[coordinate] = if negative { -magnitude } else { magnitude };
                }
                let actual = book.decode_index(original)?;
                assert_eq!(actual.values(), expected, "n={n} k={k} index={original}");
                assert_eq!(book.index_of(actual.values())?, original);
                invariant(&actual, k);
                checked += 1;
            }
            assert_eq!(book.decode_index(count), Err(PulseError::InvalidIndex));
        }
    }
    assert!(checked > 100000);
    Ok(())
}
