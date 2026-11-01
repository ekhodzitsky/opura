//! All-band spectral budget, folding and channel orchestration.
use super::{Decoder, Node, PartitionError, PartitionState};
use crate::{
    celt::{
        FrameSize, allocation::Allocation, control::TfResolution, control_tables::TF_SELECT,
        energy::BandLayout, output_tables::BANDS, pvq::Spread,
    },
    range::RangeDecoder,
};

/// Complete fixed-storage normalized Q24 spectra before anti-collapse/scaling.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Spectrum {
    values: [[i32; 960]; 2],
    masks: [[u8; 21]; 2],
    size: FrameSize,
    layout: BandLayout,
    seed: u32,
}
impl Spectrum {
    /// Encoded channel count and reconstructed band interval.
    pub fn layout(&self) -> BandLayout {
        self.layout
    }
    /// Internal 48kHz frame geometry.
    pub fn size(&self) -> FrameSize {
        self.size
    }
    /// Q24 spectrum of an active encoded channel; unused bins are zero.
    pub fn channel(&self, channel: usize) -> Option<&[i32]> {
        if channel < self.layout.channels() {
            Some(&self.values[channel][..self.size.samples()])
        } else {
            None
        }
    }
    /// Collapse flags for all bands in an active encoded channel, zero outside layout.
    pub fn collapse_masks(&self, channel: usize) -> Option<&[u8; 21]> {
        if channel < self.layout.channels() {
            Some(&self.masks[channel])
        } else {
            None
        }
    }
    /// Noise/folding seed after reconstructing all selected bands.
    pub fn seed(&self) -> u32 {
        self.seed
    }
}
/// Decode every allocated band after fine energy, before anti-collapse control
/// and final energy refinement. Allocation supplies frame/channel/band geometry.
/// TF must match that frame's standard mapping; payloads contain 2..1275 bytes.
/// Errors preserve entropy. Success may still need final frame-budget validation.
pub fn decode_spectrum(
    entropy: &mut RangeDecoder<'_>,
    allocation: &Allocation,
    tf: &TfResolution,
    spread: Spread,
    seed: u32,
    disable_inversion: bool,
) -> Result<Spectrum, PartitionError> {
    if !(16..=10200).contains(&entropy.storage_bits()) || entropy.tell() > entropy.storage_bits() {
        return Err(PartitionError::InvalidBudget);
    }
    let layout = allocation.layout();
    if !matches!(layout.bands().start, 0 | 17) {
        return Err(PartitionError::InvalidGeometry);
    }
    let size = allocation.size();
    let lm = size.lm();
    let m = 1usize << lm;
    let blocks = if allocation.transient() { m } else { 1 };
    // At LM=0 the transient flag does not select a different TF model.
    let model = if blocks > 1 { 4..8 } else { 0..4 };
    if layout
        .bands()
        .any(|i| !TF_SELECT[lm][model.clone()].contains(&tf.values()[i]))
    {
        return Err(PartitionError::InvalidGeometry);
    }
    let mut next = entropy.clone();
    let mut result = Spectrum {
        values: [[0; 960]; 2],
        masks: [[0; 21]; 2],
        size,
        layout,
        seed,
    };
    let total_bits = (next.storage_bits() * 8) as i32
        - if allocation.anti_collapse_reserved() {
            8
        } else {
            0
        };
    let mut shared = PartitionState { remaining: 0, seed };
    let mut decoder = Decoder {
        entropy: &mut next,
        state: &mut shared,
        band: layout.bands().start,
        spread,
        intensity: allocation.intensity(),
        disable_inv: disable_inversion,
        tf_change: 0,
    };
    let mut norm = [[0i32; 624]; 2];
    let norm_offset = m * usize::from(BANDS[layout.bands().start]);
    let mut lowband_offset = 0usize;
    let mut update_lowband = true;
    let mut balance = allocation.balance();
    let mut dual = allocation.dual_stereo();
    for i in layout.bands() {
        decoder.band = i;
        decoder.tf_change = tf.values()[i];
        let first = m * usize::from(BANDS[i]);
        let last = m * usize::from(BANDS[i + 1]);
        let n = last - first;
        let tell = decoder.entropy.tell_frac() as i32;
        if i != layout.bands().start {
            balance -= tell;
        }
        let remaining = total_bits - tell - 1;
        decoder.state.remaining = remaining;
        let bits = if i < allocation.coded_bands() {
            let current = balance / (allocation.coded_bands() - i).min(3) as i32;
            0.max(16383.min((remaining + 1).min(allocation.pvq_bits()[i] + current)))
        } else {
            0
        };
        if (first as i32 - n as i32 >= norm_offset as i32 || i == layout.bands().start + 1)
            && (update_lowband || lowband_offset == 0)
        {
            lowband_offset = i;
        }
        if i == layout.bands().start + 1 {
            let start = layout.bands().start;
            let n1 = m * usize::from(BANDS[start + 1] - BANDS[start]);
            let n2 = m * usize::from(BANDS[start + 2] - BANDS[start + 1]);
            // Standard adjacent widths satisfy n1<=n2<=2*n1. Only Hybrid-like
            // starting bands need the extra tail before their second band is folded.
            for channel in norm.iter_mut().take(if dual { 2 } else { 1 }) {
                channel.copy_within(2 * n1 - n2..n1, n1);
            }
        }
        let mut effective = None;
        let mut fill = [(1u32 << blocks) - 1; 2];
        if lowband_offset != 0
            && (spread != Spread::Aggressive || blocks > 1 || decoder.tf_change < 0)
        {
            let offset = (m * usize::from(BANDS[lowband_offset])).saturating_sub(norm_offset + n);
            effective = Some(offset);
            let mut fold_start = lowband_offset - 1;
            while m * usize::from(BANDS[fold_start]) > offset + norm_offset {
                fold_start -= 1;
            }
            let mut fold_end = lowband_offset;
            while fold_end < i && m * usize::from(BANDS[fold_end]) < offset + norm_offset + n {
                fold_end += 1;
            }
            fill = [0; 2];
            for folded in fold_start..fold_end {
                fill[0] |= u32::from(result.masks[0][folded]);
                fill[1] |= u32::from(result.masks[layout.channels() - 1][folded]);
            }
        }
        if dual && i == allocation.intensity() {
            dual = false;
            for j in 0..first - norm_offset {
                norm[0][j] = (norm[0][j] + norm[1][j]) >> 1;
            }
        }
        let output = i != layout.bands().end - 1;
        let out_first = first - norm_offset;
        let mut fold = [[0i32; 176]; 2];
        if let Some(offset) = effective {
            for c in 0..layout.channels() {
                fold[c][..n].copy_from_slice(&norm[c][offset..offset + n]);
            }
        }
        let (left, right) = result.values.split_at_mut(1);
        let left = &mut left[0][first..last];
        let right = &mut right[0][first..last];
        let cm;
        if dual {
            let mut masks = [0u32; 2];
            for c in 0..2 {
                let x = if c == 0 { &mut *left } else { &mut *right };
                masks[c] = decoder.band(
                    x,
                    effective.map(|_| &fold[c][..n]),
                    if output {
                        Some(&mut norm[c][out_first..out_first + n])
                    } else {
                        None
                    },
                    Node {
                        bits: bits / 2,
                        blocks,
                        lm: lm as i8,
                        gain: i32::MAX,
                        fill: fill[c],
                    },
                )?;
            }
            cm = masks;
        } else {
            let input = effective.map(|_| &fold[0][..n]);
            let out = if output {
                Some(&mut norm[0][out_first..out_first + n])
            } else {
                None
            };
            let node = Node {
                bits,
                blocks,
                lm: lm as i8,
                gain: i32::MAX,
                fill: fill[0] | fill[1],
            };
            let mask = if layout.channels() == 2 {
                decoder.stereo_band(left, right, input, out, node)?
            } else {
                decoder.band(left, input, out, node)?
            };
            cm = [mask; 2];
        }
        result.masks[0][i] = cm[0] as u8;
        if layout.channels() == 2 {
            result.masks[1][i] = cm[1] as u8;
        }
        balance += allocation.pvq_bits()[i] + tell;
        update_lowband = bits > (n as i32 * 8);
    }
    result.seed = decoder.state.seed;
    *entropy = next;
    Ok(result)
}

