//! SILK pitch, long-term prediction coefficients and excitation seed.
//!
//! Consumes side information after NLSF symbols, stopping before excitation.
//! Adapted from Opus v1.6.1; see THIRD_PARTY_NOTICES.md.
use super::gains::{FrameDuration, SignalType};
use crate::range::{RangeDecoder, RangeError};
use std::fmt;

#[path = "pitch_tables.rs"]
mod tables;
use tables::*;

/// Internal SILK sampling rate, independent of the output sampling rate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SilkRate {
    /// Narrowband, 8 kHz.
    Khz8,
    /// Mediumband, 12 kHz.
    Khz12,
    /// Wideband or the SILK part of Hybrid, 16 kHz.
    Khz16,
}
impl SilkRate {
    pub(super) fn khz(self) -> i32 {
        match self {
            Self::Khz8 => 8,
            Self::Khz12 => 12,
            Self::Khz16 => 16,
        }
    }
    fn low_bits(self) -> &'static [u8] {
        match self {
            Self::Khz8 => &UNIFORM4,
            Self::Khz12 => &UNIFORM6,
            Self::Khz16 => &UNIFORM8,
        }
    }
    fn contour(self, duration: FrameDuration) -> (&'static [u8], &'static [i8]) {
        match (self, duration) {
            (Self::Khz8, FrameDuration::Ms10) => (&CONTOUR10_NB, &LAGS10_NB),
            (Self::Khz8, FrameDuration::Ms20) => (&CONTOUR20_NB, &LAGS20_NB),
            (_, FrameDuration::Ms10) => (&CONTOUR10, &LAGS10),
            (_, FrameDuration::Ms20) => (&CONTOUR20, &LAGS20),
        }
    }
}

/// Entropy context and LTP scaling mode for an internal SILK frame.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PitchCoding {
    /// Absolute lag and a transmitted LTP scale index.
    Independent,
    /// Absolute lag with implicit scale index zero (e.g. restarted side channel).
    IndependentWithoutScaling,
    /// Delta lag when the previous frame was voiced; implicit scale index zero.
    Conditional,
}

/// Failure to consume pitch/LTP side information and seed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum PitchError {
    /// Symbols require more bits than the compressed frame contains.
    BitBudgetExceeded,
    /// An entropy operation failed.
    Entropy(RangeError),
}
impl fmt::Display for PitchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BitBudgetExceeded => f.write_str("SILK pitch exceeds frame bit budget"),
            Self::Entropy(error) => write!(f, "SILK pitch entropy error: {error}"),
        }
    }
}
impl std::error::Error for PitchError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Entropy(error) => Some(error),
            _ => None,
        }
    }
}
impl From<RangeError> for PitchError {
    fn from(error: RangeError) -> Self {
        Self::Entropy(error)
    }
}

/// Reconstructed pitch/LTP parameters and the two-bit excitation seed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FramePitch {
    lag_index: Option<i16>,
    contour: u8,
    periodicity: u8,
    scale_index: u8,
    scale: i16,
    seed: u8,
    indices: [u8; 4],
    lags: [u16; 4],
    coefficients: [[i16; 5]; 4],
    count: usize,
}
impl FramePitch {
    /// Unclamped lag index for voiced frames; absent otherwise. Malformed delta
    /// chains can leave the normal absolute-index range; reconstructed lags clamp.
    pub fn lag_index(&self) -> Option<i16> {
        self.lag_index
    }
    /// Encoded contour index; zero for inactive/unvoiced frames.
    pub fn contour_index(&self) -> u8 {
        self.contour
    }
    /// LTP codebook selector (0..=2); zero for inactive/unvoiced frames.
    pub fn periodicity_index(&self) -> u8 {
        self.periodicity
    }
    /// Encoded scale index, or implicit zero when not transmitted.
    pub fn scale_index(&self) -> u8 {
        self.scale_index
    }
    /// Q14 LTP scale, zero for inactive/unvoiced frames. Implicit index zero
    /// in voiced frames means 15565, not a zero scale.
    pub fn scale_q14(&self) -> i16 {
        self.scale
    }
    /// Seed (0..=3) for the later excitation decoder, present for every signal type.
    pub fn seed(&self) -> u8 {
        self.seed
    }
    /// Encoded LTP vector index per subframe; zero for inactive/unvoiced frames.
    pub fn ltp_indices(&self) -> &[u8] {
        &self.indices[..self.count]
    }
    /// Pitch periods in internal-rate samples; zero for inactive/unvoiced frames.
    pub fn lags(&self) -> &[u16] {
        &self.lags[..self.count]
    }
    /// Five signed Q14 LTP coefficients per subframe; zeros when not voiced.
    pub fn coefficients_q14(&self) -> &[[i16; 5]] {
        &self.coefficients[..self.count]
    }
}

