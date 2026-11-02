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
struct EncodeEntropy<'a, W> {
    writer: &'a mut RangeEncoder,
    options: AllocationEncoding,
    write_bit: W,
}
impl<W: FnMut(&mut RangeEncoder, bool, u32) -> Result<(), EncoderError>> RateEntropy
    for EncodeEntropy<'_, W>
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
        self.writer
            .encode_icdf(self.options.trim as usize, &TRIM, 7)?;
        Ok(self.options.trim)
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
