//! Contracts for the frame-local Opus entropy decoder.
use opura::range::{RangeDecoder, RangeError};

#[test]
fn initialization_zero_extension_and_raw_overlap() -> Result<(), RangeError> {
    let mut empty = RangeDecoder::new(&[]);
    assert_eq!(
        (empty.range(), empty.tell(), empty.tell_frac()),
        (1 << 31, 1, 8)
    );
    for _ in 0..100 {
        assert_eq!(empty.decode_uint(256)?, 0);
    }
    let mut decoder = RangeDecoder::new(&[0x81]);
    assert_eq!(decoder.decode_bits(4)?, 1);
    assert_eq!(decoder.decode_bits(4)?, 8);
    assert_eq!(decoder.decode_bits(1), Err(RangeError::RawBitsExhausted));
    assert_eq!(decoder.decode_bits(0)?, 0);
    // Range initialization has already read the byte; raw access must still work.
    assert_eq!(decoder.tell(), 9);
    Ok(())
}

#[test]
fn invalid_models_leave_decoder_unchanged() -> Result<(), RangeError> {
    let original = RangeDecoder::new(&[0x91, 0x47, 0xd2, 0x33]);
    let mut d = original.clone();
    for cdf in [&[][..], &[0][..], &[1, 2][..], &[0, 0][..], &[0, 4, 3][..]] {
        assert_eq!(d.decode_cdf(cdf), Err(RangeError::InvalidDistribution));
        assert_eq!(d, original);
    }
    for (table, bits) in [
        (&[][..], 8),
        (&[1][..], 8),
        (&[0, 1, 0][..], 8),
        (&[4, 0][..], 2),
        (&[0][..], 0),
        (&[0][..], 9),
    ] {
        assert!(d.decode_icdf(table, bits).is_err());
        assert_eq!(d, original);
    }
    for bits in [0, 16, u32::MAX] {
        assert_eq!(d.decode_bit_logp(bits), Err(RangeError::InvalidPrecision));
        assert_eq!(d, original);
    }
    for total in [0, 1] {
        assert_eq!(d.decode_uint(total), Err(RangeError::InvalidTotal));
        assert_eq!(d, original);
    }
    assert_eq!(d.decode_bits(26), Err(RangeError::InvalidPrecision));
    assert_eq!(d, original);
    d.decode_bits(25)?;
    let before = d.clone();
    assert_eq!(d.decode_bits(8), Err(RangeError::RawBitsExhausted));
    assert_eq!(d, before);
    Ok(())
}

#[test]
fn cdf_and_inverse_cdf_agree_with_zero_probability_symbols() -> Result<(), RangeError> {
    for input in [vec![], vec![0], vec![255], vec![0x91, 0x23, 0xd4, 0x56]] {
        let mut a = RangeDecoder::new(&input);
        let mut b = a.clone();
        for _ in 0..100 {
            assert_eq!(
                a.decode_cdf(&[0, 4, 4, 7, 8])?,
                b.decode_icdf(&[4, 4, 1, 0], 3)?
            );
            assert_eq!(
                (a.range(), a.tell(), a.tell_frac()),
                (b.range(), b.tell(), b.tell_frac())
            );
        }
    }
    Ok(())
}

#[test]
fn reference_operations_match_symbols_range_and_bit_usage() -> Result<(), Box<dyn std::error::Error>>
{
    let fixture = include_str!("range-fixtures.txt");
    let mut input = Vec::new();
    let mut decoder = RangeDecoder::new(&[]);
    let mut operations = 0;
    for (line_index, line) in fixture.lines().enumerate() {
        if line.starts_with('#') {
            continue;
        }
        if let Some(hex) = line.strip_prefix("frame ") {
            input.clear();
            if hex != "-" {
                for i in (0..hex.len()).step_by(2) {
                    input.push(u8::from_str_radix(&hex[i..i + 2], 16)?);
                }
            }
            decoder = RangeDecoder::new(&input);
            continue;
        }
        let columns: Vec<_> = line.split_whitespace().collect();
        let args: Vec<_> = columns[0].split(':').collect();
        let before = decoder.clone();
        let result = match args[0] {
            "init" => Ok(0),
            "b" => decoder.decode_bit_logp(args[1].parse()?).map(u32::from),
            "r" => decoder.decode_bits(args[1].parse()?),
            "u" => decoder.decode_uint(args[1].parse()?),
            "c" => {
                let cdf: Vec<u16> = args[1]
                    .split(',')
                    .map(str::parse)
                    .collect::<Result<_, _>>()?;
                decoder.decode_cdf(&cdf).map(|v| v as u32)
            }
            "i" => {
                let icdf: Vec<u8> = args[2]
                    .split(',')
                    .map(str::parse)
                    .collect::<Result<_, _>>()?;
                decoder
                    .decode_icdf(&icdf, args[1].parse()?)
                    .map(|v| v as u32)
            }
            _ => return Err("unknown fixture operation".into()),
        };
        if columns[1] == "corrupt" {
            assert_eq!(
                result,
                Err(RangeError::CorruptUniformValue),
                "line {}",
                line_index + 1
            );
            assert_eq!(decoder, before);
        } else {
            assert_eq!(
                result?,
                columns[1].parse::<u32>()?,
                "line {}",
                line_index + 1
            );
        }
        assert_eq!(
            (decoder.range(), decoder.tell(), decoder.tell_frac()),
            (
                columns[2].parse()?,
                columns[3].parse()?,
                columns[4].parse()?
            ),
            "line {}",
            line_index + 1
        );
        operations += 1;
    }
    assert_eq!(operations, 1539);
    Ok(())
}

