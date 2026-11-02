//! Adaptive SILK L/R-to-M/S prediction, stereo width and bitrate allocation.
//! Adapted from Opus v1.6.1 stereo encoder sources; see THIRD_PARTY_NOTICES.md.
use super::{
    bandwidth::{BandwidthError, PacketBandwidth},
    dtx::Decision,
    fec,
    math::{divide_q, energy},
    state::{AnalysisControls, SpeechState, StereoFrameError},
};
use crate::{
    range::{EncoderError, RangeEncoder},
    silk::{
        cng::sqrt_approx,
        gains::FrameDuration,
        pitch::SilkRate,
        stereo::tables::{JOINT, MID_ONLY, QUANT, UNIFORM3, UNIFORM5},
        synthesis::{mul_wb, round_shift, saturate16},
    },
};
use std::fmt;
/// Invalid stereo geometry/controls or an entropy failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum StereoEncodeError {
    /// Both planar inputs must contain one internal10/20ms frame.
    InvalidInputLength,
    /// Total bitrate/activity or side-field presence is invalid.
    InvalidControls,
    /// Stereo prefix could not be encoded; writer is unchanged.
    Entropy(EncoderError),
}
impl fmt::Display for StereoEncodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInputLength => {
                f.write_str("SILK stereo input does not match frame geometry")
            }
            Self::InvalidControls => f.write_str("invalid SILK stereo controls"),
            Self::Entropy(e) => write!(f, "SILK stereo prefix: {e}"),
        }
    }
}
impl std::error::Error for StereoEncodeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Entropy(e) => Some(e),
            _ => None,
        }
    }
}
impl From<EncoderError> for StereoEncodeError {
    fn from(e: EncoderError) -> Self {
        Self::Entropy(e)
    }
}
/// Total coding-rate target and previous-frame activity used to adapt stereo.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StereoControls {
    /// Combined mid+side target in bits/s,1..=1024000.
    pub total_rate_bps: i32,
    /// Previous mid speech activity in Q8,0..=256.
    pub previous_activity_q8: u16,
    /// Collapse width for the final frame before switching to mono.
    pub to_mono: bool,
}
/// Buffered mid and prediction-residual side, plus quantized stereo signaling.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StereoAnalysisFrame {
    mid: [i16; 320],
    side: [i16; 320],
    length: usize,
    indices: [[u8; 3]; 2],
    mid_only: bool,
    rates: [i32; 2],
}
impl StereoAnalysisFrame {
    /// Mid PCM including the native one-sample buffering delay.
    pub fn mid(&self) -> &[i16] {
        &self.mid[..self.length]
    }
    /// Prediction-subtracted side PCM, ready for its channel encoder.
    pub fn side(&self) -> &[i16] {
        &self.side[..self.length]
    }
    /// Coarse/fine/group symbols for each stereo predictor.
    pub fn indices(&self) -> &[[u8; 3]; 2] {
        &self.indices
    }
    /// Side can now be omitted after native taper/lookahead flushing completes.
    pub fn mid_only(&self) -> bool {
        self.mid_only
    }
    /// Mid and side target bitrates; side is zero only when omitted.
    pub fn rates_bps(&self) -> &[i32; 2] {
        &self.rates
    }
    /// Encode predictor symbols and the mid-only flag when side VAD is false.
    /// Side activity must agree with side omission. Failure preserves the writer.
    pub fn encode_prefix(
        &self,
        writer: &mut RangeEncoder,
        side_active: bool,
    ) -> Result<(), StereoEncodeError> {
        encode_prefix(writer, self.indices, self.mid_only, side_active)
    }
}
pub(super) fn encode_prefix(
    writer: &mut RangeEncoder,
    indices: [[u8; 3]; 2],
    mid_only: bool,
    side_active: bool,
) -> Result<(), StereoEncodeError> {
    if side_active && mid_only {
        return Err(StereoEncodeError::InvalidControls);
    }
    let mut next = writer.clone();
    let joint = 5 * usize::from(indices[0][2]) + usize::from(indices[1][2]);
    next.encode_icdf(joint, &JOINT, 8)?;
    for index in indices {
        next.encode_icdf(usize::from(index[0]), &UNIFORM3, 8)?;
        next.encode_icdf(usize::from(index[1]), &UNIFORM5, 8)?;
    }
    if !side_active {
        next.encode_icdf(usize::from(mid_only), &MID_ONLY, 8)?;
    }
    *writer = next;
    Ok(())
}
/// Native stereo analysis histories; retained across internal-rate/duration
/// changes. Reset when entering a new stereo stream, as in the original API.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StereoAnalysisState {
    prediction: [i16; 2],
    mid: [i16; 2],
    side: [i16; 2],
    amplitude: [i32; 4],
    smoothed_width: i16,
    width: i16,
    silent_side: i16,
}
impl Default for StereoAnalysisState {
    fn default() -> Self {
        Self {
            prediction: [0; 2],
            mid: [0; 2],
            side: [0; 2],
            amplitude: [0, 1, 0, 1],
            smoothed_width: 16384,
            width: 0,
            silent_side: 0,
        }
    }
}
impl StereoAnalysisState {
    /// Clear prediction, sample and width history for an independent stereo stream.
    pub fn reset(&mut self) {
        *self = Self::default();
    }
    pub(crate) fn smoothed_width(&self) -> i16 {
        self.smoothed_width
    }
    /// Convert planar internal-rate L/R PCM to native buffered mid/side. No
    /// external resampling is performed here. Invalid inputs preserve state.
    pub fn analyze(
        &mut self,
        rate: SilkRate,
        duration: FrameDuration,
        left: &[i16],
        right: &[i16],
        controls: StereoControls,
    ) -> Result<StereoAnalysisFrame, StereoEncodeError> {
        let khz = rate.khz() as usize;
        let ms = if duration == FrameDuration::Ms10 {
            10
        } else {
            20
        };
        let length = ms * khz;
        if left.len() != length || right.len() != length {
            return Err(StereoEncodeError::InvalidInputLength);
        }
        if !(1..=1024000).contains(&controls.total_rate_bps) || controls.previous_activity_q8 > 256
        {
            return Err(StereoEncodeError::InvalidControls);
        }
        let mut next = self.clone();
        let mut mid = [0i16; 322];
        let mut side = [0i16; 322];
        mid[..2].copy_from_slice(&next.mid);
        side[..2].copy_from_slice(&next.side);
        for i in 0..length {
            let sum = i32::from(left[i]) + i32::from(right[i]);
            let diff = i32::from(left[i]) - i32::from(right[i]);
            mid[i + 2] = round_shift(sum, 1) as i16;
            side[i + 2] = saturate16(round_shift(diff, 1));
        }
        next.mid.copy_from_slice(&mid[length..length + 2]);
        next.side.copy_from_slice(&side[length..length + 2]);
        let mut low = [[0i16; 320]; 2];
        let mut high = [[0i16; 320]; 2];
        for (channel, input) in [&mid, &side].into_iter().enumerate() {
            for i in 0..length {
                let sum = round_shift(
                    i32::from(input[i]) + i32::from(input[i + 2]) + 2 * i32::from(input[i + 1]),
                    2,
                );
                low[channel][i] = sum as i16;
                high[channel][i] = (i32::from(input[i + 1]) - sum) as i16;
            }
        }
        let activity = i32::from(controls.previous_activity_q8);
        let smoothing = mul_wb(activity * activity, if ms == 10 { 328 } else { 655 });
        let mut a = [next.amplitude[0], next.amplitude[1]];
        let (p0, low_ratio) = predictor(&low[0][..length], &low[1][..length], &mut a, smoothing);
        next.amplitude[..2].copy_from_slice(&a);
        let mut a = [next.amplitude[2], next.amplitude[3]];
        let (p1, high_ratio) = predictor(&high[0][..length], &high[1][..length], &mut a, smoothing);
        next.amplitude[2..].copy_from_slice(&a);
        let fraction = (high_ratio + 3 * low_ratio).min(65536);
        let fraction3 = 3 * fraction;
        let total = (controls.total_rate_bps - if ms == 10 { 1200 } else { 600 }).max(1);
        let minimum = 2000 + khz as i32 * 600;
        let mut rates = [divide_q(total, 13 * 65536 + fraction3, 19), 0];
        let mut width;
        if rates[0] < minimum {
            rates[0] = minimum;
            rates[1] = total - minimum;
            width = divide_q(
                2 * rates[1] - minimum,
                mul_wb(65536 + fraction3, minimum),
                16,
            )
            .clamp(0, 16384);
        } else {
            rates[1] = total - rates[0];
            width = 16384;
        }
        next.smoothed_width = (i32::from(next.smoothed_width)
            + mul_wb(width - i32::from(next.smoothed_width), smoothing))
            as i16;
        let mut prediction = [p0, p1];
        let mut mid_only = false;
        let indices;
        if controls.to_mono {
            width = 0;
            prediction = [0; 2];
            indices = quantize(&mut prediction);
        } else if next.width == 0
            && (8 * total < 13 * minimum || mul_wb(fraction, i32::from(next.smoothed_width)) < 819)
        {
            scale_prediction(&mut prediction, next.smoothed_width);
            indices = quantize(&mut prediction);
            width = 0;
            prediction = [0; 2];
            rates = [total, 0];
            mid_only = true;
        } else if next.width != 0
            && (8 * total < 11 * minimum || mul_wb(fraction, i32::from(next.smoothed_width)) < 328)
        {
            scale_prediction(&mut prediction, next.smoothed_width);
            indices = quantize(&mut prediction);
            width = 0;
            prediction = [0; 2];
        } else if next.smoothed_width > 15565 {
            indices = quantize(&mut prediction);
            width = 16384;
        } else {
            scale_prediction(&mut prediction, next.smoothed_width);
            indices = quantize(&mut prediction);
            width = i32::from(next.smoothed_width);
        }
        if mid_only {
            next.silent_side += (length - 8 * khz) as i16;
            if i32::from(next.silent_side) < 5 * khz as i32 {
                mid_only = false;
            } else {
                next.silent_side = 10000;
            }
        } else {
            next.silent_side = 0;
        }
        if !mid_only && rates[1] < 1 {
            rates[1] = 1;
            rates[0] = (total - 1).max(1);
        }
        let mut p0 = -i32::from(next.prediction[0]);
        let mut p1 = -i32::from(next.prediction[1]);
        let mut w = i32::from(next.width) << 10;
        let denominator = 65536 / (8 * khz) as i32;
        let delta0 = -round_shift(
            i32::from((prediction[0] - i32::from(next.prediction[0])) as i16) * denominator,
            16,
        );
        let delta1 = -round_shift(
            i32::from((prediction[1] - i32::from(next.prediction[1])) as i16) * denominator,
            16,
        );
        let delta_width = mul_wb(width - i32::from(next.width), denominator) << 10;
        let mut result = StereoAnalysisFrame {
            mid: [0; 320],
            side: [0; 320],
            length,
            indices,
            mid_only,
            rates,
        };
        result.mid[..length].copy_from_slice(&mid[1..length + 1]);
        for i in 0..length {
            if i < 8 * khz {
                p0 += delta0;
                p1 += delta1;
                w += delta_width;
            } else {
                p0 = -prediction[0];
                p1 = -prediction[1];
                w = width << 10;
            }
            let low = (i32::from(mid[i]) + i32::from(mid[i + 2]) + 2 * i32::from(mid[i + 1])) << 9;
            let sum = mul_wb(w, i32::from(side[i + 1]))
                .wrapping_add(mul_wb(low, p0))
                .wrapping_add(mul_wb(i32::from(mid[i + 1]) << 11, p1));
            result.side[i] = saturate16(round_shift(sum, 8));
        }
        next.prediction = prediction.map(|v| v as i16);
        next.width = width as i16;
        *self = next;
        Ok(result)
    }
}
fn scale_prediction(prediction: &mut [i32; 2], width: i16) {
    for p in prediction {
        *p = (i32::from(width) * *p) >> 14;
    }
}
fn predictor(x: &[i16], y: &[i16], amplitude: &mut [i32; 2], smoothing: i32) -> (i32, i32) {
    let (mut nx, sx) = energy(x);
    let (mut ny, sy) = energy(y);
    let scale = sx.max(sy);
    let scale = scale + (scale & 1);
    nx = (nx >> (scale - sx)).max(1);
    ny >>= scale - sy;
    let correlation = x.iter().zip(y).fold(0i32, |sum, (&x, &y)| {
        sum.wrapping_add((i32::from(x) * i32::from(y)) >> scale)
    });
    let prediction = divide_q(correlation, nx, 13).clamp(-16384, 16384);
    let square = mul_wb(prediction, prediction);
    let smoothing = smoothing.max(square.abs());
    let half = scale / 2;
    amplitude[0] += mul_wb((sqrt_approx(nx) << half) - amplitude[0], smoothing);
    ny = ny
        .wrapping_sub(mul_wb(correlation, prediction).wrapping_shl(4))
        .wrapping_add(mul_wb(nx, square).wrapping_shl(6));
    amplitude[1] += mul_wb((sqrt_approx(ny) << half) - amplitude[1], smoothing);
    let ratio = divide_q(amplitude[1], amplitude[0].max(1), 14).clamp(0, 32767);
    (prediction, ratio)
}
fn quantize(prediction: &mut [i32; 2]) -> [[u8; 3]; 2] {
    let mut indices = [[0u8; 3]; 2];
    for (n, p) in prediction.iter_mut().enumerate() {
        let mut best = i32::MAX;
        let mut chosen = 0;
        'search: for i in 0..15 {
            let low = i32::from(QUANT[i]);
            let step = mul_wb(i32::from(QUANT[i + 1]) - low, 6554);
            for j in 0..5 {
                let level = low + step * (2 * j + 1);
                let error = (*p - level).abs();
                if error < best {
                    best = error;
                    chosen = level;
                    indices[n] = [(i % 3) as u8, j as u8, (i / 3) as u8];
                } else {
                    break 'search;
                }
            }
        }
        *p = chosen;
    }
    prediction[0] -= prediction[1];
    indices
}

