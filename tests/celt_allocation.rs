//! Original dynamic/rate allocation and following fine-energy continuation.
use opura::{
    celt::{
        FrameSize,
        allocation::{self, AllocationError},
        control,
        energy::{BandLayout, EnergyState},
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
    entropy: &mut RangeDecoder<'_>,
    mode: usize,
    length: usize,
    start: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    if mode == 1 || mode == 7 && start == 17 {
        for _ in 0..3 {
            entropy.decode_bit_logp(3)?;
        }
    }
    if mode == 2 {
        entropy.decode_bits(5)?;
    }
    if mode == 3 {
        entropy.decode_uint(17)?;
        entropy.decode_bits(3)?;
    }
    if (4..=6).contains(&mode) {
        let target = length as u32 * 8
            - match mode {
                4 => 0,
                5 => 1,
                _ => 3,
            };
        while entropy.tell() < target {
            entropy.decode_bits((target - entropy.tell()).min(25))?;
        }
    }
    Ok(())
}
fn cursor(entropy: &RangeDecoder<'_>, row: &[&str]) -> Result<(), Box<dyn std::error::Error>> {
    assert_eq!(entropy.range(), row[0].parse::<u32>()?);
    assert_eq!(entropy.tell(), row[1].parse::<u32>()?);
    assert_eq!(entropy.tell_frac(), row[2].parse::<u32>()?);
    Ok(())
}
#[test]
fn every_allocation_field_and_fine_continuation_matches_original()
-> Result<(), Box<dyn std::error::Error>> {
    let mut lines = include_str!("celt-allocation-fixtures.txt")
        .lines()
        .filter(|l| !l.starts_with('#'));
    let mut count = 0;
    let mut layouts = [[[[false; 22]; 21]; 2]; 4];
    let mut geometry = [[[false; 2]; 2]; 4];
    let mut modes = [false; 8];
    let mut trim_seen = [false; 11];
    let mut anti = [false; 2];
    let mut dual = [false; 2];
    let mut boosted = false;
    let mut skipped = false;
    let mut balanced = false;
    while let Some(line) = lines.next() {
        let row: Vec<_> = line.split_whitespace().collect();
        assert_eq!(row[0], "frame");
        let lm = row[1].parse::<usize>()?;
        let size = SIZES[lm];
        let start = row[2].parse::<usize>()?;
        let end = row[3].parse::<usize>()?;
        let channels = row[4].parse::<usize>()?;
        let mut transient = row[5] == "1";
        let mode = row[6].parse::<usize>()?;
        let layout = BandLayout::new(channels, start, end)?;
        let data = bytes(row[7])?;
        let mut entropy = RangeDecoder::new(&data);
        let mut energy = EnergyState::default();
        position(&mut entropy, mode, data.len(), start)?;
        if mode == 7 {
            let prefix = control::decode_prefix(&mut entropy, size, start)?;
            transient = prefix.transient();
            energy.coarse(&mut entropy, layout, size, prefix.intra())?;
            control::decode_tf(&mut entropy, size, start..end, transient)?;
            control::decode_spread(&mut entropy)?;
        }
        let result =
            allocation::decode_allocation(&mut entropy, size, layout, transient).map_err(|e| {
                format!("case {count} LM{lm} C{channels} bands{start}..{end} mode{mode}: {e}")
            })?;
        let expected: Vec<_> = lines
            .next()
            .ok_or("allocation row")?
            .split_whitespace()
            .collect();
        assert_eq!(expected[0], "allocation");
        assert_eq!(result.transient(), expected[1] == "1");
        assert_eq!(result.trim(), expected[2].parse::<u8>()?);
        assert_eq!(result.anti_collapse_reserved(), expected[3] == "8");
        assert_eq!(
            result.coded_bands(),
            expected[4].parse::<usize>()?,
            "coded case{count}"
        );
        assert_eq!(result.intensity(), expected[5].parse::<usize>()?);
        assert_eq!(result.dual_stereo(), expected[6] == "1");
        assert_eq!(result.balance(), expected[7].parse::<i32>()?);
        cursor(&entropy, &expected[8..11])?;
        let values: Vec<_> = expected[11..]
            .iter()
            .map(|s| s.parse::<i32>())
            .collect::<Result<_, _>>()?;
        assert_eq!(values.len(), 105);
        assert_eq!(result.caps(), &values[..21], "caps case{count}");
        assert_eq!(result.offsets(), &values[21..42], "offsets case{count}");
        assert_eq!(
            result.pvq_bits(),
            &values[42..63],
            "PVQ budgets case{count}"
        );
        for i in 0..21 {
            assert_eq!(
                i32::from(result.fine_bits()[i]),
                values[63 + i],
                "fine case{count} band{i}"
            );
            assert_eq!(
                i32::from(result.fine_priority()[i]),
                values[84 + i],
                "priority case{count} band{i}"
            );
        }
        assert_eq!(result.layout(), layout);
        assert_eq!(result.size(), size);
        assert!(result.coded_bands() > start && result.coded_bands() <= end);
        assert!(result.pvq_bits().iter().all(|&v| v >= 0));
        assert!(result.fine_bits().iter().all(|&v| v <= 8));
        assert!(result.balance() >= 0);
        let allocated = result.pvq_bits().iter().map(|&v| i64::from(v)).sum::<i64>()
            + result
                .fine_bits()
                .iter()
                .map(|&v| i64::from(v) * channels as i64 * 8)
                .sum::<i64>()
            + i64::from(result.balance())
            + if result.anti_collapse_reserved() {
                8
            } else {
                0
            };
        assert!(
            allocated <= data.len() as i64 * 64 - i64::from(entropy.tell_frac()),
            "budget conservation case{count}"
        );
        energy.fine(&mut entropy, layout, result.fine_bits())?;
        let expected: Vec<_> = lines.next().ok_or("fine row")?.split_whitespace().collect();
        assert_eq!(expected[0], "fine");
        cursor(&entropy, &expected[1..4])?;
        let values: Vec<_> = expected[4..]
            .iter()
            .map(|s| s.parse::<i32>())
            .collect::<Result<_, _>>()?;
        for c in 0..2 {
            assert_eq!(
                energy.channel(c).ok_or("channel")?,
                &values[c * 21..(c + 1) * 21],
                "fine energies case{count}"
            );
        }
        assert!(entropy.tell() <= data.len() as u32 * 8);
        geometry[lm][channels - 1][usize::from(transient)] = true;
        modes[mode] = true;
        trim_seen[result.trim() as usize] = true;
        anti[usize::from(result.anti_collapse_reserved())] = true;
        dual[usize::from(result.dual_stereo())] = true;
        boosted |= result.offsets().iter().any(|&v| v > 0);
        skipped |= result.coded_bands() < end;
        balanced |= result.balance() > 0;
        layouts[lm][channels - 1][start][end] = true;
        count += 1;
    }
    assert_eq!(count, 16296);
    for channels in layouts {
        for intervals in channels {
            for (start, ends) in intervals.iter().enumerate() {
                for (end, &seen) in ends.iter().enumerate() {
                    assert_eq!(seen, end > start);
                }
            }
        }
    }
    assert_eq!(geometry, [[[true; 2]; 2]; 4]);
    assert_eq!(modes, [true; 8]);
    assert_eq!(trim_seen, [true; 11]);
    assert_eq!(anti, [true; 2]);
    assert_eq!(dual, [true; 2]);
    assert!(boosted && skipped && balanced);
    Ok(())
}
#[test]
fn invalid_contexts_preserve_cursor_and_normal_continuation()
-> Result<(), Box<dyn std::error::Error>> {
    let layout = BandLayout::new(2, 0, 21)?;
    let mut entropy = RangeDecoder::new(&[0x5a; 40]);
    entropy.decode_bits(3)?;
    let before = entropy.clone();
    assert_eq!(
        allocation::decode_allocation(
            &mut entropy,
            FrameSize::Ms20,
            BandLayout::new(2, 21, 21)?,
            false
        ),
        Err(AllocationError::EmptyBands)
    );
    assert_eq!(entropy, before);
    for len in [0, 1, 1276] {
        let data = vec![0; len];
        let mut cursor = RangeDecoder::new(&data);
        let before = cursor.clone();
        assert_eq!(
            allocation::decode_allocation(&mut cursor, FrameSize::Ms20, layout, true),
            Err(AllocationError::InvalidPayload)
        );
        assert_eq!(cursor, before);
    }
    let mut over = RangeDecoder::new(&[0; 2]);
    for _ in 0..20 {
        over.decode_bit_logp(1)?;
    }
    let before = over.clone();
    assert_eq!(
        allocation::decode_allocation(&mut over, FrameSize::Ms20, layout, true),
        Err(AllocationError::BitBudgetExceeded)
    );
    assert_eq!(over, before);
    let mut control = entropy.clone();
    assert_eq!(
        allocation::decode_allocation(&mut entropy, FrameSize::Ms20, layout, true)?,
        allocation::decode_allocation(&mut control, FrameSize::Ms20, layout, true)?
    );
    assert_eq!(entropy, control);
    Ok(())
}

#[test]
fn bounded_adversarial_intervals_preserve_budget_and_fine_contracts()
-> Result<(), Box<dyn std::error::Error>> {
    let mut random = 0x4249_5453u32;
    for index in 0..512 {
        let mut data = vec![0u8; 2 + (index * 73) % 1274];
        for byte in &mut data {
            random ^= random << 13;
            random ^= random >> 17;
            random ^= random << 5;
            *byte = random as u8;
        }
        let start = random as usize % 21;
        let end = start + 1 + (random as usize / 21) % (21 - start);
        let channels = 1 + (index / 4) % 2;
        let layout = BandLayout::new(channels, start, end)?;
        let mut entropy = RangeDecoder::new(&data);
        let skip = random % ((data.len() as u32 * 8 - 1).min(64));
        let mut remaining = skip;
        while remaining > 0 {
            let count = remaining.min(25);
            entropy.decode_bits(count)?;
            remaining -= count;
        }
        let result =
            allocation::decode_allocation(&mut entropy, SIZES[index % 4], layout, index % 2 == 0)?;
        assert!(result.coded_bands() > start && result.coded_bands() <= end);
        assert!(result.pvq_bits().iter().all(|&v| v >= 0));
        assert!(result.fine_bits().iter().all(|&v| v <= 8));
        let allocated = result.pvq_bits().iter().map(|&v| i64::from(v)).sum::<i64>()
            + result
                .fine_bits()
                .iter()
                .map(|&v| i64::from(v) * channels as i64 * 8)
                .sum::<i64>()
            + i64::from(result.balance())
            + if result.anti_collapse_reserved() {
                8
            } else {
                0
            };
        assert!(allocated <= data.len() as i64 * 64 - i64::from(entropy.tell_frac()));
        EnergyState::default().fine(&mut entropy, layout, result.fine_bits())?;
        assert!(entropy.tell() <= data.len() as u32 * 8);
    }
    Ok(())
}
