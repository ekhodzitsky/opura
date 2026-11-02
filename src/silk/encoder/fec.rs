//! Pending SILK LBRR frames, fixed-bandwidth eligibility and bit accounting.
use super::{
    SilkEncodeError,
    channel::{FrameError, RedundancyControls, RedundantFrame},
    mono::PacketDuration,
    stereo::encode_prefix,
};
use crate::{
    range::{EncoderError, RangeEncoder},
    silk::pitch::{PitchCoding, SilkRate},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct StereoPrefix {
    pub indices: [[u8; 3]; 2],
    pub mid_only: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct FecState<const CHANNELS: usize> {
    pub enabled: bool,
    previous_enabled: bool,
    generating: bool,
    gain_increase: u8,
    duration: Option<PacketDuration>,
    pending: [[Option<RedundantFrame>; 3]; CHANNELS],
    pub stereo: [Option<StereoPrefix>; 3],
    pub used_bits: i32,
    pub sent: [bool; CHANNELS],
    pub dropped: bool,
}
impl<const CHANNELS: usize> Default for FecState<CHANNELS> {
    fn default() -> Self {
        Self {
            enabled: false,
            previous_enabled: false,
            generating: false,
            gain_increase: 7,
            duration: None,
            pending: std::array::from_fn(|_| std::array::from_fn(|_| None)),
            stereo: [None; 3],
            used_bits: 0,
            sent: [false; CHANNELS],
            dropped: false,
        }
    }
}
impl<const CHANNELS: usize> FecState<CHANNELS> {
    pub fn has_pending(&self, duration: PacketDuration) -> bool {
        self.duration == Some(duration) && self.pending.iter().flatten().any(Option::is_some)
    }
    pub fn begin(&mut self, duration: PacketDuration, rate: SilkRate, bitrate: i32, loss: u8) {
        if self.duration != Some(duration) {
            self.clear_pending();
        }
        self.duration = Some(duration);
        let threshold = match rate {
            SilkRate::Khz8 => 12000,
            SilkRate::Khz12 => 14000,
            SilkRate::Khz16 => 16000,
        } + if self.previous_enabled { -1000 } else { 1000 };
        // Native fixed-point threshold, evaluated at the requested fixed bandwidth.
        // An explicit bandwidth is never silently reduced just to enable FEC.
        let threshold = (threshold * (125 - i32::from(loss.min(25))) * 655) >> 16;
        self.generating = self.enabled && loss > 0 && bitrate / CHANNELS as i32 > threshold;
        self.gain_increase = if self.previous_enabled {
            (7 - ((i32::from(loss) * 13107) >> 16)).max(3) as u8
        } else {
            7
        };
        self.previous_enabled = self.generating;
        self.sent = [false; CHANNELS];
        self.dropped = false;
    }
    pub(super) fn clear_pending(&mut self) {
        self.pending = std::array::from_fn(|_| std::array::from_fn(|_| None));
        self.stereo = [None; 3];
    }
    pub fn prefix<F>(
        &mut self,
        writer: &mut RangeEncoder,
        include: bool,
        mut encode: F,
    ) -> Result<(), FrameError>
    where
        F: FnMut(usize, &mut RangeEncoder, &RedundantFrame, PitchCoding) -> Result<(), FrameError>,
    {
        let frames = self.duration.ok_or(FrameError::InvalidControls)?.frames();
        let start = writer.tell();
        if include {
            let masks: [usize; CHANNELS] = std::array::from_fn(|ch| {
                self.pending[ch][..frames]
                    .iter()
                    .enumerate()
                    .fold(0, |m, (i, f)| m | (usize::from(f.is_some()) << i))
            });
            for (ch, &mask) in masks.iter().enumerate() {
                self.sent[ch] = mask != 0;
                if mask != 0 && frames > 1 {
                    writer
                        .encode_icdf(
                            mask - 1,
                            if frames == 2 {
                                &[203, 150, 0]
                            } else {
                                &[215, 195, 166, 125, 110, 82, 0]
                            },
                            8,
                        )
                        .map_err(SilkEncodeError::from)?;
                }
            }
            for i in 0..frames {
                for ch in 0..CHANNELS {
                    if let Some(frame) = &self.pending[ch][i] {
                        if CHANNELS == 2 && ch == 0 {
                            let prefix = self.stereo[i].ok_or(FrameError::InvalidControls)?;
                            encode_prefix(
                                writer,
                                prefix.indices,
                                prefix.mid_only,
                                masks[1] & (1 << i) != 0,
                            )
                            .map_err(|e| match e {
                                super::stereo::StereoEncodeError::Entropy(e) => {
                                    FrameError::from(SilkEncodeError::from(e))
                                }
                                _ => FrameError::InvalidControls,
                            })?;
                        }
                        let coding = if i > 0 && self.pending[ch][i - 1].is_some() {
                            PitchCoding::Conditional
                        } else {
                            PitchCoding::Independent
                        };
                        encode(ch, writer, frame, coding)?;
                    }
                }
            }
        } else {
            self.dropped = self.pending.iter().flatten().any(Option::is_some);
        }
        let bits = (writer.tell() - start) as i32;
        self.used_bits = if bits < 10 {
            0
        } else if self.used_bits < 10 {
            bits
        } else {
            (self.used_bits + bits) / 2
        };
        self.clear_pending();
        Ok(())
    }
    pub fn controls(&self, ch: usize, index: usize) -> Option<RedundancyControls> {
        self.generating.then(|| RedundancyControls {
            gain_increase: self.gain_increase,
            reset_gain_history: index == 0 || self.pending[ch][index - 1].is_none(),
        })
    }
    pub fn record(&mut self, ch: usize, index: usize, frame: Option<&RedundantFrame>) {
        self.pending[ch][index] = frame.cloned();
    }
    pub fn suppress(&mut self) {
        self.sent = [false; CHANNELS];
    }
}
pub(super) fn budget_failure(error: &FrameError) -> bool {
    matches!(
        error,
        FrameError::BitBudgetExceeded
            | FrameError::Serialization(SilkEncodeError::Entropy(EncoderError::CapacityExhausted))
    )
}

#[cfg(test)]
#[path = "fec_tests.rs"]
mod tests;
