//! Classic CELT prefix and post-energy TF/spreading fields.
//! Positions/order follow pinned celt_decoder.c. These are control primitives,
//! not complete CELT/Hybrid decoding. Copyright (c) 2007-2008 CSIRO;
//! 2007-2010 Xiph.Org Foundation; 2008 Gregory Maxwell. See THIRD_PARTY_NOTICES.md.
use super::{
    FrameSize,
    control_tables::{SPREAD, TAPSET, TF_SELECT},
    postfilter::{PostfilterError, PostfilterParams, Tapset},
    pvq::Spread,
};
use crate::{
    Mode, Packet,
    range::{EncoderError, RangeDecoder, RangeError},
};
use std::{fmt, ops::Range};

mod encoder;
pub use encoder::{encode_spread, encode_tf};

/// Invalid CELT controls, payload/context or entropy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ControlError {
    /// Packet wrapper accepts CELT-only mode; positioned prefix also supports Hybrid.
    UnsupportedMode,
    /// Compressed Opus frame index is absent.
    MissingFrame,
    /// Non-concealment payload must contain 2..1275 bytes.
    InvalidPayload,
    /// Prefix start band must be 0 (CELT) or 17 (Hybrid high bands).
    InvalidStart,
    /// TF band interval requires start<=end<=21.
    InvalidBands,
    /// Positioned prefix cursor already exceeds the physical frame budget.
    BitBudgetExceeded,
    /// Encoding requires a supported frame/control context (LM0 is not transient).
    InvalidContext,
    /// Entropy encoding failed without committing the writer.
    Encoding(EncoderError),
    /// Entropy operation failed.
    Entropy(RangeError),
    /// Decoded postfilter parameters are unsupported.
    Postfilter(PostfilterError),
}
impl fmt::Display for ControlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedMode => f.write_str("CELT header wrapper requires CELT-only mode"),
            Self::MissingFrame => f.write_str("CELT frame index is absent"),
            Self::InvalidPayload => f.write_str("CELT control payload must contain 2..1275 bytes"),
            Self::InvalidStart => f.write_str("CELT control prefix start must be 0 or 17"),
            Self::InvalidBands => f.write_str("invalid CELT TF band interval"),
            Self::BitBudgetExceeded => f.write_str("CELT control cursor exceeds frame budget"),
            Self::InvalidContext => f.write_str("invalid CELT encoding control context"),
            Self::Encoding(e) => write!(f, "CELT control encoding: {e}"),
            Self::Entropy(e) => write!(f, "CELT control entropy: {e}"),
            Self::Postfilter(e) => write!(f, "CELT control postfilter: {e}"),
        }
    }
}
impl std::error::Error for ControlError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Entropy(e) => Some(e),
            Self::Encoding(e) => Some(e),
            Self::Postfilter(e) => Some(e),
            _ => None,
        }
    }
}
impl From<RangeError> for ControlError {
    fn from(e: RangeError) -> Self {
        Self::Entropy(e)
    }
}
impl From<EncoderError> for ControlError {
    fn from(error: EncoderError) -> Self {
        Self::Encoding(error)
    }
}
impl From<PostfilterError> for ControlError {
    fn from(e: PostfilterError) -> Self {
        Self::Postfilter(e)
    }
}

/// Prefix before coarse energies; geometry and flags are immutable.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FramePrefix {
    size: FrameSize,
    silence: bool,
    postfilter: PostfilterParams,
    transient: bool,
    intra: bool,
}
impl FramePrefix {
    /// Internal CELT frame duration.
    pub fn size(self) -> FrameSize {
        self.size
    }
    /// Silent frame; range accounting has advanced to the physical frame end.
    pub fn silence(self) -> bool {
        self.silence
    }
    /// Newly decoded comb parameters, disabled when no field is present.
    pub fn postfilter(self) -> PostfilterParams {
        self.postfilter
    }
    /// Transient short-MDCT flag (never set for 2.5 ms prefix decoding).
    pub fn transient(self) -> bool {
        self.transient
    }
    /// Intra coarse-energy prediction flag.
    pub fn intra(self) -> bool {
        self.intra
    }
    /// Number of short blocks, or zero for a long frame.
    pub fn short_blocks(self) -> usize {
        if self.transient {
            1 << self.size.lm()
        } else {
            0
        }
    }
}

