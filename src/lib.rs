//! Pure Rust standard Opus packet decoding with no dependencies or allocation.
//!
//! [`decoder::OpusDecoder`] combines SILK/CELT/Hybrid PCM, loss/DTX, FEC and mode
//! transitions. All120 official RFC8251 vector/rate/channel cases pass.
//! The measured scalar decoder baseline trails libopus. Encoding is available in
//! [`celt::encoder::CeltEncoder`] and the fixed-bandwidth mono SILK
//! [`silk::encoder::mono::MonoEncoder`], plus fixed-band Hybrid
//! [`hybrid::encoder::HybridEncoder`]. Full adaptive encoding remains work.
//! Frame-local entropy writing is available in [`range::RangeEncoder`]. Component APIs are in
//! [`silk::decoder`], [`celt::decoder`] and [`hybrid`].
//!
//! Pass exactly one externally delimited packet with transport headers removed.
//! [`Packet`] validates RFC6716 section3 framing separately from compressed audio.
//! Containers, multistream and self-delimiting framing are not implemented.
//!
//! ```
//! let packet = opura::Packet::parse(&[0x98, 0x12, 0x34])?;
//! assert_eq!(packet.duration_samples(), 960); // 20 ms at 48 kHz
//! assert_eq!(packet.frames(), &[&[0x12, 0x34][..]]);
//! # Ok::<(), opura::PacketError>(())
//! ```

pub mod celt;
pub mod decoder;
pub mod hybrid;
pub mod range;
pub mod repacketizer;
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
    frames: Frames<'a>,
}

