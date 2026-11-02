//! Whole-spectrum encoding and native weighted stereo theta candidate selection.
use super::*;
use crate::{
    celt::partition::encoder::Encoder,
    range::{EncoderError, RangeEncoder},
};

/// Borrowed normalized full-length spectra and linear amplitudes.
#[derive(Clone, Copy, Debug)]
pub struct SpectrumInput<'a> {
    /// Mono/left Q24 spectrum, exactly frame-size samples.
    pub left: &'a [i32],
    /// Right spectrum, present exactly for a stereo allocation.
    pub right: Option<&'a [i32]>,
    /// Nonnegative linear amplitudes; inactive bands/channels are ignored.
    pub amplitudes: [[i32; 21]; 2],
}
/// Full-spectrum seed, downmix and stereo candidate policies.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SpectrumEncoding {
    /// Initial folding/noise seed; returned spectrum exposes the resulting seed.
    pub seed: u32,
    /// Suppress stereo inversion, for downmix contexts.
    pub disable_inversion: bool,
    /// Compare biased down/up theta candidates, keeping down on score ties.
    /// Native policy disables this for initially dual-stereo or mono frames.
    pub theta_rdo: bool,
}
/// Encode all allocated normalized bands before anti-collapse and final energy.
///
/// Allocation/TF must already describe this frame. Each selected input band's
/// squared Q24 norm is <=5/4, as for band encoding. Inactive coefficients are
/// ignored and return zero. Selected amplitudes are nonnegative; when stereo
/// RDO is active, each amplitude plus one third of the smaller amplitude plus
/// one must fit positive i32. The whole writer commits on success only; input
/// stays unchanged and no allocation occurs. Frame controls, allocation encoding,
/// energy stages and packet construction are separate caller responsibilities.
pub fn encode_spectrum(
    entropy: &mut RangeEncoder,
    allocation: &Allocation,
    tf: &TfResolution,
    spread: Spread,
    input: SpectrumInput<'_>,
    options: SpectrumEncoding,
) -> Result<Spectrum, PartitionError> {
    validate_spectrum(
        entropy.capacity() as u32 * 8,
        entropy.tell(),
        allocation,
        tf,
    )?;
    let layout = allocation.layout();
    let size = allocation.size();
    let n = size.samples();
    let lm = size.lm();
    if input.left.len() != n
        || input.right.is_some() != (layout.channels() == 2)
        || input.right.is_some_and(|r| r.len() != n)
    {
        return Err(PartitionError::InvalidInput);
    }
    let theta_rdo = options.theta_rdo && layout.channels() == 2 && !allocation.dual_stereo();
    for band in layout.bands() {
        for (c, values) in std::iter::once(input.left).chain(input.right).enumerate() {
            let mut energy = 0i64;
            for &x in &values[usize::from(BANDS[band]) << lm..usize::from(BANDS[band + 1]) << lm] {
                energy = energy
                    .checked_add(i64::from(x) * i64::from(x))
                    .ok_or(PartitionError::InvalidInput)?;
                if energy > 5 << 46 {
                    return Err(PartitionError::InvalidInput);
                }
            }
            if input.amplitudes[c][band] < 0 {
                return Err(PartitionError::InvalidInput);
            }
        }
        if theta_rdo && band < allocation.intensity() {
            channel_weights([input.amplitudes[0][band], input.amplitudes[1][band]])?;
        }
    }
    if entropy.is_finished() {
        return Err(EncoderError::Finished.into());
    }
    let mut writer = entropy.clone();
    let mut shared = PartitionState {
        remaining: 0,
        seed: options.seed,
    };
    let mut result = Spectrum {
        values: [[0; 960]; 2],
        masks: [[0; 21]; 2],
        size,
        layout,
        seed: options.seed,
    };
    let first = usize::from(BANDS[layout.bands().start]) << lm;
    let last = usize::from(BANDS[layout.bands().end]) << lm;
    result.values[0][first..last].copy_from_slice(&input.left[first..last]);
    if let Some(right) = input.right {
        result.values[1][first..last].copy_from_slice(&right[first..last]);
    }
    let encoder = Encoder {
        entropy: &mut writer,
        state: &mut shared,
        band: layout.bands().start,
        spread,
        avoid_split_noise: false,
        intensity: allocation.intensity(),
        disable_inv: options.disable_inversion,
        tf_change: 0,
        amplitudes: [0; 2],
        theta_round: 0,
    };
    let mut coder = SpectrumEncoder { encoder, theta_rdo };
    process_bands(
        &mut coder,
        allocation,
        tf,
        spread,
        &mut result,
        &input.amplitudes,
    )?;
    *entropy = writer;
    Ok(result)
}
struct SpectrumEncoder<'a> {
    encoder: Encoder<'a>,
    theta_rdo: bool,
}
impl BandCoder for SpectrumEncoder<'_> {
    fn storage_bits(&self) -> u32 {
        self.encoder.entropy.capacity() as u32 * 8
    }
    fn tell_frac(&self) -> u32 {
        self.encoder.entropy.tell_frac()
    }
    fn seed(&self) -> u32 {
        self.encoder.state.seed
    }
    fn setup(
        &mut self,
        band: usize,
        tf: i8,
        remaining: i32,
        amplitudes: [i32; 2],
        avoid_noise: bool,
    ) {
        self.encoder.band = band;
        self.encoder.tf_change = tf;
        self.encoder.state.remaining = remaining;
        self.encoder.amplitudes = amplitudes;
        self.encoder.avoid_split_noise = avoid_noise;
    }
    fn band(
        &mut self,
        x: &mut [i32],
        fold: Option<&[i32]>,
        out: Option<&mut [i32]>,
        node: Node,
    ) -> Result<u32, PartitionError> {
        self.encoder.band(x, fold, out, node)
    }
    fn stereo_band(
        &mut self,
        x: &mut [i32],
        y: &mut [i32],
        fold: Option<&[i32]>,
        mut out: Option<&mut [i32]>,
        node: Node,
    ) -> Result<u32, PartitionError> {
        let e = &mut self.encoder;
        if !self.theta_rdo || e.band >= e.intensity {
            e.theta_round = 0;
            return e.stereo_band(x, y, fold, out, node);
        }
        let n = x.len();
        let weights = channel_weights(e.amplitudes)?;
        let mut original = [[0; 176]; 2];
        original[0][..n].copy_from_slice(x);
        original[1][..n].copy_from_slice(y);
        let before_writer = e.entropy.clone();
        let before_state = e.state.clone();
        e.theta_round = -1;
        let mask_down = e.stereo_band(x, y, fold, out.as_deref_mut(), node)?;
        let score_down = score(&original, x, y, weights)?;
        let mut down = [[0; 176]; 3];
        down[0][..n].copy_from_slice(x);
        down[1][..n].copy_from_slice(y);
        if let Some(out) = out.as_deref() {
            down[2][..n].copy_from_slice(out);
        }
        let writer_down = e.entropy.clone();
        let state_down = e.state.clone();
        *e.entropy = before_writer;
        *e.state = before_state;
        x.copy_from_slice(&original[0][..n]);
        y.copy_from_slice(&original[1][..n]);
        e.theta_round = 1;
        let mask_up = e.stereo_band(x, y, fold, out.as_deref_mut(), node)?;
        let score_up = score(&original, x, y, weights)?;
        if score_down >= score_up {
            *e.entropy = writer_down;
            *e.state = state_down;
            x.copy_from_slice(&down[0][..n]);
            y.copy_from_slice(&down[1][..n]);
            if let Some(out) = out {
                out.copy_from_slice(&down[2][..n]);
            }
            Ok(mask_down)
        } else {
            Ok(mask_up)
        }
    }
}
fn channel_weights(amplitudes: [i32; 2]) -> Result<[i32; 2], PartitionError> {
    let min = amplitudes[0].min(amplitudes[1]);
    let mut energy = [0; 2];
    for c in 0..2 {
        energy[c] = amplitudes[c]
            .checked_add(min / 3)
            .ok_or(PartitionError::InvalidInput)?;
    }
    let max = energy[0]
        .max(energy[1])
        .checked_add(1)
        .ok_or(PartitionError::InvalidInput)?;
    let shift = 31 - max.leading_zeros() as i32 - 14;
    Ok(energy.map(|x| if shift > 0 { x >> shift } else { x << -shift }))
}
fn score(
    original: &[[i32; 176]; 2],
    left: &[i32],
    right: &[i32],
    weights: [i32; 2],
) -> Result<i32, PartitionError> {
    let mut result = 0i64;
    for (c, values) in [left, right].into_iter().enumerate() {
        let sum: i64 = original[c]
            .iter()
            .zip(values)
            .map(|(&a, &b)| i64::from(a) * i64::from(b))
            .sum();
        result += (i64::from(weights[c]) * (sum >> 20)) >> 15;
    }
    i32::try_from(result).map_err(|_| PartitionError::InvalidInput)
}

