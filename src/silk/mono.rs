//! Mono SILK packets and explicit packet loss to internal-rate PCM.
//!
//! Handles framing, headers, skipped LBRR and internal-frame scheduling. Output
//! precedes mono delay buffering and resampling. No implicit DTX, stereo, Hybrid,
//! or CELT transition reconstruction is implemented.
use super::{
    SilkError,
    channel::{ChannelError, ChannelState},
    decode_header,
    excitation::decode_excitation,
    gains::{FrameActivity, FrameDuration, GainCoding, GainState},
    nlsf::{NlsfOrder, NlsfState},
    pitch::{PitchCoding, PitchState, SilkRate},
};
use crate::{Bandwidth, Mode, Packet, PacketError, range::RangeDecoder};
use std::fmt;

/// Packet decoding failure; the decoder retains its complete preceding state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum MonoError {
    /// Invalid externally delimited Opus packet framing.
    Framing(PacketError),
    /// Only SILK-only mode is supported.
    UnsupportedMode,
    /// Stereo packets require stereo field decoding and reconstruction.
    UnsupportedChannels,
    /// An empty or one-byte compressed frame needs loss/DTX handling.
    MissingPayload,
    /// No successfully decoded SILK packet establishes the internal rate yet.
    MissingHistory,
    /// Loss duration must be 10..=120 ms in multiples of 10 ms.
    InvalidLossDuration,
    /// SILK header decoding failed.
    Header(SilkError),
    /// An internal regular or redundant channel frame failed.
    Channel(ChannelError),
    /// The SILK payload has room for a CELT transition redundancy tail.
    UnsupportedTransition,
}
impl fmt::Display for MonoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Framing(e) => write!(f, "mono SILK packet framing: {e}"),
            Self::UnsupportedMode => f.write_str("mono decoder requires SILK-only mode"),
            Self::UnsupportedChannels => f.write_str("mono decoder requires one encoded channel"),
            Self::MissingHistory => f.write_str("mono SILK concealment needs a preceding packet"),
            Self::InvalidLossDuration => {
                f.write_str("mono SILK loss duration must be 10..120 ms in 10 ms steps")
            }
            Self::MissingPayload => f.write_str(
                "missing SILK payload requires explicit concealment; implicit DTX is unsupported",
            ),
            Self::Header(e) => write!(f, "mono SILK header: {e}"),
            Self::Channel(e) => write!(f, "mono SILK packet: {e}"),
            Self::UnsupportedTransition => {
                f.write_str("SILK/CELT transition redundancy is not implemented")
            }
        }
    }
}
impl std::error::Error for MonoError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Framing(e) => Some(e),
            Self::Header(e) => Some(e),
            Self::Channel(e) => Some(e),
            _ => None,
        }
    }
}
impl From<ChannelError> for MonoError {
    fn from(value: ChannelError) -> Self {
        Self::Channel(value)
    }
}

/// At most 120 ms of signed 16-bit mono PCM at the encoded SILK rate.
///
/// Samples are raw channel synthesis, before the mono delay buffer and
/// output-rate resampler used by the Opus API. They are not aligned with that
/// API's output samples. Explicit losses and recovery include PLC/CNG glue.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MonoPcm {
    samples: [i16; 1920],
    length: usize,
    rate: SilkRate,
    final_range: u32,
}
impl MonoPcm {
    pub(super) fn rate(&self) -> SilkRate {
        self.rate
    }
    /// PCM samples, in packet transmission order.
    pub fn samples(&self) -> &[i16] {
        &self.samples[..self.length]
    }
    /// Encoded internal sampling rate in Hz (8000, 12000 or 16000).
    pub fn sample_rate(&self) -> u32 {
        self.rate.khz() as u32 * 1000
    }
    /// Final entropy range for this result, matching OPUS_GET_FINAL_RANGE.
    ///
    /// Normal decoding uses the last compressed frame's range; FEC uses the
    /// first frame's redundant prefix; explicit concealment returns zero. This is a
    /// conformance diagnostic, not an integrity check on untrusted audio.
    pub fn final_range(&self) -> u32 {
        self.final_range
    }
}

