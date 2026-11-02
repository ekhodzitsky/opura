//! SILK stereo predictor entropy and buffered mid/side reconstruction.
//!
//! Adapted from Opus v1.6.1 stereo_decode_pred.c and stereo_MS_to_LR.c;
//! see THIRD_PARTY_NOTICES.md. Packet channel scheduling and resampling are separate.
use super::{
    pitch::SilkRate,
    synthesis::{mul_wb, round_shift, saturate16},
};
use crate::range::{RangeDecoder, RangeError};
use std::fmt;

#[path = "stereo_tables.rs"]
pub(super) mod tables;
use tables::*;

/// Invalid stereo fields or geometry; no partial entropy/history is committed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum StereoError {
    /// Stereo symbols exceed the active entropy budget.
    BitBudgetExceeded,
    /// Mid and side must have the same 10/20 ms length at the supplied rate.
    IncompatibleFrame,
    /// An entropy operation failed.
    Entropy(RangeError),
}
impl fmt::Display for StereoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BitBudgetExceeded => f.write_str("SILK stereo fields exceed frame bit budget"),
            Self::IncompatibleFrame => f.write_str("incompatible SILK mid/side frame geometry"),
            Self::Entropy(e) => write!(f, "SILK stereo entropy: {e}"),
        }
    }
}
impl std::error::Error for StereoError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Entropy(e) => Some(e),
            _ => None,
        }
    }
}
impl From<RangeError> for StereoError {
    fn from(value: RangeError) -> Self {
        Self::Entropy(value)
    }
}

/// Dequantized SILK stereo predictors in the reference's optimized Q13 form.
///
/// The first value is the difference between the two reconstructed predictors.
/// Default is the zero prediction used before the first stereo frame.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StereoPrediction {
    coefficients: [i32; 2],
}
impl StereoPrediction {
    /// Decode joint/coarse/fine predictor symbols at the positioned stereo prefix.
    /// Failure leaves the entropy cursor unchanged. Mid-only is a separate field.
    pub fn decode(decoder: &mut RangeDecoder<'_>) -> Result<Self, StereoError> {
        let mut next = decoder.clone();
        let joint = next.decode_icdf(&JOINT, 8)?;
        let groups = [joint / 5, joint % 5];
        let mut coefficients = [0; 2];
        for (index, coefficient) in coefficients.iter_mut().enumerate() {
            let coarse = next.decode_icdf(&UNIFORM3, 8)? + 3 * groups[index];
            let fine = next.decode_icdf(&UNIFORM5, 8)?;
            let low = i32::from(QUANT[coarse]);
            let step = mul_wb(i32::from(QUANT[coarse + 1]) - low, 6554);
            *coefficient = low + step * (2 * fine as i32 + 1);
        }
        if !next.is_within_budget() {
            return Err(StereoError::BitBudgetExceeded);
        }
        coefficients[0] -= coefficients[1];
        *decoder = next;
        Ok(Self { coefficients })
    }
    /// Optimized Q13 predictor pair used by mid/side reconstruction.
    pub fn coefficients_q13(&self) -> &[i32; 2] {
        &self.coefficients
    }
}

/// Decode the optional mid-only symbol, committing entropy only on success.
///
/// The caller determines presence from side VAD/LBRR flags. Do not read this
/// field unconditionally; channel-frame orchestration supplies that context.
pub fn decode_mid_only(decoder: &mut RangeDecoder<'_>) -> Result<bool, StereoError> {
    let mut next = decoder.clone();
    let mid = next.decode_icdf(&MID_ONLY, 8)? != 0;
    if !next.is_within_budget() {
        return Err(StereoError::BitBudgetExceeded);
    }
    *decoder = next;
    Ok(mid)
}

/// Buffered left/right internal-rate PCM, ready for per-channel resampling.
///
/// Includes the one-sample buffering delay from reference stereo reconstruction;
/// do not add the separate mono buffering delay to these samples.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StereoFrame {
    left: [i16; 320],
    right: [i16; 320],
    length: usize,
    rate: SilkRate,
}
impl StereoFrame {
    /// Left channel's buffered internal-rate samples.
    pub fn left(&self) -> &[i16] {
        &self.left[..self.length]
    }
    /// Right channel's buffered internal-rate samples.
    pub fn right(&self) -> &[i16] {
        &self.right[..self.length]
    }
    /// Internal sampling rate, before output resampling.
    pub fn rate(&self) -> SilkRate {
        self.rate
    }
}

