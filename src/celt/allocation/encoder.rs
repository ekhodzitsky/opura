//! Native dynamic requests, band retention and stereo allocation policies.
use super::*;
use crate::range::RangeEncoder;

/// Caller-selected allocation policies, before spectral quality analysis.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AllocationEncoding {
    /// Requested positive-boost flag counts; effective offsets are eighth bits.
    /// Counts beyond the physical/cap limit are naturally clipped; inactive ignored.
    pub boosts: [u8; 21],
    /// Requested trim 0..10; effective default is five if not signalled.
    pub trim: u8,
    /// Stereo boundary in selected start..=end, clipped to retained coded bands.
    /// Mono ignores this field (but requires at most 21).
    pub intensity: usize,
    /// Request independent channel coding below intensity when its field fits.
    pub dual_stereo: bool,
    /// Previous retained-band count 0..21, including initial zero.
    pub previous_coded_bands: usize,
    /// Inclusive highest desired band index 0..20 for native skip hysteresis.
    pub signal_bandwidth: usize,
}
/// Encode dynamic/trim/band/stereo allocation and return its effective budgets.
///
/// Physical capacity is 2..1275 bytes and selected layout is nonempty. Caller
/// controls stay unchanged. Common cap/interpolation/fine/PVQ/priority/balance
/// arithmetic is shared with decoding. The whole writer commits on success only,
/// with no allocation. Choosing perceptual policies and frame assembly are separate.
pub fn encode_allocation(
    entropy: &mut RangeEncoder,
    size: FrameSize,
    layout: BandLayout,
    transient: bool,
    options: AllocationEncoding,
) -> Result<Allocation, AllocationError> {
    encode_with(
        entropy,
        size,
        layout,
        transient,
        options,
        RangeEncoder::encode_bit_logp,
    )
}
fn encode_with(
    entropy: &mut RangeEncoder,
    size: FrameSize,
    layout: BandLayout,
    transient: bool,
    options: AllocationEncoding,
    write_bit: impl FnMut(&mut RangeEncoder, bool, u32) -> Result<(), EncoderError>,
) -> Result<Allocation, AllocationError> {
    encode_with_trim(entropy, size, layout, transient, options, write_bit, || {
        Ok(options.trim)
    })
}
// Callback side effects belong to the caller's surrounding stream transaction.
// The shared rate engine invokes it only when the trim symbol fits after boosts.
pub(crate) fn encode_analyzed(
    entropy: &mut RangeEncoder,
    size: FrameSize,
    layout: BandLayout,
    transient: bool,
    options: AllocationEncoding,
    trim: impl FnMut() -> Result<u8, AllocationError>,
) -> Result<Allocation, AllocationError> {
    encode_with_trim(
        entropy,
        size,
        layout,
        transient,
        options,
        RangeEncoder::encode_bit_logp,
        trim,
    )
}
fn encode_with_trim(
    entropy: &mut RangeEncoder,
    size: FrameSize,
    layout: BandLayout,
    transient: bool,
    options: AllocationEncoding,
    write_bit: impl FnMut(&mut RangeEncoder, bool, u32) -> Result<(), EncoderError>,
    trim: impl FnMut() -> Result<u8, AllocationError>,
) -> Result<Allocation, AllocationError> {
    validate_context(entropy.capacity() as u32 * 8, entropy.tell(), layout)?;
    if options.trim > 10
        || options.previous_coded_bands > 21
        || options.signal_bandwidth > 20
        || options.intensity > 21
        || layout.channels() == 2
            && !(layout.bands().start..=layout.bands().end).contains(&options.intensity)
    {
        return Err(AllocationError::InvalidControls);
    }
    if entropy.is_finished() {
        return Err(EncoderError::Finished.into());
    }
    let mut writer = entropy.clone();
    let result = {
        let mut engine = RateEngine {
            entropy: EncodeEntropy {
                writer: &mut writer,
                options,
                write_bit,
                trim,
            },
            c: layout.channels() as i32,
            lm: size.lm() as i32,
            start: layout.bands().start,
            end: layout.bands().end,
            result: new_allocation(size, layout, transient),
        };
        let total = engine.dynamic()?;
        engine.compute(total)?;
        engine.result
    };
    *entropy = writer;
    Ok(result)
}
struct EncodeEntropy<'a, W, T> {
    writer: &'a mut RangeEncoder,
    options: AllocationEncoding,
    write_bit: W,
    trim: T,
}
impl<
    W: FnMut(&mut RangeEncoder, bool, u32) -> Result<(), EncoderError>,
    T: FnMut() -> Result<u8, AllocationError>,
