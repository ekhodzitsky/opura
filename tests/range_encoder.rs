//! Entropy writer round trips, resource boundaries and transactional errors.
use opura::range::{EncoderError, RangeDecoder, RangeEncoder};

fn random(seed: &mut u32) -> u32 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 17;
    *seed ^= *seed << 5;
    *seed
}

#[test]
fn mixed_models_round_trip_with_identical_range_and_bit_budgets()
-> Result<(), Box<dyn std::error::Error>> {
    for seed in 1..=128 {
        let mut random_state = seed;
        let mut encoder = RangeEncoder::new(1275)?;
        let mut operations = Vec::new();
        for i in 0..160 {
            let value = random(&mut random_state);
            let (kind, value, extra) = match i % 6 {
                0 => {
                    let total = [2, 3, 256, 257, 65535, 65536, u32::MAX][value as usize % 7];
                    encoder.encode_uint(value % total, total)?;
                    (0, value % total, total)
                }
                1 => {
                    let bits = value % 26;
                    let value = random(&mut random_state) & ((1 << bits) - 1);
                    encoder.encode_bits(value, bits)?;
                    (1, value, bits)
                }
                2 => {
                    let logp = value % 15 + 1;
                    let bit = value >> 31;
                    encoder.encode_bit_logp(bit != 0, logp)?;
                    (2, bit, logp)
                }
                3 => {
                    let symbol = value as usize % 4;
                    encoder.encode_cdf(symbol, &[0, 1, 7, 22, 65535])?;
                    (3, symbol as u32, 0)
                }
                4 => {
                    let symbol = value as usize % 5;
                    encoder.encode_icdf(symbol, &[255, 254, 128, 1, 0], 8)?;
                    (4, symbol as u32, 0)
                }
                _ => {
                    let actual = encoder.encode_laplace(
                        value as i32,
                        1 + (value % 32736) as u16,
                        (value % 11457) as u16,
                    )?;
                    (5, actual as u32, value)
                }
            };
            operations.push((
                kind,
                value,
                extra,
                encoder.range(),
                encoder.tell(),
                encoder.tell_frac(),
            ));
        }
        let mut decoder = RangeDecoder::new(encoder.finish()?);
        for (kind, value, extra, range, tell, fraction) in operations {
            let actual = match kind {
                0 => decoder.decode_uint(extra)?,
                1 => decoder.decode_bits(extra)?,
                2 => u32::from(decoder.decode_bit_logp(extra)?),
                3 => decoder.decode_cdf(&[0, 1, 7, 22, 65535])? as u32,
                4 => decoder.decode_icdf(&[255, 254, 128, 1, 0], 8)? as u32,
                _ => decoder.decode_laplace(1 + (extra % 32736) as u16, (extra % 11457) as u16)?
                    as u32,
            };
            assert_eq!(
                (actual, decoder.range(), decoder.tell(), decoder.tell_frac()),
                (value, range, tell, fraction),
                "seed{seed}"
            );
        }
    }
    Ok(())
}

