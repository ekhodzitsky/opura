//! Native SILK bandwidth selection handshake and variable-cutoff LP transition.
//! Adapted from Opus v1.6.1; see THIRD_PARTY_NOTICES.md.
use super::mono::PacketDuration;
use crate::silk::{
    gains::FrameDuration,
    pitch::SilkRate,
    resampler::OutputRate,
    synthesis::{mul_wb, round_shift, saturate16},
};
use std::fmt;

const B: [[i32; 3]; 5] = [
    [250767114, 501534038, 250767114],
    [209867381, 419732057, 209867381],
    [170987846, 341967853, 170987846],
    [131531482, 263046905, 131531482],
    [89306658, 178584282, 89306658],
];
const A: [[i32; 2]; 5] = [
    [506393414, 239854379],
    [411067935, 169683996],
    [306733530, 116694253],
    [185807084, 77959395],
    [35497197, 57401098],
];

/// Invalid bandwidth constraints or internal PCM geometry. Errors preserve state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum BandwidthError {
    /// Require minimum <= desired <= maximum and max_bits in1..=10200.
    InvalidControls,
    /// PCM must be exactly one10/20ms frame at its specified internal rate.
    InvalidInputLength,
}
impl fmt::Display for BandwidthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidControls => "invalid SILK bandwidth controls",
            Self::InvalidInputLength => "SILK transition PCM does not match frame geometry",
        })
    }
}
impl std::error::Error for BandwidthError {}

/// Native packet-boundary bandwidth controls. Input resampling and applying the
/// selected internal rate remain the enclosing encoder's responsibility.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BandwidthControls {
    /// Current configured internal rate, or None during initialization/reset.
    pub current: Option<SilkRate>,
    /// External PCM rate.
    pub input: OutputRate,
    /// Native lower internal-rate constraint. Initial selection is also capped
    /// by the API rate, retaining the original behavior when these conflict.
    pub minimum: SilkRate,
    /// Maximum permitted internal bandwidth.
    pub maximum: SilkRate,
    /// Desired internal bandwidth supplied by the outer mode/rate policy.
    pub desired: SilkRate,
    /// Activity policy permits beginning/progressing a transition.
    pub allow_switch: bool,
    /// Outer packet encoder acknowledges a previous switch-ready decision.
    pub can_switch: bool,
    /// Complete packet duration, used for native transition bit reservation.
    pub duration: PacketDuration,
    /// Available entropy bits,1..=10200, before reservation.
    pub max_bits: u32,
}
/// Selected internal rate, handshake and entropy budget for this packet.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BandwidthDecision {
    rate: SilkRate,
    ready: bool,
    max_bits: u32,
}
impl BandwidthDecision {
    /// Internal rate selected for input conversion and channel setup.
    pub fn rate(self) -> SilkRate {
        self.rate
    }
    /// Acknowledgement can permit the next packet to change internal rate.
    pub fn switch_ready(self) -> bool {
        self.ready
    }
    /// Native remaining bits after reserving space for a5ms transition tail.
    pub fn max_bits(self) -> u32 {
        self.max_bits
    }
}

