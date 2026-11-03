//! Borrowed SILK packet coding. The caller owns PCM conversion, all histories
//! and the whole-stream transaction; this engine never creates encoder banks.
use super::{
    bandwidth::{BandwidthError, PacketBandwidth},
    channel::{FrameControls, FrameError},
    dtx::Decision,
    mono::{MonoError, MonoOptions, MonoPacket, PacketDuration, packet, plc_packet, target_snr},
    state::{AnalysisControls, AnalyzedFrame, SpeechState},
    stereo::StereoPacketError,
};
use crate::{
    range::{EncoderError, RangeEncoder},
    silk::{
        gains::FrameDuration,
        pitch::{PitchCoding, SilkRate},
    },
};

#[derive(Clone, Copy)]
pub(crate) struct PacketControls {
    pub policy: Option<PayloadPolicy>,
    pub duration: PacketDuration,
    pub packet_bytes: usize,
    pub options: MonoOptions,
    pub digital_silence: bool,
    pub include_fec: bool,
    pub channels: usize,
    pub to_mono: bool,
}
#[derive(Clone, Copy)]
pub(crate) struct PayloadPolicy {
    pub rate: i32,
    pub fec: bool,
}

/// Prepared speech payload on a staged stream, before outer redundancy syntax.
/// The owner commits all histories only after the complete packet succeeds.
pub(crate) struct PreparedPacket {
    pub writer: RangeEncoder,
    pub decision: Decision,
    pub toc: u8,
    pub overflow: bool,
    packet_bytes: usize,
    variable: bool,
}
impl PreparedPacket {
    pub fn finish(
        mut self,
        finish: impl for<'a> FnOnce(&'a mut RangeEncoder) -> Result<&'a [u8], EncoderError>,
    ) -> Result<MonoPacket, EncoderError> {
        let length = self.writer.tell().div_ceil(8) as usize;
        if !self.overflow && length >= self.packet_bytes {
            return Err(EncoderError::CapacityExhausted);
        }
        let range = if self.overflow {
            0
        } else {
            self.writer.range()
        };
        let payload = if self.overflow {
            &[0][..]
        } else {
            self.writer.shrink(length)?;
            finish(&mut self.writer)?
        };
        Ok(if self.decision != Decision::Transmit {
            packet(self.toc, &[], 1, 0)
        } else if self.overflow {
            plc_packet(self.toc, if self.variable { 2 } else { self.packet_bytes })
        } else {
            packet(
                self.toc,
                payload,
                if self.variable {
                    payload.len() + 1
                } else {
                    self.packet_bytes
                },
                range,
            )
        })
    }
}

pub(crate) fn mono<const N: usize>(
    speech: &mut SpeechState<N>,
    bandwidth: &mut PacketBandwidth<N>,
    internal: SilkRate,
    pcm: &[i16],
    controls: PacketControls,
) -> Result<(MonoPacket, Decision), MonoError> {
    let prepared = prepare_mono(speech, bandwidth, internal, pcm, controls)?;
    let decision = prepared.decision;
    Ok((prepared.finish(RangeEncoder::finish)?, decision))
}
pub(crate) fn stereo(
    speech: &mut SpeechState<2>,
    bandwidth: &mut PacketBandwidth<2>,
    internal: SilkRate,
    pcm: &[[i16; 960]; 2],
    controls: PacketControls,
) -> Result<(MonoPacket, Decision), StereoPacketError> {
    let prepared = prepare_stereo(speech, bandwidth, internal, pcm, controls)?;
    let decision = prepared.decision;
    Ok((prepared.finish(RangeEncoder::finish)?, decision))
}

pub(crate) fn prepare_mono<const N: usize>(
    speech: &mut SpeechState<N>,
    bandwidth: &mut PacketBandwidth<N>,
    internal: SilkRate,
    pcm: &[i16],
    controls: PacketControls,
) -> Result<PreparedPacket, MonoError> {
    if controls.channels != 1 || controls.to_mono {
        return Err(FrameError::InvalidControls.into());
    }
    prepare(
        speech,
        bandwidth,
        internal,
        [pcm, &[]],
        controls,
        |s, r, d, p, c| s.analyze_mono(r, d, p[0], c).map_err(MonoError::from),
        |_, _| Ok(()),
    )
}

