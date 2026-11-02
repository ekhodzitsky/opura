//! Fixed-band eligibility from original Opus Hybrid equivalent-rate/FEC policy.
use super::Options;
use crate::{Bandwidth, silk::gains::FrameDuration};

#[derive(Clone, Copy)]
pub(super) struct FecRate {
    pub bitrate: u32,
    pub variable: bool,
}
pub(super) fn eligible(
    enabled: bool,
    previous: bool,
    bandwidth: Bandwidth,
    channels: usize,
    duration: FrameDuration,
    rate: FecRate,
    options: Options,
) -> bool {
    if !enabled || options.packet_loss_percent == 0 {
        return false;
    }
    let fps = if duration == FrameDuration::Ms10 {
        100
    } else {
        50
    };
    let mut equivalent = rate.bitrate as i32;
    if fps > 50 {
        equivalent -= (40 * channels as i32 + 20) * (fps - 50);
    }
    if !rate.variable {
        equivalent -= equivalent / 12;
    }
    equivalent = equivalent * (90 + i32::from(options.complexity)) / 100;
    if options.complexity < 2 {
        equivalent = equivalent * 4 / 5;
    }
    let loss = i32::from(options.packet_loss_percent);
    equivalent -= equivalent * loss / (6 * loss + 10);
    let threshold = if bandwidth == Bandwidth::SuperWideband {
        20000
    } else {
        22000
    } + if previous { -1000 } else { 1000 };
    let threshold = (threshold * (125 - loss.min(25)) * 655) >> 16;
    // The original general frontend can lower bandwidth and change to SILK to
    // enable FEC. A fixed Hybrid stream accepts only its first, unchanged band.
    equivalent > threshold
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_fixed_band_eligibility_matches() -> Result<(), Box<dyn std::error::Error>> {
        let mut rows = 0;
        for (fixture, variable) in [
            (
                include_str!("../../tests/hybrid-encoder-fec-policy-fixtures.txt"),
                false,
            ),
            (
                include_str!("../../tests/hybrid-encoder-vbr-fec-policy-fixtures.txt"),
                true,
            ),
        ] {
            for line in fixture.lines().filter(|l| !l.starts_with('#')) {
                let v = line
                    .split_whitespace()
                    .map(str::parse)
                    .collect::<Result<Vec<usize>, _>>()?;
                let [
                    band,
                    channels,
                    ms,
                    previous,
                    enabled,
                    complexity,
                    loss,
                    bytes,
                    expected,
                ]: [usize; 9] = v.try_into().map_err(|_| "bad row")?;
                let actual = eligible(
                    enabled != 0,
                    previous != 0,
                    if band == 1104 {
                        Bandwidth::SuperWideband
                    } else {
                        Bandwidth::Fullband
                    },
                    channels,
                    if ms == 10 {
                        FrameDuration::Ms10
                    } else {
                        FrameDuration::Ms20
                    },
                    FecRate {
                        bitrate: bytes as u32 * if ms == 10 { 800 } else { 400 },
                        variable,
                    },
                    Options {
                        complexity: complexity as u8,
                        packet_loss_percent: loss as u8,
                    },
                );
                assert_eq!(actual, expected != 0, "{line}");
                rows += 1;
            }
        }
        assert_eq!(rows, 51200);
        Ok(())
    }
}
