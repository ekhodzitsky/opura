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
