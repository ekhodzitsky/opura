//! Original unconstrained Hybrid VBR target and post-control shrink policy.
use super::{FrameSize, allocation::AllocationError};
use crate::range::RangeEncoder;

#[derive(Clone, Copy)]
pub(crate) struct HybridVbr {
    /// CELT target bitrate after subtracting the SILK target from stream rate.
    pub rate: u32,
    /// Actual mid-channel SILK quantization offset in Q10.
    pub offset: u16,
}
#[derive(Clone, Copy)]
pub(crate) struct VbrBudget {
    pub size: FrameSize,
    pub channels: usize,
    pub capacity: usize,
    pub initial_tell_q3: u32,
    pub tell_q3: u32,
    pub boost_q3: u32,
    pub temporal_q14: i16,
}
impl HybridVbr {
    pub fn analysis_bytes(self, size: FrameSize) -> usize {
        self.rate as usize / if size == FrameSize::Ms10 { 100 } else { 50 } / 8
    }
    pub fn bytes(self, b: VbrBudget) -> usize {
        let lm = b.size.lm();
        let fps = 400 >> lm;
        let vbr_rate = (self.rate / fps as u32) as i32 * 8;
        let mut target = (vbr_rate - ((9 * b.channels as i32 + 4) << 3)).max(0);
        if self.offset < 100 {
            target += (12 << 3) >> (3 - lm);
        }
        if self.offset > 100 {
            target -= (18 << 3) >> (3 - lm);
        }
        target += ((i32::from(b.temporal_q14) - 4096) * (50 << 3)) >> 14;
        if b.temporal_q14 > 11469 {
            target = target.max(50 << 3);
        }
        target += b.tell_q3 as i32;
        let minimum = (b.tell_q3 + b.boost_q3).div_ceil(64) + 2;
        let minimum = minimum.max((b.initial_tell_q3 + 37 * 8 + b.boost_q3).div_ceil(64));
        // Preserve signed rounding before the nonnegative minimum is applied.
        let requested = ((target + 32) >> 6).max(minimum as i32) as usize;
        requested.min(b.capacity.min(1275 >> (3 - lm)))
    }
    pub fn resize(
        self,
        writer: &mut RangeEncoder,
        mut b: VbrBudget,
        boost_q3: u32,
    ) -> Result<(), AllocationError> {
        b.tell_q3 = writer.tell_frac();
        b.boost_q3 = boost_q3;
        writer.shrink(self.bytes(b))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_vbr_targets_and_minimum_lengths_match() -> Result<(), Box<dyn std::error::Error>> {
        let mut rows = 0;
        for line in include_str!("../../tests/hybrid-encoder-vbr-fixtures.txt")
            .lines()
            .filter(|l| !l.starts_with('#'))
        {
            let v = line
                .split_whitespace()
                .map(str::parse)
                .collect::<Result<Vec<u32>, _>>()?;
            let [
                lm,
                channels,
                capacity,
                rate,
                offset,
                temporal,
                initial,
                tell,
                boost,
                expected,
            ]: [u32; 10] = v.try_into().map_err(|_| "bad row")?;
            let target = HybridVbr {
                rate,
                offset: offset as u16,
            };
            let size = if lm == 2 {
                FrameSize::Ms10
            } else {
                FrameSize::Ms20
            };
            assert_eq!(
                target.bytes(VbrBudget {
                    size,
                    channels: channels as usize,
                    capacity: capacity as usize,
                    initial_tell_q3: initial,
                    tell_q3: tell,
                    boost_q3: boost,
                    temporal_q14: temporal as i16
                }),
                expected as usize,
                "{line}"
            );
            assert_eq!(
                target.analysis_bytes(size),
                rate as usize / if lm == 2 { 800 } else { 400 }
            );
            rows += 1;
        }
        assert_eq!(rows, 20160);
        Ok(())
    }
}
