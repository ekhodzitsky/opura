//! Original CELT prefix and coarse-energy-positioned TF/spread decisions.
use opura::{
    Packet,
    celt::{
        FrameSize,
        control::{self, ControlError},
        energy::{BandLayout, EnergyState},
        postfilter::Tapset,
        pvq::Spread,
    },
    range::RangeDecoder,
};
const SIZES: [FrameSize; 4] = [
    FrameSize::Ms2_5,
    FrameSize::Ms5,
    FrameSize::Ms10,
    FrameSize::Ms20,
];
fn bytes(s: &str) -> Result<Vec<u8>, std::num::ParseIntError> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16))
        .collect()
}
fn position(
    dec: &mut RangeDecoder<'_>,
    mode: usize,
    length: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    match mode {
        0 => (),
        1 => {
            for _ in 0..3 {
                dec.decode_bit_logp(3)?;
            }
        }
        2 => {
            dec.decode_bits(5)?;
        }
        3 => {
            dec.decode_uint(17)?;
            dec.decode_bits(3)?;
        }
        _ => {
            let target = length as u32 * 8
                - match mode {
                    4 => 0,
                    5 => 1,
                    _ => 3,
                };
            while dec.tell() < target {
                dec.decode_bits((target - dec.tell()).min(25))?;
            }
        }
    }
    Ok(())
}
fn cursor(dec: &RangeDecoder<'_>, values: &[&str]) -> Result<(), Box<dyn std::error::Error>> {
    assert_eq!(dec.range(), values[0].parse::<u32>()?);
    assert_eq!(dec.tell(), values[1].parse::<u32>()?);
    assert_eq!(dec.tell_frac(), values[2].parse::<u32>()?);
    Ok(())
}
#[test]
fn original_prefix_tf_and_spread_values_and_entropy_match() -> Result<(), Box<dyn std::error::Error>>
{
    let mut lines = include_str!("celt-control-fixtures.txt")
        .lines()
        .filter(|l| !l.starts_with('#'));
    let mut count = 0;
    let mut tf_only = 0;
    let mut silence = 0;
    let mut periods = [false; 2];
    let mut gains = [false; 2];
    let mut taps = [false; 3];
    let mut transient = [false; 2];
    let mut spread_seen = [false; 4];
    let mut geometry = [[[false; 7]; 2]; 4];
    while let Some(line) = lines.next() {
        let row: Vec<_> = line.split_whitespace().collect();
        if row[0] == "tf_only" {
            let size = SIZES[row[1].parse::<usize>()?];
            let start = row[2].parse()?;
            let end = row[3].parse()?;
            let transient = row[4] == "1";
            let remaining = row[5].parse::<u32>()?;
            let data = bytes(row[6])?;
            let mut entropy = RangeDecoder::new(&data);
            let target = data.len() as u32 * 8 - remaining;
            while entropy.tell() < target {
                entropy.decode_bits((target - entropy.tell()).min(25))?;
            }
            let tf = control::decode_tf(&mut entropy, size, start..end, transient)?;
            cursor(&entropy, &row[7..10])?;
            let expected: Vec<_> = row[10..]
                .iter()
                .map(|s| s.parse::<i8>())
                .collect::<Result<_, _>>()?;
            assert_eq!(tf.values(), expected.as_slice(), "standalone TF {tf_only}");
            tf_only += 1;
            continue;
        }
        assert_eq!(row[0], "frame");
        let lm = row[1].parse::<usize>()?;
        let size = SIZES[lm];
        let start = row[2].parse::<usize>()?;
        let end = row[3].parse::<usize>()?;
        let channels = row[4].parse::<usize>()?;
        let mode = row[5].parse::<usize>()?;
        let data = bytes(row[6])?;
        let mut entropy = RangeDecoder::new(&data);
        position(&mut entropy, mode, data.len())?;
        let prefix = control::decode_prefix(&mut entropy, size, start)?;
        let expected: Vec<_> = lines
            .next()
            .ok_or("prefix row")?
            .split_whitespace()
            .collect();
        assert_eq!(expected[0], "prefix");
        assert_eq!(prefix.silence(), expected[1] == "1");
        assert_eq!(prefix.postfilter().period(), expected[2].parse::<u16>()?);
        assert_eq!(prefix.postfilter().gain(), expected[3].parse::<u16>()?);
        let tap = match prefix.postfilter().tapset() {
            Tapset::Set0 => 0,
            Tapset::Set1 => 1,
            Tapset::Set2 => 2,
        };
        assert_eq!(tap, expected[4].parse::<usize>()?);
        assert_eq!(prefix.transient(), expected[5] == "1");
        assert_eq!(prefix.short_blocks(), expected[6].parse::<usize>()?);
        assert_eq!(prefix.intra(), expected[7] == "1");
        assert_eq!(prefix.size(), size);
        cursor(&entropy, &expected[8..])?;
        if mode == 0 && start == 0 {
            let mut packet = vec![0x80 + 8 * lm as u8 + if channels == 2 { 4 } else { 0 }];
            packet.extend_from_slice(&data);
            let parsed = Packet::parse(&packet)?;
            let (wrapped, wrapped_cursor) = control::decode_header(&parsed, 0)?;
            assert_eq!(wrapped, prefix);
            assert_eq!(wrapped_cursor, entropy);
        }
        let mut energies = EnergyState::default();
        energies.coarse(
            &mut entropy,
            BandLayout::new(channels, start, end)?,
            size,
            prefix.intra(),
        )?;
        let expected: Vec<_> = lines
            .next()
            .ok_or("coarse row")?
            .split_whitespace()
            .collect();
        assert_eq!(expected[0], "coarse");
        cursor(&entropy, &expected[1..4])?;
        let values: Vec<_> = expected[4..]
            .iter()
            .map(|s| s.parse::<i32>())
            .collect::<Result<_, _>>()?;
        for c in 0..2 {
            assert_eq!(
                energies.channel(c).ok_or("channel")?,
                &values[21 * c..21 * (c + 1)],
                "energy case{count}"
            );
        }
        let tf = control::decode_tf(&mut entropy, size, start..end, prefix.transient())?;
        let expected: Vec<_> = lines.next().ok_or("tf row")?.split_whitespace().collect();
        assert_eq!(expected[0], "tf");
        cursor(&entropy, &expected[1..4])?;
        let values: Vec<_> = expected[4..]
            .iter()
            .map(|s| s.parse::<i8>())
            .collect::<Result<_, _>>()?;
        assert_eq!(tf.values(), values.as_slice(), "TF case{count}");
        let spread = control::decode_spread(&mut entropy)?;
        let spread = match spread {
            Spread::None => 0,
            Spread::Light => 1,
            Spread::Normal => 2,
            Spread::Aggressive => 3,
        };
        let expected: Vec<_> = lines
            .next()
            .ok_or("spread row")?
            .split_whitespace()
            .collect();
        assert_eq!(expected[0], "spread");
        assert_eq!(spread, expected[1].parse::<usize>()?);
        cursor(&entropy, &expected[2..])?;
        spread_seen[spread] = true;
        taps[tap] = true;
        transient[usize::from(prefix.transient())] = true;
        if prefix.silence() {
            silence += 1;
            assert_eq!(expected[3].parse::<usize>()?, data.len() * 8);
        }
        if prefix.postfilter().period() == 15 {
            periods[0] = true;
        }
        if prefix.postfilter().period() == 1022 {
            periods[1] = true;
        }
        if prefix.postfilter().gain() == 3072 {
            gains[0] = true;
        }
        if prefix.postfilter().gain() == 24576 {
            gains[1] = true;
        }
        geometry[lm][usize::from(start == 17)][mode] = true;
        count += 1;
    }
    assert_eq!(count, 3360);
    assert_eq!(tf_only, 1824);
    assert!(silence > 100);
    assert_eq!(periods, [true; 2]);
    assert_eq!(gains, [true; 2]);
    assert_eq!(taps, [true; 3]);
    assert_eq!(transient, [true; 2]);
    assert_eq!(spread_seen, [true; 4]);
    assert_eq!(geometry, [[[true; 7]; 2]; 4]);
    Ok(())
}
#[test]
fn invalid_payload_context_and_packet_wrapper_preserve_cursor()
-> Result<(), Box<dyn std::error::Error>> {
    let mut entropy = RangeDecoder::new(&[0; 8]);
    let before = entropy.clone();
    assert_eq!(
        control::decode_prefix(&mut entropy, FrameSize::Ms20, 1),
        Err(ControlError::InvalidStart)
    );
    assert_eq!(entropy, before);
    for (start, end) in [(10, 9), (0, 22), (usize::MAX, usize::MAX)] {
        assert_eq!(
            control::decode_tf(&mut entropy, FrameSize::Ms20, start..end, true),
            Err(ControlError::InvalidBands)
        );
        assert_eq!(entropy, before);
    }
    for len in [0, 1, 1276] {
        let data = vec![0; len];
        let mut cursor = RangeDecoder::new(&data);
        let before = cursor.clone();
        assert_eq!(
            control::decode_prefix(&mut cursor, FrameSize::Ms20, 0),
            Err(ControlError::InvalidPayload)
        );
        assert_eq!(cursor, before);
    }
    let mut over = RangeDecoder::new(&[0; 2]);
    for _ in 0..20 {
        over.decode_bit_logp(1)?;
    }
    let before = over.clone();
    assert_eq!(
        control::decode_prefix(&mut over, FrameSize::Ms20, 0),
        Err(ControlError::BitBudgetExceeded)
    );
    assert_eq!(over, before);
    for toc in [0, 0x60] {
        let bytes = [toc, 0, 0];
        assert_eq!(
            control::decode_header(&Packet::parse(&bytes)?, 0),
            Err(ControlError::UnsupportedMode)
        );
    }
    let parsed = Packet::parse(&[0x98, 0, 0])?;
    assert_eq!(
        control::decode_header(&parsed, 1),
        Err(ControlError::MissingFrame)
    );
    let mut empty = RangeDecoder::new(&[]);
    let before = empty.clone();
    assert_eq!(control::decode_spread(&mut empty)?, Spread::Normal);
    assert_eq!(empty, before);
    Ok(())
}

#[test]
fn every_two_byte_prefix_is_bounded_at_all_sizes_and_starts()
-> Result<(), Box<dyn std::error::Error>> {
    for value in 0..=u16::MAX {
        let data = value.to_le_bytes();
        for size in SIZES {
            for start in [0, 17] {
                let mut entropy = RangeDecoder::new(&data);
                let prefix = control::decode_prefix(&mut entropy, size, start)?;
                assert!(entropy.tell() <= 16);
                assert_eq!(prefix.postfilter().gain(), 0);
                if size == FrameSize::Ms2_5 {
                    assert!(!prefix.transient());
                }
                if prefix.silence() {
                    assert_eq!(entropy.tell(), 16);
                }
            }
        }
    }
    Ok(())
}
