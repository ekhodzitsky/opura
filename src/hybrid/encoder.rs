//! Fixed-band Hybrid PCM encoding and native low/high-band rate controls.
//!
//! [`HybridEncoder`] connects both layers in one entropy stream for10/20ms
//! mono/stereo SWB/FB packets with optional DTX/FEC. General mode/channel adaptation
//! remain under development. Rate controls are also available independently.
//! Adapted from Opus v1.6.1 `src/opus_encoder.c`; see THIRD_PARTY_NOTICES.md.
use crate::{Bandwidth, silk::gains::FrameDuration};
use std::fmt;

#[path = "pcm_encoder.rs"]
mod pcm;
pub use pcm::{EncodedPacket, EncoderError, HybridEncoder, Options};
pub(crate) use pcm::{
    PreparedFrame, fade,
    frame::{HybridFrameEncoder, OuterControls},
};

/// Invalid Hybrid rate allocation inputs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum RateError {
    /// Hybrid requires one or two encoded channels.
    InvalidChannels,
    /// Hybrid supports superwideband or fullband audio.
    InvalidBandwidth,
    /// Shared entropy storage must contain 2..=1275 bytes, excluding the TOC.
    InvalidBudget,
    /// Target payload bitrate must be positive and fit the physical budget.
    InvalidBitrate,
}
impl fmt::Display for RateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidChannels => "Hybrid requires one or two encoded channels",
            Self::InvalidBandwidth => "Hybrid requires superwideband or fullband audio",
            Self::InvalidBudget => "Hybrid shared entropy budget must be 2..=1275 bytes",
            Self::InvalidBitrate => "Hybrid target payload bitrate must fit the byte budget",
        })
    }
}
impl std::error::Error for RateError {}

/// Inputs after the outer encoder has removed TOC/transition redundancy costs.
/// No transition tail or surround energy mask is included in these calculations.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RateControls {
    /// Standard 10/20 ms frame duration.
    pub duration: FrameDuration,
    /// Superwideband or fullband encoded bandwidth.
    pub bandwidth: Bandwidth,
    /// Encoded channel count, one or two.
    pub channels: usize,
    /// Total target payload bitrate for both layers and all channels, in bits/s.
    /// This excludes the outer TOC cost and must not exceed the physical budget.
    pub bitrate: u32,
    /// Shared payload capacity, 2..=1275 bytes, excluding the outer TOC.
    pub payload_bytes: usize,
    /// Native VBR policy; false permits SILK to borrow 25% of the remaining bits.
    pub variable_bitrate: bool,
    /// Native selected LBRR policy, rather than merely the user's FEC request.
    pub fec: bool,
}

/// Native low-band rate and entropy limit for a shared Hybrid frame.
///
/// Very small budgets can produce unusable layer rates. This calculation does
/// not certify that the frame can be encoded; packet admission must still check
/// the actual shared writer. In CBR Hybrid, SILK uses a variable-length capped
/// search and CELT absorbs its variations to fill the complete payload.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RateAllocation {
    silk_rate: u32,
    high_band_rate: i32,
    silk_max_bits: u32,
}
impl RateAllocation {
    /// Evaluate the original per-channel interpolation, stereo correction and
    /// CBR/VBR cap. The caller supplies a payload bitrate, not a TOC-inclusive
    /// requested stream bitrate. No stream state or entropy writer is modified.
    pub fn new(c: RateControls) -> Result<Self, RateError> {
        Self::with_maximum(c, (c.payload_bytes as u32).saturating_mul(8))
    }
    pub(crate) fn with_maximum(c: RateControls, maximum: u32) -> Result<Self, RateError> {
        if !(1..=2).contains(&c.channels) {
            return Err(RateError::InvalidChannels);
        }
        if !matches!(c.bandwidth, Bandwidth::SuperWideband | Bandwidth::Fullband) {
            return Err(RateError::InvalidBandwidth);
        }
        if !(2..=1275).contains(&c.payload_bytes) {
            return Err(RateError::InvalidBudget);
        }
        let frames_per_second = match c.duration {
            FrameDuration::Ms10 => 100,
            FrameDuration::Ms20 => 50,
        };
        if maximum > c.payload_bytes as u32 * 8 || maximum == 0 {
            return Err(RateError::InvalidBudget);
        }
        if c.bitrate == 0 || c.bitrate > c.payload_bytes as u32 * 8 * frames_per_second {
            return Err(RateError::InvalidBitrate);
        }
        let target_rate = silk_rate(c, c.bitrate);
        let silk_max_bits = if c.variable_bitrate {
            silk_rate(c, maximum * frames_per_second) / frames_per_second
        } else {
            let other = maximum.saturating_sub(target_rate / frames_per_second);
            maximum - other * 3 / 4
        };
        Ok(Self {
            silk_rate: target_rate,
            high_band_rate: c.bitrate as i32 - target_rate as i32,
            silk_max_bits,
        })
    }
    pub(crate) fn apply_surround(
        &mut self,
        context: crate::encoder::SurroundContext,
        channels: usize,
        bandwidth: Bandwidth,
        variable: bool,
    ) {
        let original = self.silk_rate;
        self.silk_rate = context.silk_rate(original, bandwidth, channels, variable);
        self.high_band_rate += original as i32 - self.silk_rate as i32;
    }
    /// SILK target bits/s for all encoded channels.
    pub fn silk_bitrate(self) -> u32 {
        self.silk_rate
    }
    /// Remaining target bits/s for CELT. May be negative at unusably low rates;
    /// it is retained rather than silently altering the original rate policy.
    pub fn high_band_bitrate(self) -> i32 {
        self.high_band_rate
    }
    /// SILK's entropy-search limit, measured from the start of the shared stream.
    /// This is separate from SILK's target bitrate and the writer's capacity.
    pub fn silk_max_bits(self) -> u32 {
        self.silk_max_bits
    }
}