// Test-only late-failure injection: absent from ordinary library builds.
#[cfg(test)]
std::thread_local! {
    static FAILURE: std::cell::Cell<Option<(usize,PartitionError)>> = const { std::cell::Cell::new(None) };
}
#[cfg(test)]
pub(super) fn failure_after_band(band: usize) -> Result<(), PartitionError> {
    FAILURE.with(|slot| match slot.get() {
        Some((at, error)) if at == band => Err(error),
        _ => Ok(()),
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    struct FailureGuard;
    impl Drop for FailureGuard {
        fn drop(&mut self) {
            FAILURE.with(|slot| slot.set(None));
        }
    }
    #[test]
    fn injected_errors_after_complete_shape_work_preserve_original_writer()
    -> Result<(), Box<dyn std::error::Error>> {
        use crate::celt::{
            allocation, control,
            energy::{BandLayout, EnergyState},
        };
        let mut lines = include_str!("../../../../tests/celt-spectrum-encoder-fixtures.txt")
            .lines()
            .filter(|l| !l.starts_with('#'));
        while let Some(line) = lines.next() {
            let fields: Vec<_> = line.split_whitespace().collect();
            let symbols: Vec<i64> = lines
                .next()
                .ok_or("symbols")?
                .split_whitespace()
                .skip(1)
                .map(|v| v.parse())
                .collect::<Result<_, _>>()?;
            let values: Vec<i32> = lines
                .next()
                .ok_or("input")?
                .split_whitespace()
                .skip(1)
                .map(|v| v.parse())
                .collect::<Result<_, _>>()?;
            let amp: Vec<i32> = lines
                .next()
                .ok_or("amplitudes")?
                .split_whitespace()
                .skip(1)
                .map(|v| v.parse())
                .collect::<Result<_, _>>()?;
            let _ = lines.next().ok_or("spectrum")?;
            if fields[1] != "3"
                || fields[2] != "2"
                || fields[3] != "0"
                || fields[4] != "21"
                || fields[5] != "0"
                || fields[9] != "1"
                || fields[10].len() != 240
            {
                continue;
            }
            let bytes: Vec<u8> = (0..fields[10].len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&fields[10][i..i + 2], 16))
                .collect::<Result<_, _>>()?;
            let mut dec = RangeDecoder::new(&bytes);
            let layout = BandLayout::new(2, 0, 21)?;
            let tf = control::decode_tf(&mut dec, FrameSize::Ms20, 0..21, false)?;
            let allocation =
                allocation::decode_allocation(&mut dec, FrameSize::Ms20, layout, false)?;
            if allocation.dual_stereo() {
                continue;
            }
            EnergyState::default().fine(&mut dec, layout, allocation.fine_bits())?;
            let mut writer = RangeEncoder::new(bytes.len())?;
            let mut pos = 0;
            while pos < symbols.len() {
                let (kind, value, arg, n) = (
                    symbols[pos],
                    symbols[pos + 1] as u32,
                    symbols[pos + 2] as u32,
                    symbols[pos + 3] as usize,
                );
                pos += 4;
                match kind {
                    0 => writer.encode_bit_logp(value != 0, arg)?,
                    1 => writer.encode_bits(value, arg)?,
                    2 => writer.encode_uint(value, arg)?,
                    3 => {
                        let cdf: Vec<u8> = symbols[pos..pos + n].iter().map(|&x| x as u8).collect();
                        writer.encode_icdf(value as usize, &cdf, arg)?;
                    }
                    _ => return Err("unknown prefix kind".into()),
                }
                pos += n;
            }
            let original = writer.clone();
            let before = values.clone();
            let input = SpectrumInput {
                left: &values[..960],
                right: Some(&values[960..]),
                amplitudes: std::array::from_fn(|c| std::array::from_fn(|i| amp[c * 21 + i])),
            };
            let options = SpectrumEncoding {
                seed: 123,
                disable_inversion: false,
                theta_rdo: true,
            };
            let mut successful = writer.clone();
            let expected = encode_spectrum(
                &mut successful,
                &allocation,
                &tf,
                Spread::Normal,
                input,
                options,
            )?;
            assert_ne!(successful, original);
            for error in [
                PartitionError::Encoding(EncoderError::CapacityExhausted),
                PartitionError::Band(crate::celt::bands::BandError::InvalidVector),
            ] {
                let guard = FailureGuard;
                FAILURE.with(|slot| slot.set(Some((20, error))));
                assert_eq!(
                    encode_spectrum(
                        &mut writer,
                        &allocation,
                        &tf,
                        Spread::Normal,
                        input,
                        options
                    ),
                    Err(error)
                );
                assert_eq!(writer, original);
                assert_eq!(values, before);
                drop(guard);
            }
            assert_eq!(
                encode_spectrum(
                    &mut writer,
                    &allocation,
                    &tf,
                    Spread::Normal,
                    input,
                    options
                )?,
                expected
            );
            assert_eq!(writer, successful);
            return Ok(());
        }
        Err("missing RDO fixture context".into())
    }
    #[test]
    fn native_rdo_weights_and_scores_match() -> Result<(), Box<dyn std::error::Error>> {
        let mut count = 0;
        for line in include_str!("../../../../tests/celt-rdo-math-fixtures.txt").lines() {
            if line.starts_with('#') {
                continue;
            }
            let c: Vec<i32> = line
                .split_whitespace()
                .skip(1)
                .map(|x| x.parse())
                .collect::<Result<_, _>>()?;
            let weights = channel_weights([c[0], c[1]])?;
            assert_eq!(weights, c[10..12]);
            let mut original = [[0; 176]; 2];
            original[0][..2].copy_from_slice(&c[2..4]);
            original[1][..2].copy_from_slice(&c[4..6]);
            assert_eq!(score(&original, &c[6..8], &c[8..10], weights)?, c[12]);
            count += 1;
        }
        assert_eq!(count, 4271);
        Ok(())
    }
}
