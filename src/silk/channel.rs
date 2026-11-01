//! Transactional entropy-to-synthesis decoding of one internal SILK channel.
//!
//! The cursor starts at signal-type symbols, after headers, LBRR ordering and
//! stereo fields. Output precedes CNG/PLC glue, stereo and resampling.
use super::{
    cng::{CngError, CngState},
    excitation::{ExcitationError, decode_excitation},
    gains::{FrameActivity, FrameDuration, GainCoding, GainError, GainState},
    nlsf::{NlsfError, NlsfOrder, NlsfState},
    pitch::{PitchCoding, PitchError, PitchState, SilkRate},
    synthesis::{PcmFrame, SynthesisError, SynthesisState},
};
use crate::range::RangeDecoder;
use std::fmt;

/// A channel-frame failure; all channel history and entropy remain unchanged.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ChannelError {
    /// Signal-type or gain decoding failed, including a conditional start.
    Gains(GainError),
    /// Spectral decoding failed.
    Nlsf(NlsfError),
    /// Pitch/LTP decoding failed.
    Pitch(PitchError),
    /// Pulse/excitation decoding failed.
    Excitation(ExcitationError),
    /// Synthesis parameters were incompatible.
    Synthesis(SynthesisError),
    /// Comfort-noise estimation rejected incompatible decoded geometry.
    Cng(CngError),
}
impl fmt::Display for ChannelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SILK channel frame: {}", self.source_error())
    }
}
impl ChannelError {
    fn source_error(&self) -> &(dyn std::error::Error + 'static) {
        match self {
            Self::Gains(e) => e,
            Self::Nlsf(e) => e,
            Self::Pitch(e) => e,
            Self::Excitation(e) => e,
            Self::Synthesis(e) => e,
            Self::Cng(e) => e,
        }
    }
}
impl std::error::Error for ChannelError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.source_error())
    }
}

/// Histories for one loss-free internal SILK channel, using fixed storage.
///
/// Use separate instances for mid and side channels. A failed frame can be
/// retried without repairing any history. This is not a packet decoder: callers
/// must supply the correct activity, coding and positioned entropy cursor.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ChannelState {
    rate: Option<SilkRate>,
    gains: GainState,
    nlsf: NlsfState,
    pitch: PitchState,
    synthesis: SynthesisState,
    cng: CngState,
}
impl ChannelState {
    /// Forget every history, including the prior synthesis gain and rate.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Decode and synthesize one internal 10/20 ms channel frame atomically.
    ///
    /// `coding` selects both gain and pitch coding; the two independent modes
    /// use absolute first gains. Conditional coding requires a previous frame
    /// at the same rate. Rate changes clear gain, spectral and pitch history;
    /// synthesis clears PCM/LPC history but retains its prior linear gain,
    /// matching the reference rate-switch behavior. Duration changes retain
    /// history. No samples or state are committed if any stage fails.
    ///
    /// Successful decoding checks the entropy budget, not full Opus validity.
    /// The output is raw internal-rate synthesis before CNG/PLC processing.
    pub fn decode(
        &mut self,
        decoder: &mut RangeDecoder<'_>,
        rate: SilkRate,
        duration: FrameDuration,
        activity: FrameActivity,
        coding: PitchCoding,
    ) -> Result<PcmFrame, ChannelError> {
        let mut next = self.clone();
        let mut entropy = decoder.clone();
        if next.rate != Some(rate) {
            next.gains.reset();
            next.nlsf.reset();
            next.pitch.reset();
            next.rate = Some(rate);
        }
        let gains = next
            .gains
            .decode(
                &mut entropy,
                duration,
                activity,
                if coding == PitchCoding::Conditional {
                    GainCoding::Conditional
                } else {
                    GainCoding::Independent
                },
            )
            .map_err(ChannelError::Gains)?;
        let nlsf = next
            .nlsf
            .decode(
                &mut entropy,
                if rate == SilkRate::Khz16 {
                    NlsfOrder::Order16
                } else {
                    NlsfOrder::Order10
                },
                duration,
                gains.signal_type(),
            )
            .map_err(ChannelError::Nlsf)?;
        let pitch = next
            .pitch
            .decode(&mut entropy, rate, duration, gains.signal_type(), coding)
            .map_err(ChannelError::Pitch)?;
        let excitation = decode_excitation(
            &mut entropy,
            rate,
            duration,
            gains.signal_type(),
            gains.offset_type(),
            pitch.seed(),
        )
        .map_err(ChannelError::Excitation)?;
        let pcm = next
            .synthesis
            .synthesize(rate, &gains, &nlsf, &pitch, &excitation)
            .map_err(ChannelError::Synthesis)?;
        next.cng
            .update(rate, &gains, &nlsf, &excitation)
            .map_err(ChannelError::Cng)?;
        *self = next;
        *decoder = entropy;
        Ok(pcm)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn successful_channel_frames_retain_comfort_noise_estimates()
    -> Result<(), Box<dyn std::error::Error>> {
        let data = [0; 1024];
        let mut state = ChannelState::default();
        for rate in [SilkRate::Khz8, SilkRate::Khz12, SilkRate::Khz16] {
            state.decode(
                &mut RangeDecoder::new(&data),
                rate,
                FrameDuration::Ms20,
                FrameActivity::Inactive,
                PitchCoding::Independent,
            )?;
            assert_ne!(state.cng, CngState::default());
            let before = state.clone();
            assert!(
                state
                    .decode(
                        &mut RangeDecoder::new(&[]),
                        rate,
                        FrameDuration::Ms20,
                        FrameActivity::Inactive,
                        PitchCoding::Independent
                    )
                    .is_err()
            );
            assert_eq!(state, before);
        }
        state.reset();
        assert_eq!(state.cng, CngState::default());
        Ok(())
    }
}
