//! Standard CELT scalar fixed-point mixed-radix complex FFT.
//! Arithmetic follows pinned kiss_fft.c, including intentional wrapping and
//! stage-local downshift rounding. No allocation or runtime planning.
//! Copyright (c) 2003-2004 Mark Borgerding; 2005-2008 Xiph.Org Foundation;
//! 2008 CSIRO. See THIRD_PARTY_NOTICES.md.
use super::{FrameSize, fft_tables};
use std::fmt;

/// Signed-32 complex FFT sample (caller-selected fixed-point scale).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Complex {
    /// Real coordinate.
    pub real: i32,
    /// Imaginary coordinate.
    pub imag: i32,
}
impl Complex {
    fn add(self, b: Self) -> Self {
        Self {
            real: self.real.wrapping_add(b.real),
            imag: self.imag.wrapping_add(b.imag),
        }
    }
    fn sub(self, b: Self) -> Self {
        Self {
            real: self.real.wrapping_sub(b.real),
            imag: self.imag.wrapping_sub(b.imag),
        }
    }
    fn scale(self, coefficient: i32) -> Self {
        Self {
            real: mul(self.real, coefficient),
            imag: mul(self.imag, coefficient),
        }
    }
    fn twiddle(self, t: [i16; 2]) -> Self {
        Self {
            real: mul(self.real, i32::from(t[0])).wrapping_sub(mul(self.imag, i32::from(t[1]))),
            imag: mul(self.real, i32::from(t[1])).wrapping_add(mul(self.imag, i32::from(t[0]))),
        }
    }
}
/// Invalid FFT buffer geometry or downshift, leaving output unchanged.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum FftError {
    /// Input length must match plan and output must have at least that capacity.
    InvalidBuffer,
    /// Total transform downshift must be zero through 31.
    InvalidDownshift,
}
impl fmt::Display for FftError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidBuffer => "invalid CELT FFT buffer geometry",
            Self::InvalidDownshift => "CELT FFT downshift must be 0..31",
        })
    }
}
impl std::error::Error for FftError {}

