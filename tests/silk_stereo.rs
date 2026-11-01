//! Stereo entropy coverage and public reconstruction transaction boundaries.
use opura::{
    range::RangeDecoder,
    silk::{
        pitch::SilkRate,
        stereo::{StereoError, StereoPrediction, StereoState, decode_mid_only},
    },
};
use std::collections::BTreeSet;

fn bytes(hex: &str) -> Result<Vec<u8>, std::num::ParseIntError> {
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16))
        .collect()
}

#[test]
fn every_quantizer_pair_and_mid_flag_match_original_entropy_decoder()
-> Result<(), Box<dyn std::error::Error>> {
    let mut count = 0;
    let mut pairs = BTreeSet::new();
    let mut first = BTreeSet::new();
    let mut second = BTreeSet::new();
    let mut flags = [false; 2];
    for line in include_str!("silk-stereo-entropy-fixtures.txt")
        .lines()
        .filter(|l| !l.starts_with('#'))
    {
        let row: Vec<_> = line.split_whitespace().collect();
        let data = bytes(row[0])?;
        let values: Vec<i64> = row[1..]
            .iter()
            .map(|v| v.parse())
            .collect::<Result<_, _>>()?;
        let mut entropy = RangeDecoder::new(&data);
        let prediction = StereoPrediction::decode(&mut entropy)?;
        let p = prediction.coefficients_q13();
        assert_eq!(*p, [values[0] as i32, values[1] as i32]);
        assert_eq!(
            (entropy.range(), entropy.tell(), entropy.tell_frac()),
            (values[2] as u32, values[3] as u32, values[4] as u32)
        );
        let mid = decode_mid_only(&mut entropy)?;
        assert_eq!(mid, values[5] != 0);
        assert_eq!(
            (entropy.range(), entropy.tell(), entropy.tell_frac()),
            (values[6] as u32, values[7] as u32, values[8] as u32)
        );
        pairs.insert(*p);
        first.insert(p[0] + p[1]);
        second.insert(p[1]);
        flags[usize::from(mid)] = true;
        count += 1;
    }
    assert_eq!(count, 5625);
    assert_eq!(pairs.len(), 5625);
    assert_eq!((first.len(), second.len()), (75, 75));
    assert_eq!(flags, [true; 2]);
    Ok(())
}

#[test]
fn short_prefixes_are_bounded_and_failed_entropy_is_unchanged()
-> Result<(), Box<dyn std::error::Error>> {
    let mut failures = 0;
    for value in 0u32..=65535 {
        let data = (value as u16).to_le_bytes();
        let mut entropy = RangeDecoder::new(&data);
        let before = entropy.clone();
        match StereoPrediction::decode(&mut entropy) {
            Ok(_) => assert!(entropy.tell() <= 16),
            Err(error) => {
                assert_eq!(error, StereoError::BitBudgetExceeded);
                assert_eq!(entropy, before);
                failures += 1;
            }
        }
    }
    assert!(failures > 0);
    let mut empty = RangeDecoder::new(&[]);
    let before = empty.clone();
    assert_eq!(
        decode_mid_only(&mut empty),
        Err(StereoError::BitBudgetExceeded)
    );
    assert_eq!(empty, before);
    Ok(())
}

#[test]
fn invalid_channels_preserve_history_and_reset_repeats_delayed_output()
-> Result<(), Box<dyn std::error::Error>> {
    let data = [255; 16];
    let prediction = StereoPrediction::decode(&mut RangeDecoder::new(&data))?;
    let mut state = StereoState::default();
    let original = state.reconstruct(SilkRate::Khz8, &[1000; 80], &[500; 80], prediction)?;
    let before = state.clone();
    for (rate, mid, side) in [
        (SilkRate::Khz8, 0, 0),
        (SilkRate::Khz8, 80, 79),
        (SilkRate::Khz12, 80, 80),
        (SilkRate::Khz16, 321, 321),
    ] {
        assert_eq!(
            state.reconstruct(rate, &vec![0; mid], &vec![0; side], prediction),
            Err(StereoError::IncompatibleFrame)
        );
        assert_eq!(state, before);
    }
    state.reset();
    assert_eq!(state, StereoState::default());
    assert_eq!(
        state.reconstruct(SilkRate::Khz8, &[1000; 80], &[500; 80], prediction)?,
        original
    );
    assert_eq!(state.previous_prediction(), prediction);
    assert_eq!(state, before);
    Ok(())
}