/// Per-channel low-pass transition state. Selection updates the taper, not the
/// channel configuration. On an actual rate change, migrate channel/resampler
/// history and call `reset_filter_delay`, as native setup_fs does. VAD runs on
/// the unfiltered input; `filter` runs immediately before channel buffering.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct BandwidthTransition {
    delay: [i32; 2],
    position: u16,
    direction: i8,
    saved: Option<SilkRate>,
}
impl BandwidthTransition {
    /// Clear taper, filter and saved-rate history for a fresh stream.
    pub fn reset(&mut self) {
        *self = Self::default();
    }
    /// Retain the old internal rate across an outer bandwidth-switching reset.
    pub fn save_rate(&mut self, rate: SilkRate) {
        self.saved = Some(rate);
    }
    /// Clear only ARMA delay on an actual internal-rate change.
    pub fn reset_filter_delay(&mut self) {
        self.delay = [0; 2];
    }
    /// Current taper step,0..=256; native steps count internal frames.
    pub fn position(&self) -> u16 {
        self.position
    }
    /// Native taper direction: -2 down,0 inactive,+1 up.
    pub fn direction(&self) -> i8 {
        self.direction
    }
    /// Evaluate unchanged native switching rules. No input/history conversion
    /// or channel reset happens here. Invalid controls change no state.
    pub fn select(&mut self, c: BandwidthControls) -> Result<BandwidthDecision, BandwidthError> {
        if c.minimum.khz() > c.desired.khz()
            || c.desired.khz() > c.maximum.khz()
            || !(1..=10200).contains(&c.max_bits)
        {
            return Err(BandwidthError::InvalidControls);
        }
        let original = c.current.or(self.saved);
        let mut rate = original.unwrap_or(c.desired);
        let mut ready = false;
        let mut max_bits = c.max_bits;
        if original.is_none() {
            rate = at_most_input(c.desired, c.input);
        } else if rate.khz() * 1000 > c.input.hz() as i32
            || rate.khz() > c.maximum.khz()
            || rate.khz() < c.minimum.khz()
        {
            rate = at_most_input(c.maximum, c.input);
            if rate.khz() < c.minimum.khz() {
                rate = c.minimum;
            }
        } else {
            if self.position >= 256 {
                self.direction = 0;
            }
            if c.allow_switch || c.can_switch {
                if rate.khz() > c.desired.khz() {
                    if self.direction == 0 {
                        self.position = 256;
                        self.delay = [0; 2];
                    }
                    if c.can_switch {
                        self.direction = 0;
                        rate = if rate == SilkRate::Khz16 {
                            SilkRate::Khz12
                        } else {
                            SilkRate::Khz8
                        };
                    } else if self.position == 0 {
                        ready = true;
                    } else {
                        self.direction = -2;
                    }
                } else if rate.khz() < c.desired.khz() {
                    if c.can_switch {
                        rate = if rate == SilkRate::Khz8 {
                            SilkRate::Khz12
                        } else {
                            SilkRate::Khz16
                        };
                        self.position = 0;
                        self.delay = [0; 2];
                        self.direction = 1;
                    } else if self.direction == 0 {
                        ready = true;
                    } else {
                        self.direction = 1;
                    }
                } else if self.direction < 0 {
                    self.direction = 1;
                }
            }
        }
        if ready {
            max_bits -= max_bits * 5 / (c.duration.milliseconds() + 5);
        }
        Ok(BandwidthDecision {
            rate,
            ready,
            max_bits,
        })
    }
    /// Filter one in-place internal PCM frame with native Q28 coefficient
    /// interpolation and biquad arithmetic. Even an inactive filter validates
    /// frame geometry before changing state or samples.
    pub fn filter(
        &mut self,
        rate: SilkRate,
        duration: FrameDuration,
        frame: &mut [i16],
    ) -> Result<(), BandwidthError> {
        let ms = if duration == FrameDuration::Ms10 {
            10
        } else {
            20
        };
        if frame.len() != rate.khz() as usize * ms {
            return Err(BandwidthError::InvalidInputLength);
        }
        if self.direction == 0 {
            return Ok(());
        }
        let factor = (256 - u32::from(self.position)) << 10;
        let index = (factor >> 16) as usize;
        let fraction = (factor & 65535) as i32;
        let b = interpolate(&B, index, fraction);
        let a = interpolate(&A, index, fraction);
        self.position = (i32::from(self.position) + i32::from(self.direction)).clamp(0, 256) as u16;
        let ar = a.map(|a| a.wrapping_neg());
        for sample in frame {
            let input = i32::from(*sample);
            let output = self.delay[0]
                .wrapping_add(mul_wb(b[0], input))
                .wrapping_shl(2);
            self.delay[0] = self.delay[1]
                .wrapping_add(round_shift(mul_wb(output, ar[0] & 16383), 14))
                .wrapping_add(mul_wb(output, ar[0] >> 14))
                .wrapping_add(mul_wb(b[1], input));
            self.delay[1] = round_shift(mul_wb(output, ar[1] & 16383), 14)
                .wrapping_add(mul_wb(output, ar[1] >> 14))
                .wrapping_add(mul_wb(b[2], input));
            *sample = saturate16(output.wrapping_add(16383) >> 14);
        }
        Ok(())
    }
}
fn at_most_input(rate: SilkRate, input: OutputRate) -> SilkRate {
    if input == OutputRate::Khz8 {
        SilkRate::Khz8
    } else if input == OutputRate::Khz12 && rate == SilkRate::Khz16 {
        SilkRate::Khz12
    } else {
        rate
    }
}
fn interpolate<const N: usize>(table: &[[i32; N]; 5], index: usize, fraction: i32) -> [i32; N] {
    if index >= 4 || fraction == 0 {
        return table[index];
    }
    std::array::from_fn(|k| {
        let difference = table[index + 1][k] - table[index][k];
        if fraction < 32768 {
            table[index][k] + mul_wb(difference, fraction)
        } else {
            table[index + 1][k] + mul_wb(difference, fraction - 65536)
        }
    })
}