fn silk_rate(c: RateControls, bitrate: u32) -> u32 {
    // Native 10/20 ms columns currently coincide, retained separately so the
    // control contract follows the original table rather than an approximation.
    const TABLE: [[u32; 5]; 7] = [
        [0, 0, 0, 0, 0],
        [12000, 10000, 10000, 11000, 11000],
        [16000, 13500, 13500, 15000, 15000],
        [20000, 16000, 16000, 18000, 18000],
        [24000, 18000, 18000, 21000, 21000],
        [32000, 22000, 22000, 28000, 28000],
        [64000, 38000, 38000, 50000, 50000],
    ];
    let rate = bitrate / c.channels as u32;
    let column = 1 + usize::from(c.duration == FrameDuration::Ms20) + 2 * usize::from(c.fec);
    let mut selected = TABLE[6][column] + rate.saturating_sub(TABLE[6][0]) / 2;
    for pair in TABLE.windows(2) {
        let [low, high] = [pair[0], pair[1]];
        if rate < high[0] {
            selected = (low[column] * (high[0] - rate) + high[column] * (rate - low[0]))
                / (high[0] - low[0]);
            break;
        }
    }
    selected += u32::from(!c.variable_bitrate) * 100;
    selected += u32::from(c.bandwidth == Bandwidth::SuperWideband) * 300;
    selected *= c.channels as u32;
    if c.channels == 2 && rate >= 12000 {
        selected -= 1000;
    }
    selected
}

#[cfg(test)]
mod tests {
    use super::*;

    fn controls() -> RateControls {
        RateControls {
            duration: FrameDuration::Ms20,
            bandwidth: Bandwidth::Fullband,
            channels: 2,
            bitrate: 32000,
            payload_bytes: 120,
            variable_bitrate: false,
            fec: false,
        }
    }

    #[test]
    fn target_rate_and_physical_cap_have_separate_roles() -> Result<(), RateError> {
        let c = controls();
        let normal = RateAllocation::new(c)?;
        assert_eq!(normal.silk_bitrate(), 26200);
        assert_eq!(normal.high_band_bitrate(), 5800);
        assert_eq!(normal.silk_max_bits(), 633);
        let larger = RateAllocation::new(RateControls {
            payload_bytes: 160,
            ..c
        })?;
        assert_eq!(larger.silk_bitrate(), normal.silk_bitrate());
        assert_eq!(larger.silk_max_bits(), 713);
        let variable = RateAllocation::new(RateControls {
            variable_bitrate: true,
            ..c
        })?;
        assert_eq!(variable.silk_bitrate(), 26000);
        assert_eq!(variable.silk_max_bits(), 700);
        let fec = RateAllocation::new(RateControls { fec: true, ..c })?;
        assert_eq!(fec.silk_bitrate(), 29200);
        assert_eq!(fec.silk_max_bits(), 678);
        Ok(())
    }

    #[test]
    fn invalid_controls_fail_before_arithmetic() {
        let c = controls();
        for channels in [0, 3, usize::MAX] {
            assert_eq!(
                RateAllocation::new(RateControls { channels, ..c }),
                Err(RateError::InvalidChannels)
            );
        }
        for bandwidth in [
            Bandwidth::Narrowband,
            Bandwidth::Mediumband,
            Bandwidth::Wideband,
        ] {
            assert_eq!(
                RateAllocation::new(RateControls { bandwidth, ..c }),
                Err(RateError::InvalidBandwidth)
            );
        }
        for payload_bytes in [0, 1, 1276, usize::MAX] {
            assert_eq!(
                RateAllocation::new(RateControls { payload_bytes, ..c }),
                Err(RateError::InvalidBudget)
            );
        }
        for bitrate in [0, 48001, u32::MAX] {
            assert_eq!(
                RateAllocation::new(RateControls { bitrate, ..c }),
                Err(RateError::InvalidBitrate)
            );
        }
    }

    #[test]
    fn original_rate_split_and_shared_caps_match() -> Result<(), Box<dyn std::error::Error>> {
        let mut count = 0;
        for line in include_str!("../../tests/hybrid-encoder-rate-fixtures.txt")
            .lines()
            .filter(|line| !line.starts_with('#'))
        {
            let v = line
                .split_whitespace()
                .map(str::parse)
                .collect::<Result<Vec<i32>, _>>()?;
            assert_eq!(v.len(), 10);
            let controls = RateControls {
                duration: if v[0] == 10 {
                    FrameDuration::Ms10
                } else {
                    FrameDuration::Ms20
                },
                bandwidth: if v[1] == 1104 {
                    Bandwidth::SuperWideband
                } else {
                    Bandwidth::Fullband
                },
                channels: usize::try_from(v[2])?,
                bitrate: u32::try_from(v[3])?,
                payload_bytes: usize::try_from(v[4])?,
                variable_bitrate: v[5] != 0,
                fec: v[6] != 0,
            };
            let result = RateAllocation::new(controls)?;
            assert_eq!(
                [
                    result.silk_bitrate() as i32,
                    result.high_band_bitrate(),
                    result.silk_max_bits() as i32
                ],
                v[7..],
                "{line}"
            );
            count += 1;
        }
        assert_eq!(count, 8688);
        Ok(())
    }
}
