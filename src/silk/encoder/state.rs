//! One speech-layer history for SILK-only and Hybrid packet orchestration.
//!
//! Methods operate on the enclosing packet's staged state. Packet owners retain
//! DC/delay, bandwidth permission, entropy framing and budget/commit decisions.
use super::{
    analysis::{SpeechActivity, VoiceActivityDetector},
    channel::{ChannelFrameEncoder, FrameError},
    dtx::DtxState,
    fec::{FecState, StereoPrefix},
    mono::PacketDuration,
    stereo::{self, StereoAnalysisState, StereoControls, StereoEncodeError},
};
use crate::{
    range::RangeEncoder,
    silk::{
        gains::FrameDuration,
        pitch::SilkRate,
        resampler::{EncoderResamplerState, OutputRate, ResampleError},
    },
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SpeechState<const CHANNELS: usize> {
    #[cfg(test)]
    pub mid_input_hash: u64,
    pub resampler: [EncoderResamplerState; CHANNELS],
    pub vad: [VoiceActivityDetector; CHANNELS],
    pub channels: [ChannelFrameEncoder; CHANNELS],
    pub stereo: StereoAnalysisState,
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
            #[cfg(test)]
            mid_input_hash: 14695981039346656037,
            resampler: std::array::from_fn(|_| EncoderResamplerState::new(input, rate)),
            vad: std::array::from_fn(|_| VoiceActivityDetector::default()),
            channels: std::array::from_fn(|_| ChannelFrameEncoder::new(rate)),
            stereo: StereoAnalysisState::default(),
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
        // Native mono and stereo consume the same two-sample mid history.
        self.stereo.buffer_mono(input, &mut frame.pcm[0][..length]);
        frame.activities[0] = Some(
            vad.analyze(rate, duration, &frame.pcm[0][..length])
                .map_err(FrameError::from)?,
        );
        #[cfg(test)]
        self.record_mid_input(&frame);
        self.activity_policy(&mut frame, controls.digital_silence);
        Ok(frame)
    }
    #[cfg(test)]
    fn record_mid_input(&mut self, frame: &AnalyzedFrame) {
        for byte in frame.pcm[0][..frame.length]
            .iter()
            .flat_map(|v| v.to_le_bytes())
        {
            self.mid_input_hash =
                (self.mid_input_hash ^ u64::from(byte)).wrapping_mul(1099511628211);
        }
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
    pub fn begin_channels(&mut self, previous: usize, channels: usize, rate: SilkRate) {
        if channels != previous {
            self.fec.clear_pending();
        }
        if channels == 2 && previous == 1 {
            self.channels[1] = ChannelFrameEncoder::new(rate);
            self.vad[1] = VoiceActivityDetector::default();
            self.dtx.restart_side();
            self.stereo.restart_stereo();
        }
    }
    // Called on staged state, after any rate migration and before frame coding.
    pub fn resample_stereo_input(
        &mut self,
        input: &[i16],
        output: &mut [[i16; 960]; 2],
        rate: OutputRate,
        duration: PacketDuration,
        channels: usize,
        previous: usize,
    ) -> Result<(), ResampleError> {
        let samples = rate.hz() as usize / 1000 * duration.milliseconds() as usize;
        if input.len() != samples * 2
            || !(1..=2).contains(&channels)
            || !(1..=2).contains(&previous)
        {
            return Err(ResampleError::InvalidInputLength);
        }
        let mut planar = [[0i16; 2880]; 2];
        for (i, pair) in input.chunks_exact(2).enumerate() {
            if channels == 2 {
                planar[0][i] = pair[0];
                planar[1][i] = pair[1];
            } else {
                planar[0][i] = ((i32::from(pair[0]) + i32::from(pair[1]) + 1) >> 1) as i16;
            }
        }
        if channels == 2 && previous == 1 {
            // Clone before channel0 consumes any input, including long packets.
            self.resampler[1] = self.resampler[0].clone();
        }
        // Native silk_Encode invokes each resampler once per internal frame.
        // Even overwritten delay-buffer slots must retain that call geometry.
        let frame_samples = samples / duration.frames();
        let mut offset = 0;
        for index in 0..duration.frames() {
            let start = index * frame_samples;
            let end = start + frame_samples;
            let length =
                self.resampler[0].process(&planar[0][start..end], &mut output[0][offset..])?;
            if channels == 2 {
                self.resampler[1].process(&planar[1][start..end], &mut output[1][offset..])?;
            } else if previous == 2 && index == 0 {
                // Only the first internal frame uses the departing side resampler.
                self.resampler[1].process(&planar[0][start..end], &mut output[1])?;
                for i in 0..length {
                    output[0][i] =
                        ((i32::from(output[0][i]) + i32::from(output[1][i])) >> 1) as i16;
                }
            }
            offset += length;
        }
        Ok(())
    }
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
        #[cfg(test)]
        self.record_mid_input(&frame);
        self.activity_policy(&mut frame, controls.digital_silence);
        Ok(frame)
    }
}
