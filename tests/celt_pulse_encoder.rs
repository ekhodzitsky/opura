//! CWRS ranking and entropy bytes against independent original encoder routines.
use opura::{
    celt::pulses::{PulseCodebook, PulseError},
    range::{EncoderError, RangeDecoder, RangeEncoder},
};

fn bytes(hex: &str) -> Result<Vec<u8>, std::num::ParseIntError> {
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16))
        .collect()
}
#[test]
fn all_representable_books_match_original_indices_streams_and_counters()
-> Result<(), Box<dyn std::error::Error>> {
    let mut lines = include_str!("celt-pulse-encoder-fixtures.txt")
        .lines()
        .filter(|l| !l.starts_with('#'));
    let mut seen = [[false; 129]; 177];
    let mut books = 0;
    let mut steps = 0;
    while let Some(line) = lines.next() {
        let header: Vec<_> = line.split_whitespace().collect();
        assert_eq!(header[0], "stream");
        let n = header[1].parse::<usize>()?;
        let k = header[2].parse::<usize>()?;
        assert!(!seen[n][k]);
        seen[n][k] = true;
        let book = PulseCodebook::new(n, k)?;
        assert_eq!(book.cardinality(), header[3].parse::<u32>()?);
        let data = bytes(header[4])?;
        let count = header[5].parse::<usize>()?;
        let mut writer = RangeEncoder::new(256)?;
        let mut decoder = RangeDecoder::new(&data);
        for _ in 0..count {
            let row: Vec<_> = lines
                .next()
                .ok_or("missing native step")?
                .split_whitespace()
                .collect();
            assert_eq!(row[0], "step");
            let index = row[1].parse::<u32>()?;
            let expected = (
                row[2].parse::<u32>()?,
                row[3].parse::<u32>()?,
                row[4].parse::<u32>()?,
            );
            let vector: Vec<i16> = row[5..]
                .iter()
                .map(|x| x.parse())
                .collect::<Result<_, _>>()?;
            let before = vector.clone();
            assert_eq!(vector.len(), n);
            assert_eq!(book.index_of(&vector)?, index, "n{n} k{k}");
            book.encode(&mut writer, &vector)?;
            assert_eq!(vector, before);
            assert_eq!(
                (writer.range(), writer.tell(), writer.tell_frac()),
                expected
            );
            assert_eq!(book.decode_index(index)?.values(), vector);
            assert_eq!(book.decode(&mut decoder)?.values(), vector);
            assert_eq!(
                (decoder.range(), decoder.tell(), decoder.tell_frac()),
                expected
            );
            steps += 1;
        }
        writer.shrink(data.len())?;
        assert_eq!(writer.finish()?, data, "n{n} k{k}");
        books += 1;
    }
    assert_eq!((books, steps), (1834, 13395));
    for (n, row) in seen.iter().enumerate().take(177).skip(2) {
        for (k, &present) in row.iter().enumerate() {
            assert_eq!(PulseCodebook::new(n, k).is_ok(), present, "book{n}/{k}");
        }
    }
    Ok(())
}

#[test]
fn exhaustive_small_books_and_deterministic_wide_indices_are_inverses() -> Result<(), PulseError> {
    let mut count = 0;
    for n in 2..=8 {
        for k in 0..=8 {
            let book = PulseCodebook::new(n, k)?;
            if book.cardinality() > 20000 {
                continue;
            }
            for index in 0..book.cardinality() {
                let vector = book.decode_index(index)?;
                assert_eq!(book.index_of(vector.values())?, index);
                count += 1;
            }
        }
    }
    assert_eq!(count, 70423);
    let mut seed = 0x43575253u32;
    for n in 2..=176 {
        for k in 0..=128 {
            let Ok(book) = PulseCodebook::new(n, k) else {
                continue;
            };
            for _ in 0..8 {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                let index = seed % book.cardinality();
                assert_eq!(book.index_of(book.decode_index(index)?.values())?, index);
            }
        }
    }
    Ok(())
}

#[test]
fn invalid_vectors_entropy_capacity_and_lifecycle_preserve_the_writer()
-> Result<(), Box<dyn std::error::Error>> {
    let book = PulseCodebook::new(4, 3)?;
    let mut writer = RangeEncoder::new(32)?;
    writer.encode_uint(173, 256)?;
    let before = writer.clone();
    for values in [
        &[][..],
        &[3, 0, 0][..],
        &[3, 0, 0, 0, 0][..],
        &[0, 0, 0, 0][..],
        &[4, 0, 0, 0][..],
        &[2, -2, 0, 0][..],
        &[i16::MIN, 0, 0, 0][..],
        &[i16::MAX; 4][..],
    ] {
        assert_eq!(book.index_of(values), Err(PulseError::InvalidVector));
        assert_eq!(
            book.encode(&mut writer, values),
            Err(PulseError::InvalidVector)
        );
        assert_eq!(writer, before);
    }
    let big = PulseCodebook::new(176, 1)?;
    assert_eq!(
        big.index_of(&[i16::MIN; 176]),
        Err(PulseError::InvalidVector)
    );
    let mut tight = RangeEncoder::new(1)?;
    tight.encode_uint(0, 256)?;
    tight.encode_uint(0, 256)?;
    let before = tight.clone();
    let book = PulseCodebook::new(2, 128)?;
    assert_eq!(
        book.encode(&mut tight, &[128, 0]),
        Err(PulseError::Encoding(EncoderError::CapacityExhausted))
    );
    assert_eq!(tight, before);
    let zero = PulseCodebook::new(176, 0)?;
    let before = writer.clone();
    zero.encode(&mut writer, &[0; 176])?;
    assert_eq!(writer, before);
    assert_eq!(zero.index_of(&[0; 176])?, 0);
    writer.finish()?;
    let finished = writer.clone();
    assert_eq!(
        zero.encode(&mut writer, &[0; 176]),
        Err(PulseError::Encoding(EncoderError::Finished))
    );
    assert_eq!(
        book.encode(&mut writer, &[-128, 0]),
        Err(PulseError::Encoding(EncoderError::Finished))
    );
    assert_eq!(writer, finished);
    Ok(())
}