/// Persistent, allocation-free mono SILK packet decoder with explicit loss handling.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MonoDecoder {
    channel: ChannelState,
    last_frame: Option<(SilkRate, FrameDuration)>,
}
impl MonoDecoder {
    /// Forget all state before starting a new independent stream.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Conceal a missing interval at the last successfully decoded SILK rate.
    ///
    /// `duration_ms` must be 10..=120 in 10 ms steps. Scheduling matches Opus:
    /// after a 10 ms Opus frame, each concealed frame is 10 ms; otherwise use
    /// 20 ms frames followed by 10 ms if needed. The result includes PLC/CNG
    /// and recovery bookkeeping, with final range zero. No FEC is decoded.
    ///
    /// A preceding successful packet must establish the internal rate and
    /// frame duration. Invalid duration or missing history leaves state unchanged.
    /// All output and state commit together. Output still precedes mono delay
    /// buffering and resampling, like [`Self::decode`].
    pub fn conceal(&mut self, duration_ms: u16) -> Result<MonoPcm, MonoError> {
        if !(10..=120).contains(&duration_ms) || duration_ms % 10 != 0 {
            return Err(MonoError::InvalidLossDuration);
        }
        let (rate, last_duration) = self.last_frame.ok_or(MonoError::MissingHistory)?;
        let mut next = self.clone();
        let mut output = MonoPcm {
            samples: [0; 1920],
            length: 0,
            rate,
            final_range: 0,
        };
        let mut remaining = duration_ms;
        while remaining != 0 {
            let duration = if last_duration == FrameDuration::Ms10 || remaining == 10 {
                FrameDuration::Ms10
            } else {
                FrameDuration::Ms20
            };
            let pcm = next.channel.conceal(rate, duration)?;
            let end = output.length + pcm.samples().len();
            output.samples[output.length..end].copy_from_slice(pcm.samples());
            output.length = end;
            remaining -= if duration == FrameDuration::Ms10 {
                10
            } else {
                20
            };
        }
        *self = next;
        Ok(output)
    }

    /// Recover the preceding interval using in-band FEC from this packet.
    ///
    /// Like Opus with `decode_fec = 1` and a buffer of one Opus frame's duration,
    /// this method recovers the interval represented by the first compressed
    /// frame, even in a multi-frame packet. Present LBRR frames are decoded;
    /// absent ones use PLC/CNG. The remaining primary audio and later compressed
    /// frames are not consumed. Call [`Self::decode`] on the same packet next
    /// to obtain its current audio and advance the recovered state normally.
    ///
    /// A fresh decoder can use the rate signaled by this packet; missing LBRR
    /// then produces cold concealment. Missing FEC is not a decoding error.
    /// All state/output commit atomically. The returned final range describes
    /// the FEC entropy prefix, matching the upstream FEC call. CELT transition
    /// tails are not interpreted in FEC mode. Stereo, Hybrid, CELT and empty or
    /// one-byte first payloads are rejected. Output precedes mono resampling.
    pub fn decode_fec(&mut self, data: &[u8]) -> Result<MonoPcm, MonoError> {
        let packet = Packet::parse(data).map_err(MonoError::Framing)?;
        let rate = packet_rate(&packet)?;
        if packet.frames()[0].len() <= 1 {
            return Err(MonoError::MissingPayload);
        }
        let (header, mut entropy) = decode_header(&packet, 0).map_err(MonoError::Header)?;
        entropy.allow_silk_zero_extension();
        let duration = if header.frame_duration_ms() == 10 {
            FrameDuration::Ms10
        } else {
            FrameDuration::Ms20
        };
        let flags = header.redundancy(0).ok_or(MonoError::UnsupportedChannels)?;
        let mut next = self.clone();
        let mut output = MonoPcm {
            samples: [0; 1920],
            length: 0,
            rate,
            final_range: 0,
        };
        for (index, &present) in flags.iter().enumerate() {
            let pcm = if present {
                next.channel.decode(
                    &mut entropy,
                    rate,
                    duration,
                    FrameActivity::Redundant,
                    if index > 0 && flags[index - 1] {
                        PitchCoding::Conditional
                    } else {
                        PitchCoding::Independent
                    },
                )?
            } else {
                next.channel.conceal_fec_gap(rate, duration)?
            };
            let end = output.length + pcm.samples().len();
            output.samples[output.length..end].copy_from_slice(pcm.samples());
            output.length = end;
        }
        output.final_range = entropy.range();
        next.last_frame = Some((rate, duration));
        *self = next;
        Ok(output)
    }

