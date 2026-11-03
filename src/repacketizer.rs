//! Borrowed RFC6716 packet assembly without allocation.
//!
//! Appending validates framing and compatibility, not compressed audio. Original
//! transport padding is discarded. Explicit self-delimited writes support
//! non-final multistream subpackets; extension padding is outside this API. Payload frames remain borrowed.
//! Canonical packing follows Opus v1.6.1 `src/repacketizer.c`; see
//! THIRD_PARTY_NOTICES.md. Copyright (c) 2011 Xiph.Org Foundation.
use crate::{Packet, PacketError};
use std::{fmt, ops::Range};

/// Invalid framing/selection or insufficient destination capacity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum RepacketizerError {
    /// An appended externally delimited packet has invalid framing.
    Packet(PacketError),
    /// All frames must share TOC configuration and encoded channel count.
    IncompatibleConfiguration,
    /// Combined duration exceeds120ms (at most48 frames).
    DurationTooLong,
    /// No frames have been appended.
    NoFrames,
    /// Output range must be nonempty and contained in the buffered frames.
    InvalidRange,
    /// Destination or requested padded length cannot hold the selected frames.
    BufferTooSmall,
}
impl fmt::Display for RepacketizerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Packet(e) => write!(f, "invalid appended Opus packet: {e}"),
            Self::IncompatibleConfiguration => f.write_str("Opus frame configurations differ"),
            Self::DurationTooLong => f.write_str("assembled Opus duration exceeds120ms"),
            Self::NoFrames => f.write_str("no Opus frames to assemble"),
            Self::InvalidRange => f.write_str("invalid Opus frame selection"),
            Self::BufferTooSmall => f.write_str("Opus packet destination is too small"),
        }
    }
}
impl std::error::Error for RepacketizerError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Packet(e) => Some(e),
            _ => None,
        }
    }
}
impl From<PacketError> for RepacketizerError {
    fn from(error: PacketError) -> Self {
        Self::Packet(error)
    }
}