/// Immutable standard plan for 60/120/240/480 complex points.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FftPlan {
    frame: FrameSize,
}
impl FftPlan {
    /// Select the complex FFT used by the frame's 120/240/480/960-sample MDCT.
    pub fn new(frame: FrameSize) -> Self {
        Self { frame }
    }
    /// Number of complex points, half the internal CELT frame sample count.
    pub fn points(self) -> usize {
        self.frame.samples() / 2
    }
    /// Forward negative-exponent FFT from natural input to natural output order.
    /// No initial scale; downshift is distributed across stages as upstream.
    /// All coordinates use explicit signed wrapping; caller controls headroom.
    pub fn forward(
        self,
        input: &[Complex],
        output: &mut [Complex],
        downshift: u8,
    ) -> Result<(), FftError> {
        self.check(input, output, downshift)?;
        self.prepare(input, output, false, false);
        self.transform(&mut output[..self.points()], downshift);
        Ok(())
    }
    /// Canonical scaled-forward wrapper, matching opus_fft_c (approximately 1/N).
    pub fn forward_scaled(self, input: &[Complex], output: &mut [Complex]) -> Result<(), FftError> {
        self.check(input, output, 0)?;
        self.prepare(input, output, true, false);
        self.transform(
            &mut output[..self.points()],
            fft_tables::SCALE_SHIFT[self.frame.lm()] - 1,
        );
        Ok(())
    }
    /// Unscaled positive-exponent inverse, matching opus_ifft_c.
    pub fn inverse(self, input: &[Complex], output: &mut [Complex]) -> Result<(), FftError> {
        self.check(input, output, 0)?;
        self.prepare(input, output, false, true);
        self.transform(&mut output[..self.points()], 0);
        for value in &mut output[..self.points()] {
            value.imag = value.imag.wrapping_neg();
        }
        Ok(())
    }
    fn check(self, input: &[Complex], output: &[Complex], downshift: u8) -> Result<(), FftError> {
        if input.len() != self.points() || output.len() < self.points() {
            return Err(FftError::InvalidBuffer);
        }
        if downshift > 31 {
            return Err(FftError::InvalidDownshift);
        }
        Ok(())
    }
    fn prepare(self, input: &[Complex], output: &mut [Complex], scaled: bool, inverse: bool) {
        let bitrev = fft_tables::BITREV[self.frame.lm()];
        for (i, &value) in input.iter().enumerate() {
            let mut value = value;
            if scaled {
                value.real = mul_q16(value.real, fft_tables::SCALE[self.frame.lm()]);
                value.imag = mul_q16(value.imag, fft_tables::SCALE[self.frame.lm()]);
            }
            if inverse {
                value.imag = value.imag.wrapping_neg();
            }
            output[usize::from(bitrev[i])] = value;
        }
    }
    fn transform(self, x: &mut [Complex], mut downshift: u8) {
        let factors = fft_tables::FACTORS[self.frame.lm()];
        let shift = fft_tables::TWIDDLE_SHIFT[self.frame.lm()];
        let mut strides = [1usize; 6];
        let mut count = 0;
        for &(radix, m) in factors {
            strides[count + 1] = strides[count] * radix;
            count += 1;
            if m == 1 {
                break;
            }
        }
        let mut m = 1;
        for i in (0..count).rev() {
            let radix = factors[i].0;
            let m2 = if i == 0 { 1 } else { factors[i - 1].1 };
            let step = match radix {
                2 => 1,
                3 | 4 => 2,
                _ => 3,
            };
            downscale(x, &mut downshift, step);
            let stride = strides[i] << shift;
            match radix {
                2 => butterfly2(x, strides[i]),
                3 => butterfly3(x, stride, m, strides[i], m2),
                4 => butterfly4(x, stride, m, strides[i], m2),
                _ => butterfly5(x, stride, m, strides[i], m2),
            }
            m = m2;
        }
        downscale(x, &mut downshift, 31);
    }
}
fn downscale(x: &mut [Complex], remaining: &mut u8, step: u8) {
    let shift = step.min(*remaining);
    *remaining -= shift;
    if shift == 0 {
        return;
    }
    for value in x {
        if shift == 1 {
            value.real >>= 1;
            value.imag >>= 1;
        } else {
            let rounding = 1i32 << (shift - 1);
            value.real = value.real.wrapping_add(rounding) >> shift;
            value.imag = value.imag.wrapping_add(rounding) >> shift;
        }
    }
}
fn mul(a: i32, b: i32) -> i32 {
    ((i64::from(a) * i64::from(b as i16)) >> 15) as i32
}
fn mul_q16(a: i32, b: i32) -> i32 {
    ((i64::from(a) * i64::from(b as i16)) >> 16) as i32
}
fn twiddle(x: Complex, index: usize) -> Complex {
    x.twiddle(fft_tables::TWIDDLES[index])
}
fn butterfly2(x: &mut [Complex], groups: usize) {
    for group in 0..groups {
        let b = group * 8;
        for j in 0..4 {
            let v = x[b + 4 + j];
            let t = match j {
                0 => v,
                1 => Complex {
                    real: mul(v.real.wrapping_add(v.imag), 23170),
                    imag: mul(v.imag.wrapping_sub(v.real), 23170),
                },
                2 => Complex {
                    real: v.imag,
                    imag: v.real.wrapping_neg(),
                },
                _ => Complex {
                    real: mul(v.imag.wrapping_sub(v.real), 23170),
                    imag: mul(v.imag.wrapping_add(v.real).wrapping_neg(), 23170),
                },
            };
            let a = x[b + j];
            x[b + 4 + j] = a.sub(t);
            x[b + j] = a.add(t);
        }
    }
}
fn butterfly4(x: &mut [Complex], stride: usize, m: usize, groups: usize, spacing: usize) {
    for group in 0..groups {
        for j in 0..m {
            let b = if m == 1 {
                group * 4
            } else {
                group * spacing + j
            };
            let a = x[b];
            let (v1, v2, v3) = if m == 1 {
                (x[b + 1], x[b + 2], x[b + 3])
            } else {
                (
                    twiddle(x[b + m], j * stride),
                    twiddle(x[b + 2 * m], 2 * j * stride),
                    twiddle(x[b + 3 * m], 3 * j * stride),
                )
            };
            let difference = a.sub(v2);
            let base = a.add(v2);
            let sum = v1.add(v3);
            let side = v1.sub(v3);
            x[b] = base.add(sum);
            x[b + 2 * m] = base.sub(sum);
            x[b + m] = Complex {
                real: difference.real.wrapping_add(side.imag),
                imag: difference.imag.wrapping_sub(side.real),
            };
            x[b + 3 * m] = Complex {
                real: difference.real.wrapping_sub(side.imag),
                imag: difference.imag.wrapping_add(side.real),
            };
        }
    }
}
fn butterfly3(x: &mut [Complex], stride: usize, m: usize, groups: usize, spacing: usize) {
    for group in 0..groups {
        for j in 0..m {
            let b = group * spacing + j;
            let v1 = twiddle(x[b + m], j * stride);
            let v2 = twiddle(x[b + 2 * m], 2 * j * stride);
            let sum = v1.add(v2);
            let difference = v1.sub(v2).scale(-28378);
            let base = x[b].sub(Complex {
                real: sum.real >> 1,
                imag: sum.imag >> 1,
            });
            x[b] = x[b].add(sum);
            x[b + 2 * m] = Complex {
                real: base.real.wrapping_add(difference.imag),
                imag: base.imag.wrapping_sub(difference.real),
            };
            x[b + m] = Complex {
                real: base.real.wrapping_sub(difference.imag),
                imag: base.imag.wrapping_add(difference.real),
            };
        }
    }
}
fn butterfly5(x: &mut [Complex], stride: usize, m: usize, groups: usize, spacing: usize) {
    for group in 0..groups {
        for j in 0..m {
            let b = group * spacing + j;
            let a = x[b];
            let v1 = twiddle(x[b + m], j * stride);
            let v2 = twiddle(x[b + 2 * m], 2 * j * stride);
            let v3 = twiddle(x[b + 3 * m], 3 * j * stride);
            let v4 = twiddle(x[b + 4 * m], 4 * j * stride);
            let s7 = v1.add(v4);
            let s10 = v1.sub(v4);
            let s8 = v2.add(v3);
            let s9 = v2.sub(v3);
            x[b] = a.add(s7.add(s8));
            let s5 = a.add(s7.scale(10126).add(s8.scale(-26510)));
            let r = s10.scale(-31164).add(s9.scale(-19261));
            let s6 = Complex {
                real: r.imag,
                imag: r.real.wrapping_neg(),
            };
            x[b + m] = s5.sub(s6);
            x[b + 4 * m] = s5.add(s6);
            let s11 = a.add(s7.scale(-26510).add(s8.scale(10126)));
            let t = s9.scale(-31164).sub(s10.scale(-19261));
            let s12 = Complex {
                real: t.imag,
                imag: t.real.wrapping_neg(),
            };
            x[b + 2 * m] = s11.add(s12);
            x[b + 3 * m] = s11.sub(s12);
        }
    }
}