#[test]
fn invalid_models_values_and_lifecycle_preserve_the_whole_writer()
-> Result<(), Box<dyn std::error::Error>> {
    assert_eq!(RangeEncoder::new(0), Err(EncoderError::InvalidCapacity));
    assert_eq!(RangeEncoder::new(1276), Err(EncoderError::InvalidCapacity));
    let mut e = RangeEncoder::new(32)?;
    e.encode_uint(173, 256)?;
    e.encode_bits(12345, 17)?;
    let before = e.clone();
    for cdf in [&[][..], &[0][..], &[1, 2][..], &[0, 2, 1][..], &[0, 0][..]] {
        assert!(e.encode_cdf(0, cdf).is_err());
        assert_eq!(e, before);
    }
    for (symbol, cdf) in [
        (0, &[0, 0, 2][..]),
        (2, &[0, 1, 2][..]),
        (usize::MAX, &[0, 1][..]),
    ] {
        assert!(e.encode_cdf(symbol, cdf).is_err());
        assert_eq!(e, before);
    }
    for icdf in [&[][..], &[1][..], &[0, 1, 0][..]] {
        assert!(e.encode_icdf(0, icdf, 8).is_err());
        assert_eq!(e, before);
    }
    for (symbol, icdf, bits) in [
        (0, &[2, 0][..], 1),
        (1, &[1, 1, 0][..], 1),
        (2, &[1, 0][..], 1),
        (0, &[0][..], 0),
        (0, &[0][..], 9),
    ] {
        assert!(e.encode_icdf(symbol, icdf, bits).is_err());
        assert_eq!(e, before);
    }
    for result in [
        e.encode_uint(0, 0),
        e.encode_uint(0, 1),
        e.encode_uint(3, 3),
        e.encode_bits(1, 0),
        e.encode_bits(8, 3),
        e.encode_bits(0, 26),
        e.encode_bit_logp(false, 0),
        e.encode_bit_logp(true, 16),
        e.patch_initial_bits(256, 8),
        e.patch_initial_bits(0, 9),
        e.shrink(33),
        e.shrink(0),
    ] {
        assert!(result.is_err());
    }
    assert_eq!(e, before);
    for (value, zero, decay) in [(0, 0, 0), (0, 32737, 0), (0, 1, 11457), (i32::MIN, 1, 0)] {
        assert!(e.encode_laplace(value, zero, decay).is_err());
        assert_eq!(e, before);
    }
    let mut empty = RangeEncoder::new(1)?;
    let original = empty.clone();
    assert_eq!(
        empty.patch_initial_bits(1, 1),
        Err(EncoderError::UnavailablePrefix)
    );
    assert_eq!(empty, original);
    assert_eq!(e.bytes(), None);
    let bytes = e.finish()?.to_vec();
    let finished = e.clone();
    assert_eq!(e.finish()?, bytes);
    assert_eq!(e.encode_uint(0, 2), Err(EncoderError::Finished));
    assert_eq!(e.encode_bits(0, 0), Err(EncoderError::Finished));
    assert_eq!(e.patch_initial_bits(0, 0), Err(EncoderError::Finished));
    assert_eq!(e.shrink(1), Err(EncoderError::Finished));
    assert_eq!(e, finished);
    e.reset();
    assert_eq!(e, RangeEncoder::new(32)?);
    Ok(())
}

#[test]
fn tight_storage_finish_and_late_write_failures_are_atomic()
-> Result<(), Box<dyn std::error::Error>> {
    // Eight exact equiprobable bits can fit in one byte even though tell is9.
    let mut exact = RangeEncoder::new(1)?;
    exact.encode_uint(255, 256)?;
    assert_eq!(exact.tell(), 9);
    assert_eq!(exact.finish()?, [255]);
    // Seven raw bits share the unused low bits of the terminating range byte.
    let mut shared = RangeEncoder::new(1)?;
    shared.encode_bit_logp(false, 1)?;
    shared.encode_bits(127, 7)?;
    assert_eq!(shared.finish()?, [127]);
    for capacity in 1..=16 {
        let mut e = RangeEncoder::new(capacity)?;
        let mut errors = 0;
        for i in 0..256 {
            let before = e.clone();
            let result = match i % 3 {
                0 => e.encode_uint(65534, 65535),
                1 => e.encode_bits(33554431, 25),
                _ => e.encode_bit_logp(true, 15),
            };
            if result.is_err() {
                assert_eq!(e, before);
                errors += 1;
            }
        }
        let before = e.clone();
        assert_eq!(e.finish(), Err(EncoderError::CapacityExhausted));
        assert_eq!(e, before);
        assert!(errors > 0);
        e.reset();
        assert_eq!(e, RangeEncoder::new(capacity)?);
    }
    let mut e = RangeEncoder::new(8)?;
    e.encode_bits(33554431, 25)?;
    e.encode_bits(33554431, 25)?;
    let before = e.clone();
    assert_eq!(e.shrink(2), Err(EncoderError::CapacityExhausted));
    assert_eq!(e, before);
    e.shrink(7)?;
    let bytes = e.finish()?;
    let mut decoder = RangeDecoder::new(bytes);
    assert_eq!(decoder.decode_bits(25)?, 33554431);
    assert_eq!(decoder.decode_bits(25)?, 33554431);
    Ok(())
}

#[test]
fn laplace_extreme_values_round_trip_for_every_zero_frequency()
-> Result<(), Box<dyn std::error::Error>> {
    for zero in 1..=32736 {
        for decay in [0, 11456] {
            let value = if zero % 2 == 0 { i32::MAX } else { -i32::MAX };
            let mut encoder = RangeEncoder::new(8)?;
            let actual = encoder.encode_laplace(value, zero, decay)?;
            assert_eq!(actual.signum(), value.signum());
            assert!(actual.abs() <= 16384);
            let mut decoder = RangeDecoder::new(encoder.finish()?);
            assert_eq!(
                decoder.decode_laplace(zero, decay)?,
                actual,
                "zero{zero} decay{decay}"
            );
        }
    }
    Ok(())
}