/// Conditional lag history for one SILK channel.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PitchState {
    rate: Option<SilkRate>,
    previous: Option<i16>,
}
impl PitchState {
    /// Prior lag index only when the immediately preceding frame was voiced.
    pub fn previous_lag_index(&self) -> Option<i16> {
        self.previous
    }
    /// Clear history after a channel reset. Sampling-rate changes reset it
    /// automatically. Packet-loss concealment policy belongs to the audio decoder.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Read pitch/LTP and seed from a cursor positioned just after NLSF symbols.
    ///
    /// Inactive/unvoiced frames consume only the seed and clear voiced history.
    /// Conditional decoding without voiced history uses an absolute lag.
    /// Failure changes neither entropy nor history. For a complete frame, callers
    /// must commit cloned states only after excitation decoding also succeeds.
    pub fn decode(
        &mut self,
        decoder: &mut RangeDecoder<'_>,
        rate: SilkRate,
        duration: FrameDuration,
        signal: SignalType,
        coding: PitchCoding,
    ) -> Result<FramePitch, PitchError> {
        let mut next = decoder.clone();
        let count = if duration == FrameDuration::Ms10 {
            2
        } else {
            4
        };
        let mut frame = FramePitch {
            lag_index: None,
            contour: 0,
            periodicity: 0,
            scale_index: 0,
            scale: 0,
            seed: 0,
            indices: [0; 4],
            lags: [0; 4],
            coefficients: [[0; 5]; 4],
            count,
        };
        if signal == SignalType::Voiced {
            let mut lag = None;
            if coding == PitchCoding::Conditional && self.rate == Some(rate) {
                if let Some(previous) = self.previous {
                    let delta = next.decode_icdf(&DELTA, 8)? as i16;
                    if delta != 0 {
                        lag = Some(previous.wrapping_add(delta - 9));
                    }
                }
            }
            let lag = match lag {
                Some(value) => value,
                None => {
                    (next.decode_icdf(&LAG, 8)? as i16 * (rate.khz() as i16 / 2))
                        + next.decode_icdf(rate.low_bits(), 8)? as i16
                }
            };
            frame.lag_index = Some(lag);
            let (contour, offsets) = rate.contour(duration);
            frame.contour = next.decode_icdf(contour, 8)? as u8;
            for i in 0..count {
                let period = 2 * rate.khz()
                    + i32::from(lag)
                    + i32::from(offsets[i * contour.len() + usize::from(frame.contour)]);
                frame.lags[i] = period.clamp(2 * rate.khz(), 18 * rate.khz()) as u16;
            }
            frame.periodicity = next.decode_icdf(&PERIODICITY, 8)? as u8;
            let (icdf, coefficients): (&[u8], &[i8]) = match frame.periodicity {
                0 => (&GAIN0, &COEFFICIENTS0),
                1 => (&GAIN1, &COEFFICIENTS1),
                _ => (&GAIN2, &COEFFICIENTS2),
            };
            for i in 0..count {
                let index = next.decode_icdf(icdf, 8)?;
                frame.indices[i] = index as u8;
                for j in 0..5 {
                    frame.coefficients[i][j] = i16::from(coefficients[index * 5 + j]) * 128;
                }
            }
            if coding == PitchCoding::Independent {
                frame.scale_index = next.decode_icdf(&SCALE, 8)? as u8;
            }
            frame.scale = SCALES_Q14[usize::from(frame.scale_index)];
        }
        frame.seed = next.decode_icdf(&UNIFORM4, 8)? as u8;
        if !next.is_within_budget() {
            return Err(PitchError::BitBudgetExceeded);
        }
        self.rate = Some(rate);
        self.previous = frame.lag_index;
        *decoder = next;
        Ok(frame)
    }
}
