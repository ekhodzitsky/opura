//! Public PVQ band encoding, mixed entropy sequences and atomic domain failures.
use opura::{
    celt::{
        pulses::{PulseCodebook, PulseError},
        pvq::{self, PvqError, Spread},
    },
    range::{EncoderError, RangeDecoder, RangeEncoder},
};

#[test]
fn mixed_band_sequences_round_trip_with_identical_samples_masks_and_counters()
-> Result<(), Box<dyn std::error::Error>> {
    let mut writer = RangeEncoder::new(1275)?;
    writer.encode_uint(173, 257)?;
    let mut expected = Vec::new();
    for index in 0..48 {
        let n = [2, 3, 4, 8, 16, 32, 64, 176][index % 8];
        let requested = [1, 2, 3, 4, 8, 13, 36, 128][index / 8 % 8];
        let book = match PulseCodebook::new(n, requested) {
            Ok(book) => book,
            Err(PulseError::CardinalityOverflow) => PulseCodebook::new(n, 1)?,
            Err(e) => return Err(e.into()),
        };
        let valid_blocks: Vec<_> = [1, 2, 4, 8].into_iter().filter(|b| n % b == 0).collect();
        let blocks = valid_blocks[(index / 4) % valid_blocks.len()];
        let spread = [
            Spread::None,
            Spread::Light,
            Spread::Normal,
            Spread::Aggressive,
        ][index % 4];
        let gain = [0, 1 << 30, i32::MAX][index % 3];
        let mut input = vec![0; n];
        if index % 3 == 1 {
            input[index % n] = if index % 2 == 0 { 1 << 24 } else { -(1 << 24) };
        }
        if index % 3 == 2 {
            for (i, v) in input.iter_mut().enumerate() {
                *v = (((index * 97 + i * 13) % 257) as i32 - 128) << 10;
            }
        }
        let before = input.clone();
        let output = pvq::encode_band(&mut writer, book, &input, spread, blocks, gain)?;
        assert_eq!(input, before);
        expected.push((
            book,
            spread,
            blocks,
            gain,
            output,
            writer.range(),
            writer.tell(),
            writer.tell_frac(),
        ));
    }
    writer.shrink(writer.tell().div_ceil(8) as usize)?;
    let mut decoder = RangeDecoder::new(writer.finish()?);
    assert_eq!(decoder.decode_uint(257)?, 173);
    for (book, spread, blocks, gain, expected, range, tell, fraction) in expected {
        let output = pvq::decode_band(&mut decoder, book, spread, blocks, gain)?;
        assert_eq!(output, expected);
        assert_eq!(
            (decoder.range(), decoder.tell(), decoder.tell_frac()),
            (range, tell, fraction)
        );
    }
    Ok(())
}

#[test]
fn exact_energy_boundary_is_supported_and_invalid_domains_preserve_writer()
-> Result<(), Box<dyn std::error::Error>> {
    let mut writer = RangeEncoder::new(64)?;
    writer.encode_uint(173, 257)?;
    let book = PulseCodebook::new(2, 128)?;
    pvq::encode_band(
        &mut writer,
        book,
        &[1 << 24, 1 << 24],
        Spread::None,
        1,
        i32::MAX,
    )?;
    let before = writer.clone();
    for input in [
        &[][..],
        &[0][..],
        &[0, 0, 0][..],
        &[i32::MIN, 0][..],
        &[i32::MAX, 0][..],
        &[1 << 24, (1 << 24) + 1][..],
    ] {
        assert_eq!(
            pvq::encode_band(&mut writer, book, input, Spread::None, 1, 0),
            Err(PvqError::InvalidInput)
        );
        assert_eq!(writer, before);
    }
    for blocks in [0, 3, 5, 8, 16, usize::MAX] {
        assert_eq!(
            pvq::encode_band(&mut writer, book, &[0, 0], Spread::None, blocks, 0),
            Err(PvqError::InvalidBlocks)
        );
        assert_eq!(writer, before);
    }
    assert_eq!(
        pvq::encode_band(&mut writer, book, &[0, 0], Spread::None, 1, -1),
        Err(PvqError::InvalidGain)
    );
    assert_eq!(writer, before);
    assert_eq!(
        pvq::encode_band(
            &mut writer,
            PulseCodebook::new(2, 0)?,
            &[0, 0],
            Spread::None,
            1,
            0
        ),
        Err(PvqError::NoPulses)
    );
    assert_eq!(writer, before);
    let mut tight = RangeEncoder::new(1)?;
    tight.encode_uint(0, 256)?;
    tight.encode_uint(0, 256)?;
    let original = tight.clone();
    assert_eq!(
        pvq::encode_band(
            &mut tight,
            book,
            &[1 << 24, 0],
            Spread::Aggressive,
            1,
            i32::MAX
        ),
        Err(PvqError::Pulses(PulseError::Encoding(
            EncoderError::CapacityExhausted
        )))
    );
    assert_eq!(tight, original);
    writer.finish()?;
    let original = writer.clone();
    assert_eq!(
        pvq::encode_band(&mut writer, book, &[0, 0], Spread::None, 1, 0),
        Err(PvqError::Pulses(PulseError::Encoding(
            EncoderError::Finished
        )))
    );
    assert_eq!(writer, original);
    Ok(())
}
