//! Pure Rust Opus packet framing, entropy decoding and SILK primitives.
//! No dependencies or allocation; audio synthesis is not implemented yet.
//!
//! This validates RFC 6716 section 3 framing, not compressed audio contents.
//! Audio decoding, containers, multistream and self-delimiting framing are
//! not implemented. Pass exactly one packet, with its transport headers removed.
//!
//! ```
//! let packet = opura::Packet::parse(&[0x98, 0x12, 0x34])?;
//! assert_eq!(packet.duration_samples(), 960); // 20 ms at 48 kHz
//! assert_eq!(packet.frames(), &[&[0x12, 0x34][..]]);
//! # Ok::<(), opura::PacketError>(())
//! ```

pub mod range;
pub mod silk;

use std::fmt;

/// Coding mode signaled by the TOC byte.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Mode {
    /// Linear prediction only.
    Silk,
    /// Combined SILK and CELT.
    Hybrid,
    /// Transform coding only.
    Celt,
}

/// Encoded audio bandwidth, independent of the decoder output sample rate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Bandwidth {
    /// 4 kHz.
    Narrowband,
    /// 6 kHz.
    Mediumband,
    /// 8 kHz.
    Wideband,
    /// 12 kHz.
    SuperWideband,
    /// 20 kHz.
    Fullband,
}

/// Invalid packet framing. If several rules fail, the first detected error wins.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum PacketError {
    /// The mandatory TOC byte is absent.
    EmptyPacket,
    /// A header, declared frame, or padding extends beyond the packet.
    TruncatedPacket,
    /// The frame count is zero or exceeds 48.
    InvalidFrameCount,
    /// CBR payload length is not divisible by the frame count.
    InvalidFrameLength,
    /// A compressed frame exceeds 1275 bytes.
    FrameTooLarge,
    /// The packet contains more than 120 ms of audio.
    DurationTooLong,
}

impl fmt::Display for PacketError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::EmptyPacket => "missing Opus TOC byte",
            Self::TruncatedPacket => "truncated Opus packet",
            Self::InvalidFrameCount => "Opus frame count must be between 1 and 48",
            Self::InvalidFrameLength => "unequal frame lengths in CBR packet",
            Self::FrameTooLarge => "Opus frame exceeds 1275 bytes",
            Self::DurationTooLong => "Opus packet exceeds 120 ms",
        })
    }
}

impl std::error::Error for PacketError {}

/// A validated packet borrowing its compressed frames from the input.
///
/// Zero-length frames are valid (DTX or loss). Frames exclude framing headers
/// and padding. Successful parsing makes no claim about audio decodability.
#[derive(Debug)]
pub struct Packet<'a> {
    toc: u8,
    frames: [&'a [u8]; 48],
    count: usize,
}

impl<'a> Packet<'a> {
    /// Parse one externally delimited Opus packet.
    ///
    /// Uses fixed storage and no heap allocation. Work is bounded by 48 frame
    /// headers plus the padding-length header (linear in its encoded length).
    /// Padding contents are ignored, including nonzero bytes.
    pub fn parse(input: &'a [u8]) -> Result<Self, PacketError> {
        let (&toc, mut payload) = input.split_first().ok_or(PacketError::EmptyPacket)?;
        let code = toc & 3;
        let mut count = if code == 0 { 1 } else { 2 };
        let mut vbr = code == 2;
        if code == 3 {
            let header = read_byte(&mut payload)?;
            count = usize::from(header & 63);
            vbr = header & 128 != 0;
            if count == 0 || count > 48 {
                return Err(PacketError::InvalidFrameCount);
            }
            if header & 64 != 0 {
                loop {
                    let byte = read_byte(&mut payload)?;
                    let padding = if byte == 255 { 254 } else { usize::from(byte) };
                    // Remove padding from the end as each header byte is read.
                    // This bounds arithmetic by the remaining input length.
                    let end = payload
                        .len()
                        .checked_sub(padding)
                        .ok_or(PacketError::TruncatedPacket)?;
                    payload = &payload[..end];
                    if byte != 255 {
                        break;
                    }
                }
            }
        }
        let mut packet = Self {
            toc,
            frames: [&[]; 48],
            count,
        };
        if u32::from(packet.frame_samples()) * count as u32 > 5760 {
            return Err(PacketError::DurationTooLong);
        }
        let mut lengths = [0usize; 48];
        if vbr {
            let mut total = 0;
            for length in &mut lengths[..count - 1] {
                let first = usize::from(read_byte(&mut payload)?);
                *length = if first < 252 {
                    first
                } else {
                    first + 4 * usize::from(read_byte(&mut payload)?)
                };
                // At most 47 lengths, each at most 1275: cannot overflow usize.
                total += *length;
            }
            lengths[count - 1] = payload
                .len()
                .checked_sub(total)
                .ok_or(PacketError::TruncatedPacket)?;
        } else {
            if payload.len() % count != 0 {
                return Err(PacketError::InvalidFrameLength);
            }
            lengths[..count].fill(payload.len() / count);
        }
        for (frame, length) in packet.frames[..count].iter_mut().zip(&lengths[..count]) {
            if *length > 1275 {
                return Err(PacketError::FrameTooLarge);
            }
            let (head, tail) = payload
                .split_at_checked(*length)
                .ok_or(PacketError::TruncatedPacket)?;
            *frame = head;
            payload = tail;
        }
        Ok(packet)
    }

    /// Configuration number (0 through 31) from RFC 6716 Table 2.
    pub fn configuration(&self) -> u8 {
        self.toc >> 3
    }

    /// Signaled coding mode.
    pub fn mode(&self) -> Mode {
        match self.configuration() {
            0..=11 => Mode::Silk,
            12..=15 => Mode::Hybrid,
            _ => Mode::Celt,
        }
    }

    /// Signaled audio bandwidth.
    pub fn bandwidth(&self) -> Bandwidth {
        match self.configuration() {
            0..=3 | 16..=19 => Bandwidth::Narrowband,
            4..=7 => Bandwidth::Mediumband,
            8..=11 | 20..=23 => Bandwidth::Wideband,
            12..=13 | 24..=27 => Bandwidth::SuperWideband,
            _ => Bandwidth::Fullband,
        }
    }

    /// Encoded channel count: one or two.
    pub fn channels(&self) -> u8 {
        1 + ((self.toc >> 2) & 1)
    }

    /// Samples per channel in each frame, expressed on a 48 kHz clock.
    pub fn frame_samples(&self) -> u16 {
        let config = self.configuration();
        match config {
            0..=11 => match config & 3 {
                0 => 480,
                1 => 960,
                2 => 1920,
                _ => 2880,
            },
            12..=15 => 480 << (config & 1),
            _ => 120 << (config & 3),
        }
    }

    /// Total samples per channel on a 48 kHz clock, at most 5760 (120 ms).
    pub fn duration_samples(&self) -> u16 {
        // parse() validates the product in u32 before constructing a public packet.
        self.frame_samples() * self.count as u16
    }

    /// Compressed frames in transmission order; zero-length frames are valid.
    pub fn frames(&self) -> &[&'a [u8]] {
        &self.frames[..self.count]
    }
}

fn read_byte(input: &mut &[u8]) -> Result<u8, PacketError> {
    let (&byte, rest) = input.split_first().ok_or(PacketError::TruncatedPacket)?;
    *input = rest;
    Ok(byte)
}
