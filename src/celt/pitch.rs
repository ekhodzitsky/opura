//! Classic CELT PLC pitch analysis over decoded internal signal history.
//! Copyright (c) 2007-2008 CSIRO; 2007-2009 Xiph.Org Foundation.
//! Downsampling/whitening and coarse/fine search follow pinned pitch.c;
//! see THIRD_PARTY_NOTICES.md. No encoder pitch/remove-doubling is provided.
use super::lpc::{self, LpcError};
use std::fmt;
/// Invalid history geometry/signal or LPC whitening failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum PitchError {
    /// Each channel must contain exactly2048 internal samples.
    InvalidGeometry,
    /// Internal signal must fit the decoder saturation domain +/-536870911.
    InvalidSignal,
    /// Whitening analysis failed.
    Lpc(LpcError),
}
impl fmt::Display for PitchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidGeometry => f.write_str("CELT PLC pitch needs2048 samples per channel"),
            Self::InvalidSignal => f.write_str("CELT PLC pitch signal exceeds decoder saturation"),
            Self::Lpc(e) => write!(f, "CELT pitch whitening: {e}"),
        }
    }
}
impl std::error::Error for PitchError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Lpc(e) => Some(e),
            _ => None,
        }
    }
}
impl From<LpcError> for PitchError {
    fn from(e: LpcError) -> Self {
        Self::Lpc(e)
    }
}
/// Estimated48kHz period and fixed-storage whitened half-rate analysis signal.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlcPitch {
    period: u16,
    whitened: [i16; 1024],
}
impl PlcPitch {
    /// Concealment period in48kHz samples, within100..=720.
    pub fn period(&self) -> u16 {
        self.period
    }
    /// Reference factor2 lowpass plus LPC4/FIR5 whitening, before coarse scaling.
    pub fn whitened(&self) -> &[i16; 1024] {
        &self.whitened
    }
}
/// Find classic periodic-PLC pitch from mono/stereo decoded history. Inputs
/// are the latest2048 processed Q12 samples, bounded by decoder SIG_SAT.
/// Analysis uses fixed storage and bounded work, preserving borrowed input.
pub fn plc_pitch(left: &[i32], right: Option<&[i32]>) -> Result<PlcPitch, PitchError> {
    if left.len() != 2048 || right.is_some_and(|r| r.len() != 2048) {
        return Err(PitchError::InvalidGeometry);
    }
    if left
        .iter()
        .chain(right.into_iter().flatten())
        .any(|v| !(-536870911..=536870911).contains(v))
    {
        return Err(PitchError::InvalidSignal);
    }
    let max = left
        .iter()
        .chain(right.into_iter().flatten())
        .fold(1, |max, v| max.max(v.abs()));
    let shift = (ilog(max) - 10).max(0) as u32 + u32::from(right.is_some());
    let mut lp = [0i16; 1024];
    for channel in std::iter::once(left).chain(right) {
        for i in 0..1024 {
            let center = channel[2 * i] >> (shift + 1);
            let previous = if i > 0 {
                channel[2 * i - 1] >> (shift + 2)
            } else {
                0
            };
            let next = channel[2 * i + 1] >> (shift + 2);
            lp[i] = lp[i].wrapping_add((previous + next + center) as i16);
        }
    }
    let correlation = lpc::autocorrelation(&lp, 4, false)?;
    let mut ac = [0i32; 5];
    ac.copy_from_slice(correlation.values());
    ac[0] += ac[0] >> 13;
    for (i, v) in ac.iter_mut().enumerate().skip(1) {
        *v -= (((2 * i * i) as i64 * i64::from(*v)) >> 15) as i32;
    }
    let mut coeff = [0i16; 4];
    lpc::analyse(&ac, &mut coeff)?;
    let mut tmp = 32767;
    for coefficient in &mut coeff {
        tmp = (29491 * tmp) >> 15;
        *coefficient = ((i32::from(*coefficient) * tmp) >> 15) as i16;
    }
    let mut fir = [0i16; 5];
    fir[0] = coeff[0].wrapping_add(3277);
    for i in 1..4 {
        fir[i] = coeff[i].wrapping_add(((26214 * i32::from(coeff[i - 1])) >> 15) as i16);
    }
    fir[4] = ((26214 * i32::from(coeff[3])) >> 15) as i16;
    let mut memory = [0i16; 5];
    for value in &mut lp {
        let input = *value;
        let mut sum = i32::from(input) << 12;
        for j in 0..5 {
            sum = sum.wrapping_add(i32::from(fir[j]) * i32::from(memory[j]));
        }
        memory.copy_within(0..4, 1);
        memory[0] = input;
        *value = (sum.wrapping_add(2048) >> 12) as i16;
    }
    let period = 720 - search(&lp);
    Ok(PlcPitch {
        period: period as u16,
        whitened: lp,
    })
}
fn search(lp: &[i16; 1024]) -> i32 {
    // PLC uses len=2048-720=1328 and max_pitch=720-100=620.
    let mut x4 = [0i16; 332];
    let mut y4 = [0i16; 487];
    for (j, v) in x4.iter_mut().enumerate() {
        *v = lp[360 + 2 * j];
    }
    for (j, v) in y4.iter_mut().enumerate() {
        *v = lp[2 * j];
    }
    let max = x4
        .iter()
        .chain(&y4)
        .fold(1, |max, &v| max.max(i32::from(v).abs()));
    let mut shift = (ilog(max) - 14 + 5).max(0) as u32;
    if shift > 0 {
        for v in x4.iter_mut().chain(&mut y4) {
            *v >>= shift;
        }
        shift *= 2;
    }
    let mut corr = [0i32; 310];
    let mut maxcorr = 1;
    for i in 0..155 {
        let mut sum = 0i32;
        for j in 0..332 {
            sum = sum.wrapping_add(i32::from(x4[j]) * i32::from(y4[i + j]));
        }
        corr[i] = sum;
        maxcorr = maxcorr.max(sum);
    }
    let coarse = find_best(&corr[..155], &y4, 332, 0, maxcorr);
    maxcorr = 1;
    for i in 0..310 {
        corr[i] = 0;
        if i.abs_diff(2 * coarse[0]) > 2 && i.abs_diff(2 * coarse[1]) > 2 {
            continue;
        }
        let mut sum = 0i32;
        for j in 0..664 {
            sum = sum.wrapping_add((i32::from(lp[360 + j]) * i32::from(lp[i + j])) >> shift);
        }
        corr[i] = sum.max(-1);
        maxcorr = maxcorr.max(sum);
    }
    let best = find_best(&corr, lp, 664, shift + 1, maxcorr)[0];
    let offset = if best > 0 && best < 309 {
        let a = corr[best - 1];
        let b = corr[best];
        let c = corr[best + 1];
        if c.wrapping_sub(a) > mul15(22938, b.wrapping_sub(a)) {
            1
        } else if a.wrapping_sub(c) > mul15(22938, b.wrapping_sub(c)) {
            -1
        } else {
            0
        }
    } else {
        0
    };
    2 * best as i32 - offset
}
fn find_best(corr: &[i32], y: &[i16], length: usize, yshift: u32, maxcorr: i32) -> [usize; 2] {
    let xshift = ilog(maxcorr) - 14;
    let mut energy = 1i32;
    for &v in &y[..length] {
        energy = energy.wrapping_add((i32::from(v) * i32::from(v)) >> yshift);
    }
    let mut best = [0, 1];
    let mut numerators = [-1i32; 2];
    let mut denominators = [0i32; 2];
    for (i, &value) in corr.iter().enumerate() {
        if value > 0 {
            let normalized = if xshift > 0 {
                value >> xshift
            } else {
                value.wrapping_shl((-xshift) as u32)
            } as i16 as i32;
            let numerator = (normalized * normalized) >> 15;
            if mul15(numerator, denominators[1]) > mul15(numerators[1], energy) {
                if mul15(numerator, denominators[0]) > mul15(numerators[0], energy) {
                    numerators[1] = numerators[0];
                    denominators[1] = denominators[0];
                    best[1] = best[0];
                    numerators[0] = numerator;
                    denominators[0] = energy;
                    best[0] = i;
                } else {
                    numerators[1] = numerator;
                    denominators[1] = energy;
                    best[1] = i;
                }
            }
        }
        energy = energy
            .wrapping_add((i32::from(y[i + length]) * i32::from(y[i + length])) >> yshift)
            .wrapping_sub((i32::from(y[i]) * i32::from(y[i])) >> yshift)
            .max(1);
    }
    best
}
fn mul15(a: i32, b: i32) -> i32 {
    ((i64::from(a as i16) * i64::from(b)) >> 15) as i32
}
fn ilog(x: i32) -> i32 {
    31 - x.leading_zeros() as i32
}
