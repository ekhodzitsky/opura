//! Fixed-point Opus AUDIO bandwidth policy for fixed-layout SILK streams.
//! Adapted from Opus v1.6.1 opus_encoder.c; see THIRD_PARTY_NOTICES.md.
use super::*;
use crate::silk::encoder::mono::MonoOptions;

#[derive(Clone, Copy)]
pub(super) struct AutomaticControls {
    pub input: OutputRate,
    pub duration: PacketDuration,
    pub packet_bytes: u32,
    pub channels: u8,
    pub options: MonoOptions,
    pub fec: bool,
    pub allowed: bool,
    pub in_wb: bool,
    pub voice_q7: u8,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct AutomaticBandwidth {
    initialized: bool,
    auto_bandwidth: u8,
    bandwidth: u8,
    pub fec: bool,
}
impl Default for AutomaticBandwidth {
    fn default() -> Self {
        Self {
            initialized: false,
            auto_bandwidth: 0,
            bandwidth: 4,
            fec: false,
        }
    }
}
impl AutomaticBandwidth {
    pub fn select(&mut self, c: AutomaticControls) -> (SilkRate, SilkRate) {
        let bitrate = c.packet_bytes as i32 * 8000 / c.duration.milliseconds() as i32;
        let equiv = equivalent_rate(bitrate, c.channels, c.duration, c.options);
        if !self.initialized || c.allowed {
            // Native mono/stereo tables coincide. Keep all five automatic bands
            // until after the original FEC decision and restricted-SILK cap.
            let music = [9000, 700, 9000, 700, 11000, 1000, 12000, 2000];
            let voice = [9000, 700, 9000, 700, 13500, 1000, 14000, 2000];
            let thresholds: [i32; 8] = std::array::from_fn(|i| {
                music[i] + ((i32::from(c.voice_q7).pow(2) * (voice[i] - music[i])) >> 14)
            });
            let mut band = 4;
            while band > 0 {
                let index = (band - 1) as usize * 2;
                let mut threshold = thresholds[index];
                if self.initialized {
                    threshold += if self.auto_bandwidth >= band {
                        -thresholds[index + 1]
                    } else {
                        thresholds[index + 1]
                    };
                }
                if equiv >= threshold {
                    break;
                }
                band -= 1;
            }
            if band == 1 {
                band = 2;
            }
            self.auto_bandwidth = band;
            self.bandwidth = band;
            if self.initialized && !c.in_wb {
                self.bandwidth = self.bandwidth.min(2);
            }
        }
        if bitrate < 15000 {
            self.bandwidth = self.bandwidth.min(2);
        }
        self.bandwidth = self.bandwidth.min(match c.input {
            OutputRate::Khz8 => 0,
            OutputRate::Khz12 => 1,
            OutputRate::Khz16 => 2,
            OutputRate::Khz24 => 3,
            OutputRate::Khz48 => 4,
        });
        let (band, fec) = fec_decision(
            c.fec,
            c.options.packet_loss_percent,
            self.fec,
            self.bandwidth,
            equiv,
        );
        // Original restricted-SILK cap follows the bandwidth-reducing FEC policy.
        self.bandwidth = band.min(2);
        self.fec = fec;
        let mut effective_max = bitrate;
        if c.duration == PacketDuration::Ms10 {
            effective_max = effective_max * 2 / 3;
        }
        let maximum = if effective_max < 7000 {
            SilkRate::Khz8
        } else if effective_max < 8000 {
            SilkRate::Khz12
        } else {
            SilkRate::Khz16
        };
        let desired = match self.bandwidth {
            0 => SilkRate::Khz8,
            1 => SilkRate::Khz12,
            _ => SilkRate::Khz16,
        };
        self.initialized = true;
        (
            if desired.khz() > maximum.khz() {
                maximum
            } else {
                desired
            },
            maximum,
        )
    }
}
fn equivalent_rate(
    mut bitrate: i32,
    channels: u8,
    duration: PacketDuration,
    options: MonoOptions,
) -> i32 {
    let frame_rate = 1000 / duration.milliseconds() as i32;
    if frame_rate > 50 {
        bitrate -= (40 * i32::from(channels) + 20) * (frame_rate - 50);
    }
    if !options.variable_bitrate {
        bitrate -= bitrate / 12;
    }
    bitrate = bitrate * (90 + i32::from(options.complexity)) / 100;
    if options.complexity < 2 {
        bitrate = bitrate * 4 / 5;
    }
    bitrate -= bitrate * i32::from(options.packet_loss_percent)
        / (6 * i32::from(options.packet_loss_percent) + 10);
    bitrate
}
fn fec_decision(enabled: bool, loss: u8, previous: bool, original: u8, equiv: i32) -> (u8, bool) {
    if !enabled || loss == 0 {
        return (original, false);
    }
    let mut band = original;
    loop {
        let threshold = [12000, 14000, 16000, 20000, 22000][band as usize]
            + if previous { -1000 } else { 1000 };
        let threshold = mul_wb(threshold * (125 - i32::from(loss.min(25))), 655);
        if equiv > threshold {
            return (band, true);
        }
        if loss <= 5 {
            return (band, false);
        }
        if band == 0 {
            break;
        }
        band -= 1;
    }
    (original, false)
}

#[cfg(test)]
#[path = "bandwidth_automatic_tests.rs"]
mod tests;
