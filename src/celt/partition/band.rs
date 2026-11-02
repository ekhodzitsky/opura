//! Time-frequency and stereo orchestration over classic partitions.
use super::{
    Decoder, Node, PartitionControls, PartitionError, PartitionState, Theta, bands, mul31,
    validate_context,
};
use crate::celt::bands::BandError;
use crate::celt::{control_tables::TF_SELECT, math};
use crate::range::RangeDecoder;
mod encoder;
pub use encoder::{BandEncoding, encode_band};

/// Stereo angle/intensity policy for a decoded band.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StereoControls {
    /// First intensity-stereo band, 0..=21.
    pub intensity: usize,
    /// Consume but ignore the inversion flag, as required by some downmix contexts.
    pub disable_inversion: bool,
}
/// A mono/stereo band request, before all-band budget/folding orchestration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BandControls {
    /// LM must be 0..3, blocks 1 or 2^LM; only low block-count fill bits may be set.
    /// Stereo requires unit gain (i32::MAX). One-coordinate bands use unit signs.
    pub partition: PartitionControls,
    /// Standard TF mapping for this LM and long/transient block context.
    pub tf_change: i8,
    /// None for mono; otherwise stereo theta/intensity/inversion decoding.
    pub stereo: Option<StereoControls>,
}
/// Reconstructed Q24 channel vectors, collapse mask and data for later folding.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Band {
    left: [i32; 176],
    right: [i32; 176],
    fold: [i32; 176],
    length: usize,
    stereo: bool,
    collapse: u8,
}
impl Band {
    /// Mono or left channel in frequency-interleaved order after inverse TF.
    pub fn left(&self) -> &[i32] {
        &self.left[..self.length]
    }
    /// Right channel, if stereo was requested.
    pub fn right(&self) -> Option<&[i32]> {
        if self.stereo {
            Some(&self.right[..self.length])
        } else {
            None
        }
    }
    /// Mono/mid basis scaled for folding; stereo N=2 captures its stronger basis.
    pub fn folding(&self) -> &[i32] {
        &self.fold[..self.length]
    }
    /// Combined nonempty short-block flags after inverse TF.
    pub fn collapse_mask(&self) -> u8 {
        self.collapse
    }
}
/// Decode one mono/stereo band with signs, TF and folding output.
/// Errors preserve entropy/state and borrowed fold input. Payload and fold
/// bounds follow decode_partition; complete frame orchestration is separate.
pub fn decode_band(
    entropy: &mut RangeDecoder<'_>,
    state: &mut PartitionState,
    controls: BandControls,
    fold: Option<&[i32]>,
) -> Result<Band, PartitionError> {
    let c = controls.partition;
    let n = validate_band(
        entropy.storage_bits(),
        entropy.tell(),
        state,
        controls,
        fold,
    )?;
    let mut next = entropy.clone();
    let mut shared = state.clone();
    let mut result = Band {
        left: [0; 176],
        right: [0; 176],
        fold: [0; 176],
        length: n,
        stereo: controls.stereo.is_some(),
        collapse: 0,
    };
    let stereo = controls.stereo.unwrap_or(StereoControls {
        intensity: 21,
        disable_inversion: false,
    });
    let mut decoder = Decoder {
        entropy: &mut next,
        state: &mut shared,
        band: c.band,
        spread: c.spread,
        intensity: stereo.intensity,
        disable_inv: stereo.disable_inversion,
        tf_change: controls.tf_change,
    };
    let node = Node {
        bits: c.bits,
        blocks: c.blocks,
        lm: c.lm,
        gain: c.gain,
        fill: u32::from(c.fill),
    };
    let cm = if result.stereo {
        decoder.stereo_band(
            &mut result.left[..n],
            &mut result.right[..n],
            fold,
            Some(&mut result.fold[..n]),
            node,
        )?
    } else {
        decoder.band(
            &mut result.left[..n],
            fold,
            Some(&mut result.fold[..n]),
            node,
        )?
    };
    result.collapse = cm as u8;
    *entropy = next;
    *state = shared;
    Ok(result)
}
impl Decoder<'_, '_> {
    fn signs(
        &mut self,
        x: &mut [i32],
        y: Option<&mut [i32]>,
        out: Option<&mut [i32]>,
    ) -> Result<u32, PartitionError> {
        for channel in std::iter::once(&mut *x).chain(y) {
            let sign = if self.state.remaining >= 8 {
                self.state.remaining -= 8;
                self.entropy.decode_bits(1)? != 0
            } else {
                false
            };
            channel[0] = if sign { -(1 << 24) } else { 1 << 24 };
        }
        if let Some(out) = out {
            out[0] = x[0] >> 4;
        }
        Ok(1)
    }
    pub(super) fn band(
        &mut self,
        x: &mut [i32],
        fold: Option<&[i32]>,
        out: Option<&mut [i32]>,
        node: Node,
    ) -> Result<u32, PartitionError> {
        let Node {
            bits,
            blocks,
            lm,
            gain,
            fill,
        } = node;
        let n = x.len();
        if n == 1 {
            return self.signs(x, None, out);
        }
        let transform = BandTransform::prepare(None, fold, n, blocks, fill, self.tf_change)?;
        let cm = self.partition(
            x,
            transform.low(n),
            Node {
                bits,
                blocks: transform.blocks,
                lm,
                gain,
                fill: transform.fill,
            },
        )?;
        transform.finish(x, cm, out)
    }
    pub(super) fn stereo_band(
        &mut self,
        x: &mut [i32],
        y: &mut [i32],
        fold: Option<&[i32]>,
        out: Option<&mut [i32]>,
        node: Node,
    ) -> Result<u32, PartitionError> {
        let Node {
            mut bits,
            blocks,
            lm,
            gain: _,
            mut fill,
        } = node;
        let n = x.len();
        if n == 1 {
            return self.signs(x, Some(y), out);
        }
        let original_fill = fill;
        let split = self.theta(
            &mut bits,
            &mut fill,
            Theta {
                n,
                blocks,
                original_blocks: blocks,
                lm,
                stereo: true,
            },
        )?;
        let mid = i32::from(split.gains[0]) << 16;
        let side = i32::from(split.gains[1]) << 16;
        let cm = if n == 2 {
            let side_bits = if split.angle != 0 && split.angle != 16384 {
                8
            } else {
                0
            };
            let mid_bits = bits - side_bits;
            self.state.remaining -= split.cost + side_bits;
            let sign = if side_bits != 0 {
                1 - 2 * self.entropy.decode_bits(1)? as i32
            } else {
                1
            };
            let (primary, secondary) = if split.angle > 8192 {
                (&mut *y, &mut *x)
            } else {
                (&mut *x, &mut *y)
            };
            let cm = self.band(
                primary,
                fold,
                out,
                Node {
                    bits: mid_bits,
                    blocks,
                    lm,
                    gain: i32::MAX,
                    fill: original_fill,
                },
            )?;
            secondary[0] = (-sign).wrapping_mul(primary[1]);
            secondary[1] = sign.wrapping_mul(primary[0]);
            for j in 0..2 {
                let m = mul31(mid, x[j]);
                let s = mul31(side, y[j]);
                x[j] = m.wrapping_sub(s);
                y[j] = m.wrapping_add(s);
            }
            cm
        } else {
            let mut mid_bits = 0.max(bits.min((bits - split.delta) / 2));
            let mut side_bits = bits - mid_bits;
            self.state.remaining -= split.cost;
            let before = self.state.remaining;
            if mid_bits >= side_bits {
                let cm = self.band(
                    x,
                    fold,
                    out,
                    Node {
                        bits: mid_bits,
                        blocks,
                        lm,
                        gain: i32::MAX,
                        fill,
                    },
                )?;
                let rebalance = mid_bits - (before - self.state.remaining);
                if rebalance > 24 && split.angle != 0 {
                    side_bits += rebalance - 24;
                }
                cm | self.band(
                    y,
                    None,
                    None,
                    Node {
                        bits: side_bits,
                        blocks,
                        lm,
                        gain: side,
                        fill: fill >> blocks,
                    },
                )?
            } else {
                let cm = self.band(
                    y,
                    None,
                    None,
                    Node {
                        bits: side_bits,
                        blocks,
                        lm,
                        gain: side,
                        fill: fill >> blocks,
                    },
                )?;
                let rebalance = side_bits - (before - self.state.remaining);
                if rebalance > 24 && split.angle != 16384 {
                    mid_bits += rebalance - 24;
                }
                cm | self.band(
                    x,
                    fold,
                    out,
                    Node {
                        bits: mid_bits,
                        blocks,
                        lm,
                        gain: i32::MAX,
                        fill,
                    },
                )?
            }
        };
        if n != 2 {
            bands::stereo_merge(x, y, mid)?;
        }
        if split.inv {
            for value in y {
                *value = value.wrapping_neg();
            }
        }
        Ok(cm)
    }
}