// Smaller inline variants avoid initializing 48 slice pointers for short packets.
#[derive(Debug)]
#[allow(
    clippy::large_enum_variant,
    reason = "Keep fixed, allocation-free storage; small variants avoid initializing unused frame views"
)]
enum Frames<'a> {
    One([&'a [u8]; 1]),
    Two([&'a [u8]; 2]),
    Few {
        frames: [&'a [u8]; 8],
        count: usize,
    },
    Many {
        frames: [&'a [u8]; 48],
        count: usize,
    },
}

impl<'a> Packet<'a> {
    /// Parse one externally delimited Opus packet.
    ///
    /// Uses fixed storage and no heap allocation. Work is bounded by 48 frame
    /// headers plus the padding-length header (linear in its encoded length).
    /// Padding contents are ignored, including nonzero bytes.
    #[inline]
    pub fn parse(input: &'a [u8]) -> Result<Self, PacketError> {
        let (&toc, mut payload) = input.split_first().ok_or(PacketError::EmptyPacket)?;
        let code = toc & 3;
        let (count, vbr) = if code == 3 {
            let header = read_byte(&mut payload)?;
            let count = usize::from(header & 63);
            let vbr = header & 128 != 0;
            if count == 0 || count > 48 {
                return Err(PacketError::InvalidFrameCount);
            }
            if header & 64 != 0 {
                loop {
                    let byte = read_byte(&mut payload)?;
                    let padding = if byte == 255 { 254 } else { usize::from(byte) };
                    // Bound padding arithmetic by the remaining input length.
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
            if u32::from(frame_samples(toc)) * count as u32 > 5760 {
                return Err(PacketError::DurationTooLong);
            }
            (count, vbr)
        } else {
            (if code == 0 { 1 } else { 2 }, code == 2)
        };
        if count == 1 {
            if payload.len() > 1275 {
                return Err(PacketError::FrameTooLarge);
            }
            return Ok(Self {
                toc,
                frames: Frames::One([payload]),
            });
        }
        if count == 2 {
            let first_length = if !vbr {
                if payload.len() % 2 != 0 {
                    return Err(PacketError::InvalidFrameLength);
                }
                payload.len() / 2
            } else {
                let first = usize::from(read_byte(&mut payload)?);
                if first < 252 {
                    first
                } else {
                    first + 4 * usize::from(read_byte(&mut payload)?)
                }
            };
            let second_length = payload
                .len()
                .checked_sub(first_length)
                .ok_or(PacketError::TruncatedPacket)?;
            if first_length > 1275 || second_length > 1275 {
                return Err(PacketError::FrameTooLarge);
            }
            let (first, second) = payload.split_at(first_length);
            return Ok(Self {
                toc,
                frames: Frames::Two([first, second]),
            });
        }
        if !vbr {
            if payload.len() % count != 0 {
                return Err(PacketError::InvalidFrameLength);
            }
            let length = payload.len() / count;
            if length > 1275 {
                return Err(PacketError::FrameTooLarge);
            }
            if count <= 8 {
                return Ok(Self {
                    toc,
                    frames: Frames::Few {
                        frames: cbr_frame_views(payload, count, length),
                        count,
                    },
                });
            }
            return Ok(Self {
                toc,
                frames: Frames::Many {
                    frames: cbr_frame_views(payload, count, length),
                    count,
                },
            });
        }
        let mut lengths = [0u16; 48];
        let mut total = 0;
        for length in &mut lengths[..count - 1] {
            let first = u16::from(read_byte(&mut payload)?);
            *length = if first < 252 {
                first
            } else {
                first + 4 * u16::from(read_byte(&mut payload)?)
            };
            // At most 47 lengths, each at most 1275: cannot overflow usize.
            total += usize::from(*length);
        }
        let last_length = payload
            .len()
            .checked_sub(total)
            .ok_or(PacketError::TruncatedPacket)?;
        if last_length > 1275 {
            return Err(PacketError::FrameTooLarge);
        }
        lengths[count - 1] = last_length as u16;
        if count <= 8 {
            return Ok(Self {
                toc,
                frames: Frames::Few {
                    frames: frame_views(payload, &lengths[..count]),
                    count,
                },
            });
        }
        Ok(Self {
            toc,
            frames: Frames::Many {
                frames: frame_views(payload, &lengths[..count]),
                count,
            },
        })
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
        frame_samples(self.toc)
    }

    /// Total samples per channel on a 48 kHz clock, at most 5760 (120 ms).
    pub fn duration_samples(&self) -> u16 {
        // parse() validates the product in u32 before constructing a public packet.
        self.frame_samples() * self.frames().len() as u16
    }

    /// Compressed frames in transmission order; zero-length frames are valid.
    #[inline]
    pub fn frames(&self) -> &[&'a [u8]] {
        match &self.frames {
            Frames::One(frames) => frames,
            Frames::Two(frames) => frames,
            Frames::Few { frames, count } => &frames[..*count],
            Frames::Many { frames, count } => &frames[..*count],
        }
    }
}

fn read_byte(input: &mut &[u8]) -> Result<u8, PacketError> {
    let (&byte, rest) = input.split_first().ok_or(PacketError::TruncatedPacket)?;
    *input = rest;
    Ok(byte)
}

fn frame_samples(toc: u8) -> u16 {
    let config = toc >> 3;
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

// Callers bound the count by N and validate the sum of lengths against payload.
// Inline to avoid copying the fixed array across a helper call boundary.
#[inline(always)]
fn frame_views<'a, const N: usize>(mut payload: &'a [u8], lengths: &[u16]) -> [&'a [u8]; N] {
    let mut frames = [&[][..]; N];
    for (frame, &length) in frames[..lengths.len()].iter_mut().zip(lengths) {
        let (head, tail) = payload.split_at(usize::from(length));
        *frame = head;
        payload = tail;
    }
    frames
}

// Count and frame length are validated before slicing or constructing views.
#[inline(always)]
fn cbr_frame_views<const N: usize>(mut payload: &[u8], count: usize, length: usize) -> [&[u8]; N] {
    let mut frames = [&[][..]; N];
    for frame in &mut frames[..count] {
        let (head, tail) = payload.split_at(length);
        *frame = head;
        payload = tail;
    }
    frames
}