#[test]
fn raw_widths_and_uniform_boundaries() -> Result<(), RangeError> {
    for bits in 0..=25 {
        let mut d = RangeDecoder::new(&[0x12, 0x34, 0x56, 0x78]);
        assert_eq!(d.decode_bits(bits)?, 0x1234_5678 & ((1u32 << bits) - 1));
        assert_eq!(d.tell(), 1 + bits);
        assert_eq!(d.tell_frac(), 8 + bits * 8);
    }
    for total in [2, 3, 255, 256, 257, 65535, 65536, 65537, u32::MAX] {
        let mut zero = RangeDecoder::new(&[0; 16]);
        assert_eq!(zero.decode_uint(total)?, 0);
        let mut ones = RangeDecoder::new(&[255; 16]);
        let before = ones.clone();
        match ones.decode_uint(total) {
            Ok(value) => assert!(value < total),
            Err(error) => {
                assert_eq!(error, RangeError::CorruptUniformValue);
                assert_eq!(ones, before);
            }
        }
    }
    let mut corrupt = RangeDecoder::new(&[255; 8]);
    let before = corrupt.clone();
    assert_eq!(
        corrupt.decode_uint(257),
        Err(RangeError::CorruptUniformValue)
    );
    assert_eq!(corrupt, before);
    // The uniform prefix was decoded before discovering missing raw suffix bits.
    let mut empty = RangeDecoder::new(&[]);
    let before = empty.clone();
    assert_eq!(
        empty.decode_uint(u32::MAX),
        Err(RangeError::RawBitsExhausted)
    );
    assert_eq!(empty, before);
    Ok(())
}

#[test]
fn frames_do_not_read_neighboring_payload_or_padding() -> Result<(), Box<dyn std::error::Error>> {
    let a = [3, 0xc2, 2, 1, 0x91, 0x00, 0x00, 0x00];
    let b = [3, 0xc2, 2, 1, 0x91, 0xff, 0xff, 0xff];
    let packet_a = opura::Packet::parse(&a)?;
    let packet_b = opura::Packet::parse(&b)?;
    let mut left = RangeDecoder::new(packet_a.frames()[0]);
    let mut right = RangeDecoder::new(packet_b.frames()[0]);
    for _ in 0..128 {
        assert_eq!(left.decode_uint(251)?, right.decode_uint(251)?);
        assert_eq!(
            (left.range(), left.tell_frac()),
            (right.range(), right.tell_frac())
        );
    }
    assert_eq!(left.decode_bits(8)?, 0x91);
    assert_eq!(right.decode_bits(8)?, 0x91);
    assert_eq!(left.decode_bits(1), Err(RangeError::RawBitsExhausted));
    Ok(())
}

#[test]
fn seeded_inputs_preserve_range_and_accounting_invariants() -> Result<(), RangeError> {
    let mut seed = 0x48f9_aa31u32;
    for size in 0..=1275 {
        let mut input = Vec::with_capacity(size);
        for _ in 0..size {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            input.push(seed as u8);
        }
        let mut decoder = RangeDecoder::new(&input);
        for step in 0..32 {
            let prior = decoder.tell_frac();
            decoder.decode_bit_logp(1 + step % 15)?;
            decoder.decode_cdf(&[0, 0, 1, 2, 65534, 65535])?;
            let precision = 1 + step % 8;
            decoder.decode_icdf(&[1, 1, 0], precision)?;
            let saved = decoder.clone();
            if let Err(error) = decoder.decode_uint(u32::MAX - step) {
                assert!(matches!(
                    error,
                    RangeError::RawBitsExhausted | RangeError::CorruptUniformValue
                ));
                assert_eq!(decoder, saved);
            }
            assert!((1 << 23..=1 << 31).contains(&decoder.range()));
            assert!(decoder.tell_frac() >= prior);
            assert_eq!(decoder.tell(), decoder.tell_frac().div_ceil(8));
        }
    }
    Ok(())
}
