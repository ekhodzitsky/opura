//! Mono PCM-to-Opus packets with explicit SILK bandwidth and explicit byte budgets.
use super::{
    bandwidth::{BandwidthError, PacketBandwidth},
    channel::FrameError,
    dtx::Decision,
    fec,
    state::SpeechState,
};
use crate::{
    encoder::pcm::PcmState,
    range::EncoderError,
    silk::{
        gains::FrameDuration,
        pitch::SilkRate,
        resampler::{OutputRate, ResampleError},
    },
};
use std::fmt;
#[path = "rate_tables.rs"]
mod tables;
/// Standard SILK Opus packet duration; 40/60 ms contain two/three 20 ms frames.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PacketDuration {
    /// One 10 ms internal frame.
    Ms10,
    /// One 20 ms internal frame.
    Ms20,
    /// Two 20 ms internal frames.
    Ms40,
    /// Three 20 ms internal frames.
    Ms60,
}
impl PacketDuration {
    /// Packet duration in milliseconds.
    pub fn milliseconds(self) -> u32 {
        match self {
            Self::Ms10 => 10,
            Self::Ms20 => 20,
            Self::Ms40 => 40,
            Self::Ms60 => 60,
        }
    }
    pub(super) fn frames(self) -> usize {
        match self {
            Self::Ms10 | Self::Ms20 => 1,
            Self::Ms40 => 2,
            Self::Ms60 => 3,
        }
    }
    pub(super) fn frame_duration(self) -> FrameDuration {
        if self == Self::Ms10 {
            FrameDuration::Ms10
        } else {
            FrameDuration::Ms20
        }
    }
}
/// Mono encoder settings. Packet byte budgets are supplied per call.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MonoOptions {
    /// Native complexity, 0..=10.
    pub complexity: u8,
    /// Use variable output length, bounded by the supplied packet bytes.
    /// False fills the exact packet budget using transport-level Opus padding.
    pub variable_bitrate: bool,
    /// Loss percentage, 0..=100, for LTP scaling and opt-in FEC policy.
    /// Does not request FEC by itself; call `set_inband_fec`. Zero disables new backups.
    pub packet_loss_percent: u8,
}
impl Default for MonoOptions {
    fn default() -> Self {
        Self {
            complexity: 10,
            variable_bitrate: false,
            packet_loss_percent: 0,
        }
    }
}
/// Invalid input/configuration, resampling or packet budget failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum MonoError {
    /// PCM length must match rate and packet duration.
    InvalidInputLength,
    /// Byte budget must be 3..=1276, complexity 0..=10 and loss 0..=100.
    InvalidOptions,
    /// Encoder input conversion failed.
    Resampler(ResampleError),
    /// Channel PCM encoding failed.
    Frame(FrameError),
    /// Bandwidth transition controls or internal filtering failed.
    Bandwidth(BandwidthError),
    /// Packet header/termination failed.
    Entropy(EncoderError),
}
impl fmt::Display for MonoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInputLength => {
                f.write_str("SILK mono input length does not match packet duration")
            }
            Self::InvalidOptions => f.write_str("invalid SILK mono options or packet budget"),
            Self::Resampler(e) => write!(f, "SILK input: {e}"),
            Self::Frame(e) => write!(f, "SILK packet: {e}"),
            Self::Bandwidth(e) => write!(f, "SILK bandwidth transition: {e}"),
            Self::Entropy(e) => write!(f, "SILK packet entropy: {e}"),
        }
    }
}
impl std::error::Error for MonoError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Resampler(e) => Some(e),
            Self::Frame(e) => Some(e),
            Self::Bandwidth(e) => Some(e),
            Self::Entropy(e) => Some(e),
            _ => None,
        }
    }
}
impl From<ResampleError> for MonoError {
    fn from(e: ResampleError) -> Self {
        Self::Resampler(e)
    }
}
impl From<FrameError> for MonoError {
    fn from(e: FrameError) -> Self {
        Self::Frame(e)
    }
}
impl From<BandwidthError> for MonoError {
    fn from(e: BandwidthError) -> Self {
        Self::Bandwidth(e)
    }
}
impl From<EncoderError> for MonoError {
    fn from(e: EncoderError) -> Self {
        Self::Entropy(e)
    }
}
/// Complete one-frame Opus packet (possibly containing multiple internal SILK frames).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MonoPacket {
    bytes: [u8; 1276],
    length: usize,
    range: u32,
    plc: bool,
}
impl MonoPacket {
    /// Opus TOC, compressed frame and optional packet-level padding.
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.length]
    }
    /// Automatic-mode output requested decoder concealment because the complete
    /// native SILK payload exceeded the packet cap. This is distinct from DTX.
    pub fn used_plc(&self) -> bool {
        self.plc
    }
    /// Final entropy range for cross-decoder validation.
    pub fn final_range(&self) -> u32 {
        self.range
    }
}
/// Explicit-bandwidth mono PCM encoder for all five external rates. Uses native
/// DC rejection, encoder resampling, VAD, SNR tables and packet bit reservoir.
/// Supports DTX/FEC, gradual external targets and opt-in automatic bandwidth.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MonoEncoder {
    input: OutputRate,
    internal: SilkRate,
    pcm: PcmState,
    speech: SpeechState<1>,
    bandwidth_control: PacketBandwidth<1>,
}
impl MonoEncoder {
    /// Start an independent mono stream at a fixed input rate/internal bandwidth.
    pub fn new(input: OutputRate, internal: SilkRate) -> Self {
        Self {
            input,
            internal,
            pcm: PcmState::mono(input),
            speech: SpeechState::new(input, internal),
            bandwidth_control: PacketBandwidth::default(),
        }
    }
    /// Nominal pre-skip on the 48 kHz Opus clock: 6.5 ms, including the native
    /// SILK input/resampling compensation and fixed channel lookahead.
    pub fn lookahead_48k(&self) -> u32 {
        312
    }
    /// Current internal SILK bandwidth used for subsequent packets.
    pub fn bandwidth(&self) -> SilkRate {
        self.internal
    }
    /// Whether native fixed-point AUDIO bandwidth selection is enabled.
    pub fn automatic_bandwidth(&self) -> bool {
        self.bandwidth_control.automatic
    }
    /// Select SILK bandwidth from the full packet byte budget, complexity, loss
    /// and native AUDIO thresholds/hysteresis. Fixed channel layout is retained.
    /// FEC may lower the target bandwidth. Disabled by default; enabling clears
    /// an external target, disabling stops the taper at the current rate. Reset
    /// retains this mode. All policy/transition state rolls back on packet errors.
    pub fn set_automatic_bandwidth(&mut self, enabled: bool) {
        self.bandwidth_control.set_automatic(enabled);
    }
    /// Desired internal bandwidth for activity-permitted native LP transitions.
    /// None means no external target (fixed or automatic selection). The actual rate is `bandwidth()`;
    /// the desired rate is capped by the external PCM rate during selection.
    pub fn bandwidth_target(&self) -> Option<SilkRate> {
        self.bandwidth_control.target
    }
    /// Request native gradual transitions toward an external target, disabling automatic selection.
    /// This does not select a target from bitrate/music analysis. None stops the
    /// taper at the current rate. Reset retains this setting. Packet errors
    /// preserve all transition/filter state and pending switch acknowledgements.
    pub fn set_bandwidth_target(&mut self, target: Option<SilkRate>) {
        self.bandwidth_control.set_target(target);
    }
    /// Force a fixed bandwidth at a packet boundary, retaining converted input
    /// history, VAD/DC, frame counters, DTX and rate-control history. Incompatible
    /// pending FEC is cleared; last-packet diagnostics retain their meaning.
    /// A changed rate stops gradual targeting; an identical-rate call preserves it.
    /// Same-band calls are no-ops; errors preserve the entire stream.
    pub fn set_bandwidth(&mut self, rate: SilkRate) -> Result<(), MonoError> {
        if rate == self.internal {
            return Ok(());
        }
        let mut next = self.clone();
        next.change_bandwidth(rate)?;
        next.bandwidth_control.set_target(None);
        *self = next;
        Ok(())
    }
    fn change_bandwidth(&mut self, rate: SilkRate) -> Result<(), MonoError> {
        if rate == self.internal {
            return Ok(());
        }
        let mut next = self.clone();
        next.speech.change_rate(self.input, rate)?;
        next.internal = rate;
        *self = next;
        Ok(())
    }
    /// Enable native silence suppression. Disabled by default. Suppressed packets
    /// contain only TOC (one byte), with final range zero, even in CBR mode.
    /// Encoding still advances all analysis/quantizer state and can fail normally.
    pub fn set_dtx(&mut self, enabled: bool) {
        self.speech.dtx.enabled = enabled;
    }
    /// Request in-band FEC using `MonoOptions::packet_loss_percent`. Disabled
    /// by default. Fixed-band operation preserves its rate; automatic bandwidth
    /// may lower its target for protection. Protection requires sufficient
    /// bitrate and activity. Previously queued redundancy can still be carried
    /// by the next packet after disabling this setting.
    pub fn set_inband_fec(&mut self, enabled: bool) {
        self.speech.fec.enabled = enabled;
    }
    /// Whether the last successful packet carries prior-packet redundancy.
    pub fn has_fec(&self) -> bool {
        self.speech.fec.sent[0]
    }
    /// Whether the last successfully encoded packet suppressed its payload.
    pub fn in_dtx(&self) -> bool {
        self.speech.dtx.suppressed
    }
    /// Reset all input, VAD, predictor and bitrate history, retaining rates, bandwidth mode/target and DTX/FEC settings.
    pub fn reset(&mut self) {
        let automatic = self.automatic_bandwidth();
        let target = self.bandwidth_control.target;
        let enabled = self.speech.dtx.enabled;
        let fec = self.speech.fec.enabled;
        *self = Self::new(self.input, self.internal);
        self.set_dtx(enabled);
        self.set_inband_fec(fec);
        self.set_bandwidth_target(target);
        self.set_automatic_bandwidth(automatic);
    }
    /// Encode exactly one duration of mono PCM. packet_bytes includes TOC and
    /// packet padding, excludes transport/container overhead, and is 3..=1276.
    /// VBR is a hard byte cap; CBR produces exactly packet_bytes unless DTX suppresses it. Any error
    /// preserves all input/analysis/quantizer and rate-control history.
    /// Automatic mode can return native PLC output on packet overflow; inspect
    /// the returned packet's `used_plc()` flag separately from DTX.
    pub fn encode(
        &mut self,
        duration: PacketDuration,
        input: &[i16],
        packet_bytes: usize,
        options: MonoOptions,
    ) -> Result<MonoPacket, MonoError> {
        let ms = duration.milliseconds() as usize;
        let khz = self.input.hz() as usize / 1000;
        if input.len() != ms * khz {
            return Err(MonoError::InvalidInputLength);
        }
        if !(3..=1276).contains(&packet_bytes)
            || options.complexity > 10
            || options.packet_loss_percent > 100
        {
            return Err(MonoError::InvalidOptions);
        }
        let mut staged = self.clone();
        let rate = staged.bandwidth_control.prepare(
            self.input,
            self.internal,
            duration,
            packet_bytes,
            options,
            self.speech.fec.enabled,
        )?;
        staged.change_bandwidth(rate)?;
        let digital_silence = input.iter().all(|&v| v == 0);
        staged.speech.dtx.begin(digital_silence);
        let mut filtered = [0i16; 2880];
        staged
            .pcm
            .filter(input, &mut filtered[..input.len()])
            .map_err(|_| MonoError::InvalidInputLength)?;
        let mut pcm = [0i16; 960];
        let converted = staged.speech.resampler[0].process(&filtered[..input.len()], &mut pcm)?;
        let backup = staged
            .speech
            .fec
            .has_pending(duration)
            .then(|| staged.clone());
        let attempt = staged.encode_packet(
            duration,
            &pcm[..converted],
            packet_bytes,
            options,
            digital_silence,
            true,
        );
        let retry = match &attempt {
            Err(MonoError::Frame(e)) => fec::budget_failure(e),
            Err(MonoError::Entropy(EncoderError::CapacityExhausted)) => true,
            _ => false,
        };
        if retry {
            if let Some(mut next) = backup {
                let (packet, decision) = next.encode_packet(
                    duration,
                    &pcm[..converted],
                    packet_bytes,
                    options,
                    digital_silence,
                    false,
                )?;
                if decision != Decision::Silk {
                    next.pcm
                        .commit(&filtered[..input.len()])
                        .map_err(|_| MonoError::InvalidInputLength)?;
                }
                *self = next;
                return Ok(packet);
            }
        }
        let (packet, decision) = attempt?;
        if decision != Decision::Silk {
            staged
                .pcm
                .commit(&filtered[..input.len()])
                .map_err(|_| MonoError::InvalidInputLength)?;
        }
        *self = staged;
        Ok(packet)
    }
    fn encode_packet(
        &mut self,
        duration: PacketDuration,
        pcm: &[i16],
        packet_bytes: usize,
        options: MonoOptions,
        digital_silence: bool,
        include_fec: bool,
    ) -> Result<(MonoPacket, Decision), MonoError> {
        super::packet::mono(
            &mut self.speech,
            &mut self.bandwidth_control,
            self.internal,
            pcm,
            super::packet::PacketControls {
                duration,
                packet_bytes,
                options,
                digital_silence,
                include_fec,
                channels: 1,
                to_mono: false,
            },
        )
    }
}
pub(crate) fn target_snr(rate: SilkRate, duration: FrameDuration, mut bitrate: i32) -> u16 {
    if duration == FrameDuration::Ms10 {
        bitrate -= 2000 + rate.khz() / 16;
    }
    let table: &[u8] = match rate {
        SilkRate::Khz8 => &tables::NB,
        SilkRate::Khz12 => &tables::MB,
        SilkRate::Khz16 => &tables::WB,
    };
    let index = ((bitrate + 200) / 400 - 10).min(table.len() as i32 - 1);
    if index <= 0 {
        0
    } else {
        u16::from(table[index as usize]) * 21
    }
}
pub(super) fn plc_packet(toc: u8, length: usize) -> MonoPacket {
    let mut result = packet(toc, &[0], length, 0);
    result.plc = true;
    result
}
pub(super) fn packet(toc: u8, payload: &[u8], length: usize, range: u32) -> MonoPacket {
    let mut result = MonoPacket {
        bytes: [0; 1276],
        length,
        range,
        plc: false,
    };
    let extra = length - payload.len() - 1;
    if extra == 0 {
        result.bytes[0] = toc;
        result.bytes[1..1 + payload.len()].copy_from_slice(payload);
        return result;
    }
    result.bytes[0] = toc | 3;
    result.bytes[1] = 1;
    let mut at = 2;
    if extra > 1 {
        result.bytes[1] |= 0x40;
        let padding = extra - 1;
        let full = (padding - 1) / 255;
        for _ in 0..full {
            result.bytes[at] = 255;
            at += 1;
        }
        result.bytes[at] = (padding - 255 * full - 1) as u8;
        at += 1;
    }
    result.bytes[at..at + payload.len()].copy_from_slice(payload);
    result
}

#[cfg(test)]
#[path = "mono_tests.rs"]
mod tests;
