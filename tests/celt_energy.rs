//! Original CELT energy stages and Laplace symbols, retaining complete histories.
use opura::{
    celt::{
        FrameSize,
        energy::{BandLayout, EnergyError, EnergyState},
    },
    range::{RangeDecoder, RangeError},
};
fn bytes(s: &str) -> Result<Vec<u8>, std::num::ParseIntError> {
    if s == "-" {
        return Ok(Vec::new());
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16))
        .collect()
}
fn observe(
    line: &str,
    kind: &str,
    state: &EnergyState,
    entropy: &RangeDecoder<'_>,
) -> Result<(), Box<dyn std::error::Error>> {
    let row: Vec<_> = line.split_whitespace().collect();
    assert_eq!(row[0], kind);
    assert_eq!(entropy.range(), row[1].parse::<u32>()?, "range {kind}");
    assert_eq!(entropy.tell(), row[2].parse::<u32>()?, "tell {kind}");
    assert_eq!(entropy.tell_frac(), row[3].parse::<u32>()?, "frac {kind}");
    let expected: Vec<_> = row[4..]
        .iter()
        .map(|s| s.parse::<i32>())
        .collect::<Result<_, _>>()?;
    assert_eq!(expected.len(), 42);
    for channel in 0..2 {
        assert_eq!(
            state.channel(channel).ok_or("missing channel")?,
            &expected[channel * 21..(channel + 1) * 21],
            "energies {kind} channel {channel}"
        );
    }
    Ok(())
}
#[test]
fn reference_stages_and_laplace_entropy_match() -> Result<(), Box<dyn std::error::Error>> {
    let mut lines = include_str!("celt-energy-fixtures.txt")
        .lines()
        .filter(|l| !l.starts_with('#'));
    let mut state = EnergyState::default();
    let mut frames = 0;
    let mut laplace = 0;
    let mut contexts = [[[false; 2]; 2]; 4];
    while let Some(line) = lines.next() {
        let row: Vec<_> = line.split_whitespace().collect();
        if row[0] == "laplace" {
            let data = bytes(row[1])?;
            let mut entropy = RangeDecoder::new(&data);
            for i in 0..16 {
                let at = 4 + 4 * i;
                assert_eq!(
                    entropy.decode_laplace(row[2].parse()?, row[3].parse()?)?,
                    row[at].parse::<i32>()?
                );
                assert_eq!(entropy.range(), row[at + 1].parse::<u32>()?);
                assert_eq!(entropy.tell(), row[at + 2].parse::<u32>()?);
                assert_eq!(entropy.tell_frac(), row[at + 3].parse::<u32>()?);
                laplace += 1;
            }
            continue;
        }
        assert_eq!(row[0], "frame");
        if row[1] == "1" {
            state.reset();
        }
        let lm = row[2].parse::<usize>()?;
        let channels = row[3].parse()?;
        let layout = BandLayout::new(channels, row[4].parse()?, row[5].parse()?)?;
        let intra = row[6] == "1";
        contexts[lm][channels - 1][usize::from(intra)] = true;
        let data = bytes(row[7])?;
        let mut entropy = RangeDecoder::new(&data);
        let bits: [u8; 21] = bytes(row[8])?.try_into().map_err(|_| "bits shape")?;
        let priority: [bool; 21] = row[9]
            .bytes()
            .map(|b| b == b'1')
            .collect::<Vec<_>>()
            .try_into()
            .map_err(|_| "priority shape")?;
        state.coarse(
            &mut entropy,
            layout,
            [
                FrameSize::Ms2_5,
                FrameSize::Ms5,
                FrameSize::Ms10,
                FrameSize::Ms20,
            ][lm],
            intra,
        )?;
        observe(
            lines.next().ok_or("coarse row")?,
            "coarse",
            &state,
            &entropy,
        )
        .map_err(|e| format!("frame {frames}: {e}"))?;
        state.fine(&mut entropy, layout, &bits)?;
        observe(lines.next().ok_or("fine row")?, "fine", &state, &entropy)
            .map_err(|e| format!("frame {frames}: {e}"))?;
        state.finalise(&mut entropy, layout, &bits, &priority, row[10].parse()?)?;
        observe(lines.next().ok_or("final row")?, "final", &state, &entropy)
            .map_err(|e| format!("frame {frames}: {e}"))?;
        frames += 1;
    }
    assert_eq!(frames, 768);
    assert_eq!(laplace, 2688);
    assert_eq!(contexts, [[[true; 2]; 2]; 4]);
    Ok(())
}

#[test]
fn invalid_models_layouts_refinement_and_budget_leave_state_unchanged()
-> Result<(), Box<dyn std::error::Error>> {
    for (channels, start, end) in [
        (0, 0, 21),
        (3, 0, 21),
        (2, 22, 22),
        (2, 10, 9),
        (usize::MAX, 0, 21),
    ] {
        assert_eq!(
            BandLayout::new(channels, start, end),
            Err(EnergyError::InvalidLayout)
        );
    }
    let mut entropy = RangeDecoder::new(&[0x55; 64]);
    for (fs, decay) in [
        (0, 0),
        (32737, 100),
        (32768, 11456),
        (100, 11457),
        (u16::MAX, u16::MAX),
    ] {
        let before = entropy.clone();
        assert_eq!(
            entropy.decode_laplace(fs, decay),
            Err(RangeError::InvalidDistribution)
        );
        assert_eq!(entropy, before);
    }
    let layout = BandLayout::new(2, 0, 21)?;
    let mut state = EnergyState::default();
    state.coarse(&mut entropy, layout, FrameSize::Ms20, false)?;
    let before = state.clone();
    let cursor = entropy.clone();
    assert_eq!(
        state.fine(&mut entropy, layout, &[9; 21]),
        Err(EnergyError::InvalidFineBits)
    );
    assert_eq!(
        state.finalise(&mut entropy, layout, &[0; 21], &[false; 21], u32::MAX),
        Err(EnergyError::InvalidBudget)
    );
    assert_eq!(state, before);
    assert_eq!(entropy, cursor);
    let mut control = before;
    let mut control_cursor = cursor;
    state.fine(&mut entropy, layout, &[2; 21])?;
    control.fine(&mut control_cursor, layout, &[2; 21])?;
    assert_eq!(state, control);
    assert_eq!(entropy, control_cursor);
    state.reset();
    assert_eq!(state, EnergyState::default());
    assert_eq!(state.channel(2), None);
    Ok(())
}

#[test]
fn every_two_byte_prefix_has_bounded_laplace_output_and_normalized_range()
-> Result<(), Box<dyn std::error::Error>> {
    for value in 0..=u16::MAX {
        let data = value.to_le_bytes();
        for (zero, decay) in [(1, 11456), (9216, 8192), (32736, 0)] {
            let mut entropy = RangeDecoder::new(&data);
            let symbol = entropy.decode_laplace(zero, decay)?;
            assert!(symbol.abs() <= 16384);
            assert!(entropy.range() > 1 << 23);
        }
    }
    Ok(())
}