#[cfg(test)]
#[path = "stereo_tests.rs"]
mod tests;

/// Shared complexity, byte-policy and loss controls for stereo encoding.
pub use super::mono::MonoOptions as StereoOptions;
use super::{
    channel::{FrameControls, FrameError},
    mono::{MonoPacket, PacketDuration, dc_reject, packet, plc_packet, target_snr},
};
use crate::silk::{
    pitch::PitchCoding,
    resampler::{OutputRate, ResampleError},
};
/// Invalid stereo packet input/controls or a failed input/channel stage.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum StereoPacketError {
    /// Interleaved PCM length does not match two channels at the requested duration.
    InvalidInputLength,
    /// Packet byte budget, complexity or loss percentage is invalid.
    InvalidOptions,
    /// Input resampling failed.
    Resampler(ResampleError),
    /// M/S analysis or stereo field encoding failed.
    Stereo(StereoEncodeError),
    /// A mid/side channel frame could not be encoded.
    Frame(FrameError),
    /// Bandwidth transition controls or internal filtering failed.
    Bandwidth(BandwidthError),
    /// Packet headers or entropy termination failed.
    Entropy(EncoderError),
}
impl fmt::Display for StereoPacketError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInputLength => {
                f.write_str("SILK stereo PCM length does not match packet duration")
            }
            Self::InvalidOptions => f.write_str("invalid SILK stereo options or packet budget"),
            Self::Resampler(e) => write!(f, "SILK stereo input: {e}"),
            Self::Stereo(e) => write!(f, "SILK stereo analysis: {e}"),
            Self::Frame(e) => write!(f, "SILK stereo channel: {e}"),
            Self::Bandwidth(e) => write!(f, "SILK bandwidth transition: {e}"),
            Self::Entropy(e) => write!(f, "SILK stereo packet: {e}"),
        }
    }
}
impl std::error::Error for StereoPacketError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Resampler(e) => Some(e),
            Self::Stereo(e) => Some(e),
            Self::Frame(e) => Some(e),
            Self::Bandwidth(e) => Some(e),
            Self::Entropy(e) => Some(e),
            _ => None,
        }
    }
}
impl From<ResampleError> for StereoPacketError {
    fn from(e: ResampleError) -> Self {
        Self::Resampler(e)
    }
}
impl From<StereoEncodeError> for StereoPacketError {
    fn from(e: StereoEncodeError) -> Self {
        Self::Stereo(e)
    }
}
impl From<StereoFrameError> for StereoPacketError {
    fn from(error: StereoFrameError) -> Self {
        match error {
            StereoFrameError::Stereo(e) => Self::Stereo(e),
            StereoFrameError::Frame(e) => Self::Frame(e),
        }
    }
}
impl From<FrameError> for StereoPacketError {
    fn from(e: FrameError) -> Self {
        Self::Frame(e)
    }
}
impl From<BandwidthError> for StereoPacketError {
    fn from(e: BandwidthError) -> Self {
        Self::Bandwidth(e)
    }
}
impl From<EncoderError> for StereoPacketError {
    fn from(e: EncoderError) -> Self {
        Self::Entropy(e)
    }
}
/// Complete stereo Opus packet, including TOC and optional packet-level padding.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StereoPacket {
    packet: MonoPacket,
}
impl StereoPacket {
    /// Complete Opus packet bytes, excluding any transport/container overhead.
    pub fn as_bytes(&self) -> &[u8] {
        self.packet.as_bytes()
    }
    /// Automatic-mode output requested decoder concealment after a native SILK
    /// packet budget overflow. This is distinct from DTX.
    pub fn used_plc(&self) -> bool {
        self.packet.used_plc()
    }
    /// Final entropy range for independent decoder validation.
    pub fn final_range(&self) -> u32 {
        self.packet.final_range()
    }
}
/// Raw interleaved stereo PCM to explicit-bandwidth SILK Opus packets. Supports
/// adaptive M/S prediction/width, panned mid-only omission and side restarts.
/// External rate/channel count remain fixed; internal bandwidth can be changed
/// at packet boundaries. Supports DTX/FEC, gradual targets and automatic bandwidth.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StereoEncoder {
    input: OutputRate,
    internal: SilkRate,
    dc: [i32; 2],
    speech: SpeechState<2>,
    bandwidth_control: PacketBandwidth<2>,
}
impl StereoEncoder {
    /// Start a stereo stream at any standard PCM rate and fixed SILK bandwidth.
    pub fn new(input: OutputRate, internal: SilkRate) -> Self {
        Self {
            input,
            internal,
            dc: [0; 2],
            speech: SpeechState::new(input, internal),
            bandwidth_control: PacketBandwidth::default(),
        }
    }
    /// Nominal pre-skip on the48kHz clock, including stereo/input compensation.
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
    /// Force a fixed bandwidth at a packet boundary. Both channel histories and
    /// live resamplers migrate atomically; stereo predictors, VAD/DC, counters,
    /// DTX and reservoir persist. Incompatible queued FEC is cleared. Last-packet
    /// diagnostics keep their meaning. A changed rate stops gradual targeting;
    /// an identical-rate call preserves it.
    /// Same-band calls are no-ops; errors preserve the entire stream.
    pub fn set_bandwidth(&mut self, rate: SilkRate) -> Result<(), StereoPacketError> {
        if rate == self.internal {
            return Ok(());
        }
        let mut next = self.clone();
        next.change_bandwidth(rate)?;
        next.bandwidth_control.set_target(None);
        *self = next;
        Ok(())
    }
    fn change_bandwidth(&mut self, rate: SilkRate) -> Result<(), StereoPacketError> {
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
    /// Request in-band FEC using `StereoOptions::packet_loss_percent`. Disabled
    /// by default. Fixed bandwidth is preserved; bitrate/activity gate protection.
    /// Queued redundancy can still be carried by the next packet after disabling.
    pub fn set_inband_fec(&mut self, enabled: bool) {
        self.speech.fec.enabled = enabled;
    }
    /// Whether the last successful packet carries prior-packet redundancy.
    pub fn has_fec(&self) -> bool {
        self.speech.fec.sent.iter().any(|&v| v)
    }
    /// Whether the last successfully encoded packet suppressed its payload.
    pub fn in_dtx(&self) -> bool {
        self.speech.dtx.suppressed
    }
    /// Clear every stream history, retaining rates, bandwidth mode/target and DTX/FEC settings.
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
    /// Encode one10/20/40/60ms interleaved L/R packet. Byte budget includes TOC /
    /// packet padding and is3..=1276. CBR fills the budget, variable mode caps it.
    /// Enabled DTX can instead emit a one-byte packet in either mode.
    /// Any error preserves all resampling, VAD, stereo and channel state.
    /// Automatic mode can return native PLC output on packet overflow; inspect
    /// the returned packet's `used_plc()` flag separately from DTX.
    pub fn encode(
        &mut self,
        duration: PacketDuration,
        input: &[i16],
        packet_bytes: usize,
        options: StereoOptions,
    ) -> Result<StereoPacket, StereoPacketError> {
        let ms = duration.milliseconds() as usize;
        let samples = ms * self.input.hz() as usize / 1000;
        if input.len() != samples * 2 {
            return Err(StereoPacketError::InvalidInputLength);
        }
        if !(3..=1276).contains(&packet_bytes)
            || options.complexity > 10
            || options.packet_loss_percent > 100
        {
            return Err(StereoPacketError::InvalidOptions);
        }
        let mut next = self.clone();
        let rate = next.bandwidth_control.prepare(
            self.input,
            self.internal,
            duration,
            packet_bytes,
            options,
            self.speech.fec.enabled,
        )?;
        next.change_bandwidth(rate)?;
        let digital_silence = input.iter().all(|&v| v == 0);
        next.speech.dtx.begin(digital_silence);
        let mut pcm = [[0i16; 960]; 2];
        for (channel, converted) in pcm.iter_mut().enumerate() {
            let mut planar = [0i16; 2880];
            let mut filtered = [0i16; 2880];
            for (sample, pair) in planar[..samples].iter_mut().zip(input.chunks_exact(2)) {
                *sample = pair[channel];
            }
            dc_reject(
                &mut next.dc[channel],
                self.input,
                &planar[..samples],
                &mut filtered[..samples],
            );
            next.speech.resampler[channel].process(&filtered[..samples], converted)?;
        }
        let backup = next.speech.fec.has_pending(duration).then(|| next.clone());
        let attempt =
            next.encode_packet(duration, &pcm, packet_bytes, options, digital_silence, true);
        let retry = match &attempt {
            Err(StereoPacketError::Frame(e)) => fec::budget_failure(e),
            Err(StereoPacketError::Entropy(EncoderError::CapacityExhausted))
            | Err(StereoPacketError::Stereo(StereoEncodeError::Entropy(
                EncoderError::CapacityExhausted,
            ))) => true,
            _ => false,
        };
        if retry {
            if let Some(mut backup) = backup {
                let packet = backup.encode_packet(
                    duration,
                    &pcm,
                    packet_bytes,
                    options,
                    digital_silence,
                    false,
                )?;
                *self = backup;
                return Ok(packet);
            }
        }
        let packet = attempt?;
        *self = next;
        Ok(packet)
    }
    fn encode_packet(
        &mut self,
        duration: PacketDuration,
        pcm: &[[i16; 960]; 2],
        packet_bytes: usize,
        options: StereoOptions,
        digital_silence: bool,
        include_fec: bool,
    ) -> Result<StereoPacket, StereoPacketError> {
        let ms = duration.milliseconds() as usize;
        let frames = duration.frames();
        let frame_samples = ms * self.internal.khz() as usize / frames;
        let maximum = self.bandwidth_control.maximum;
        let bitrate = (packet_bytes as i32 - 1) * 8 * 1000 / ms as i32;
        let nominal_bits = bitrate * ms as i32 / 1000;

        let mut writer = RangeEncoder::new(1275)?;
        let header_bits = (frames as u32 + 1) * 2;
        writer.encode_icdf(0, &[(256 - (256 >> header_bits)) as u8, 0], 8)?;
        self.speech.fec.begin_with_policy(
            duration,
            self.internal,
            bitrate,
            options.packet_loss_percent,
            self.bandwidth_control.fec_override(),
        );
        let channels = &mut self.speech.channels;
        self.speech
            .fec
            .prefix(&mut writer, include_fec, |ch, writer, frame, coding| {
                channels[ch].encode_redundant(writer, frame, coding)
            })?;
        let per_frame = (nominal_bits - self.speech.fec.used_bits) / frames as i32;
        let mut flags = [[false; 3]; 2];
        for index in 0..frames {
            let mut target = per_frame
                * if duration == PacketDuration::Ms10 {
                    100
                } else {
                    50
                }
                - self.speech.excess * 2;
            if index > 0 {
                target -=
                    (writer.tell() as i32 - self.speech.fec.used_bits - per_frame * index as i32)
                        * 2;
            }
            target = target.clamp(bitrate.min(5000), bitrate.max(5000));
            let start = index * frame_samples;
            let end = start + frame_samples;
            let stereo = self.speech.analyze_stereo(
                self.internal,
                duration.frame_duration(),
                [&pcm[0][start..end], &pcm[1][start..end]],
                AnalysisControls {
                    target_rate: target,
                    digital_silence,
                    to_mono: false,
                },
            )?;
            let mid_activity = stereo.activities[0].ok_or(FrameError::InvalidControls)?;
            let side_activity = stereo.activities[1];
            flags[0][index] = stereo.active(0);
            flags[1][index] = stereo.active(1);
            self.speech.fec.stereo[index] = stereo.stereo;
            stereo.encode_prefix(&mut writer)?;
            let limit = match (frames, index) {
                (2, 0) => maximum * 3 / 5,
                (3, 0) => maximum * 2 / 5,
                (3, 1) => maximum * 3 / 4,
                _ => maximum,
            };
            for (channel, channel_flags) in flags.iter().enumerate() {
                if channel == 1 && stereo.mid_only() {
                    continue;
                }
                let activity = if channel == 0 {
                    mid_activity
                } else {
                    side_activity.ok_or(StereoPacketError::InvalidOptions)?
                };
                let mut cap = limit;
                let mut fill = !options.variable_bitrate && index + 1 == frames;
                if channel == 0 && stereo.rates[1] > 0 {
                    fill = false;
                    cap = cap.saturating_sub(maximum / (frames as u32 * 2));
                }
                let coding = if index == 0 {
                    PitchCoding::Independent
                } else if channel == 1 && self.speech.previous_mid_only {
                    PitchCoding::IndependentWithoutScaling
                } else {
                    PitchCoding::Conditional
                };
                let fec_controls = self.speech.fec.controls(channel, index);
                let hard = if self.automatic_bandwidth() {
                    writer.capacity() as u32 * 8
                } else if fec_controls.is_some() || self.speech.fec.sent.iter().any(|&v| v) {
                    if channel == 0 && stereo.rates[1] > 0 {
                        maximum.saturating_sub(
                            maximum.saturating_sub(writer.tell()) / (frames as u32 * 2),
                        )
                    } else {
                        maximum
                    }
                } else {
                    cap
                };
                let frame = if channel == 0 {
                    &stereo.pcm[0][..stereo.length]
                } else {
                    &stereo.pcm[1][..stereo.length]
                };
                let mut tapered = [0i16; 320];
                tapered[..frame.len()].copy_from_slice(frame);
                self.bandwidth_control.transitions[channel].filter(
                    self.internal,
                    duration.frame_duration(),
                    &mut tapered[..frame.len()],
                )?;
                let encoded = self.speech.channels[channel].encode_with_redundancy(
                    &mut writer,
                    &tapered[..frame.len()],
                    FrameControls {
                        duration: duration.frame_duration(),
                        complexity: options.complexity,
                        snr_db_q7: target_snr(
                            self.internal,
                            duration.frame_duration(),
                            stereo.rates[channel],
                        ),
                        activity,
                        active: channel_flags[index],
                        constant_bitrate: !options.variable_bitrate,
                        fill_budget: fill,
                        coding,
                        max_bits: cap,
                        packet_loss_percent: options.packet_loss_percent,
                        frames_per_packet: frames as u8,
                        redundancy: self.speech.fec.sent[channel],
                    },
                    hard,
                    fec_controls,
                )?;
                self.speech.fec.record(channel, index, encoded.redundancy());
            }
            self.speech.finish_analysis(&stereo)?;
        }
        let mut header = 0;
        for (channel, has_fec) in flags.into_iter().zip(self.speech.fec.sent) {
            for &flag in &channel[..frames] {
                header = (header << 1) | u32::from(flag);
            }
            header = (header << 1) | u32::from(has_fec);
        }
        writer.patch_initial_bits(header, header_bits)?;
        let encoded_length = writer.tell().div_ceil(8) as usize;
        let overflow = encoded_length >= packet_bytes;
        // Native outer Opus converts a completed SILK overshoot into PLC. Give
        // queued FEC one prefix-free retry first, preserving primary audio.
        if overflow
            && (!self.automatic_bandwidth()
                || (include_fec && self.speech.fec.sent.iter().any(|&v| v)))
        {
            return Err(FrameError::BitBudgetExceeded.into());
        }
        let range = if overflow { 0 } else { writer.range() };
        let payload = if overflow {
            &[0][..]
        } else {
            writer.shrink(encoded_length)?;
            writer.finish()?
        };
        let base = match self.internal {
            SilkRate::Khz8 => 0,
            SilkRate::Khz12 => 4,
            SilkRate::Khz16 => 8,
        };
        let config = base
            + match duration {
                PacketDuration::Ms10 => 0,
                PacketDuration::Ms20 => 1,
                PacketDuration::Ms40 => 2,
                PacketDuration::Ms60 => 3,
            };
        let decision = self.speech.dtx.finish(duration, 2, digital_silence);
        if decision != Decision::Transmit || overflow {
            self.speech.fec.suppress();
        }
        let result = StereoPacket {
            packet: if decision != Decision::Transmit {
                packet((config << 3) | 4, &[], 1, 0)
            } else if overflow {
                plc_packet(
                    (config << 3) | 4,
                    if options.variable_bitrate {
                        2
                    } else {
                        packet_bytes
                    },
                )
            } else {
                packet(
                    (config << 3) | 4,
                    payload,
                    if options.variable_bitrate {
                        payload.len() + 1
                    } else {
                        packet_bytes
                    },
                    range,
                )
            },
        };
        let payload_bits = if decision == Decision::Silk {
            0
        } else {
            encoded_length as i32 * 8
        };
        self.speech.excess = (self.speech.excess + payload_bits - nominal_bits).clamp(0, 10000);
        self.bandwidth_control
            .finish(self.speech.activity, duration);
        Ok(result)
    }
}
