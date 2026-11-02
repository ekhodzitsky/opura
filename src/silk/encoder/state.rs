//! One speech-layer history for SILK-only and Hybrid packet orchestration.
//!
//! Methods operate on the enclosing packet's staged state. Packet owners retain
//! DC/delay, bandwidth permission, entropy framing and budget/commit decisions.
use super::{
    analysis::{SpeechActivity, VoiceActivityDetector},
    channel::{ChannelFrameEncoder, FrameError},
    dtx::DtxState,
    fec::{FecState, StereoPrefix},
    stereo::{self, StereoAnalysisState, StereoControls, StereoEncodeError},
};
use crate::{
    range::RangeEncoder,
    silk::{
        gains::FrameDuration,
        pitch::SilkRate,
        resampler::{EncoderResamplerState, OutputRate},
    },
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SpeechState<const CHANNELS: usize> {
    pub resampler: [EncoderResamplerState; CHANNELS],
    pub vad: [VoiceActivityDetector; CHANNELS],
    pub channels: [ChannelFrameEncoder; CHANNELS],
    pub stereo: StereoAnalysisState,
    pub previous: i16,
    pub previous_mid_only: bool,
    pub activity: u8,
    pub excess: i32,
    pub dtx: DtxState,
    pub fec: FecState<CHANNELS>,
}

#[derive(Clone, Copy)]
pub(crate) struct AnalysisControls {
    pub target_rate: i32,
    pub digital_silence: bool,
    pub to_mono: bool,
}

pub(crate) struct AnalyzedFrame {
    pub pcm: [[i16; 320]; 2],
    pub length: usize,
    pub activities: [Option<SpeechActivity>; 2],
    pub rates: [i32; 2],
    pub stereo: Option<StereoPrefix>,
}
impl AnalyzedFrame {
    pub fn mid_only(&self) -> bool {
        self.stereo.is_some_and(|s| s.mid_only)
    }
    pub fn active(&self, channel: usize) -> bool {
        self.activities
            .get(channel)
            .is_some_and(|a| a.is_some_and(|a| a.probability_q8 >= 13))
    }
    pub fn encode_prefix(&self, writer: &mut RangeEncoder) -> Result<(), StereoEncodeError> {
        if let Some(stereo) = self.stereo {
            stereo::encode_prefix(writer, stereo.indices, stereo.mid_only, self.active(1))?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum StereoFrameError {
    Stereo(StereoEncodeError),
    Frame(FrameError),
}
impl From<StereoEncodeError> for StereoFrameError {
    fn from(error: StereoEncodeError) -> Self {
        Self::Stereo(error)
    }
}
impl From<FrameError> for StereoFrameError {
    fn from(error: FrameError) -> Self {
        Self::Frame(error)
    }
}

impl<const CHANNELS: usize> SpeechState<CHANNELS> {
    pub fn new(input: OutputRate, rate: SilkRate) -> Self {
        Self {
            resampler: std::array::from_fn(|_| EncoderResamplerState::new(input, rate)),
            vad: std::array::from_fn(|_| VoiceActivityDetector::default()),
            channels: std::array::from_fn(|_| ChannelFrameEncoder::new(rate)),
            stereo: StereoAnalysisState::default(),
            previous: 0,
            previous_mid_only: false,
            activity: 0,
            excess: 0,
            dtx: DtxState::default(),
            fec: FecState::default(),
        }
    }
    pub fn reset(&mut self, input: OutputRate, rate: SilkRate) {
        let dtx = self.dtx.enabled;
        let fec = self.fec.enabled;
        *self = Self::new(input, rate);
        self.dtx.enabled = dtx;
        self.fec.enabled = fec;
    }
    pub fn change_rate(&mut self, input: OutputRate, rate: SilkRate) -> Result<(), FrameError> {
        let duration = self.fec.previous_frame_duration();
        for (channel, resampler) in self.channels.iter_mut().zip(&mut self.resampler) {
            channel.change_rate(input, rate, duration, resampler)?;
        }
        self.fec.clear_pending();
        Ok(())
    }
    pub fn analyze_mono(
        &mut self,
        rate: SilkRate,
        duration: FrameDuration,
        input: &[i16],
        controls: AnalysisControls,
    ) -> Result<AnalyzedFrame, FrameError> {
        let length = rate.khz() as usize
            * if duration == FrameDuration::Ms10 {
                10
            } else {
                20
            };
        if input.len() != length || length > 320 {
            return Err(FrameError::InvalidInputLength);
        }
        if !(1..=1024000).contains(&controls.target_rate) {
            return Err(FrameError::InvalidControls);
        }
        let vad = self.vad.first_mut().ok_or(FrameError::InvalidControls)?;
        let mut frame = AnalyzedFrame {
            pcm: [[0; 320]; 2],
            length,
            activities: [None; 2],
            rates: [controls.target_rate, 0],
            stereo: None,
        };
        frame.pcm[0][..length].copy_from_slice(input);
        // Segmenting a packet into frames must not add a delay per frame: this
        // is one persistent sample, also used by the Hybrid mono path.
        for sample in &mut frame.pcm[0][..length] {
            std::mem::swap(sample, &mut self.previous);
        }
        frame.activities[0] = Some(
            vad.analyze(rate, duration, &frame.pcm[0][..length])
                .map_err(FrameError::from)?,
        );
        self.activity_policy(&mut frame, controls.digital_silence);
        Ok(frame)
    }
    fn activity_policy(&mut self, frame: &mut AnalyzedFrame, digital_silence: bool) {
        for (channel, activity) in frame.activities.iter_mut().enumerate() {
            if let Some(activity) = activity {
                if (self.dtx.enabled || self.fec.enabled) && digital_silence {
                    activity.probability_q8 = activity.probability_q8.min(12);
                }
                self.dtx
                    .frame(channel, activity.probability_q8 >= 13 && !digital_silence);
            }
        }
    }
    pub fn finish_analysis(&mut self, frame: &AnalyzedFrame) -> Result<(), FrameError> {
        let activity = frame.activities[0].ok_or(FrameError::InvalidControls)?;
        self.activity = activity.probability_q8;
        self.previous_mid_only = frame.mid_only();
        Ok(())
    }
}
impl SpeechState<2> {
    pub fn analyze_stereo(
        &mut self,
        rate: SilkRate,
        duration: FrameDuration,
        input: [&[i16]; 2],
        controls: AnalysisControls,
    ) -> Result<AnalyzedFrame, StereoFrameError> {
        let stereo = self.stereo.analyze(
            rate,
            duration,
            input[0],
            input[1],
            StereoControls {
                total_rate_bps: controls.target_rate,
                previous_activity_q8: u16::from(self.activity),
                to_mono: controls.to_mono,
            },
        )?;
        if !stereo.mid_only() && self.previous_mid_only {
            self.channels[1].restart_prediction();
        }
        let mut frame = AnalyzedFrame {
            pcm: [[0; 320]; 2],
            length: stereo.mid().len(),
            activities: [None; 2],
            rates: *stereo.rates_bps(),
            stereo: Some(StereoPrefix {
                indices: *stereo.indices(),
                mid_only: stereo.mid_only(),
            }),
        };
        frame.pcm[0][..frame.length].copy_from_slice(stereo.mid());
        frame.pcm[1][..frame.length].copy_from_slice(stereo.side());
        for channel in 0..2 {
            if channel == 1 && stereo.mid_only() {
                continue;
            }
            frame.activities[channel] = Some(
                self.vad[channel]
                    .analyze(rate, duration, &frame.pcm[channel][..frame.length])
                    .map_err(FrameError::from)?,
            );
        }
        self.activity_policy(&mut frame, controls.digital_silence);
        Ok(frame)
    }
}
