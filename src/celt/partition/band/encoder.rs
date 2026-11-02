//! Immutable band encoding and native stereo/TF orchestration.
use super::*;
use crate::celt::partition::{EncoderError, RangeEncoder, encoder::Encoder};

/// Encoder decisions for one band; these do not select frame-wide controls.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BandEncoding {
    /// Protect mono split endpoints from bit-starved noise injection.
    pub avoid_split_noise: bool,
    /// Stereo angle candidate: -1 biased down, 0 nearest, +1 biased up.
    pub theta_round: i8,
    /// Nonnegative linear left/right band amplitudes, used only for stereo.
    pub amplitudes: [i32; 2],
}
/// Encode a mono/stereo normalized band and return its reconstructed channels.
///
/// Left and optional right must match the standard dimension. Right is present
/// exactly when stereo controls are present. Each input Q24 squared norm is at
/// most 5/4 (`sum(x*x)<=5*2^46`), allowing native band-normalization rounding.
/// Amplitudes are nonnegative; theta rounding is -1, 0 or 1. Geometry, TF, gain,
/// fold and budget domains follow [`decode_band`]. The writer, budget and seed
/// commit together on success only; inputs remain immutable. No allocation or
/// frame-wide stereo candidate selection is performed.
pub fn encode_band(
    entropy: &mut RangeEncoder,
    state: &mut PartitionState,
    controls: BandControls,
    left: &[i32],
    right: Option<&[i32]>,
    fold: Option<&[i32]>,
    options: BandEncoding,
) -> Result<Band, PartitionError> {
    let c = controls.partition;
    let n = validate_band(
        entropy.capacity() as u32 * 8,
        entropy.tell(),
        state,
        controls,
        fold,
    )?;
    if right.is_some() != controls.stereo.is_some()
        || !(-1..=1).contains(&options.theta_round)
        || options.amplitudes.iter().any(|&x| x < 0)
    {
        return Err(PartitionError::InvalidInput);
    }
    for input in std::iter::once(left).chain(right) {
        if input.len() != n {
            return Err(PartitionError::InvalidInput);
        }
        let mut energy = 0i64;
        for &x in input {
            energy = energy
                .checked_add(i64::from(x) * i64::from(x))
                .ok_or(PartitionError::InvalidInput)?;
            if energy > 5 << 46 {
                return Err(PartitionError::InvalidInput);
            }
        }
    }
    if entropy.is_finished() {
        return Err(EncoderError::Finished.into());
    }
    let mut writer = entropy.clone();
    let mut shared = state.clone();
    let mut result = Band {
        left: [0; 176],
        right: [0; 176],
        fold: [0; 176],
        length: n,
        stereo: right.is_some(),
        collapse: 0,
    };
    result.left[..n].copy_from_slice(left);
    if let Some(right) = right {
        result.right[..n].copy_from_slice(right);
    }
    let stereo = controls.stereo.unwrap_or(StereoControls {
        intensity: 21,
        disable_inversion: false,
    });
    let mut encoder = Encoder {
        entropy: &mut writer,
        state: &mut shared,
        band: c.band,
        spread: c.spread,
        avoid_split_noise: options.avoid_split_noise,
        intensity: stereo.intensity,
        disable_inv: stereo.disable_inversion,
        tf_change: controls.tf_change,
        amplitudes: options.amplitudes,
        theta_round: options.theta_round,
    };
    let node = Node {
        bits: c.bits,
        blocks: c.blocks,
        lm: c.lm,
        gain: c.gain,
        fill: u32::from(c.fill),
    };
    result.collapse = if result.stereo {
        encoder.stereo_band(
            &mut result.left[..n],
            &mut result.right[..n],
            fold,
            Some(&mut result.fold[..n]),
            node,
        )?
    } else {
        encoder.band(
            &mut result.left[..n],
            fold,
            Some(&mut result.fold[..n]),
            node,
        )?
    } as u8;
    *entropy = writer;
    *state = shared;
    Ok(result)
}
impl Encoder<'_> {
    fn signs(
        &mut self,
        x: &mut [i32],
        y: Option<&mut [i32]>,
        out: Option<&mut [i32]>,
    ) -> Result<u32, PartitionError> {
        for channel in std::iter::once(&mut *x).chain(y) {
            let sign = if self.state.remaining >= 8 {
                self.state.remaining -= 8;
                let sign = channel[0] < 0;
                self.entropy.encode_bits(u32::from(sign), 1)?;
                sign
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
        let transform =
            BandTransform::prepare(Some(&mut *x), fold, n, blocks, fill, self.tf_change)?;
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
        if self.amplitudes[0] < 2 || self.amplitudes[1] < 2 {
            if self.amplitudes[0] > self.amplitudes[1] {
                y.copy_from_slice(x);
            } else {
                x.copy_from_slice(y);
            }
        }
        let split = self.theta(
            x,
            y,
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
            let (primary, secondary) = if split.angle > 8192 {
                (&mut *y, &mut *x)
            } else {
                (&mut *x, &mut *y)
            };
            let sign = if side_bits != 0 {
                let negative =
                    mul31(primary[0], secondary[1]) - mul31(primary[1], secondary[0]) < 0;
                self.entropy.encode_bits(u32::from(negative), 1)?;
                1 - 2 * i32::from(negative)
            } else {
                1
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
