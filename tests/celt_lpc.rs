//! Original scalar autocorrelation, LPC coefficient fitting and filter outputs.
use opura::celt::lpc::{self, LpcError};
#[test]
fn original_correlations_coefficients_filters_and_memory_match()
-> Result<(), Box<dyn std::error::Error>> {
    let mut counts = [0; 4];
    for (line_no, line) in include_str!("celt-lpc-fixtures.txt").lines().enumerate() {
        if line.starts_with('#') {
            continue;
        }
        let r: Vec<_> = line.split_whitespace().collect();
        match r[0] {
            "acf" | "dc" => {
                let n: usize = r[1].parse()?;
                let lag: usize = r[2].parse()?;
                let windowed = r[3] == "1";
                let (input, index): (Vec<i16>, usize) = if r[0] == "dc" {
                    (vec![r[3].parse()?; n], 4)
                } else {
                    (
                        r[4..4 + n]
                            .iter()
                            .map(|s| s.parse())
                            .collect::<Result<_, _>>()?,
                        4 + n,
                    )
                };
                let windowed = windowed && r[0] != "dc";
                let expected_shift = r[index].parse()?;
                let expected: Vec<i32> = r[index + 1..]
                    .iter()
                    .map(|s| s.parse())
                    .collect::<Result<_, _>>()?;
                let actual = lpc::autocorrelation(&input, lag, windowed)?;
                assert_eq!(actual.values(), expected, "acf line{line_no}");
                assert_eq!(actual.shift(), expected_shift);
                assert!((1 << 28..1 << 29).contains(&actual.values()[0]));
                assert!(
                    actual
                        .values()
                        .iter()
                        .all(|&v| i64::from(v).abs() <= i64::from(actual.values()[0]))
                );
                counts[0] += 1;
            }
            "lpc" => {
                let p: usize = r[1].parse()?;
                let ac: Vec<i32> = r[2..3 + p]
                    .iter()
                    .map(|s| s.parse())
                    .collect::<Result<_, _>>()?;
                let mut actual: Vec<i16> = r[3 + p..3 + 2 * p]
                    .iter()
                    .map(|s| s.parse())
                    .collect::<Result<_, _>>()?;
                let expected: Vec<i16> = r[3 + 2 * p..]
                    .iter()
                    .map(|s| s.parse())
                    .collect::<Result<_, _>>()?;
                lpc::analyse(&ac, &mut actual)
                    .map_err(|e| format!("LPC line{line_no} p{p}: {e}"))?;
                assert_eq!(actual, expected, "lpc line{line_no}");
                counts[1] += 1;
            }
            "fir" => {
                let n: usize = r[1].parse()?;
                let p: usize = r[2].parse()?;
                let coeff: Vec<i16> = r[3..3 + p]
                    .iter()
                    .map(|s| s.parse())
                    .collect::<Result<_, _>>()?;
                let input: Vec<i16> = r[3 + p..3 + 2 * p + n]
                    .iter()
                    .map(|s| s.parse())
                    .collect::<Result<_, _>>()?;
                let expected: Vec<i16> = r[3 + 2 * p + n..]
                    .iter()
                    .map(|s| s.parse())
                    .collect::<Result<_, _>>()?;
                let mut actual = vec![0i16; n];
                lpc::fir(&input, &coeff, &mut actual)?;
                assert_eq!(actual, expected, "fir line{line_no} n{n} p{p}");
                counts[2] += 1;
            }
            "iir" => {
                let n: usize = r[1].parse()?;
                let p: usize = r[2].parse()?;
                let coeff: Vec<i16> = r[3..3 + p]
                    .iter()
                    .map(|s| s.parse())
                    .collect::<Result<_, _>>()?;
                let mut memory: Vec<i16> = r[3 + p..3 + 2 * p]
                    .iter()
                    .map(|s| s.parse())
                    .collect::<Result<_, _>>()?;
                let mut actual: Vec<i32> = r[3 + 2 * p..3 + 2 * p + n]
                    .iter()
                    .map(|s| s.parse())
                    .collect::<Result<_, _>>()?;
                let expected: Vec<i32> = r[3 + 2 * p + n..3 + 2 * p + 2 * n]
                    .iter()
                    .map(|s| s.parse())
                    .collect::<Result<_, _>>()?;
                let expected_mem: Vec<i16> = r[3 + 2 * p + 2 * n..]
                    .iter()
                    .map(|s| s.parse())
                    .collect::<Result<_, _>>()?;
                lpc::iir(&mut actual, &coeff, &mut memory)?;
                assert_eq!(actual, expected, "iir line{line_no} n{n} p{p}");
                assert_eq!(memory, expected_mem, "iir memory line{line_no}");
                counts[3] += 1;
            }
            _ => panic!("fixture kind"),
        }
    }
    assert_eq!(counts, [1601, 2680, 832, 336]);
    Ok(())
}
#[test]
fn invalid_geometry_and_unstable_correlations_preserve_outputs()
-> Result<(), Box<dyn std::error::Error>> {
    for (n, lag, windowed) in [
        (0, 0, false),
        (1, 0, false),
        (2, 0, false),
        (2049, 0, false),
        (32, 25, false),
        (24, 24, false),
        (239, 4, true),
    ] {
        assert_eq!(
            lpc::autocorrelation(&vec![0; n], lag, windowed),
            Err(LpcError::InvalidGeometry)
        );
    }
    for (ac, p) in [
        (vec![0], 0),
        (vec![0; 26], 25),
        (vec![0; 4], 4),
        (vec![-1, 0], 1),
        (vec![0, 1], 1),
        (vec![1, 2], 1),
        (vec![(1 << 30) + 1, 0], 1),
        (vec![1 << 28, 1 << 27, -(1 << 28)], 2),
    ] {
        let mut actual = vec![123; p];
        let before = actual.clone();
        assert!(lpc::analyse(&ac, &mut actual).is_err());
        assert_eq!(actual, before);
    }
    for (n, p, length) in [
        (0, 4, 4),
        (1025, 4, 1029),
        (8, 0, 8),
        (8, 2, 10),
        (8, 25, 33),
        (8, 4, 11),
        (8, 4, 13),
    ] {
        let mut out = vec![123; n];
        let before = out.clone();
        assert_eq!(
            lpc::fir(&vec![1; length], &vec![1; p], &mut out),
            Err(LpcError::InvalidGeometry)
        );
        assert_eq!(out, before);
    }
    for (n, p, m) in [
        (0, 4, 4),
        (2, 4, 4),
        (1084, 4, 4),
        (7, 4, 4),
        (24, 3, 3),
        (24, 5, 5),
        (32, 28, 28),
        (24, 4, 3),
        (24, 4, 5),
    ] {
        let mut values = vec![123; n];
        let mut memory = vec![321; m];
        let before = (values.clone(), memory.clone());
        assert_eq!(
            lpc::iir(&mut values, &vec![1; p], &mut memory),
            Err(LpcError::InvalidGeometry)
        );
        assert_eq!((values, memory), before);
    }
    let mut coefficients = [123i16; 24];
    lpc::analyse(&[0; 25], &mut coefficients)?;
    assert_eq!(coefficients, [0; 24]);
    let input = [0, 0, 0, -32768, -1, 0, 1, 32767];
    let mut out = [0; 5];
    lpc::fir(&input, &[0; 3], &mut out)?;
    assert_eq!(out, [-32767, -1, 0, 1, 32767]);
    let mut signal = [i32::MIN, -123456, 0, i32::MAX, 4096, 0, 1, 42];
    let before = signal;
    let mut memory = [-32768; 4];
    lpc::iir(&mut signal, &[0; 4], &mut memory)?;
    assert_eq!(signal, before);
    assert_eq!(memory, [42, 1, 0, 4096]);
    Ok(())
}