// Both entropy directions reconstruct the same TF basis and folding output.
struct BandTransform {
    low: [i32; 176],
    folded: bool,
    blocks: usize,
    fill: u32,
    recombine: usize,
    time_divide: usize,
    long: bool,
}
impl BandTransform {
    fn prepare(
        mut input: Option<&mut [i32]>,
        fold: Option<&[i32]>,
        n: usize,
        mut blocks: usize,
        mut fill: u32,
        tf_change: i8,
    ) -> Result<Self, PartitionError> {
        let mut low = [0; 176];
        if let Some(fold) = fold {
            low[..n].copy_from_slice(fold);
        }
        let long = blocks == 1;
        let recombine = tf_change.max(0) as usize;
        let mut time_divide = 0;
        let mut tf = tf_change;
        let mut per = n / blocks;
        for k in 0..recombine {
            const INTERLEAVE: [u32; 16] = [0, 1, 1, 1, 2, 3, 3, 3, 2, 3, 3, 3, 2, 3, 3, 3];
            if let Some(x) = input.as_deref_mut() {
                bands::haar(x, 1 << k)?;
            }
            if fold.is_some() {
                bands::haar(&mut low[..n], 1 << k)?;
            }
            fill = INTERLEAVE[(fill & 15) as usize] | INTERLEAVE[(fill >> 4) as usize] << 2;
        }
        blocks >>= recombine;
        per <<= recombine;
        while per % 2 == 0 && tf < 0 {
            if let Some(x) = input.as_deref_mut() {
                bands::haar(x, blocks)?;
            }
            if fold.is_some() {
                bands::haar(&mut low[..n], blocks)?;
            }
            fill |= fill << blocks;
            blocks <<= 1;
            per >>= 1;
            time_divide += 1;
            tf += 1;
        }
        if blocks > 1 {
            if let Some(x) = input {
                bands::deinterleave(x, blocks << recombine, long)?;
            }
            if fold.is_some() {
                bands::deinterleave(&mut low[..n], blocks << recombine, long)?;
            }
        }
        Ok(Self {
            low,
            folded: fold.is_some(),
            blocks,
            fill,
            recombine,
            time_divide,
            long,
        })
    }
    fn low(&self, n: usize) -> Option<&[i32]> {
        if self.folded {
            Some(&self.low[..n])
        } else {
            None
        }
    }
    fn finish(
        &self,
        x: &mut [i32],
        mut cm: u32,
        out: Option<&mut [i32]>,
    ) -> Result<u32, PartitionError> {
        let n = x.len();
        let mut blocks = self.blocks;
        if blocks > 1 {
            bands::interleave(x, blocks << self.recombine, self.long)?;
        }
        for _ in 0..self.time_divide {
            blocks >>= 1;
            cm |= cm >> blocks;
            bands::haar(x, blocks)?;
        }
        for k in 0..self.recombine {
            const DEINTERLEAVE: [u32; 16] = [
                0, 3, 12, 15, 48, 51, 60, 63, 192, 195, 204, 207, 240, 243, 252, 255,
            ];
            cm = DEINTERLEAVE[cm as usize];
            bands::haar(x, 1 << k)?;
        }
        blocks <<= self.recombine;
        if let Some(out) = out {
            let scale = math::sqrt((n as i32) << 22).map_err(BandError::from)? as i16;
            for (out, &value) in out.iter_mut().zip(x.iter()) {
                *out = ((i64::from(scale) * i64::from(value)) >> 15) as i32;
            }
        }
        Ok(cm & ((1 << blocks) - 1))
    }
}

fn validate_band(
    storage_bits: u32,
    tell: u32,
    state: &PartitionState,
    controls: BandControls,
    fold: Option<&[i32]>,
) -> Result<usize, PartitionError> {
    let c = controls.partition;
    if !(0..=3).contains(&c.lm) || !(c.blocks == 1 || c.blocks == 1usize << c.lm) {
        return Err(PartitionError::InvalidGeometry);
    }
    let n = validate_context(storage_bits, tell, state, c, fold, 1)?;
    let transient = c.blocks > 1;
    if !TF_SELECT[c.lm as usize][if transient { 4..8 } else { 0..4 }].contains(&controls.tf_change)
        || u32::from(c.fill) > ((1u32 << c.blocks) - 1)
    {
        return Err(PartitionError::InvalidGeometry);
    }
    if controls.stereo.is_some_and(|s| s.intensity > 21) {
        return Err(PartitionError::InvalidGeometry);
    }
    if controls.stereo.is_some() && c.gain != i32::MAX {
        return Err(PartitionError::InvalidGain);
    }
    Ok(n)
}
