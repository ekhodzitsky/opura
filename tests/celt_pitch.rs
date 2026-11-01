//! Original complete PLC downsampling/whitening/search across signal recipes.
use opura::celt::pitch::{self, PitchError};
fn signal(
    period: usize,
    shape: usize,
    amplitude: i32,
    offset: usize,
    mut seed: u32,
) -> [i32; 2048] {
    let mut values = [0; 2048];
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
