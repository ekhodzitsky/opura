//! Fixed-point NLSF-to-LPC conversion for validated SILK spectra.
//!
//! Follows Opus v1.6.1 coefficient fitting and bounded stability correction.
//! See THIRD_PARTY_NOTICES.md. This stage does not synthesize audio.
use super::nlsf::FrameNlsf;

#[path = "lpc_table.rs"]
mod table;
use table::COS_Q12;

/// Q12 prediction coefficients for the two halves of an internal SILK frame.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FrameLpc {
    current: [i16; 16],
    first_half: [i16; 16],
    order: usize,
}
impl FrameLpc {
    /// Prediction filter for the second half (both halves for 10 ms frames).
    /// The implicit leading coefficient of one is omitted.
    pub fn current_q12(&self) -> &[i16] {
        &self.current[..self.order]
    }
    /// Prediction filter for the first half, with NLSF interpolation applied.
    /// The implicit leading coefficient of one is omitted.
    pub fn first_half_q12(&self) -> &[i16] {
        &self.first_half[..self.order]
    }
}
impl FrameNlsf {
    /// Convert validated spectral parameters into fixed-point prediction filters.
    /// Uses fixed storage and bounded correction loops; consumes no entropy and
    /// changes no history. Post-loss bandwidth expansion belongs to the future
    /// audio decoder and is not applied here.
    pub fn to_lpc(&self) -> FrameLpc {
        let current = convert(self.current_q15());
        let first_half = if self.interpolation_q2() == 4 {
            current
        } else {
            convert(self.first_half_q15())
        };
        FrameLpc {
            current,
            first_half,
            order: self.current_q15().len(),
        }
    }
}

// Validated FrameNlsf or bounded CNG-smoothed spectra reach this helper:
// length 10 or 16,
// ordered frequencies in 0..=32767. The polynomial ordering matches the reference.
pub(super) fn convert(spectrum: &[i16]) -> [i16; 16] {
    const ORDER10: [usize; 10] = [0, 9, 6, 3, 4, 5, 8, 1, 2, 7];
    const ORDER16: [usize; 16] = [0, 15, 8, 7, 4, 11, 12, 3, 2, 13, 10, 5, 6, 9, 14, 1];
    let count = spectrum.len();
    let ordering: &[usize] = if count == 10 { &ORDER10 } else { &ORDER16 };
    let mut cosine = [0_i32; 16];
    for (i, &frequency) in spectrum.iter().enumerate() {
        let index = (frequency >> 8) as usize;
        let fraction = i32::from(frequency & 255);
        let value = i32::from(COS_Q12[index]);
        let delta = i32::from(COS_Q12[index + 1]) - value;
        cosine[ordering[i]] = round_shift(i64::from(value * 256 + delta * fraction), 4) as i32;
    }
    let half = count / 2;
    let p = polynomial(&cosine, half);
    let q = polynomial(&cosine[1..], half);
    let mut coefficients = [0_i32; 16];
    for k in 0..half {
        let even = p[k + 1] + p[k];
        let odd = q[k + 1] - q[k];
        coefficients[k] = -odd - even;
        coefficients[count - k - 1] = odd - even;
    }
    let mut output = [0_i16; 16];
    fit(&mut coefficients[..count], &mut output[..count]);
    for iteration in 0..16 {
        if inverse_gain(&output[..count]) != 0 {
            break;
        }
        expand(&mut coefficients[..count], 65536 - (2 << iteration));
        for i in 0..count {
            output[i] = round_shift(i64::from(coefficients[i]), 5) as i16;
        }
    }
    output
}

fn round_shift(value: i64, bits: u32) -> i64 {
    (value + (1_i64 << (bits - 1))) >> bits
}

fn polynomial(cosine: &[i32], half: usize) -> [i32; 9] {
    let mut out = [0_i32; 9];
    out[0] = 65536;
    out[1] = -cosine[0];
    for k in 1..half {
        let value = i64::from(cosine[2 * k]);
        out[k + 1] = 2 * out[k - 1] - round_shift(value * i64::from(out[k]), 16) as i32;
        for n in (2..=k).rev() {
            out[n] += out[n - 2] - round_shift(value * i64::from(out[n - 1]), 16) as i32;
        }
        out[1] -= value as i32;
    }
    out
}

fn expand(coefficients: &mut [i32], mut chirp: i32) {
    let delta = chirp - 65536;
    for coefficient in coefficients {
        *coefficient = ((i64::from(chirp) * i64::from(*coefficient)) >> 16) as i32;
        chirp += round_shift(i64::from(chirp) * i64::from(delta), 16) as i32;
    }
}

