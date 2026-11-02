use super::*;
type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

fn rate(hz: u32) -> OutputRate {
    match hz {
        8000 => OutputRate::Khz8,
        12000 => OutputRate::Khz12,
        16000 => OutputRate::Khz16,
        24000 => OutputRate::Khz24,
        _ => OutputRate::Khz48,
    }
}
fn fingerprint(samples: &[i16]) -> u64 {
    samples
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .fold(14695981039346656037, |h, b| {
            (h ^ u64::from(b)).wrapping_mul(1099511628211)
        })
}
fn input(mut seed: u32, count: usize, pattern: u64) -> Vec<i16> {
    (0..count)
        .map(|i| {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            let sample = (seed & 65535) as i32 - 32768;
            match pattern {
                0 => 0,
                1 => {
                    if i % 2 == 0 {
                        i16::MIN
                    } else {
                        i16::MAX
                    }
                }
                2 => (sample >> 8) as i16,
                _ => sample as i16,
            }
        })
        .collect()
}

#[test]
fn unchanged_native_pcm_history_across_short_long_and_skipped_frames() -> Result {
    let mut state = PcmState::mono(OutputRate::Khz8);
    let mut cases = 0;
    let mut skipped = 0;
    for line in include_str!("../../../tests/opus-encoder-pcm-fixtures.txt").lines() {
        if line.starts_with('#') {
            continue;
        }
        // DC memories are signed; fingerprints deliberately use the full u64.
        let fields: Vec<_> = line.split_whitespace().collect();
        let v: Vec<u64> = fields[..7]
            .iter()
            .map(|s| s.parse())
            .collect::<std::result::Result<_, _>>()?;
        let r = rate(v[0] as u32);
        if v[2] == 0 {
            state = if v[1] == 1 {
                PcmState::mono(r)
            } else {
                PcmState::stereo(r)
            };
        }
        let count = v[3] as usize * r.hz() as usize / 48000 * v[1] as usize;
        let pcm = input(v[5] as u32, count, v[6]);
        let before = state.history().to_vec();
        let mut filtered = vec![0; count];
        let mut aligned = vec![0; count];
        state.filter(&pcm, &mut filtered)?;
        state.aligned(&filtered, &mut aligned)?;
        if v[4] != 0 {
            state.commit(&filtered)?;
        } else {
            assert_eq!(state.history(), before);
            skipped += 1;
        }
        assert_eq!(state.dc, [fields[7].parse()?, fields[8].parse()?], "{line}");
        assert_eq!(fingerprint(&filtered), fields[9].parse()?, "filter: {line}");
        assert_eq!(
            fingerprint(&aligned),
            fields[10].parse()?,
            "alignment: {line}"
        );
        assert_eq!(
            fingerprint(state.history()),
            fields[11].parse()?,
            "history: {line}"
        );
        cases += 1;
    }
    assert_eq!(cases, 1440);
    assert_eq!(skipped, 330);
    Ok(())
}

#[test]
fn invalid_pcm_calls_preserve_history_filters_and_output() -> Result {
    let mut state = PcmState::stereo(OutputRate::Khz48);
    let mut out = [0; 240];
    state.filter(&[1234; 240], &mut out)?;
    state.commit(&out)?;
    let before = state.clone();
    for pcm in [vec![], vec![0; 239], vec![0; 11522]] {
        let mut output = vec![123; pcm.len()];
        assert!(state.filter(&pcm, &mut output).is_err());
        assert!(state.aligned(&pcm, &mut output).is_err());
        assert!(state.commit(&pcm).is_err());
        assert_eq!(output, vec![123; pcm.len()]);
        assert_eq!(state, before);
    }
    let mut short = [456; 238];
    assert!(state.filter(&[0; 240], &mut short).is_err());
    assert!(state.aligned(&[0; 240], &mut short).is_err());
    assert_eq!(short, [456; 238]);
    assert_eq!(state, before);
    let mut long = [123; 242];
    state.filter(&[0; 240], &mut long)?;
    assert_eq!(&long[240..], &[123; 2]);
    state.aligned(&[0; 240], &mut long)?;
    assert_eq!(&long[240..], &[123; 2]);
    Ok(())
}
