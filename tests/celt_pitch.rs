//! Original complete PLC downsampling/whitening/search across signal recipes.
use opura::celt::pitch::{self, PitchError};
fn signal(period: usize, shape: usize, amplitude: i32, offset: usize, seed: u32) -> [i32; 2048] {
    signal_length::<2048>(period, shape, amplitude, offset, seed)
}
fn signal_length<const N: usize>(
    period: usize,
    shape: usize,
    amplitude: i32,
    offset: usize,
    mut seed: u32,
) -> [i32; N] {
    let mut values = [0; N];
    for (i, v) in values.iter_mut().enumerate() {
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        let phase = (i + offset) % period;
        *v = match shape {
            0 => {
                let triangle = phase.min(period - phase);
                (i64::from(amplitude) * (4 * triangle as i64 - period as i64) / period as i64)
                    as i32
            }
            1 => {
                if phase < period / 2 {
                    amplitude
                } else {
                    -amplitude
                }
            }
            2 => {
                if phase == 0 {
                    amplitude
                } else {
                    0
                }
            }
            3 => (i64::from(amplitude) * (2 * phase as i64 - period as i64) / period as i64) as i32,
            4 => ((i64::from(seed as i32) * i64::from(amplitude)) >> 31) as i32,
            5 => 0,
            6 => amplitude,
            7 => {
                if i % 2 != 0 {
                    amplitude
                } else {
                    -amplitude
                }
            }
            _ => panic!("fixture shape"),
        };
    }
    values
}
#[test]
fn all_whitened_samples_and_plc_periods_match_original() -> Result<(), Box<dyn std::error::Error>> {
    let mut count = 0;
    let mut seen = [[false; 2]; 621];
    for (line_no, line) in include_str!("celt-pitch-fixtures.txt").lines().enumerate() {
        if line.starts_with('#') {
            continue;
        }
        let r: Vec<_> = line.split_whitespace().collect();
        assert_eq!(r[0], "pitch");
        let period: usize = r[1].parse()?;
        let shape: usize = r[2].parse()?;
        let amp = r[3].parse()?;
        let channels: usize = r[4].parse()?;
        let side: usize = r[5].parse()?;
        let offset: usize = r[6].parse()?;
        let seed: u32 = r[7].parse()?;
        let left = signal(period, shape, amp, offset, seed);
        let right = match side {
            0 => left,
            1 => left.map(|v| -v),
            2 | 3 => signal(
                if side == 2 { period } else { period / 2 + 37 },
                shape,
                amp,
                offset + period / 3,
                seed ^ 0x9e3779b9,
            ),
            _ => panic!("side mode"),
        };
        let result = pitch::plc_pitch(&left, if channels == 2 { Some(&right) } else { None })
            .map_err(|e| {
                format!("line{line_no} P{period} shape{shape} amp{amp} C{channels}: {e}")
            })?;
        assert_eq!(result.period(), r[8].parse()?, "period line{line_no}");
        assert!((100..=720).contains(&result.period()));
        let expected: Vec<i16> = r[9..].iter().map(|s| s.parse()).collect::<Result<_, _>>()?;
        assert_eq!(
            result.whitened().as_slice(),
            expected,
            "whitened line{line_no} P{period} shape{shape} amp{amp} C{channels}"
        );
        seen[period - 100][channels - 1] = true;
        count += 1;
    }
    assert_eq!(count, 2202);
    assert!(seen.iter().flatten().all(|&v| v));
    Ok(())
}
#[test]
fn invalid_histories_and_silence_are_bounded() -> Result<(), Box<dyn std::error::Error>> {
    for n in [0, 1, 2047, 2049] {
        assert_eq!(
            pitch::plc_pitch(&vec![0; n], None),
            Err(PitchError::InvalidGeometry)
        );
        assert_eq!(
            pitch::plc_pitch(&[0; 2048], Some(&vec![0; n])),
            Err(PitchError::InvalidGeometry)
        );
    }
    for value in [i32::MIN, -536870912, 536870912, i32::MAX] {
        let mut data = [0; 2048];
        data[2047] = value;
        assert_eq!(
            pitch::plc_pitch(&data, None),
            Err(PitchError::InvalidSignal)
        );
        assert_eq!(
            pitch::plc_pitch(&[0; 2048], Some(&data)),
            Err(PitchError::InvalidSignal)
        );
    }
    for right in [None, Some(&[0i32; 2048][..])] {
        let result = pitch::plc_pitch(&[0; 2048], right)?;
        assert_eq!(result.period(), 720);
        assert_eq!(result.whitened(), &[0; 1024]);
    }
    Ok(())
}

