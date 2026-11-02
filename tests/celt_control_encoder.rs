//! Pinned differential TF and spreading field bytes/mappings/accounting.
use opura::{
    celt::{
        FrameSize,
        control::{self, ControlError},
        pvq::Spread,
    },
    range::{EncoderError, RangeDecoder, RangeEncoder},
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
fn prefix(
    writer: &mut RangeEncoder,
    position: usize,
    target: u32,
    pattern: u32,
) -> Result<(), EncoderError> {
    if position == 1 {
        writer.encode_uint(13, 17)?;
    }
    if position == 2 {
        for i in 0..3 {
            writer.encode_bit_logp(i % 2 != 0, 2 + i)?;
        }
    }
    let mut random = 0x54464354u32 + pattern;
    while writer.tell() < target {
        let width = (target - writer.tell()).min(25);
        random = random.wrapping_mul(1664525).wrapping_add(1013904223);
        let value = match pattern {
            0 => 0,
            1 => u32::MAX,
            _ => random,
        } & ((1 << width) - 1);
        writer.encode_bits(value, width)?;
    }
    Ok(())
}
fn decode_prefix(
    decoder: &mut RangeDecoder<'_>,
    position: usize,
    target: u32,
    pattern: u32,
) -> Result<(), Box<dyn std::error::Error>> {
    if position == 1 {
        assert_eq!(decoder.decode_uint(17)?, 13);
    }
    if position == 2 {
        for i in 0..3 {
            assert_eq!(decoder.decode_bit_logp(2 + i)?, i % 2 != 0);
        }
    }
    let mut random = 0x54464354u32 + pattern;
    while decoder.tell() < target {
        let width = (target - decoder.tell()).min(25);
        random = random.wrapping_mul(1664525).wrapping_add(1013904223);
        let value = match pattern {
            0 => 0,
            1 => u32::MAX,
            _ => random,
        } & ((1 << width) - 1);
        assert_eq!(decoder.decode_bits(width)?, value);
    }
    Ok(())
}
#[test]
fn original_tf_spread_effective_values_complete_bytes_and_counters_match()
-> Result<(), Box<dyn std::error::Error>> {
    let mut rows = 0;
    let mut empty = 0;
    let mut fallback = 0;
    for (line_index, line) in include_str!("celt-control-encoder-fixtures.txt")
        .lines()
        .enumerate()
    {
        if line.starts_with('#') {
            continue;
        }
        let c: Vec<_> = line.split_whitespace().collect();
        assert_eq!(c[0], "controls");
        let size = SIZES[c[1].parse::<usize>()?];
        let start = c[2].parse::<usize>()?;
        let end = c[3].parse::<usize>()?;
        let transient = c[4] == "1";
        let position = c[5].parse::<usize>()?;
        let remaining = c[6].parse::<u32>()?;
        let pattern = c[7].parse::<u32>()?;
        let select = c[8] == "1";
        let requested = SPREADS[c[9].parse::<usize>()?];
        let data: Vec<u8> = (0..c[10].len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&c[10][i..i + 2], 16))
            .collect::<Result<_, _>>()?;
        let decisions = std::array::from_fn(|i| c[11 + i] == "1");
        let before = decisions;
        let expected: Vec<i64> = c[32..]
            .iter()
            .map(|s| s.parse())
            .collect::<Result<_, _>>()?;
        assert_eq!(expected.len(), 31);
        let mut writer = RangeEncoder::new(data.len())?;
        let target = data.len() as u32 * 8 - remaining;
        prefix(&mut writer, position, target, pattern)?;
        assert_eq!(
            [
                i64::from(writer.range()),
                i64::from(writer.tell()),
                i64::from(writer.tell_frac())
            ],
            expected[..3]
        );
        let result =
            control::encode_tf(&mut writer, size, start..end, transient, &decisions, select)?;
        assert_eq!(decisions, before);
        assert_eq!(
            [
                i64::from(writer.range()),
                i64::from(writer.tell()),
                i64::from(writer.tell_frac())
            ],
            expected[3..6],
            "TF state line{line_index}"
        );
        for i in 0..21 {
            assert_eq!(
                i64::from(result.values()[i]),
                expected[6 + i],
                "mapping line{line_index}"
            );
        }
        let effective = control::encode_spread(&mut writer, requested)?;
        assert_eq!(effective, SPREADS[expected[27] as usize]);
        assert_eq!(
            [
                i64::from(writer.range()),
                i64::from(writer.tell()),
                i64::from(writer.tell_frac())
            ],
            expected[28..31]
        );
        assert_eq!(writer.finish()?, data, "bytes line{line_index}");
        let mut decoder = RangeDecoder::new(&data);
        decode_prefix(&mut decoder, position, target, pattern)?;
        assert_eq!(
            control::decode_tf(&mut decoder, size, start..end, transient)?,
            result
        );
        assert_eq!(control::decode_spread(&mut decoder)?, effective);
        assert_eq!(
            [decoder.range(), decoder.tell(), decoder.tell_frac()],
            [writer.range(), writer.tell(), writer.tell_frac()]
        );
        empty += usize::from(start == end);
        fallback += usize::from(effective != requested);
        rows += 1;
    }
    eprintln!("matched {rows} controls, empty{empty}, spread fallback{fallback}");
    assert_eq!((rows, empty, fallback), (34944, 8736, 15345));
    Ok(())
}
#[test]
fn invalid_control_domains_and_finished_fallback_preserve_writer()
-> Result<(), Box<dyn std::error::Error>> {
    let decisions = [true; 21];
    let mut writer = RangeEncoder::new(32)?;
    writer.encode_uint(13, 17)?;
    let before = writer.clone();
    for (start, end) in [(2, 1), (0, 22), (usize::MAX, usize::MAX)] {
        let bands = start..end;
        assert_eq!(
            control::encode_tf(&mut writer, FrameSize::Ms20, bands, false, &decisions, true),
            Err(ControlError::InvalidBands)
        );
        assert_eq!(writer, before);
    }
    assert_eq!(
        control::encode_tf(&mut writer, FrameSize::Ms2_5, 0..21, true, &decisions, true),
        Err(ControlError::InvalidContext)
    );
    assert_eq!(writer, before);
    let mut one = RangeEncoder::new(1)?;
    let before = one.clone();
    assert_eq!(
        control::encode_tf(&mut one, FrameSize::Ms20, 0..21, false, &decisions, true),
        Err(ControlError::InvalidPayload)
    );
    assert_eq!(
        control::encode_spread(&mut one, Spread::Light),
        Err(ControlError::InvalidPayload)
    );
    assert_eq!(one, before);
    let mut over = RangeEncoder::new(2)?;
    over.encode_bits(0, 16)?;
    let before = over.clone();
    assert_eq!(
        control::encode_tf(&mut over, FrameSize::Ms20, 0..21, false, &decisions, true),
        Err(ControlError::BitBudgetExceeded)
    );
    assert_eq!(
        control::encode_spread(&mut over, Spread::Light),
        Err(ControlError::BitBudgetExceeded)
    );
    assert_eq!(over, before);
    let mut tight = RangeEncoder::new(2)?;
    tight.encode_bits(0, 14)?;
    let before = tight.clone();
    assert_eq!(
        control::encode_spread(&mut tight, Spread::Light)?,
        Spread::Normal
    );
    assert_eq!(tight, before);
    tight.finish()?;
    let finished = tight.clone();
    assert_eq!(
        control::encode_tf(&mut tight, FrameSize::Ms20, 0..0, false, &decisions, true),
        Err(ControlError::Encoding(EncoderError::Finished))
    );
    assert_eq!(
        control::encode_spread(&mut tight, Spread::Light),
        Err(ControlError::Encoding(EncoderError::Finished))
    );
    assert_eq!(tight, finished);
    Ok(())
}