fn fit(coefficients: &mut [i32], output: &mut [i16]) {
    for _ in 0..10 {
        let mut maximum = 0_i32;
        let mut index = 0;
        for (i, &coefficient) in coefficients.iter().enumerate() {
            if coefficient.abs() > maximum {
                maximum = coefficient.abs();
                index = i;
            }
        }
        maximum = round_shift(i64::from(maximum), 5) as i32;
        if maximum <= 32767 {
            for (out, &value) in output.iter_mut().zip(coefficients.iter()) {
                *out = round_shift(i64::from(value), 5) as i16;
            }
            return;
        }
        maximum = maximum.min(163838);
        let chirp = 65470 - ((maximum - 32767) * 16384) / ((maximum * (index as i32 + 1)) >> 2);
        expand(coefficients, chirp);
    }
    for (out, value) in output.iter_mut().zip(coefficients) {
        *out = round_shift(i64::from(*value), 5).clamp(-32768, 32767) as i16;
        *value = i32::from(*out) * 32;
    }
}

// Returns zero for unstable filters or excessive prediction power gain.
fn inverse_gain(coefficients: &[i16]) -> i32 {
    const LIMIT: i32 = 16773022; // round(0.99975 * 2^24)
    const MIN_GAIN: i32 = 107374; // round(2^30 / 10000)
    if coefficients.iter().copied().map(i32::from).sum::<i32>() >= 4096 {
        return 0;
    }
    let mut a = [0_i32; 16];
    for (out, &value) in a.iter_mut().zip(coefficients) {
        *out = i32::from(value) * 4096;
    }
    let mut gain = 1_i32 << 30;
    for k in (0..coefficients.len()).rev() {
        if !(-LIMIT..=LIMIT).contains(&a[k]) {
            return 0;
        }
        let reflection = -a[k] * 128;
        let multiplier = (1 << 30) - ((i64::from(reflection) * i64::from(reflection)) >> 32) as i32;
        gain = (((i64::from(gain) * i64::from(multiplier)) >> 32) << 2) as i32;
        if gain < MIN_GAIN {
            return 0;
        }
        if k == 0 {
            break;
        }
        let bits = 32 - multiplier.leading_zeros();
        let reciprocal = reciprocal(multiplier);
        for n in 0..k.div_ceil(2) {
            let left = a[n];
            let right = a[k - n - 1];
            let cross_right = round_shift(i64::from(right) * i64::from(reflection), 31) as i32;
            let cross_left = round_shift(i64::from(left) * i64::from(reflection), 31) as i32;
            let next_left = round_shift(
                i64::from(left.saturating_sub(cross_right)) * i64::from(reciprocal),
                bits,
            );
            let next_right = round_shift(
                i64::from(right.saturating_sub(cross_left)) * i64::from(reciprocal),
                bits,
            );
            let (Ok(next_left), Ok(next_right)) =
                (i32::try_from(next_left), i32::try_from(next_right))
            else {
                return 0;
            };
            a[n] = next_left;
            a[k - n - 1] = next_right;
        }
    }
    gain
}

// Upstream INVERSE32_varQ with Qres = bit_length(value) + 30. Normalization
// therefore needs no final shift. The positive denominator is bounded by the
// reflection-coefficient limit above; its normalized top word is never zero.
fn reciprocal(value: i32) -> i32 {
    let normalized = value << (value.leading_zeros() - 1);
    let inverse = (i32::MAX >> 2) / (normalized >> 16);
    let error = ((1 << 29) - ((i64::from(normalized) * i64::from(inverse)) >> 16) as i32) * 8;
    ((i64::from(inverse) << 16) + ((i64::from(error) * i64::from(inverse)) >> 16)) as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upstream_coefficients_and_inverse_gain() -> Result<(), Box<dyn std::error::Error>> {
        let mut cases = 0;
        for (line_number, line) in include_str!("../../tests/silk-lpc-fixtures.txt")
            .lines()
            .enumerate()
        {
            if line.starts_with('#') {
                continue;
            }
            let values: Vec<i32> = line
                .split_whitespace()
                .map(str::parse)
                .collect::<Result<_, _>>()?;
            let order = values[0] as usize;
            let spectrum: Vec<i16> = values[1..=order].iter().map(|&v| v as i16).collect();
            let expected: Vec<i16> = values[order + 1..=2 * order]
                .iter()
                .map(|&v| v as i16)
                .collect();
            let actual = convert(&spectrum);
            assert_eq!(
                &actual[..order],
                expected,
                "fixture line {}",
                line_number + 1
            );
            assert_eq!(
                inverse_gain(&actual[..order]),
                values[2 * order + 1],
                "inverse gain line {}",
                line_number + 1
            );
            assert!(inverse_gain(&actual[..order]) > 0);
            cases += 1;
        }
        assert_eq!(cases, 2281);
        Ok(())
    }

    #[test]
    fn inverse_gain_rejects_unstable_filters() {
        assert_eq!(inverse_gain(&[0; 10]), 1 << 30);
        let mut coefficients = [0; 16];
        coefficients[0] = 4096;
        assert_eq!(inverse_gain(&coefficients), 0);
        coefficients[0] = -4096;
        assert_eq!(inverse_gain(&coefficients), 0);
        coefficients[0] = 0;
        coefficients[15] = -4096;
        assert_eq!(inverse_gain(&coefficients), 0);
    }
}