/// Persistent predictor and raw mid/side history for stereo reconstruction.
///
/// Input-rate changes retain this history, as upstream does; packet-level
/// channel transitions must additionally apply their own reset policy.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct StereoState {
    prediction: StereoPrediction,
    mid: [i16; 2],
    side: [i16; 2],
}
impl StereoState {
    pub(super) fn enter_stereo(&mut self) {
        self.prediction = StereoPrediction::default();
        self.side.fill(0);
    }

    // mono buffering shares the raw mid history used by MS_to_LR.
    pub(super) fn buffer_mono(&mut self, input: &[i16]) -> [i16; 320] {
        let mut output = [0; 320];
        output[0] = self.mid[1];
        output[1..input.len()].copy_from_slice(&input[..input.len() - 1]);
        self.mid.copy_from_slice(&input[input.len() - 2..]);
        output
    }

    /// Clear predictor and buffered mid/side samples before a new stream.
    pub fn reset(&mut self) {
        *self = Self::default();
    }
    /// Last prediction, reusable during loss when no stereo symbols are decoded.
    pub fn previous_prediction(&self) -> StereoPrediction {
        self.prediction
    }

    /// Convert one raw mid/side internal frame with 8 ms predictor interpolation.
    ///
    /// Inputs must be equal-length 10/20 ms PCM at the supplied rate. Invalid
    /// geometry leaves all history unchanged. The predictor type limits values
    /// to decoded/reference forms; the caller supplies matching channel frames.
    /// Uses fixed buffers and bounded loops with no allocation or unsafe Rust.
    pub fn reconstruct(
        &mut self,
        rate: SilkRate,
        mid: &[i16],
        side: &[i16],
        prediction: StereoPrediction,
    ) -> Result<StereoFrame, StereoError> {
        let khz = rate.khz() as usize;
        let length = mid.len();
        if side.len() != length || !(length == khz * 10 || length == khz * 20) {
            return Err(StereoError::IncompatibleFrame);
        }
        let mut a = [0i16; 322];
        let mut b = [0i16; 322];
        a[..2].copy_from_slice(&self.mid);
        b[..2].copy_from_slice(&self.side);
        a[2..length + 2].copy_from_slice(mid);
        b[2..length + 2].copy_from_slice(side);
        let interpolate = 8 * khz;
        let denominator = 65536 / interpolate as i32;
        let mut predictors = self.prediction.coefficients;
        let mut delta = [0; 2];
        for i in 0..2 {
            // silk_SMULBB truncates the predictor difference to signed 16 bits.
            // Extreme quantizer changes intentionally retain that behavior.
            let difference = i32::from((prediction.coefficients[i] - predictors[i]) as i16);
            delta[i] = round_shift(difference * denominator, 16);
        }
        for n in 0..length {
            if n < interpolate {
                for i in 0..2 {
                    predictors[i] += delta[i];
                }
            } else {
                predictors = prediction.coefficients;
            }
            let smoothed =
                (i32::from(a[n]) + i32::from(a[n + 2]) + (i32::from(a[n + 1]) << 1)) << 9;
            let side_q8 = (i32::from(b[n + 1]) << 8)
                .wrapping_add(mul_wb(smoothed, predictors[0]))
                .wrapping_add(mul_wb(i32::from(a[n + 1]) << 11, predictors[1]));
            b[n + 1] = saturate16(round_shift(side_q8, 8));
        }
        let mut output = StereoFrame {
            left: [0; 320],
            right: [0; 320],
            length,
            rate,
        };
        for n in 0..length {
            output.left[n] = saturate16(i32::from(a[n + 1]) + i32::from(b[n + 1]));
            output.right[n] = saturate16(i32::from(a[n + 1]) - i32::from(b[n + 1]));
        }
        self.prediction = prediction;
        self.mid.copy_from_slice(&a[length..length + 2]);
        // Save unpredicted raw side samples, not the reconstructed b buffer.
        self.side.copy_from_slice(&side[length - 2..]);
        Ok(output)
    }
}

#[cfg(test)]
#[path = "stereo_tests.rs"]
mod tests;