/// Decode prefix from a correctly positioned CELT/Hybrid entropy cursor atomically.
/// Start 0 includes the comb field; start17 is used after SILK in Hybrid frames.
/// Frame payload must be 2..1275 bytes; empty/one-byte concealment is separate.
/// Original cached-tell decisions and silence bit-accounting are preserved.
pub fn decode_prefix(
    entropy: &mut RangeDecoder<'_>,
    size: FrameSize,
    start: usize,
) -> Result<FramePrefix, ControlError> {
    if !matches!(start, 0 | 17) {
        return Err(ControlError::InvalidStart);
    }
    let budget = entropy.storage_bits();
    if !(16..=1275 * 8).contains(&budget) {
        return Err(ControlError::InvalidPayload);
    }
    if entropy.tell() > budget {
        return Err(ControlError::BitBudgetExceeded);
    }
    let mut decoder = entropy.clone();
    let mut tell = decoder.tell();
    let silence = if tell >= budget {
        true
    } else if tell == 1 {
        decoder.decode_bit_logp(15)?
    } else {
        false
    };
    if silence {
        tell = budget;
        decoder.pad_to_storage_end()?;
    }
    let mut postfilter = PostfilterParams::default();
    if start == 0 && tell + 16 <= budget {
        if decoder.decode_bit_logp(1)? {
            let octave = decoder.decode_uint(6)?;
            let period = (16 << octave) + decoder.decode_bits(4 + octave)? - 1;
            let qg = decoder.decode_bits(3)?;
            let tap = if decoder.tell() + 2 <= budget {
                decoder.decode_icdf(&TAPSET, 2)?
            } else {
                0
            };
            postfilter = PostfilterParams::new(
                period as u16,
                (3072 * (qg + 1)) as u16,
                [Tapset::Set0, Tapset::Set1, Tapset::Set2][tap],
            )?;
        }
        tell = decoder.tell();
    }
    let transient = if size != FrameSize::Ms2_5 && tell + 3 <= budget {
        let flag = decoder.decode_bit_logp(3)?;
        tell = decoder.tell();
        flag
    } else {
        false
    };
    let intra = if tell + 3 <= budget {
        decoder.decode_bit_logp(3)?
    } else {
        false
    };
    *entropy = decoder;
    Ok(FramePrefix {
        size,
        silence,
        postfilter,
        transient,
        intra,
    })
}

/// Decode a CELT-only packet frame prefix and return positioned borrowed entropy.
/// Hybrid requires decode_prefix on the entropy cursor after SILK, not this wrapper.
pub fn decode_header<'a>(
    packet: &Packet<'a>,
    index: usize,
) -> Result<(FramePrefix, RangeDecoder<'a>), ControlError> {
    if packet.mode() != Mode::Celt {
        return Err(ControlError::UnsupportedMode);
    }
    let payload = packet
        .frames()
        .get(index)
        .copied()
        .ok_or(ControlError::MissingFrame)?;
    let size = match packet.frame_samples() {
        120 => FrameSize::Ms2_5,
        240 => FrameSize::Ms5,
        480 => FrameSize::Ms10,
        960 => FrameSize::Ms20,
        _ => return Err(ControlError::UnsupportedMode),
    };
    let mut entropy = RangeDecoder::new(payload);
    let prefix = decode_prefix(&mut entropy, size, 0)?;
    Ok((prefix, entropy))
}

/// Per-band TF resolution, zero outside the selected frame interval.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TfResolution {
    values: [i8; 21],
}
impl TfResolution {
    /// All 21 values; positive means higher frequency resolution, negative time.
    pub fn values(&self) -> &[i8; 21] {
        &self.values
    }
}
/// Decode the differential TF flags after coarse energies, before spreading.
/// A reserved select bit is consumed only when its mapping can change resolution.
/// Invalid bands/entropy leave the complete cursor unchanged.
pub fn decode_tf(
    entropy: &mut RangeDecoder<'_>,
    size: FrameSize,
    bands: Range<usize>,
    transient: bool,
) -> Result<TfResolution, ControlError> {
    if bands.start > bands.end || bands.end > 21 {
        return Err(ControlError::InvalidBands);
    }
    let mut decoder = entropy.clone();
    let mut budget = decoder.storage_bits();
    let mut tell = decoder.tell();
    let mut logp = if transient { 2 } else { 4 };
    let reserved = size != FrameSize::Ms2_5 && tell + logp < budget;
    budget -= u32::from(reserved);
    let mut changed = 0usize;
    let mut current = 0usize;
    let mut result = TfResolution { values: [0; 21] };
    for band in bands.clone() {
        if tell + logp <= budget {
            current ^= usize::from(decoder.decode_bit_logp(logp)?);
            tell = decoder.tell();
            changed |= current;
        }
        result.values[band] = current as i8;
        logp = if transient { 4 } else { 5 };
    }
    let base = 4 * usize::from(transient);
    let table = TF_SELECT[size.lm()];
    let select = if reserved && table[base + changed] != table[base + 2 + changed] {
        usize::from(decoder.decode_bit_logp(1)?)
    } else {
        0
    };
    for band in bands {
        result.values[band] = table[base + 2 * select + result.values[band] as usize];
    }
    *entropy = decoder;
    Ok(result)
}
/// Decode the spreading decision after TF flags, before dynamic allocation.
/// Fewer than four remaining whole bits use Normal without consuming entropy.
pub fn decode_spread(entropy: &mut RangeDecoder<'_>) -> Result<Spread, ControlError> {
    if entropy.tell() + 4 > entropy.storage_bits() {
        return Ok(Spread::Normal);
    }
    let mut decoder = entropy.clone();
    let symbol = decoder.decode_icdf(&SPREAD, 5)?;
    let spread = [
        Spread::None,
        Spread::Light,
        Spread::Normal,
        Spread::Aggressive,
    ][symbol];
    *entropy = decoder;
    Ok(spread)
}
