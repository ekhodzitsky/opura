//! Private per-packet surround controls shared by the common owner and CELT.
//! Copyright (c)2010-2011 Xiph.Org Foundation, Skype Limited; written by
//! Jean-Marc Valin and Koen Vos. See THIRD_PARTY_NOTICES.md.
use super::FrontendError;
use crate::Bandwidth;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct SurroundContext {
    pub lfe: bool,
    pub mask: Option<[[i32; 21]; 2]>,
}
impl SurroundContext {
    pub fn new(
        lfe: bool,
        masks: Option<&[[i32; 21]]>,
        channels: usize,
    ) -> Result<Self, FrontendError> {
        if !(1..=2).contains(&channels) || (lfe && channels != 1) {
            return Err(FrontendError::InvalidGeometry);
        }
        let mask = if let Some(masks) = masks {
            if masks.len() != channels
                || masks
                    .iter()
                    .flatten()
                    .any(|&v| !(-(32 << 24)..=32 << 24).contains(&v))
            {
                return Err(FrontendError::InvalidControl);
            }
            let mut values = [[0; 21]; 2];
            values[..channels].copy_from_slice(masks);
            Some(values)
        } else {
            None
        };
        Ok(Self { lfe, mask })
    }
    pub fn masked(self) -> bool {
        self.mask.is_some()
    }
    // Native common-owner VBR speech masking; called after base Hybrid split.
    pub fn silk_rate(self, base: u32, band: Bandwidth, channels: usize, variable: bool) -> u32 {
        if !variable || self.lfe {
            return base;
        }
        let Some(mask) = self.mask else {
            return base;
        };
        let (end, rate) = match band {
            Bandwidth::Narrowband => (13, 8000),
            Bandwidth::Mediumband => (15, 12000),
            _ => (17, 16000),
        };
        let mut sum = 0;
        for values in &mask[..channels] {
            for &value in &values[..end] {
                let mut value = value.clamp(-(2 << 24), 1 << 23);
                if value > 0 {
                    value >>= 1;
                }
                sum += value;
            }
        }
        // Preserve the original left-associative division/multiplication.
        let depth = sum / end as i32 * channels as i32 + 3355443;
        let offset = ((rate * (depth >> 14) + 512) >> 10).max(-(2 * base as i32) / 3);
        let offset = if matches!(band, Bandwidth::SuperWideband | Bandwidth::Fullband) {
            3 * offset / 5
        } else {
            offset
        };
        (base as i32 + offset) as u32
    }
}
