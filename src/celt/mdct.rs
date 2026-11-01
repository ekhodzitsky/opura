//! Standard CELT inverse MDCT and 120-sample TDAC synthesis window.
//! Returns signed-32 internal signal, before CELT postfilter/de-emphasis/PCM.
//! Algorithms and Q15 coefficients follow pinned Opus mdct.c/static modes.
//! Copyright (c) 2007-2008 CSIRO; 2007-2009 Xiph.Org Foundation.
//! See THIRD_PARTY_NOTICES.md.
use super::{
    FrameSize,
    fft::{Complex, FftError, FftPlan},
    mdct_tables::{TRIG, WINDOW},
};
use std::fmt;

/// Invalid inverse-MDCT geometry or FFT operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum MdctError {
    /// Stride must be 1/2/4/8, select at most 960 bins, and input must contain
    /// every selected value within a buffer of at most 960 entries.
    InvalidGeometry,
    /// FFT validation failed; no overlap is committed.
    Fft(FftError),
}
impl fmt::Display for MdctError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidGeometry => f.write_str("invalid CELT inverse MDCT spectral geometry"),
            Self::Fft(e) => write!(f, "CELT inverse MDCT: {e}"),
        }
    }
}
impl std::error::Error for MdctError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Fft(e) => Some(e),
            _ => None,
        }
    }
}
impl From<FftError> for MdctError {
    fn from(e: FftError) -> Self {
        Self::Fft(e)
    }
}

/// One windowed internal signal frame, not final signed-16 PCM.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TimeFrame {
    samples: [i32; 960],
    length: usize,
}
impl TimeFrame {
    /// 120/240/480/960 internal-rate time samples after TDAC overlap mixing.
    pub fn samples(&self) -> &[i32] {
        &self.samples[..self.length]
    }
}
/// Per-channel 60-sample inverse-MDCT overlap, retained across frame-size changes.
/// Every operation commits its complete output and overlap together.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MdctState {
    overlap: [i32; 60],
}
impl Default for MdctState {
    fn default() -> Self {
        Self { overlap: [0; 60] }
    }
}
impl MdctState {
    pub(crate) fn set_overlap(&mut self, overlap: &[i32; 60]) {
        self.overlap = *overlap;
    }

    /// Clear the overlap before a fresh independent stream.
    pub fn reset(&mut self) {
        self.overlap.fill(0);
    }
    /// Inspect the pending 60-sample overlap for the following block.
    pub fn overlap(&self) -> &[i32; 60] {
        &self.overlap
    }
    /// Synthesize a long or short block using the standard 120-sample window.
    ///
    /// Input bin i is `input[i*stride]`; other input entries are ignored. Size
    /// chooses 120/240/480/960 bins/time samples. Interleaved short blocks use
    /// separate calls on successive input offsets, preserving overlap each time.
    /// Input signal scale matches pinned clt_mdct_backward; caller handles
    /// energy denormalization, postfiltering and final PCM conversion.
    pub fn synthesize(
        &mut self,
        size: FrameSize,
        input: &[i32],
        stride: usize,
    ) -> Result<TimeFrame, MdctError> {
        let n = size.samples();
        let half = n / 2;
        if !matches!(stride, 1 | 2 | 4 | 8)
            || n * stride > 960
            || input.len() < (n - 1) * stride + 1
            || input.len() > 960
        {
            return Err(MdctError::InvalidGeometry);
        }
        let trig = &TRIG[1920 - 2 * n..1920 - n];
        let mut sum = n as i32;
        let mut maximum = 0i32;
        for i in 0..n {
            let value = input[i * stride];
            maximum = maximum.max(value.wrapping_abs());
            sum = sum.wrapping_add((value >> 11).wrapping_abs());
        }
        // zlog treats nonpositive values as zero. Preserve the reference's
        // signed wrapping of 1+maximum, including pathological full-scale input.
        let pre = (29 - zlog(maximum.wrapping_add(1))).max(0) as u32;
        let post = (19 - zlog(sum.wrapping_abs())).max(0).min(pre as i32) as u32;
        let fft_shift = (pre - post) as u8;
        let mut spectrum = [Complex::default(); 480];
        for i in 0..half {
            let x1 = input[2 * i * stride].wrapping_shl(pre);
            let x2 = input[(n - 1 - 2 * i) * stride].wrapping_shl(pre);
            let a = mul(x2, trig[i]).wrapping_add(mul(x1, trig[half + i]));
            let b = mul(x1, trig[i]).wrapping_sub(mul(x2, trig[half + i]));
            spectrum[i] = Complex { real: b, imag: a };
        }
        let mut transformed = [Complex::default(); 480];
        FftPlan::new(size).forward(&spectrum[..half], &mut transformed[..half], fft_shift)?;
        let mut output = [0i32; 1020];
        output[..60].copy_from_slice(&self.overlap);
        for (i, value) in transformed[..half].iter().enumerate() {
            output[60 + 2 * i] = value.real;
            output[61 + 2 * i] = value.imag;
        }
        for i in 0..half / 2 {
            let left = 60 + 2 * i;
            let right = 60 + n - 2 - 2 * i;
            let re = output[left + 1];
            let im = output[left];
            let yr = round(mul(re, trig[i]).wrapping_add(mul(im, trig[half + i])), post);
            let yi = round(mul(re, trig[half + i]).wrapping_sub(mul(im, trig[i])), post);
            // Read the opposite pair before the four in-place de-shuffle writes.
            let re = output[right + 1];
            let im = output[right];
            output[left] = yr;
            output[right + 1] = yi;
            let t0 = trig[half - i - 1];
            let t1 = trig[n - i - 1];
            output[right] = round(mul(re, t0).wrapping_add(mul(im, t1)), post);
            output[left + 1] = round(mul(re, t1).wrapping_sub(mul(im, t0)), post);
        }
        for i in 0..60 {
            let opposite = 119 - i;
            let x1 = output[opposite];
            let x2 = output[i];
            output[i] = mul(x2, WINDOW[119 - i]).wrapping_sub(mul(x1, WINDOW[i]));
            output[opposite] = mul(x2, WINDOW[i]).wrapping_add(mul(x1, WINDOW[119 - i]));
        }
        let mut result = TimeFrame {
            samples: [0; 960],
            length: n,
        };
        result.samples[..n].copy_from_slice(&output[..n]);
        self.overlap.copy_from_slice(&output[n..n + 60]);
        Ok(result)
    }
}
fn mul(x: i32, coefficient: i16) -> i32 {
    ((i64::from(x) * i64::from(coefficient)) >> 15) as i32
}
fn round(x: i32, shift: u32) -> i32 {
    if shift == 0 {
        x
    } else {
        x.wrapping_add(1 << (shift - 1)) >> shift
    }
}
fn zlog(x: i32) -> i32 {
    if x <= 0 {
        0
    } else {
        31 - x.leading_zeros() as i32
    }
}