> RateEntropy for EncodeEntropy<'_, W, T>
{
    fn storage_bits(&self) -> u32 {
        self.writer.capacity() as u32 * 8
    }
    fn tell_frac(&self) -> u32 {
        self.writer.tell_frac()
    }
    fn boost(&mut self, band: usize, successful: i32, logp: u32) -> Result<bool, AllocationError> {
        let flag = successful < i32::from(self.options.boosts[band]);
        (self.write_bit)(self.writer, flag, logp)?;
        Ok(flag)
    }
    fn trim(&mut self) -> Result<u8, AllocationError> {
        let trim = (self.trim)()?;
        if trim > 10 {
            return Err(AllocationError::InvalidControls);
        }
        self.writer.encode_icdf(trim as usize, &TRIM, 7)?;
        Ok(trim)
    }
    fn keep_band(&mut self, c: SkipContext) -> Result<bool, AllocationError> {
        let band = c.coded - 1;
        let depth = if c.coded > 17 {
            if band < self.options.previous_coded_bands {
                7
            } else {
                9
            }
        } else {
            0
        };
        let keep = c.coded <= c.start + 2
            || c.band_bits > (((depth * c.band_width) << c.lm << 3) >> 4)
                && band <= self.options.signal_bandwidth;
        (self.write_bit)(self.writer, keep, 1)?;
        Ok(keep)
    }
    fn intensity(&mut self, start: usize, coded: usize) -> Result<usize, AllocationError> {
        let value = self.options.intensity.min(coded);
        self.writer
            .encode_uint((value - start) as u32, (coded + 1 - start) as u32)?;
        Ok(value)
    }
    fn dual(&mut self) -> Result<bool, AllocationError> {
        let flag = self.options.dual_stereo;
        (self.write_bit)(self.writer, flag, 1)?;
        Ok(flag)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_trim_entropy_tails_preserve_saving_and_allocation()
    -> Result<(), Box<dyn std::error::Error>> {
        use crate::celt::analysis::{self, IntensityState, TrimOptions, TrimState};
        let sizes = [
            FrameSize::Ms2_5,
            FrameSize::Ms5,
            FrameSize::Ms10,
            FrameSize::Ms20,
        ];
        let mut counts = [0; 2];
        for line in include_str!("../../../tests/celt-allocation-tail-fixtures.txt")
            .lines()
            .filter(|l| !l.starts_with('#'))
        {
            let fields: Vec<_> = line.split_whitespace().collect();
            assert_eq!(fields[0], "tail");
            assert_eq!(fields.len(), 103);
            let v: Vec<i64> = fields[1..102]
                .iter()
                .map(|s| s.parse())
                .collect::<Result<_, _>>()?;
            let size = sizes[usize::try_from(v[0])?];
            let c = usize::try_from(v[1])?;
            let bytes = usize::try_from(v[2])?;
            let remaining = usize::try_from(v[3])?;
            let boosts = if v[4] != 0 { [255; 21] } else { [0; 21] };
            let mut state = TrimState::from_saving(u16::try_from(v[5])?)?;
            let layout = BandLayout::new(c, 0, 21)?;
            let rate = analysis::equivalent_rate(size, c, bytes)?;
            let intensity = if c == 2 {
                IntensityState::from_band(8)?.select((rate / 1000) as i16, layout)
            } else {
                21
            };
            let mut values = vec![0; c * size.samples()];
            for channel in 0..c {
                for &band in &BANDS[..21] {
                    values[channel * size.samples() + (usize::from(band) << size.lm())] = 1 << 24;
                }
            }
            let mut writer = RangeEncoder::new(bytes)?;
            let mut left = bytes * 8 - 1 - remaining;
            while left > 0 {
                let bits = left.min(25) as u32;
                writer.encode_bits((1u32 << bits) - 1, bits)?;
                left -= bits as usize;
            }
            let mut calls = 0;
            let result = encode_analyzed(
                &mut writer,
                size,
                layout,
                false,
                AllocationEncoding {
                    boosts,
                    trim: 5,
                    intensity,
                    dual_stereo: false,
                    previous_coded_bands: 21,
                    signal_bandwidth: 20,
                },
                || {
                    calls += 1;
                    state
                        .process(
                            size,
                            &values,
                            &[[0; 21]; 2],
                            TrimOptions {
                                channels: c,
                                end: 21,
                                intensity,
                                equivalent_rate: rate,
                                tf_estimate: 8192,
                                surround_trim: 0,
                            },
                        )
                        .map(|r| r.index())
                        .map_err(AllocationError::Analysis)
                },
            )?;
            let native = &v[6..];
            assert_eq!(i64::from(state.stereo_saving()), native[0]);
            assert_eq!(if c == 2 { intensity as i64 } else { 8 }, native[1]);
            assert_eq!(
                [
                    i64::from(result.trim()),
                    result.intensity() as i64,
                    i64::from(result.dual_stereo()),
                    calls,
                    i64::from(writer.range()),
                    i64::from(writer.tell()),
                    i64::from(writer.tell_frac()),
                    result.coded_bands() as i64,
                    i64::from(result.balance())
                ],
                native[2..11]
            );
            counts[usize::try_from(calls)?] += 1;
            for band in 0..21 {
                assert_eq!(
                    [
                        i64::from(result.offsets()[band]),
                        i64::from(result.fine_bits()[band]),
                        i64::from(result.pvq_bits()[band]),
                        i64::from(result.fine_priority()[band])
                    ],
                    [
                        native[11 + band],
                        native[32 + band],
                        native[53 + band],
                        native[74 + band]
                    ]
                );
            }
            let expected: Vec<u8> = (0..fields[102].len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&fields[102][i..i + 2], 16))
                .collect::<Result<_, _>>()?;
            assert_eq!(writer.finish()?, expected);
        }
        assert_eq!(counts.iter().sum::<usize>(), 1056);
        assert!(counts[0] > 0 && counts[1] > 0);
        Ok(())
    }
    #[test]
    fn lazy_trim_errors_and_invalid_results_preserve_writer()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut writer = RangeEncoder::new(64)?;
        writer.encode_uint(13, 17)?;
        let before = writer.clone();
        let options = AllocationEncoding {
            boosts: [1; 21],
            trim: 5,
            intensity: 21,
            dual_stereo: true,
            previous_coded_bands: 21,
            signal_bandwidth: 20,
        };
        for error in [
            None,
            Some(AllocationError::Analysis(
                crate::celt::analysis::AnalysisError::ArithmeticOverflow,
            )),
        ] {
            let mut called = false;
            assert_eq!(
                encode_analyzed(
                    &mut writer,
                    FrameSize::Ms20,
                    BandLayout::new(2, 0, 21)?,
                    false,
                    options,
                    || {
                        called = true;
                        match error {
                            Some(e) => Err(e),
                            None => Ok(11),
                        }
                    }
                ),
                Err(error.unwrap_or(AllocationError::InvalidControls))
            );
            assert!(called);
            assert_eq!(writer, before);
        }
        Ok(())
    }
    #[test]
    fn injected_boost_skip_and_final_dual_failures_preserve_whole_writer()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut writer = RangeEncoder::new(128)?;
        writer.encode_uint(13, 17)?;
        let original = writer.clone();
        let options = AllocationEncoding {
            boosts: [0; 21],
            trim: 5,
            intensity: 21,
            dual_stereo: true,
            previous_coded_bands: 21,
            signal_bandwidth: 20,
        };
        let layout = BandLayout::new(2, 0, 21)?;
        let mut successful = writer.clone();
        let mut count = 0;
        let expected = encode_with(
            &mut successful,
            FrameSize::Ms20,
            layout,
            true,
            options,
            |writer, value, logp| {
                count += 1;
                writer.encode_bit_logp(value, logp)
            },
        )?;
        assert!(count >= 23);
        assert!(expected.dual_stereo());
        assert_ne!(successful, original);
        for fail_at in [1, 2, count / 2, count] {
            let mut attempts = 0;
            assert_eq!(
                encode_with(
                    &mut writer,
                    FrameSize::Ms20,
                    layout,
                    true,
                    options,
                    |staged, value, logp| {
                        attempts += 1;
                        if attempts == fail_at {
                            if attempts > 1 {
                                assert_ne!(staged, &original);
                            }
                            Err(EncoderError::CapacityExhausted)
                        } else {
                            staged.encode_bit_logp(value, logp)
                        }
                    }
                ),
                Err(AllocationError::Encoding(EncoderError::CapacityExhausted))
            );
            assert_eq!(attempts, fail_at);
            assert_eq!(writer, original);
        }
        assert_eq!(
            encode_allocation(&mut writer, FrameSize::Ms20, layout, true, options)?,
            expected
        );
        assert_eq!(writer, successful);
        Ok(())
    }
}