/// Invalid anti-collapse context, energy domain or scalar/vector arithmetic.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum AntiCollapseError {
    /// Matching allocation must reserve anti-collapse for a transient 10/20ms frame.
    InvalidContext,
    /// Every current/history Q24 energy must lie within -32..=32 log2 units.
    InvalidEnergy,
    /// Band normalization failed.
    Band(crate::celt::bands::BandError),
}
impl std::fmt::Display for AntiCollapseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidContext => f.write_str("invalid CELT anti-collapse frame context"),
            Self::InvalidEnergy => f.write_str("CELT anti-collapse energy outside -32..32"),
            Self::Band(e) => write!(f, "CELT anti-collapse: {e}"),
        }
    }
}
impl std::error::Error for AntiCollapseError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Band(e) => Some(e),
            _ => None,
        }
    }
}
impl From<crate::celt::bands::BandError> for AntiCollapseError {
    fn from(e: crate::celt::bands::BandError) -> Self {
        Self::Band(e)
    }
}
impl Spectrum {
    /// Fill collapsed transient blocks after final energy, when the reserved bit is
    /// on. Allocation must match this spectrum and reserve anti-collapse. Energies
    /// and both histories contain two channels even for mono (which uses their max).
    /// Errors preserve the complete spectrum. The stored folding seed is unchanged:
    /// original anti-collapse consumes a local copy, not persistent decoder RNG.
    pub fn anti_collapse(
        &mut self,
        allocation: &Allocation,
        current: &[[i32; 21]; 2],
        previous: &[[i32; 21]; 2],
        previous2: &[[i32; 21]; 2],
    ) -> Result<(), AntiCollapseError> {
        use crate::celt::bands::BandError;
        use crate::celt::{bands, math};
        let lm = self.size.lm();
        if self.size != allocation.size()
            || self.layout != allocation.layout()
            || !(2..=3).contains(&lm)
            || !allocation.transient()
            || !allocation.anti_collapse_reserved()
        {
            return Err(AntiCollapseError::InvalidContext);
        }
        if [current, previous, previous2].iter().any(|a| {
            a.iter()
                .flatten()
                .any(|&v| !(-(32 << 24)..=(32 << 24)).contains(&v))
        }) {
            return Err(AntiCollapseError::InvalidEnergy);
        }
        let mut next = self.clone();
        let mut seed = self.seed;
        let blocks = 1usize << lm;
        for i in self.layout.bands() {
            let width = usize::from(BANDS[i + 1] - BANDS[i]);
            let n = width << lm;
            let depth = ((1 + allocation.pvq_bits()[i]) / width as i32) >> lm;
            let exponent = ((depth << 7) as i16).wrapping_neg();
            let threshold = 32767.min(math::exp2(exponent) >> 1) >> 1;
            let shift = (31 - (n as u32).leading_zeros()) >> 1;
            let root = i32::from(
                math::rsqrt_normalized((n as i32) << ((7 - shift) * 2)).map_err(BandError::from)?,
            );
            for c in 0..self.layout.channels() {
                let mut p1 = previous[c][i];
                let mut p2 = previous2[c][i];
                if self.layout.channels() == 1 {
                    p1 = p1.max(previous[1][i]);
                    p2 = p2.max(previous2[1][i]);
                }
                let difference = (current[c][i] - p1.min(p2)).max(0);
                let mut amplitude = if difference < (16 << 24) {
                    2 * 16383.min(math::exp2(((-difference + 8192) >> 14) as i16) >> 1)
                } else {
                    0
                };
                if lm == 3 {
                    amplitude = (23170 * amplitude.min(23169)) >> 14;
                }
                amplitude = amplitude.min(threshold) >> 1;
                amplitude = ((root * amplitude) >> 15) << (10 - shift);
                let first = usize::from(BANDS[i]) << lm;
                let x = &mut next.values[c][first..first + n];
                let mut changed = false;
                for block in 0..blocks {
                    if self.masks[c][i] & (1 << block) == 0 {
                        for j in 0..width {
                            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                            x[(j << lm) + block] = if seed & 0x8000 != 0 {
                                amplitude
                            } else {
                                -amplitude
                            };
                        }
                        changed = true;
                    }
                }
                if changed {
                    bands::renormalize(x, i32::MAX)?;
                }
            }
        }
        *self = next;
        Ok(())
    }
}