/// Up to48 compatible borrowed frames with a total duration of at most120ms.
/// Failed append leaves every buffered frame unchanged; failed writes leave
/// both the destination and this value unchanged. Successful writes affect
/// only the returned prefix of the caller's destination.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Repacketizer<'a> {
    frames: [&'a [u8]; 48],
    count: usize,
    toc: u8,
    samples: u16,
}
impl Default for Repacketizer<'_> {
    fn default() -> Self {
        Self::new()
    }
}
impl<'a> Repacketizer<'a> {
    /// Start an empty borrowed assembly.
    pub const fn new() -> Self {
        Self {
            frames: [&[]; 48],
            count: 0,
            toc: 0,
            samples: 0,
        }
    }
    /// Release all buffered frame references and restore an empty assembly.
    pub fn reset(&mut self) {
        *self = Self::new();
    }
    /// Number of buffered compressed frames.
    pub fn frame_count(&self) -> usize {
        self.count
    }
    /// Whether no compressed frames are buffered.
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }
    /// Combined duration on the48kHz clock, zero for an empty assembly.
    pub fn duration_samples(&self) -> u16 {
        self.samples * self.count as u16
    }
    /// Validated compressed frame views, excluding transport headers/padding.
    pub fn frames(&self) -> &[&'a [u8]] {
        &self.frames[..self.count]
    }
    /// Append an externally delimited packet. Zero-length frames are valid.
    /// A successful append borrows only payloads, discarding transport padding.
    pub fn add_packet(&mut self, bytes: &'a [u8]) -> Result<(), RepacketizerError> {
        let packet = Packet::parse(bytes)?;
        let toc = bytes[0] & 0xfc;
        if self.count != 0 && self.toc != toc {
            return Err(RepacketizerError::IncompatibleConfiguration);
        }
        let count = self.count + packet.frames().len();
        if count > 48 || usize::from(packet.frame_samples()) * count > 5760 {
            return Err(RepacketizerError::DurationTooLong);
        }
        self.frames[self.count..count].copy_from_slice(packet.frames());
        self.count = count;
        self.toc = toc;
        self.samples = packet.frame_samples();
        Ok(())
    }
    /// Write all frames using the shortest standard framing.
    pub fn write(&self, output: &mut [u8]) -> Result<usize, RepacketizerError> {
        self.write_range(0..self.count, output, None)
    }
    /// Write all frames with exactly `length` bytes, adding transport padding.
    pub fn write_padded(
        &self,
        output: &mut [u8],
        length: usize,
    ) -> Result<usize, RepacketizerError> {
        self.write_range(0..self.count, output, Some(length))
    }
    /// Write all frames as a self-delimited non-final multistream subpacket.
    /// The explicit last-frame size is included in the returned length.
    pub fn write_self_delimited(&self, output: &mut [u8]) -> Result<usize, RepacketizerError> {
        self.write_range_impl(0..self.count, output, None, true)
    }
    /// Write a self-delimited subpacket with exactly `length` bytes, including
    /// its trailing transport padding. Failure leaves the destination unchanged.
    pub fn write_self_delimited_padded(
        &self,
        output: &mut [u8],
        length: usize,
    ) -> Result<usize, RepacketizerError> {
        self.write_range_impl(0..self.count, output, Some(length), true)
    }
    /// Write a nonempty frame range with canonical framing, or exactly the
    /// requested padded length. The rest of the destination is untouched.
    pub fn write_range(
        &self,
        range: Range<usize>,
        output: &mut [u8],
        padded_length: Option<usize>,
    ) -> Result<usize, RepacketizerError> {
        self.write_range_impl(range, output, padded_length, false)
    }
    fn write_range_impl(
        &self,
        range: Range<usize>,
        output: &mut [u8],
        padded_length: Option<usize>,
        self_delimited: bool,
    ) -> Result<usize, RepacketizerError> {
        if self.is_empty() {
            return Err(RepacketizerError::NoFrames);
        }
        if range.start >= range.end || range.end > self.count {
            return Err(RepacketizerError::InvalidRange);
        }
        let frames = &self.frames[range];
        let count = frames.len();
        let variable = frames.iter().any(|f| f.len() != frames[0].len());
        let payload: usize = frames.iter().map(|f| f.len()).sum();
        let sizes: usize = if variable {
            frames[..count - 1]
                .iter()
                .map(|f| size_bytes(f.len()))
                .sum()
        } else {
            0
        };
        let short_code = if count == 1 {
            0
        } else if count == 2 {
            if variable { 2 } else { 1 }
        } else {
            3
        };
        let last_size = if self_delimited {
            size_bytes(frames[count - 1].len())
        } else {
            0
        };
        let short_header = last_size
            + match short_code {
                0 | 1 => 1,
                2 => 1 + sizes,
                _ => 2 + sizes,
            };
        let canonical = short_header + payload;
        let code = if padded_length.is_some_and(|n| n > canonical) {
            3
        } else {
            short_code
        };
        let header = if code == 3 {
            2 + sizes + last_size
        } else {
            short_header
        };
        let minimum = header + payload;
        let length = padded_length.unwrap_or(minimum);
        if length < minimum || length > output.len() {
            return Err(RepacketizerError::BufferTooSmall);
        }
        let padding = length - minimum;
        // Everything below is infallible after preflight. The fixed frame
        // limits bound unpadded payload/header arithmetic to fewer than64KiB.
        output[0] = self.toc | code;
        let mut cursor = 1;
        if code == 3 {
            output[1] =
                count as u8 | if variable { 0x80 } else { 0 } | if padding != 0 { 0x40 } else { 0 };
            cursor = 2;
            if padding != 0 {
                let full = (padding - 1) / 255;
                output[cursor..cursor + full].fill(255);
                cursor += full;
                output[cursor] = (padding - 1 - 255 * full) as u8;
                cursor += 1;
            }
        }
        if code == 2 || code == 3 && variable {
            for frame in &frames[..count - 1] {
                cursor += write_size(frame.len(), &mut output[cursor..]);
            }
        }
        if self_delimited {
            cursor += write_size(frames[count - 1].len(), &mut output[cursor..]);
        }
        for frame in frames {
            output[cursor..cursor + frame.len()].copy_from_slice(frame);
            cursor += frame.len();
        }
        output[cursor..length].fill(0);
        Ok(length)
    }
}
fn size_bytes(length: usize) -> usize {
    1 + usize::from(length >= 252)
}
fn write_size(length: usize, output: &mut [u8]) -> usize {
    if length < 252 {
        output[0] = length as u8;
        1
    } else {
        let first = 252 + (length & 3);
        output[0] = first as u8;
        output[1] = ((length - first) / 4) as u8;
        2
    }
}
