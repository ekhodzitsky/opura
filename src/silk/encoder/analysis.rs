//! Fixed-point SILK encoder parameter analysis and quantization.
//! Complete PCM channel/packet orchestration is still in progress.
//! Adapted from Opus v1.6.1; see THIRD_PARTY_NOTICES.md.
mod gain_processing;
mod lpc;
mod ltp;
mod nlsf;
mod pitch;
mod shaping;
mod spectrum;
mod vad;
use super::math::lin2log;
use crate::silk::{
    gains::{FrameDuration, linear_gain},
    synthesis::mul_wb,
};
pub use gain_processing::{GainProcessState, ProcessedGains, ResidualEnergies};
pub use lpc::{LpcAnalysis, analyze_lpc, lpc_to_nlsf};
pub use ltp::{LtpFrame, LtpResidual, LtpState};
pub use nlsf::{QuantizedNlsf, laroia_weights, quantize_nlsf};
pub use pitch::{PitchFrame, PitchOptions, PitchState};
pub use shaping::{ShapeFrame, ShapeOptions, ShapeState};
pub use spectrum::{SpectralFrame, SpectralOptions, SpectralState};
use std::fmt;
pub use vad::{SpeechActivity, VoiceActivityDetector};
/// Invalid analysis geometry, controls, or unrepresentable arithmetic result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum AnalysisError {
    /// Input vector length does not match the requested order/frame geometry.
    InvalidLength,
    /// A complexity, quality control, rate weight, survivor count or gain is invalid.
    InvalidParameters,
    /// A pathological input exceeds the native fixed-point cost domain.
    ArithmeticRange,
}
impl fmt::Display for AnalysisError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidLength => "invalid SILK analysis vector length",
            Self::InvalidParameters => "invalid SILK analysis parameters",
            Self::ArithmeticRange => "SILK analysis exceeds fixed-point arithmetic domain",
        })
    }
}
impl std::error::Error for AnalysisError {}
/// Quantized per-subframe gain symbols and corresponding Q16 scales.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QuantizedGains {
    indices: [u8; 4],
    gains: [i32; 4],
    count: usize,
}
impl QuantizedGains {
    /// Absolute or delta first symbol, followed by delta symbols.
    pub fn indices(&self) -> &[u8] {
        &self.indices[..self.count]
    }
    /// Reconstructed positive Q16 gain scales used by NSQ.
    pub fn gains_q16(&self) -> &[i32] {
        &self.gains[..self.count]
    }
}
/// Native hysteretic gain quantizer, initialized at logarithmic gain index ten.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GainQuantizer {
    previous: u8,
}
impl Default for GainQuantizer {
    fn default() -> Self {
        Self { previous: 10 }
    }
}
impl GainQuantizer {
    /// Restore initial gain history.
    pub fn reset(&mut self) {
        *self = Self::default();
    }
    /// Last reconstructed logarithmic gain index, 0..=63.
    pub fn previous_index(&self) -> u8 {
        self.previous
    }
    /// Quantize two/four positive Q16 gains with native hysteresis/delta limits.
    /// A failed validation changes neither history nor caller input.
    pub fn quantize(
        &mut self,
        duration: FrameDuration,
        input: &[i32],
        conditional: bool,
    ) -> Result<QuantizedGains, AnalysisError> {
        let count = if duration == FrameDuration::Ms10 {
            2
        } else {
            4
        };
        if input.len() != count {
            return Err(AnalysisError::InvalidLength);
        }
        if input.iter().any(|&v| v <= 0) {
            return Err(AnalysisError::InvalidParameters);
        }
        let mut result = QuantizedGains {
            indices: [0; 4],
            gains: [0; 4],
            count,
        };
        let mut previous = i32::from(self.previous);
        for (k, &gain) in input.iter().enumerate() {
            let mut index =
                mul_wb((65536 * 63) / ((86 * 128) / 6), lin2log(gain) - 2090) as i8 as i32;
            if index < previous {
                index += 1;
            }
            index = index.clamp(0, 63);
            if k == 0 && !conditional {
                index = index.clamp(previous - 4, 63);
                previous = index;
            } else {
                index -= previous;
                let threshold = 72 - 64 + previous;
                if index > threshold {
                    index = threshold + ((index - threshold + 1) >> 1);
                }
                index = index.clamp(-4, 36);
                if index > threshold {
                    previous = (previous + 2 * index - threshold).min(63);
                } else {
                    previous += index;
                }
                index += 4;
            }
            result.indices[k] = index as u8;
            result.gains[k] = linear_gain(previous as u8);
        }
        self.previous = previous as u8;
        Ok(result)
    }
}

#[cfg(test)]
#[path = "analysis_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "shaping_tests.rs"]
mod shaping_tests;
