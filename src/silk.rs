//! SILK-layer headers, gains and spectral side information, not yet audio synthesis.
//!
//! Implements RFC 6716 sections 4.2.3–4.2.4 for SILK and Hybrid frames.
//! Flags describe internal 10/20 ms SILK frames, not Opus packet frame count.
//! Stereo channel zero is mid and channel one is side (not left/right).

pub mod excitation;
pub mod gains;
pub mod lpc;
pub mod nlsf;
pub mod pitch;

use crate::{
    Mode, Packet,
    range::{RangeDecoder, RangeError},
};
use std::fmt;

/// A header cannot be decoded from the requested packet frame.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum SilkError {
    /// CELT-only packets do not contain SILK headers.
    UnsupportedMode,
    /// The requested Opus frame is absent.
    InvalidFrameIndex,
    /// A zero-length frame represents loss/DTX and has no header to decode.
    MissingPayload,
    /// Header decoding uses more bits than the compressed frame contains.
    HeaderBudgetExceeded,
    /// An underlying entropy operation failed.
    Entropy(RangeError),
}

impl fmt::Display for SilkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedMode => f.write_str("CELT-only packet has no SILK header"),
            Self::InvalidFrameIndex => f.write_str("Opus frame index is out of bounds"),
            Self::MissingPayload => f.write_str("empty Opus frame has no SILK header"),
            Self::HeaderBudgetExceeded => f.write_str("SILK header exceeds frame bit budget"),
            Self::Entropy(error) => write!(f, "SILK entropy error: {error}"),
        }
    }
}
impl std::error::Error for SilkError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Entropy(error) => Some(error),
            _ => None,
        }
    }
}
impl From<RangeError> for SilkError {
    fn from(error: RangeError) -> Self {
        Self::Entropy(error)
    }
}

/// Channel-specific voice activity and low-bitrate redundancy (LBRR) flags.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FrameHeader {
    vad: [[bool; 3]; 2],
    lbrr: [[bool; 3]; 2],
    channels: usize,
    frames: usize,
    duration_ms: u8,
}
impl FrameHeader {
    /// One for mono; two for mid/side stereo.
    pub fn channel_count(&self) -> usize {
        self.channels
    }
    /// Number of internal SILK frames per channel (one, two, or three).
    pub fn frame_count(&self) -> usize {
        self.frames
    }
    /// Duration of each internal SILK frame, in milliseconds (10 or 20).
    pub fn frame_duration_ms(&self) -> u8 {
        self.duration_ms
    }
    /// Voice activity flags, in temporal order, or None for an absent channel.
    pub fn voice_activity(&self, channel: usize) -> Option<&[bool]> {
        self.vad
            .get(..self.channels)?
            .get(channel)
            .map(|v| &v[..self.frames])
    }
    /// LBRR flags in temporal order, or None for an absent channel.
    /// True identifies redundant audio for the preceding time interval;
    /// it does not mean that redundant audio has been decoded.
    pub fn redundancy(&self, channel: usize) -> Option<&[bool]> {
        self.lbrr
            .get(..self.channels)?
            .get(channel)
            .map(|v| &v[..self.frames])
    }
}

/// Decode the SILK header at the start of one validated Opus frame.
///
/// Returns the header and a range decoder positioned immediately after the
/// flags. LBRR payload, if present, comes next; callers must not jump directly
/// to regular frame data. No data from neighboring frames or padding is read.
/// Successful parsing does not validate the rest of the frame or produce PCM.
/// Packet metadata supplies duration/channels, so incompatible manual settings
/// cannot be passed. Errors expose no partially advanced entropy state.
///
/// ```
/// let packet = opura::Packet::parse(&[0x08, 0x00, 0x00])?;
/// let (header, decoder) = opura::silk::decode_header(&packet, 0)?;
/// assert_eq!(header.voice_activity(0), Some(&[false][..]));
/// assert_eq!(decoder.tell(), 3);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn decode_header<'a>(
    packet: &Packet<'a>,
    frame_index: usize,
) -> Result<(FrameHeader, RangeDecoder<'a>), SilkError> {
    if packet.mode() == Mode::Celt {
        return Err(SilkError::UnsupportedMode);
    }
    let frame = *packet
        .frames()
        .get(frame_index)
        .ok_or(SilkError::InvalidFrameIndex)?;
    if frame.is_empty() {
        return Err(SilkError::MissingPayload);
    }
    let mut decoder = RangeDecoder::new(frame);
    let mut header = FrameHeader {
        vad: [[false; 3]; 2],
        lbrr: [[false; 3]; 2],
        channels: usize::from(packet.channels()),
        frames: match packet.frame_samples() {
            1920 => 2,
            2880 => 3,
            _ => 1,
        },
        duration_ms: if packet.frame_samples() == 480 {
            10
        } else {
            20
        },
    };
    let mut has_lbrr = [false; 2];
    for (channel, flag) in has_lbrr.iter_mut().enumerate().take(header.channels) {
        for vad in &mut header.vad[channel][..header.frames] {
            *vad = decoder.decode_bit_logp(1)?;
        }
        *flag = decoder.decode_bit_logp(1)?;
    }
    for (channel, &present) in has_lbrr.iter().enumerate().take(header.channels) {
        if present {
            // RFC Table 4: zero-mask probability is zero; ICDF omits it.
            let mask = match header.frames {
                2 => decoder.decode_icdf(&[203, 150, 0], 8)? + 1,
                3 => decoder.decode_icdf(&[215, 195, 166, 125, 110, 82, 0], 8)? + 1,
                _ => 1,
            };
            for (index, flag) in header.lbrr[channel][..header.frames].iter_mut().enumerate() {
                *flag = mask & (1 << index) != 0;
            }
        }
    }
    // Packet validation limits each frame to 1275 bytes, so this cannot overflow.
    if decoder.tell() > frame.len() as u32 * 8 {
        return Err(SilkError::HeaderBudgetExceeded);
    }
    Ok((header, decoder))
}