#[test]
fn original_encoder_pitch_whitening_periods_and_gains_match()
-> Result<(), Box<dyn std::error::Error>> {
    use opura::celt::{FrameSize, pitch::PitchHistory};
    let sizes = [
        FrameSize::Ms2_5,
        FrameSize::Ms5,
        FrameSize::Ms10,
        FrameSize::Ms20,
    ];
    let mut count = 0;
    let mut overflows = 0;
    let mut refined = 0;
    let mut sequence_history = None;
    let mut sequences = 0;
    for line in include_str!("celt-encoder-pitch-fixtures.txt")
        .lines()
        .filter(|l| !l.starts_with('#'))
    {
        let f: Vec<_> = line.split_whitespace().collect();
        if f[0] == "sequence" {
            sequence_history = Some(PitchHistory::default());
            sequences += 1;
            continue;
        }
        assert_eq!(f[0], "pitch");
        assert_eq!(f.len(), 16);
        let v: Vec<i64> = f[1..15]
            .iter()
            .map(|s| s.parse())
            .collect::<Result<_, _>>()?;
        let size = sizes[usize::try_from(v[0])?];
        let length = 1024 + size.samples();
        let period = usize::try_from(v[1])?;
        let shape = usize::try_from(v[2])?;
        let amplitude = i32::try_from(v[3])?;
        let c = v[4];
        let side = v[5];
        let offset = usize::try_from(v[6])?;
        let seed = u32::try_from(v[7])?;
        let left = signal_length::<1984>(period, shape, amplitude, offset, seed);
        let right = match side {
            0 => left,
            1 => left.map(|x| -x),
            _ => signal_length::<1984>(
                if side == 2 { period } else { period / 2 + 37 },
                shape,
                amplitude,
                offset + period / 3,
                seed ^ 0x9e3779b9,
            ),
        };
        let previous = PitchHistory::new(u16::try_from(v[8])?, u16::try_from(v[9])?)?;
        if let Some(history) = sequence_history {
            assert_eq!(history, previous, "sequence history {line}");
        }
        let input = left;
        let side_input = right;
        let result = pitch::encoder_pitch(
            size,
            &left[..length],
            if c == 2 { Some(&right[..length]) } else { None },
            previous,
        );
        if v[13] != 0 {
            assert_eq!(result, Err(PitchError::ArithmeticOverflow));
            overflows += 1;
        } else {
            let result = result.map_err(|e| format!("{line}: {e}"))?;
            assert_eq!(
                [
                    i64::from(result.coarse_period()),
                    i64::from(result.gain()),
                    i64::from(result.period())
                ],
                v[10..13],
                "{line}"
            );
            assert_eq!(result.whitened().len(), length / 2);
            let mut hash = 14695981039346656037u64;
            for value in result.whitened() {
                for byte in value.to_le_bytes() {
                    hash = (hash ^ u64::from(byte)).wrapping_mul(1099511628211);
                }
            }
            assert_eq!(hash, f[15].parse::<u64>()?, "whitening {line}");
            assert!((15..=1023).contains(&result.period()));
            refined += usize::from(result.period() != result.coarse_period());
            if sequence_history.is_some() {
                sequence_history = Some(PitchHistory::new(
                    result.period().min(1022),
                    result.gain().clamp(0, 24576) as u16,
                )?);
            }
        }
        assert_eq!(left, input);
        assert_eq!(right, side_input);
        count += 1;
    }
    eprintln!("encoder contexts{count} overflows{overflows} refined{refined}");
    assert_eq!(count, 11972);
    assert_eq!(sequences, 8);
    assert!(refined > 100);
    Ok(())
}

