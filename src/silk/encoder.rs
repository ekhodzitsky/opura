//! SILK PCM analysis, quantization and frame bitstream components.
//!
//! Serializes quantized side information and signed noise-shaped pulses into an
//! existing Opus entropy writer. This is not yet a PCM-to-SILK encoder. Header,
//! stereo/LBRR ordering and packet finalization belong to the enclosing encoder.
//! Adapted from Opus v1.6.1; see THIRD_PARTY_NOTICES.md.
pub mod analysis;
mod math;
pub mod nsq;
mod pulses;
use super::{
    gains::{DELTA_ICDF, FrameActivity, FrameDuration, GAIN_ICDF, SignalType},
    nlsf::NlsfOrder,
    pitch::{PitchCoding, SilkRate, tables as pitch_tables},
};
use crate::range::{EncoderError, RangeEncoder};
use std::fmt;

/// Quantized side information of one internal 10/20 ms SILK channel frame.
/// Only the active subframes/order are read from fixed-size arrays.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FrameIndices {
    /// Signal classification, consistent with the enclosing VAD/LBRR context.
    pub signal: SignalType,
    /// Quantization offset selector, zero or one.
    pub offset: u8,
    /// First gain index (absolute or delta), then subframe delta indices.
    pub gains: [u8; 4],
    /// First-stage NLSF codebook index followed by signed residual indices.
    pub nlsf: [i8; 17],
    /// NLSF interpolation factor 0..=4; must be four for a 10 ms frame.
    pub interpolation_q2: u8,
    /// Voiced lag index, less than 16 times the internal rate in kHz.
    pub lag: u16,
    /// Voiced pitch-contour index, selected by rate and duration.
    pub contour: u8,
    /// Voiced LTP codebook selector, 0..=2.
    pub periodicity: u8,
    /// Voiced per-subframe LTP codebook indices.
    pub ltp: [u8; 4],
    /// Voiced LTP scale index 0..=2; zero unless independently scaled.
    pub ltp_scale: u8,
    /// Excitation random seed, 0..=3.
    pub seed: u8,
}
/// Geometry and coding context supplied by packet orchestration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FrameContext {
    /// Internal SILK rate, independent of the external PCM rate.
    pub rate: SilkRate,
    /// Duration of this internal frame.
    pub duration: FrameDuration,
    /// VAD/LBRR context already signaled in the packet header.
    pub activity: FrameActivity,
    /// Conditional/independent channel coding, including the LTP scale rule.
    pub coding: PitchCoding,
}
impl FrameContext {
    fn subframes(self) -> usize {
        if self.duration == FrameDuration::Ms10 {
            2
        } else {
            4
        }
    }
    fn order(self) -> NlsfOrder {
        if self.rate == SilkRate::Khz16 {
            NlsfOrder::Order16
        } else {
            NlsfOrder::Order10
        }
    }
    fn samples(self) -> usize {
        self.rate.khz() as usize * self.subframes() * 5
    }
}
/// A frame encoding failure leaves both entropy and channel context unchanged.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum SilkEncodeError {
    /// A side-information index or signal/activity combination is invalid.
    InvalidIndices,
    /// Pulse length differs from the internal rate/duration geometry.
    InvalidPulseLength,
    /// Conditional coding requires prior channel history at the same rate.
    MissingHistory,
    /// The entropy writer rejected an operation or lacked capacity.
    Entropy(EncoderError),
}
impl fmt::Display for SilkEncodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidIndices => f.write_str("invalid SILK encoding indices/context"),
            Self::InvalidPulseLength => {
                f.write_str("SILK pulse length does not match frame geometry")
            }
            Self::MissingHistory => {
                f.write_str("conditional SILK encoding requires same-rate history")
            }
            Self::Entropy(e) => write!(f, "SILK encoding: {e}"),
        }
    }
}
impl std::error::Error for SilkEncodeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Entropy(e) => Some(e),
            _ => None,
        }
    }
}
impl From<EncoderError> for SilkEncodeError {
    fn from(e: EncoderError) -> Self {
        Self::Entropy(e)
    }
}
/// Entropy prediction context for one encoded channel, including LBRR traversal.
/// Enclosing packet code must preserve separate normal/redundancy coding order.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct FrameEncoderState {
    rate: Option<SilkRate>,
    previous_signal: Option<SignalType>,
    previous_lag: u16,
}
impl FrameEncoderState {
    /// Forget previous signal/lag and reject conditional coding until a new frame.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Encode all quantized fields and signed pulses for one internal frame.
    ///
    /// The caller's writer must already contain header/stereo fields. This does
    /// not finalize it. Input pulses are unchanged, including 120-sample padding
    /// handled internally. Errors, including a late pulse-capacity failure,
    /// commit neither the writer nor this state. This validates wire indices;
    /// PCM analysis and perceptual/bitrate decisions are outside this API.
    pub fn encode(
        &mut self,
        encoder: &mut RangeEncoder,
        context: FrameContext,
        indices: &FrameIndices,
        samples: &[i8],
    ) -> Result<(), SilkEncodeError> {
        self.validate(context, indices, samples)?;
        let mut next = encoder.clone();
        self.indices(&mut next, context, indices)?;
        pulses::encode(&mut next, indices.signal, indices.offset, samples)?;
        self.rate = Some(context.rate);
        self.previous_signal = Some(indices.signal);
        if indices.signal == SignalType::Voiced {
            self.previous_lag = indices.lag;
        }
        *encoder = next;
        Ok(())
    }
    fn validate(
        &self,
        c: FrameContext,
        i: &FrameIndices,
        pulses: &[i8],
    ) -> Result<(), SilkEncodeError> {
        if pulses.len() != c.samples() {
            return Err(SilkEncodeError::InvalidPulseLength);
        }
        if c.coding == PitchCoding::Conditional && self.rate != Some(c.rate) {
            return Err(SilkEncodeError::MissingHistory);
        }
        let inactive = i.signal == SignalType::Inactive;
        let gain_max = if c.coding == PitchCoding::Conditional {
            40
        } else {
            63
        };
        let order = c.order().len();
        if (c.activity == FrameActivity::Inactive) != inactive
            || i.offset > 1
            || i.seed > 3
            || i.gains[0] > gain_max
            || i.gains[1..c.subframes()].iter().any(|&v| v > 40)
            || !(0..32).contains(&i.nlsf[0])
            || i.nlsf[1..=order].iter().any(|&v| !(-10..=10).contains(&v))
            || i.interpolation_q2 > 4
            || (c.duration == FrameDuration::Ms10 && i.interpolation_q2 != 4)
        {
            return Err(SilkEncodeError::InvalidIndices);
        }
        if i.signal == SignalType::Voiced {
            let contour_count = c.rate.contour(c.duration).0.len();
            if i.lag >= 16 * c.rate.khz() as u16
                || usize::from(i.contour) >= contour_count
                || i.periodicity > 2
                || i.ltp_scale > 2
                || (c.coding != PitchCoding::Independent && i.ltp_scale != 0)
            {
                return Err(SilkEncodeError::InvalidIndices);
            }
            let limit = 8 << i.periodicity;
            if i.ltp[..c.subframes()].iter().any(|&v| v >= limit) {
                return Err(SilkEncodeError::InvalidIndices);
            }
        }
        Ok(())
    }
    fn indices(
        &self,
        enc: &mut RangeEncoder,
        c: FrameContext,
        i: &FrameIndices,
    ) -> Result<(), EncoderError> {
        let signal = signal_index(i.signal);
        let type_offset = 2 * signal + usize::from(i.offset);
        if i.signal == SignalType::Inactive {
            enc.encode_icdf(type_offset, &[230, 0], 8)?;
        } else {
            enc.encode_icdf(type_offset - 2, &[232, 158, 10, 0], 8)?;
        }
        if c.coding == PitchCoding::Conditional {
            enc.encode_icdf(usize::from(i.gains[0]), &DELTA_ICDF, 8)?;
        } else {
            enc.encode_icdf(usize::from(i.gains[0] >> 3), &GAIN_ICDF[signal], 8)?;
            enc.encode_icdf(usize::from(i.gains[0] & 7), &pitch_tables::UNIFORM8, 8)?;
        }
        for &gain in &i.gains[1..c.subframes()] {
            enc.encode_icdf(usize::from(gain), &DELTA_ICDF, 8)?;
        }
        let count = c.order().len();
        let cb = c.order().codebook();
        let first = i.nlsf[0] as usize;
        let offset = (signal >> 1) * 32;
        enc.encode_icdf(first, &cb.icdf1[offset..offset + 32], 8)?;
        let (entropy, _) = cb.unpack(first, count);
        for (j, &index) in i.nlsf[1..=count].iter().enumerate() {
            let value = i32::from(index);
            enc.encode_icdf(
                (value + 4).clamp(0, 8) as usize,
                &cb.icdf2[entropy[j]..entropy[j] + 9],
                8,
            )?;
            if value.abs() >= 4 {
                enc.encode_icdf((value.abs() - 4) as usize, &[100, 40, 16, 7, 3, 1, 0], 8)?;
            }
        }
        if c.duration == FrameDuration::Ms20 {
            enc.encode_icdf(usize::from(i.interpolation_q2), &[243, 221, 192, 181, 0], 8)?;
        }
        if i.signal == SignalType::Voiced {
            let mut absolute = true;
            if c.coding == PitchCoding::Conditional
                && self.previous_signal == Some(SignalType::Voiced)
            {
                let delta = i32::from(i.lag) - i32::from(self.previous_lag);
                let symbol = if (-8..=11).contains(&delta) {
                    absolute = false;
                    (delta + 9) as usize
                } else {
                    0
                };
                enc.encode_icdf(symbol, &pitch_tables::DELTA, 8)?;
            }
            if absolute {
                let divisor = c.rate.khz() as usize / 2;
                enc.encode_icdf(usize::from(i.lag) / divisor, &pitch_tables::LAG, 8)?;
                enc.encode_icdf(usize::from(i.lag) % divisor, c.rate.low_bits(), 8)?;
            }
            enc.encode_icdf(usize::from(i.contour), c.rate.contour(c.duration).0, 8)?;
            enc.encode_icdf(usize::from(i.periodicity), &pitch_tables::PERIODICITY, 8)?;
            let table: &[u8] = match i.periodicity {
                0 => &pitch_tables::GAIN0,
                1 => &pitch_tables::GAIN1,
                _ => &pitch_tables::GAIN2,
            };
            for &ltp in &i.ltp[..c.subframes()] {
                enc.encode_icdf(usize::from(ltp), table, 8)?;
            }
            if c.coding == PitchCoding::Independent {
                enc.encode_icdf(usize::from(i.ltp_scale), &pitch_tables::SCALE, 8)?;
            }
        }
        enc.encode_icdf(usize::from(i.seed), &pitch_tables::UNIFORM4, 8)
    }
}
fn signal_index(signal: SignalType) -> usize {
    match signal {
        SignalType::Inactive => 0,
        SignalType::Unvoiced => 1,
        SignalType::Voiced => 2,
    }
}
