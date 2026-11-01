//! Exact original FFT output and independent transform-order/scaling checks.
use opura::celt::{
    FrameSize,
    fft::{Complex, FftError, FftPlan},
};
const SIZES: [FrameSize; 4] = [
    FrameSize::Ms2_5,
    FrameSize::Ms5,
    FrameSize::Ms10,
    FrameSize::Ms20,
];
#[test]
fn every_complex_output_matches_original_plans_and_downshifts()
-> Result<(), Box<dyn std::error::Error>> {
    let mut count = 0;
    let mut shifts = [[false; 32]; 4];
    let mut wrappers = [[false; 2]; 4];
    for line in include_str!("celt-fft-fixtures.txt")
        .lines()
        .filter(|l| !l.starts_with('#'))
    {
        let row: Vec<_> = line.split_whitespace().collect();
        assert_eq!(row[0], "case");
        let lm = row[1].parse::<usize>()?;
        let kind = row[2].parse::<u8>()?;
        let shift = row[3].parse::<u8>()?;
        let plan = FftPlan::new(SIZES[lm]);
        let n = plan.points();
        let values: Vec<_> = row[4..]
            .iter()
            .map(|s| s.parse::<i32>())
            .collect::<Result<_, _>>()?;
        assert_eq!(values.len(), 4 * n);
        let input: Vec<_> = values[..2 * n]
            .chunks_exact(2)
            .map(|p| Complex {
                real: p[0],
                imag: p[1],
            })
            .collect();
        let sentinel = Complex {
            real: 12345,
            imag: -6789,
        };
        let mut output = vec![sentinel; n + 3];
        match kind {
            0 => {
                plan.forward(&input, &mut output, shift)?;
                shifts[lm][usize::from(shift)] = true;
            }
            1 => {
                plan.forward_scaled(&input, &mut output)?;
                wrappers[lm][0] = true;
            }
            2 => {
                plan.inverse(&input, &mut output)?;
                wrappers[lm][1] = true;
            }
            _ => panic!("kind"),
        }
        for (i, pair) in values[2 * n..].chunks_exact(2).enumerate() {
            assert_eq!(
                output[i],
                Complex {
                    real: pair[0],
                    imag: pair[1]
                },
                "case {count} LM{lm} kind{kind} shift{shift} at{i}"
            );
        }
        assert_eq!(&output[n..], &[sentinel; 3]);
        count += 1;
    }
    assert_eq!(count, 1632);
    assert_eq!(shifts, [[true; 32]; 4]);
    assert_eq!(wrappers, [[true; 2]; 4]);
    Ok(())
}
#[test]
fn natural_order_sign_and_scale_agree_with_direct_dft() -> Result<(), Box<dyn std::error::Error>> {
    for size in SIZES {
        let plan = FftPlan::new(size);
        let n = plan.points();
        let input: Vec<_> = (0..n)
            .map(|i| Complex {
                real: ((i % 17) as i32 - 8) * 101,
                imag: ((i % 13) as i32 - 6) * 31,
            })
            .collect();
        let mut output = vec![Complex::default(); n];
        for kind in [0, 2] {
            match kind {
                0 => plan.forward(&input, &mut output, 0)?,
                1 => plan.forward_scaled(&input, &mut output)?,
                _ => plan.inverse(&input, &mut output)?,
            }
            for (frequency, actual) in output.iter().enumerate() {
                let mut re = 0.;
                let mut im = 0.;
                for (i, value) in input.iter().enumerate() {
                    let angle = std::f64::consts::TAU * (frequency * i) as f64 / n as f64
                        * if kind == 2 { 1. } else { -1. };
                    let (s, c) = angle.sin_cos();
                    re += f64::from(value.real) * c - f64::from(value.imag) * s;
                    im += f64::from(value.real) * s + f64::from(value.imag) * c;
                }
                let tolerance = 64. + n as f64 / 4.;
                assert!(
                    (f64::from(actual.real) - re).abs() <= tolerance,
                    "real {n} {kind} bin{frequency}: {} versus {re}, bound {tolerance}",
                    actual.real
                );
                assert!(
                    (f64::from(actual.imag) - im).abs() <= tolerance,
                    "imag {n} {kind} bin{frequency}: {} versus {im}, bound {tolerance}",
                    actual.imag
                );
            }
        }
        let mut impulse = vec![Complex::default(); n];
        impulse[0].real = 1 << 20;
        plan.forward(&impulse, &mut output, 0)?;
        assert!(output.iter().all(|v| *v
            == Complex {
                real: 1 << 20,
                imag: 0
            }));
        // Scaled-forward low-level rounding is not a correctly rounded DFT:
        // late radix stages can amplify prior quantization noise. An impulse
        // independently verifies its 1/N scale without that accumulated noise.
        plan.forward_scaled(&impulse, &mut output)?;
        let expected = ((1 << 20) as f64 / n as f64).round() as i32;
        assert!(output.iter().all(|v| *v
            == Complex {
                real: expected,
                imag: 0
            }));
    }
    Ok(())
}
#[test]
fn invalid_buffers_and_downshift_preserve_entire_output() -> Result<(), Box<dyn std::error::Error>>
{
    for size in SIZES {
        let plan = FftPlan::new(size);
        let n = plan.points();
        let input = vec![Complex::default(); n];
        let mut output = vec![Complex { real: 7, imag: -9 }; n + 1];
        let before = output.clone();
        for invalid in [0, n - 1, n + 1] {
            let short = vec![Complex::default(); invalid];
            assert_eq!(
                plan.forward(&short, &mut output, 0),
                Err(FftError::InvalidBuffer)
            );
            assert_eq!(
                plan.forward_scaled(&short, &mut output),
                Err(FftError::InvalidBuffer)
            );
            assert_eq!(
                plan.inverse(&short, &mut output),
                Err(FftError::InvalidBuffer)
            );
            assert_eq!(output, before);
        }
        assert_eq!(
            plan.forward(&input, &mut output[..n - 1], 0),
            Err(FftError::InvalidBuffer)
        );
        assert_eq!(
            plan.forward(&input, &mut output, 32),
            Err(FftError::InvalidDownshift)
        );
        assert_eq!(
            plan.forward(&input, &mut output, u8::MAX),
            Err(FftError::InvalidDownshift)
        );
        assert_eq!(output, before);
        plan.forward(&input, &mut output, 31)?;
        assert!(output[..n].iter().all(|v| *v == Complex::default()));
        assert_eq!(output[n], before[n]);
    }
    Ok(())
}
