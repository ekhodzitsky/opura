//! Scalar fixed-point stereo-width analysis from Opus v1.6.1 opus_encoder.c.
//! Copyright (c) 2010-2011 Xiph.Org Foundation, Skype Limited.
//! See THIRD_PARTY_NOTICES.md.
use super::{FrontendError, q15, valid_duration};
use crate::celt::{input::InputRate, math};

/// Energy/correlation and one-second width history for raw stereo PCM.
///
/// Analyze before DC filtering or input delay. Use one instance per stream;
/// rate and frame duration may change without resetting this history.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct StereoWidthState {
    xx: i32,
    xy: i32,
    yy: i32,
    smoothed: i32,
    follower: i32,
}
impl StereoWidthState {
    /// Clear the stream history.
    pub fn reset(&mut self) {
        *self = Self::default();
    }
    /// Energy XX/XY/YY followed by smoothed width and peak follower in Q15.
    pub fn snapshot(&self) -> [i32; 5] {
        [self.xx, self.xy, self.yy, self.smoothed, self.follower]
    }
    /// Analyze exactly one interleaved stereo frame, returning width in Q15.
    ///
    /// `samples48` is a standard 2.5/5/10/20/40/60/80/100/120ms duration on
    /// the 48kHz clock. Input length is exactly `2*samples48/rate.factor()`.
    /// Like native analysis, the last two sample pairs at 12kHz/2.5ms are
    /// discarded. Quiet input retains the previous width/follower. No
    /// allocation occurs and errors preserve all history.
    pub fn analyze(
        &mut self,
        rate: InputRate,
        samples48: usize,
        pcm: &[i16],
    ) -> Result<i16, FrontendError> {
        if !valid_duration(samples48) || pcm.len() != 2 * (samples48 / rate.factor()) {
            return Err(FrontendError::InvalidGeometry);
        }
        let n = samples48 / rate.factor();
        let fps = rate.hz() as i32 / n as i32;
        let shift = n.ilog2() - 2;
        let alpha = 25 * 32767 / fps.max(50);
        let mut energy = [0_i32; 3];
        for block in pcm.chunks_exact(8) {
            let mut partial = [0_i32; 3];
            for pair in block.chunks_exact(2) {
                let (x, y) = (i32::from(pair[0]), i32::from(pair[1]));
                partial[0] += (x * x) >> 2;
                partial[1] += (x * y) >> 2;
                partial[2] += (y * y) >> 2;
            }
            for (sum, part) in energy.iter_mut().zip(partial) {
                *sum += part >> shift;
            }
        }
        let mut next = self.clone();
        next.xx = (self.xx + q15(alpha, energy[0] - self.xx)).max(0);
        // Avoid the potentially overflowing XY delta on abrupt sign changes.
        next.xy = (q15(32767 - alpha, self.xy) + q15(alpha, energy[1])).max(0);
        next.yy = (self.yy + q15(alpha, energy[2] - self.yy)).max(0);
        if next.xx.max(next.yy) > 210 {
            let sx = math::sqrt(next.xx)?;
            let sy = math::sqrt(next.yy)?;
            let qx = math::sqrt(sx)?;
            let qy = math::sqrt(sy)?;
            next.xy = next.xy.min(sx * sy);
            let correlation = math::fractional_divide(next.xy, 1 + sx * sy)? >> 16;
            let difference = 32767 * (qx - qy).abs() / (1 + qx + qy);
            let width = q15(
                math::sqrt((1 << 30) - correlation * correlation)?.min(32767),
                difference,
            );
            next.smoothed += (width - next.smoothed) / fps;
            next.follower = (next.follower - 655 / fps).max(next.smoothed);
        }
        let result = (20 * next.follower).min(32767) as i16;
        *self = next;
        Ok(result)
    }
}

#[cfg(test)]
mod tests;