pub(crate) fn prepare_stereo(
    speech: &mut SpeechState<2>,
    bandwidth: &mut PacketBandwidth<2>,
    internal: SilkRate,
    pcm: &[[i16; 960]; 2],
    controls: PacketControls,
) -> Result<PreparedPacket, StereoPacketError> {
    let length = internal.khz() as usize * controls.duration.milliseconds() as usize;
    prepare(
        speech,
        bandwidth,
        internal,
        [&pcm[0][..length], &pcm[1][..length]],
        controls,
        |s, r, d, p, c| {
            if controls.channels == 2 {
                s.analyze_stereo(r, d, p, c)
                    .map_err(StereoPacketError::from)
            } else {
                s.analyze_mono(r, d, p[0], c)
                    .map_err(StereoPacketError::from)
            }
        },
        |f, w| f.encode_prefix(w).map_err(StereoPacketError::from),
    )
}

fn prepare<const N: usize, E, A, P>(
    speech: &mut SpeechState<N>,
    bandwidth: &mut PacketBandwidth<N>,
    internal: SilkRate,
    pcm: [&[i16]; 2],
    controls: PacketControls,
    mut analyze: A,
    mut prefix: P,
) -> Result<PreparedPacket, E>
where
    E: From<FrameError> + From<BandwidthError> + From<EncoderError>,
    A: FnMut(
        &mut SpeechState<N>,
        SilkRate,
        FrameDuration,
        [&[i16]; 2],
        AnalysisControls,
    ) -> Result<AnalyzedFrame, E>,
    P: FnMut(&AnalyzedFrame, &mut RangeEncoder) -> Result<(), E>,
{
    let length = internal.khz() as usize * controls.duration.milliseconds() as usize;
    if (controls.to_mono && controls.channels != 2)
        || !(1..=N.min(2)).contains(&controls.channels)
        || !(3..=1276).contains(&controls.packet_bytes)
        || controls.options.complexity > 10
        || controls.options.packet_loss_percent > 100
        || controls
            .policy
            .is_some_and(|p| !(1..=1024000).contains(&p.rate))
        || pcm[0].len() != length
        || (controls.channels == 2 && pcm[1].len() != length)
    {
        return Err(FrameError::InvalidControls.into());
    }
    let PacketControls {
        duration,
        packet_bytes,
        options,
        digital_silence,
        include_fec,
        ..
    } = controls;
    let ms = duration.milliseconds() as usize;
    let frames = duration.frames();
    let frame_samples = ms * internal.khz() as usize / frames;
    let maximum = bandwidth.maximum;
    let bitrate = controls
        .policy
        .map_or((packet_bytes as i32 - 1) * 8 * 1000 / ms as i32, |p| p.rate);
    let nominal_bits = bitrate * ms as i32 / 1000;

    let mut writer = RangeEncoder::new(1275)?;
    let wire_channels = controls.channels;
    let header_bits = (frames as u32 + 1) * wire_channels as u32;
    writer.encode_icdf(0, &[(256 - (256 >> header_bits)) as u8, 0], 8)?;
    speech.fec.begin_for_channels(
        duration,
        internal,
        bitrate,
        options.packet_loss_percent,
        controls
            .policy
            .map(|p| p.fec)
            .or_else(|| bandwidth.fec_override()),
        wire_channels,
    )?;
    let channels = &mut speech.channels;
    speech.fec.prefix_for_channels(
        &mut writer,
        include_fec,
        wire_channels,
        |ch, writer, frame, coding| channels[ch].encode_redundant(writer, frame, coding),
    )?;
    let per_frame = (nominal_bits - speech.fec.used_bits) / frames as i32;
    let mut flags = [[false; 3]; 2];
    for index in 0..frames {
        let mut target = per_frame
            * if duration == PacketDuration::Ms10 {
                100
            } else {
                50
            }
            - speech.excess * 2;
        if index > 0 {
            target -= (writer.tell() as i32 - speech.fec.used_bits - per_frame * index as i32) * 2;
        }
        target = target.clamp(bitrate.min(5000), bitrate.max(5000));
        let start = index * frame_samples;
        let end = start + frame_samples;
        let analyzed = analyze(
            speech,
            internal,
            duration.frame_duration(),
            [
                &pcm[0][start..end],
                if wire_channels == 2 {
                    &pcm[1][start..end]
                } else {
                    &[]
                },
            ],
            AnalysisControls {
                native_activity: controls.policy.is_some(),
                target_rate: target,
                digital_silence,
                to_mono: controls.to_mono,
            },
        )?;
        let mid_activity = analyzed.activities[0].ok_or(FrameError::InvalidControls)?;
        let side_activity = analyzed.activities[1];
        flags[0][index] = analyzed.active(0);
        flags[1][index] = analyzed.active(1);
        speech.fec.stereo[index] = analyzed.stereo;
        prefix(&analyzed, &mut writer)?;
        let limit = match (frames, index) {
            (2, 0) => maximum * 3 / 5,
            (3, 0) => maximum * 2 / 5,
            (3, 1) => maximum * 3 / 4,
            _ => maximum,
        };
        for (channel, channel_flags) in flags.iter().enumerate().take(wire_channels) {
            if channel == 1 && analyzed.mid_only() {
                continue;
            }
            let activity = if channel == 0 {
                mid_activity
            } else {
                side_activity.ok_or(FrameError::InvalidControls)?
            };
            let mut cap = limit;
            let mut fill = !options.variable_bitrate && index + 1 == frames;
            if channel == 0 && analyzed.rates[1] > 0 {
                fill = false;
                cap = cap.saturating_sub(maximum / (frames as u32 * 2));
            }
            let coding = if index == 0 {
                PitchCoding::Independent
            } else if channel == 1 && speech.previous_mid_only {
                PitchCoding::IndependentWithoutScaling
            } else {
                PitchCoding::Conditional
            };
            let fec_controls = speech.fec.controls(channel, index);
            let hard = if bandwidth.automatic || controls.policy.is_some() {
                writer.capacity() as u32 * 8
            } else if fec_controls.is_some() || speech.fec.sent.iter().any(|&v| v) {
                if channel == 0 && analyzed.rates[1] > 0 {
                    maximum
                        .saturating_sub(maximum.saturating_sub(writer.tell()) / (frames as u32 * 2))
                } else {
                    maximum
                }
            } else {
                cap
            };
            let frame = if channel == 0 {
                &analyzed.pcm[0][..analyzed.length]
            } else {
                &analyzed.pcm[1][..analyzed.length]
            };
            let mut tapered = [0i16; 320];
            tapered[..frame.len()].copy_from_slice(frame);
            bandwidth.transitions[channel].filter(
                internal,
                duration.frame_duration(),
                &mut tapered[..frame.len()],
            )?;
            let encoded = speech.channels[channel].encode_with_redundancy(
                &mut writer,
                &tapered[..frame.len()],
                FrameControls {
                    duration: duration.frame_duration(),
                    complexity: options.complexity,
                    snr_db_q7: target_snr(
                        internal,
                        duration.frame_duration(),
                        analyzed.rates[channel],
                    ),
                    activity,
                    active: channel_flags[index],
                    constant_bitrate: !options.variable_bitrate,
                    fill_budget: fill,
                    coding,
                    max_bits: cap,
                    packet_loss_percent: options.packet_loss_percent,
                    frames_per_packet: frames as u8,
                    redundancy: speech.fec.sent[channel],
                },
                hard,
                fec_controls,
            )?;
            speech.fec.record(channel, index, encoded.redundancy());
        }
        speech.finish_analysis(&analyzed)?;
    }
    let mut header = 0;
    for (channel, has_fec) in flags.into_iter().zip(speech.fec.sent).take(wire_channels) {
        for &flag in &channel[..frames] {
            header = (header << 1) | u32::from(flag);
        }
        header = (header << 1) | u32::from(has_fec);
    }
    writer.patch_initial_bits(header, header_bits)?;
    let encoded_length = writer.tell().div_ceil(8) as usize;
    let overflow = encoded_length >= packet_bytes;
    // Native outer Opus converts a completed SILK overshoot into PLC. Give
    // queued FEC one prefix-free retry first, preserving primary audio.
    if overflow
        && ((controls.policy.is_none() && !bandwidth.automatic)
            || (include_fec && speech.fec.sent.iter().any(|&v| v)))
    {
        return Err(FrameError::BitBudgetExceeded.into());
    }
    let base = match internal {
        SilkRate::Khz8 => 0,
        SilkRate::Khz12 => 4,
        SilkRate::Khz16 => 8,
    };
    let config = base
        + match duration {
            PacketDuration::Ms10 => 0,
            PacketDuration::Ms20 => 1,
            PacketDuration::Ms40 => 2,
            PacketDuration::Ms60 => 3,
        };
    let decision = speech.dtx.finish(duration, wire_channels, digital_silence);
    if decision != Decision::Transmit || overflow {
        speech.fec.suppress();
    }
    let toc = (config << 3) | if wire_channels == 2 { 4 } else { 0 };
    let payload_bits = if decision == Decision::Silk {
        0
    } else {
        encoded_length as i32 * 8
    };
    speech.excess = (speech.excess + payload_bits - nominal_bits).clamp(0, 10000);
    bandwidth.finish(speech.activity, duration);
    Ok(PreparedPacket {
        writer,
        decision,
        toc,
        overflow,
        packet_bytes,
        variable: options.variable_bitrate,
    })
}

#[cfg(test)]
#[path = "packet_tests.rs"]
mod tests;