    /// Decode exactly one externally delimited Opus packet, without I/O.
    ///
    /// Supports 10/20/40/60 ms SILK Opus frames and multi-frame packets up to
    /// 120 ms. LBRR symbols are consumed but do not produce FEC audio or alter
    /// regular gain, spectral or synthesis history. The first internal frame
    /// of each Opus frame is independent; later internal frames are conditional.
    /// Rate changes follow [`ChannelState::decode`].
    ///
    /// Any error discards all samples and state updates for this packet,
    /// including successful earlier frames. Unsupported inputs are explicit
    /// errors; do not use this API to conceal packet loss or decode transitions
    /// through CELT/Hybrid. Successful decoding is not an integrity guarantee.
    pub fn decode(&mut self, data: &[u8]) -> Result<MonoPcm, MonoError> {
        let packet = Packet::parse(data).map_err(MonoError::Framing)?;
        let rate = packet_rate(&packet)?;
        if packet.frames().iter().any(|frame| frame.len() <= 1) {
            return Err(MonoError::MissingPayload);
        }
        let mut next = self.clone();
        let mut output = MonoPcm {
            samples: [0; 1920],
            length: 0,
            rate,
            final_range: 0,
        };
        for (index, frame) in packet.frames().iter().enumerate() {
            let (header, mut entropy) = decode_header(&packet, index).map_err(MonoError::Header)?;
            entropy.allow_silk_zero_extension();
            let duration = if header.frame_duration_ms() == 10 {
                FrameDuration::Ms10
            } else {
                FrameDuration::Ms20
            };
            let redundancy = header.redundancy(0).ok_or(MonoError::UnsupportedChannels)?;
            skip_redundancy(&mut entropy, rate, duration, redundancy)?;
            let activity = header
                .voice_activity(0)
                .ok_or(MonoError::UnsupportedChannels)?;
            for (internal, &active) in activity.iter().enumerate() {
                let pcm = next.channel.decode(
                    &mut entropy,
                    rate,
                    duration,
                    if active {
                        FrameActivity::Active
                    } else {
                        FrameActivity::Inactive
                    },
                    if internal == 0 {
                        PitchCoding::Independent
                    } else {
                        PitchCoding::Conditional
                    },
                )?;
                // Packet framing bounds total duration to 120 ms, or 1920
                // samples at the maximum internal rate. Channel geometry is typed.
                let end = output.length + pcm.samples().len();
                output.samples[output.length..end].copy_from_slice(pcm.samples());
                output.length = end;
            }
            // opus_decode_frame interprets >=17 remaining whole bits in a
            // SILK-only frame as a CELT transition tail, including its direction
            // bit. Do not silently return incomplete PCM or the wrong final range.
            if entropy.tell() + 17 <= frame.len() as u32 * 8 {
                return Err(MonoError::UnsupportedTransition);
            }
            output.final_range = entropy.range();
        }
        next.last_frame = Some((
            rate,
            if packet.frame_samples() == 480 {
                FrameDuration::Ms10
            } else {
                FrameDuration::Ms20
            },
        ));
        *self = next;
        Ok(output)
    }
}

fn packet_rate(packet: &Packet<'_>) -> Result<SilkRate, MonoError> {
    if packet.mode() != Mode::Silk {
        return Err(MonoError::UnsupportedMode);
    }
    if packet.channels() != 1 {
        return Err(MonoError::UnsupportedChannels);
    }
    Ok(match packet.bandwidth() {
        Bandwidth::Narrowband => SilkRate::Khz8,
        Bandwidth::Mediumband => SilkRate::Khz12,
        _ => SilkRate::Khz16, // SILK-only TOCs have at most wideband bandwidth.
    })
}

// Normal dec_API skips LBRR indices/pulses only: never dequantize them into
// the regular decoder's gain/NLSF/synthesis history. Scratch parameter states
// let the existing primitives consume exactly those symbols without synthesis.
#[derive(Default)]
pub(super) struct RedundancyState {
    gains: GainState,
    nlsf: NlsfState,
    pitch: PitchState,
}
impl RedundancyState {
    pub(super) fn consume(
        &mut self,
        entropy: &mut RangeDecoder<'_>,
        rate: SilkRate,
        duration: FrameDuration,
        conditional: bool,
    ) -> Result<(), ChannelError> {
        let gain = self
            .gains
            .decode(
                entropy,
                duration,
                FrameActivity::Redundant,
                if conditional {
                    GainCoding::Conditional
                } else {
                    GainCoding::Independent
                },
            )
            .map_err(ChannelError::Gains)?;
        self.nlsf
            .decode(
                entropy,
                if rate == SilkRate::Khz16 {
                    NlsfOrder::Order16
                } else {
                    NlsfOrder::Order10
                },
                duration,
                gain.signal_type(),
            )
            .map_err(ChannelError::Nlsf)?;
        let prediction = self
            .pitch
            .decode(
                entropy,
                rate,
                duration,
                gain.signal_type(),
                if conditional {
                    PitchCoding::Conditional
                } else {
                    PitchCoding::Independent
                },
            )
            .map_err(ChannelError::Pitch)?;
        decode_excitation(
            entropy,
            rate,
            duration,
            gain.signal_type(),
            gain.offset_type(),
            prediction.seed(),
        )
        .map_err(ChannelError::Excitation)?;
        Ok(())
    }
}
fn skip_redundancy(
    entropy: &mut RangeDecoder<'_>,
    rate: SilkRate,
    duration: FrameDuration,
    flags: &[bool],
) -> Result<(), ChannelError> {
    let mut state = RedundancyState::default();
    for (index, &present) in flags.iter().enumerate() {
        if present {
            state.consume(entropy, rate, duration, index > 0 && flags[index - 1])?;
        }
    }
    Ok(())
}