// Shared packet scheduling for the raw mono/stereo SILK frontends. The outer
// Opus mode/rate policy supplies a desired rate; this layer owns native timing.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct PacketBandwidth<const N: usize> {
    pub target: Option<SilkRate>,
    pub transitions: [BandwidthTransition; N],
    started: bool,
    allowed: bool,
    elapsed_ms: i32,
    can_switch: bool,
    ready: bool,
    pub maximum: u32,
}
impl<const N: usize> Default for PacketBandwidth<N> {
    fn default() -> Self {
        Self {
            target: None,
            transitions: std::array::from_fn(|_| BandwidthTransition::default()),
            started: false,
            allowed: false,
            elapsed_ms: 0,
            can_switch: false,
            ready: false,
            maximum: 0,
        }
    }
}
impl<const N: usize> PacketBandwidth<N> {
    pub fn set_target(&mut self, target: Option<SilkRate>) {
        if target.is_none() && self.target.is_some() {
            // Returning to fixed operation stops any in-progress taper.
            self.transitions = std::array::from_fn(|_| BandwidthTransition::default());
            self.can_switch = false;
            self.ready = false;
        }
        self.target = target;
    }
    pub fn begin(
        &mut self,
        input: OutputRate,
        current: SilkRate,
        duration: PacketDuration,
        maximum: u32,
    ) -> Result<SilkRate, BandwidthError> {
        self.maximum = maximum;
        self.ready = false;
        let Some(target) = self.target else {
            return Ok(current);
        };
        let mut rate = current;
        for (channel, transition) in self.transitions.iter_mut().enumerate() {
            let decision = transition.select(BandwidthControls {
                current: self.started.then_some(current),
                input,
                minimum: SilkRate::Khz8,
                maximum: SilkRate::Khz16,
                desired: at_most_input(target, input),
                allow_switch: self.allowed,
                can_switch: self.can_switch,
                duration,
                max_bits: self.maximum,
            })?;
            // Native side control runs its own handshake/budget before setup_fs
            // forces the side to the mid's actual rate.
            if channel == 0 {
                rate = decision.rate();
            }
            self.ready |= decision.switch_ready();
            self.maximum = decision.max_bits();
        }
        if rate != current {
            for transition in &mut self.transitions {
                transition.reset_filter_delay();
            }
        }
        Ok(rate)
    }
    pub fn finish(&mut self, activity_q8: u8, duration: PacketDuration) {
        // Original silk_Encode activity permission uses the final mid frame.
        let threshold = 13 + mul_wb(3188, self.elapsed_ms);
        self.allowed = i32::from(activity_q8) < threshold;
        if self.allowed {
            self.elapsed_ms = 0;
        } else {
            self.elapsed_ms += duration.milliseconds() as i32;
        }
        self.started = true;
        // Acknowledge only after the whole packet (including DTX) succeeds.
        self.can_switch = self.ready;
    }
}

#[cfg(test)]
#[path = "bandwidth_tests.rs"]
mod tests;