#[test]
fn encoder_pitch_rejects_invalid_geometry_signal_and_histories()
-> Result<(), Box<dyn std::error::Error>> {
    use opura::celt::{FrameSize, pitch::PitchHistory};
    for period in [0, 1, 14, 15, 16, 1022, 1023, u16::MAX] {
        for gain in [0, 1, 3072, 24576, 24577, u16::MAX] {
            let result = PitchHistory::new(period, gain);
            if gain <= 24576 && (period == 0 && gain == 0 || (15..=1022).contains(&period)) {
                let history = result?;
                assert_eq!(history.period(), period);
                assert_eq!(history.gain(), gain);
            } else {
                assert_eq!(result, Err(PitchError::InvalidHistory));
            }
        }
    }
    assert_eq!(PitchHistory::default(), PitchHistory::new(0, 0)?);
    for size in [
        FrameSize::Ms2_5,
        FrameSize::Ms5,
        FrameSize::Ms10,
        FrameSize::Ms20,
    ] {
        let n = 1024 + size.samples();
        let zeros = vec![0; n];
        for length in [0, 1, 1024, n - 1, n + 1, 2048] {
            if length == n {
                continue;
            }
            let data = vec![0; length];
            assert_eq!(
                pitch::encoder_pitch(size, &data, None, PitchHistory::default()),
                Err(PitchError::InvalidEncoderGeometry)
            );
            assert_eq!(
                pitch::encoder_pitch(size, &zeros, Some(&data), PitchHistory::default()),
                Err(PitchError::InvalidEncoderGeometry)
            );
        }
        for invalid in [i32::MIN, -536870912, 536870912, i32::MAX] {
            for at in [0, 1023, 1024, n - 1] {
                let mut data = zeros.clone();
                data[at] = invalid;
                let before = data.clone();
                assert_eq!(
                    pitch::encoder_pitch(size, &data, None, PitchHistory::default()),
                    Err(PitchError::InvalidSignal)
                );
                assert_eq!(
                    pitch::encoder_pitch(size, &zeros, Some(&data), PitchHistory::default()),
                    Err(PitchError::InvalidSignal)
                );
                assert_eq!(data, before);
            }
        }
        let silence = pitch::encoder_pitch(size, &zeros, Some(&zeros), PitchHistory::default())?;
        assert_eq!(
            (silence.coarse_period(), silence.period(), silence.gain()),
            (1024, 1022, 0)
        );
        assert!(silence.whitened().iter().all(|&x| x == 0));
        for amplitude in [-536870911, 536870911] {
            let data = vec![amplitude; n];
            let result = pitch::encoder_pitch(size, &data, None, PitchHistory::default())?;
            assert!((15..=1023).contains(&result.period()));
        }
    }
    Ok(())
}
#[test]
fn bounded_encoder_pitch_contexts_preserve_inputs_and_histories()
-> Result<(), Box<dyn std::error::Error>> {
    use opura::celt::{FrameSize, pitch::PitchHistory};
    let sizes = [
        FrameSize::Ms2_5,
        FrameSize::Ms5,
        FrameSize::Ms10,
        FrameSize::Ms20,
    ];
    let mut random = 0x504954434852414eu64;
    for context in 0..1024 {
        let size = sizes[context % 4];
        let n = 1024 + size.samples();
        let mut next = || {
            random ^= random << 13;
            random ^= random >> 7;
            random ^= random << 17;
            random
        };
        let left: Vec<i32> = (0..n).map(|_| ((next() >> 32) as i32) >> 3).collect();
        let right: Vec<i32> = (0..n).map(|_| ((next() >> 32) as i32) >> 3).collect();
        let previous = PitchHistory::new(15 + (next() % 1008) as u16, (next() % 24577) as u16)?;
        let before = previous;
        let l = left.clone();
        let r = right.clone();
        match pitch::encoder_pitch(
            size,
            &left,
            if context % 2 == 0 { Some(&right) } else { None },
            previous,
        ) {
            Ok(result) => {
                assert!((15..=1023).contains(&result.period()));
                assert!(result.gain() >= -32767);
                assert_eq!(result.whitened().len(), n / 2);
            }
            Err(PitchError::ArithmeticOverflow) => {}
            Err(e) => return Err(format!("bounded context{context}: {e}").into()),
        }
        assert_eq!(left, l);
        assert_eq!(right, r);
        assert_eq!(previous, before);
    }
    Ok(())
}
