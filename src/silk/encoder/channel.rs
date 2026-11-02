//! Internal-rate PCM channel frames with fixed-delay buffering and bitrate RDO.
//! The enclosing input/packet layer owns resampling, VAD, transition filtering,
//! stereo/LBRR/header ordering and final packet termination.
use super::{
    FrameContext, FrameEncoderState, FrameIndices, SilkEncodeError,
    analysis::{
        AnalysisError, GainProcessState, LtpState, PitchOptions, PitchState, ProcessedGains,
        QuantizedGains, ResidualEnergies, ShapeOptions, ShapeState, SpectralState, SpeechActivity,
    },
    math::log2lin,
    nsq::{NoiseShapingQuantizer, NsqError, NsqParameters},
};
use crate::{
    range::{EncoderError, RangeEncoder},
    silk::{
        gains::{FrameActivity, FrameDuration, SignalType, linear_gain, reconstruct_index},
        pitch::{PitchCoding, SilkRate, tables::SCALES_Q14},
        resampler::{EncoderResamplerState, OutputRate, ResampleError, ResamplerState},
    },
};
use std::fmt;
/// Channel analysis, loss scaling and absolute packet bit-budget controls.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FrameControls {
    /// One internal 10/20 ms frame.
    pub duration: FrameDuration,
    /// Encoder complexity, 0..=10.
    pub complexity: u8,
    /// Coding SNR in dB Q7, 0..=8192.
    pub snr_db_q7: u16,
    /// Original VAD output, evaluated before channel delay/transition filtering.
    pub activity: SpeechActivity,
    /// Outer VAD/DTX decision; inactive frames suppress voiced pitch search.
    pub active: bool,
    /// CBR analysis policy (disables background-activity SNR reduction).
    pub constant_bitrate: bool,
    /// Fill the available budget. Usually true only for the final CBR channel/frame.
    pub fill_budget: bool,
    /// Independent/conditional gain, lag and LTP-scale signaling.
    pub coding: PitchCoding,
    /// Total entropy tell() target, including preceding packet fields. This is
    /// a hard bound for `encode`; `encode_with_redundancy` takes a separate hard
    /// packet bound to allow native damage control past a partial-frame quota.
    pub max_bits: u32,
    /// Expected packet loss percentage, 0..=100, for LTP scaling.
    pub packet_loss_percent: u8,
    /// Number of internal frames in this packet, 1..=3.
    pub frames_per_packet: u8,
    /// Whether the packet's redundancy reduces effective LTP loss.
    pub redundancy: bool,
}
/// Invalid geometry/controls, fixed-point domain, or an unattainable bit budget.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum FrameError {
    /// Input length does not match the internal rate/duration.
    InvalidInputLength,
    /// Invalid quality, loss, complexity or bit-budget controls.
    InvalidControls,
    /// No legal frame fits the requested bit/storage budget after bounded search.
    BitBudgetExceeded,
    /// PCM analysis exceeded its validated domain.
    Analysis(AnalysisError),
    /// Noise-shaped quantization rejected a parameter.
    Quantizer(NsqError),
    /// Header lifecycle or frame entropy encoding failed.
    Serialization(SilkEncodeError),
    /// Channel-history conversion or input-resampler priming failed.
    Resampler(ResampleError),
}
impl fmt::Display for FrameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInputLength => {
                f.write_str("SILK channel PCM length does not match frame geometry")
            }
            Self::InvalidControls => f.write_str("invalid SILK channel frame controls"),
            Self::BitBudgetExceeded => f.write_str("SILK frame cannot fit bit budget"),
            Self::Analysis(e) => write!(f, "SILK frame analysis: {e}"),
            Self::Quantizer(e) => write!(f, "SILK frame quantization: {e}"),
            Self::Serialization(e) => write!(f, "SILK frame serialization: {e}"),
            Self::Resampler(e) => write!(f, "SILK channel rate change: {e}"),
        }
    }
}
impl std::error::Error for FrameError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Analysis(e) => Some(e),
            Self::Quantizer(e) => Some(e),
            Self::Serialization(e) => Some(e),
            Self::Resampler(e) => Some(e),
            _ => None,
        }
    }
}
impl From<AnalysisError> for FrameError {
    fn from(e: AnalysisError) -> Self {
        Self::Analysis(e)
    }
}
impl From<NsqError> for FrameError {
    fn from(e: NsqError) -> Self {
        Self::Quantizer(e)
    }
}
impl From<SilkEncodeError> for FrameError {
    fn from(e: SilkEncodeError) -> Self {
        Self::Serialization(e)
    }
}
impl From<ResampleError> for FrameError {
    fn from(e: ResampleError) -> Self {
        Self::Resampler(e)
    }
}
/// Selected frame symbols/pulses and bounded-search accounting.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EncodedFrame {
    indices: FrameIndices,
    pulses: [i8; 320],
    pcm: [i16; 320],
    length: usize,
    bits: u32,
    iterations: u8,
    calls: u8,
    damaged: bool,
    redundant: Option<RedundantFrame>,
}
impl EncodedFrame {
    /// Lower-rate redundancy captured before main bitrate search, if requested
    /// and speech activity exceeded the native LBRR threshold. The enclosing
    /// packet encoder must retain this for the following packet.
    pub fn redundancy(&self) -> Option<&RedundantFrame> {
        self.redundant.as_ref()
    }
    /// Actual selected/transmitted side information, including the winning seed.
    pub fn indices(&self) -> &FrameIndices {
        &self.indices
    }
    /// Actual selected/transmitted excitation pulses.
    pub fn pulses(&self) -> &[i8] {
        &self.pulses[..self.length]
    }
    /// Internal NSQ reconstruction. In the final native damage-control fallback,
    /// pulses are cleared after quantization; this PCM then differs from decoding.
    pub fn quantizer_pcm(&self) -> &[i16] {
        &self.pcm[..self.length]
    }
    /// Total writer tell() after this frame, including preceding packet fields.
    pub fn bits(&self) -> u32 {
        self.bits
    }
    /// Search iterations, including gain-cache hits, at most seven.
    pub fn iterations(&self) -> u8 {
        self.iterations
    }
    /// Actual quantizer calls, excluding gain-cache hits, at most seven.
    pub fn quantizer_calls(&self) -> u8 {
        self.calls
    }
    /// Final fallback held the previous gain and cleared pulses to fit the cap.
    pub fn used_damage_control(&self) -> bool {
        self.damaged
    }
}
/// Native lower-rate redundant quantization controls for one analyzed frame.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RedundancyControls {
    /// First gain-symbol increase, 0..=7. Native packet policy starts at seven
    /// and adapts to 3..=7 based on packet loss percentage.
    pub gain_increase: u8,
    /// First redundant frame in the packet or after a nonredundant frame.
    /// Rebase redundant gain history on this frame's main quantized gain index.
    pub reset_gain_history: bool,
}
/// Lower-rate redundant frame, to be carried by the following packet. Its NSQ
/// starts from a copy of the main pre-search state and never replaces that state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RedundantFrame {
    rate: SilkRate,
    duration: FrameDuration,
    indices: FrameIndices,
    pulses: [i8; 320],
    length: usize,
}
impl RedundantFrame {
    /// Quantized symbols, including the redundant quantizer's winning seed.
    pub fn indices(&self) -> &FrameIndices {
        &self.indices
    }
    /// Lower-rate excitation pulses.
    pub fn pulses(&self) -> &[i8] {
        &self.pulses[..self.length]
    }
}
/// One fixed-internal-rate encoder channel. PCM has already passed the input
/// front end; activity describes the undelayed VAD frame. Maintains the native
/// fixed 5 ms input delay, independent of the complexity-dependent shape window.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChannelFrameEncoder {
    rate: SilkRate,
    history: [i16; 720],
    counter: u32,
    first: bool,
    pitch: PitchState,
    shape: ShapeState,
    ltp: LtpState,
    spectrum: SpectralState,
    gains: GainProcessState,
    nsq: NoiseShapingQuantizer,
    entropy: FrameEncoderState,
    redundant_gain: u8,
}
impl ChannelFrameEncoder {
    /// Start a fresh channel at an internal rate. All external-rate conversion
    /// and midstream internal-rate history conversion belong to the input layer.
    pub fn new(rate: SilkRate) -> Self {
        Self {
            rate,
            history: [0; 720],
            counter: 0,
            first: true,
            pitch: PitchState::default(),
            shape: ShapeState::default(),
            ltp: LtpState::default(),
            spectrum: SpectralState::default(),
            gains: GainProcessState::default(),
            nsq: NoiseShapingQuantizer::default(),
            entropy: FrameEncoderState::default(),
            redundant_gain: 0,
        }
    }
    /// Clear every channel history while retaining the internal rate.
    pub fn reset(&mut self) {
        *self = Self::new(self.rate);
    }
    /// Currently configured internal PCM rate.
    pub fn internal_rate(&self) -> SilkRate {
        self.rate
    }
    /// Change internal rate at a packet boundary while migrating native channel
    /// history through old internal -> external API -> new internal resampling.
    /// `previous_duration` is the previous configured internal frame duration,
    /// including for an omitted side channel; it is not the next packet duration.
    /// The supplied input resampler must match this channel's old rates. It is
    /// replaced by the new, history-primed state only on success. Prediction,
    /// shaping and NSQ restart; frame count, entropy and redundant gain history
    /// are retained. VAD, stereo, DTX, LP state and pending FEC belong to the
    /// enclosing encoder and must follow their own native switching rules.
    /// Same-rate calls are no-ops. Any error preserves both states.
    pub fn change_rate(
        &mut self,
        input: OutputRate,
        rate: SilkRate,
        previous_duration: FrameDuration,
        resampler: &mut EncoderResamplerState,
    ) -> Result<(), FrameError> {
        if !resampler.has_rates(input, self.rate) {
            return Err(FrameError::InvalidControls);
        }
        if rate == self.rate {
            return Ok(());
        }
        let ms = if previous_duration == FrameDuration::Ms10 {
            25
        } else {
            45
        };
        let old_length = ms * self.rate.khz() as usize;
        let api_length = ms * input.hz() as usize / 1000;
        let new_length = ms * rate.khz() as usize;
        let mut api = [0i16; 2160];
        let mut bridge = ResamplerState::new(input);
        bridge.process(
            self.rate,
            &self.history[..old_length],
            &mut api[..api_length],
        )?;
        let mut next = self.clone();
        let mut converted = EncoderResamplerState::new(input, rate);
        converted.process(&api[..api_length], &mut next.history[..new_length])?;
        next.rate = rate;
        next.restart_prediction();
        *self = next;
        *resampler = converted;
        Ok(())
    }
    // Native side restart clears prediction/shape state while preserving input
    // delay, frame counter and entropy history. Independent-without-scaling
    // coding restarts the side inside a packet after mid-only omission.
    pub(crate) fn restart_prediction(&mut self) {
        self.first = true;
        self.pitch.reset();
        self.shape.reset();
        self.spectrum.reset();
        self.gains.reset();
        self.nsq.reset();
    }
    /// Buffer a prefill frame without analysis/entropy coding, as used when an
    /// enclosing Opus encoder warms SILK state before switching modes.
    pub fn prefill(&mut self, duration: FrameDuration, input: &[i16]) -> Result<(), FrameError> {
        let count = subframes(duration);
        let length = count * 5 * self.rate.khz() as usize;
        if input.len() != length {
            return Err(FrameError::InvalidInputLength);
        }
        self.insert(input);
        self.advance(length);
        self.counter = self.counter.wrapping_add(1);
        Ok(())
    }
    /// Encode one filtered PCM frame into an unfinished packet writer. Headers
    /// and preceding redundancy/stereo fields must already be present. Uses at
    /// most seven NSQ trials; overflow candidates are retried as over-budget.
    /// The writer remains unfinished. Any error preserves writer and all channel
    /// history, including buffering, gain, spectral state and frame counter.
    pub fn encode(
        &mut self,
        writer: &mut RangeEncoder,
        input: &[i16],
        controls: FrameControls,
    ) -> Result<EncodedFrame, FrameError> {
        self.encode_with_redundancy(writer, input, controls, controls.max_bits, None)
    }
    /// Encode with optional native LBRR generation before the main bitrate
    /// search. Redundancy does not enter this packet automatically; the returned
    /// frame belongs in the next packet. `FrameControls::redundancy` separately
    /// selects the existing LTP loss-scaling policy. Errors preserve both writer
    /// and all main/redundant channel histories. `controls.max_bits` is the native
    /// RDO frame target. `packet_max_bits` is a separate hard bound, at least that
    /// target: final damage control may exceed a partial-frame quota after an
    /// LBRR prefix, but must still fit the enclosing packet.
    pub fn encode_with_redundancy(
        &mut self,
        writer: &mut RangeEncoder,
        input: &[i16],
        controls: FrameControls,
        packet_max_bits: u32,
        redundancy: Option<RedundancyControls>,
    ) -> Result<EncodedFrame, FrameError> {
        let count = subframes(controls.duration);
        let khz = self.rate.khz() as usize;
        let length = count * 5 * khz;
        if input.len() != length {
            return Err(FrameError::InvalidInputLength);
        }
        if controls.complexity > 10
            || controls.snr_db_q7 > 8192
            || controls.packet_loss_percent > 100
            || !(1..=3).contains(&controls.frames_per_packet)
            || controls.max_bits == 0
            || controls.max_bits > packet_max_bits
            || packet_max_bits > writer.capacity() as u32 * 8
            || controls.activity.quality_bands_q15.iter().any(|&v| v < 0)
            || redundancy.is_some_and(|r| r.gain_increase > 7)
            || (redundancy.is_some() && !controls.active && controls.activity.probability_q8 > 77)
        {
            return Err(FrameError::InvalidControls);
        }
        if writer.is_finished() {
            return Err(SilkEncodeError::Entropy(EncoderError::Finished).into());
        }
        if writer.tell() >= packet_max_bits {
            return Err(FrameError::BitBudgetExceeded);
        }
        if controls.coding == PitchCoding::Conditional && self.first {
            return Err(SilkEncodeError::MissingHistory.into());
        }
        let mut staged = self.clone();
        staged.insert(input);
        let pitch = staged.pitch.analyze(
            self.rate,
            controls.duration,
            &staged.history[..(22 + count * 5) * khz],
            PitchOptions {
                complexity: controls.complexity,
                active: controls.active,
                speech_activity_q8: u16::from(controls.activity.probability_q8),
                input_tilt_q15: controls.activity.tilt_q15,
            },
        )?;
        let lookahead = if matches!(controls.complexity, 0 | 2) {
            3
        } else {
            5
        };
        let shape = staged.shape.analyze(
            self.rate,
            controls.duration,
            &staged.history[..(20 + count * 5 + lookahead) * khz],
            &pitch,
            ShapeOptions {
                complexity: controls.complexity,
                snr_db_q7: controls.snr_db_q7,
                activity: controls.activity,
                constant_bitrate: controls.constant_bitrate,
            },
        )?;
        let ltp = staged.ltp.analyze(&pitch, false);
        let inverse = shape.inverse_gains_q16();
        let normalized =
            ltp.normalized_residual(&staged.history[..(22 + count * 5) * khz], &inverse[..count])?;
        let spectral = staged.spectrum.analyze(
            self.rate,
            controls.duration,
            normalized.samples(),
            shape.spectral_options(&ltp, staged.first)?,
        )?;
        let energies = ResidualEnergies::analyze(
            self.rate,
            controls.duration,
            &normalized,
            &spectral,
            &inverse[..count],
        )?;
        let conditional = controls.coding == PitchCoding::Conditional;
        let processed = staged.gains.process(&shape, &energies, &ltp, conditional)?;
        let scale = ltp_scale(pitch.signal(), ltp.prediction_gain_db_q7(), controls);
        let mut parameters = NsqParameters {
            signal: pitch.signal(),
            offset: processed.offset(),
            seed: (staged.counter & 3) as u8,
            interpolation_q2: spectral.interpolation_q2(),
            predictor_q12: *spectral.predictors_q12(),
            ltp_q14: [[0; 5]; 4],
            shaping_order: shape.order() as u8,
            shaping_q13: [[0; 24]; 4],
            harmonic_q14: [0; 4],
            tilt_q14: [0; 4],
            low_frequency_q14: [0; 4],
            gains_q16: [0; 4],
            pitch: [0; 4],
            lambda_q10: processed.lambda_q10(),
            ltp_scale_q14: if pitch.signal() == SignalType::Voiced {
                SCALES_Q14[usize::from(scale)] as u16
            } else {
                0
            },
        };
        parameters.ltp_q14[..count].copy_from_slice(ltp.coefficients_q14());
        parameters.shaping_q13[..count].copy_from_slice(shape.coefficients_q13());
        parameters.harmonic_q14[..count].copy_from_slice(shape.harmonic_q14());
        parameters.tilt_q14[..count].copy_from_slice(shape.tilt_q14());
        parameters.low_frequency_q14[..count].copy_from_slice(shape.low_frequency_q14());
        parameters.pitch[..count].copy_from_slice(pitch.lags());
        let mut indices = FrameIndices {
            signal: pitch.signal(),
            offset: processed.offset(),
            gains: [0; 4],
            nlsf: [0; 17],
            interpolation_q2: spectral.interpolation_q2(),
            lag: pitch.lag_index(),
            contour: pitch.contour_index(),
            periodicity: ltp.periodicity_index(),
            ltp: [0; 4],
            ltp_scale: scale,
            seed: parameters.seed,
        };
        let symbols = spectral.quantized().indices();
        indices.nlsf[..symbols.len()].copy_from_slice(symbols);
        indices.ltp[..count].copy_from_slice(ltp.indices());
        let context = FrameContext {
            rate: self.rate,
            duration: controls.duration,
            activity: if controls.active {
                FrameActivity::Active
            } else {
                FrameActivity::Inactive
            },
            coding: controls.coding,
        };
        let mut current_pcm = [0i16; 320];
        current_pcm[..length].copy_from_slice(&staged.history[20 * khz..20 * khz + length]);
        let redundant = if let Some(r) =
            redundancy.filter(|_| controls.activity.probability_q8 > 77)
        {
            let mut redundant_indices = indices.clone();
            redundant_indices.gains[..count].copy_from_slice(processed.quantized().indices());
            let mut redundant_parameters = parameters.clone();
            if r.reset_gain_history {
                staged.redundant_gain = staged.gains.quantized_index();
                redundant_indices.gains[0] = (redundant_indices.gains[0] + r.gain_increase).min(63);
            }
            for k in 0..count {
                staged.redundant_gain = reconstruct_index(
                    staged.redundant_gain,
                    redundant_indices.gains[k],
                    k == 0 && !conditional,
                );
                redundant_parameters.gains_q16[k] = linear_gain(staged.redundant_gain);
            }
            let mut quantizer = staged.nsq.clone();
            let states = decision_states(controls.complexity);
            let quantized = if states > 1 || shape.warping_q16() > 0 {
                quantizer.quantize_delayed(
                    self.rate,
                    controls.duration,
                    &current_pcm[..length],
                    &redundant_parameters,
                    states,
                    shape.warping_q16(),
                )?
            } else {
                quantizer.quantize(
                    self.rate,
                    controls.duration,
                    &current_pcm[..length],
                    &redundant_parameters,
                )?
            };
            redundant_indices.seed = quantized.seed();
            let mut pulses = [0i8; 320];
            pulses[..length].copy_from_slice(quantized.pulses());
            Some(RedundantFrame {
                rate: self.rate,
                duration: controls.duration,
                indices: redundant_indices,
                pulses,
                length,
            })
        } else {
            None
        };
        let data = SearchData {
            context,
            input: &current_pcm[..length],
            parameters,
            indices,
            quantized: processed.quantized().clone(),
            processed: &processed,
            controls,
            warping: shape.warping_q16(),
            packet_max_bits,
        };
        let selected = staged.search(writer, data)?;
        let Trial {
            writer: next,
            entropy,
            nsq,
            gains,
            indices,
            pulses,
            pcm,
            bits,
            damaged,
            ..
        } = selected.trial;
        staged.entropy = entropy;
        staged.nsq = nsq;
        staged.gains = gains;
        staged.advance(length);
        staged.counter = staged.counter.wrapping_add(1);
        staged.first = false;
        let result = EncodedFrame {
            indices,
            pulses,
            pcm,
            length,
            bits,
            iterations: selected.iterations,
            calls: selected.calls,
            damaged,
            redundant,
        };
        *writer = next;
        *self = staged;
        Ok(result)
    }
    /// Serialize a saved redundant frame before the current packet's normal
    /// frames, after its header/mask/stereo fields. Conditional coding requires
    /// a preceding redundant frame for this channel. The caller owns packet
    /// ordering; errors preserve entropy writer and all channel state.
    pub fn encode_redundant(
        &mut self,
        writer: &mut RangeEncoder,
        frame: &RedundantFrame,
        coding: PitchCoding,
    ) -> Result<(), FrameError> {
        if frame.rate != self.rate || coding == PitchCoding::IndependentWithoutScaling {
            return Err(FrameError::InvalidControls);
        }
        self.entropy.encode(
            writer,
            FrameContext {
                rate: frame.rate,
                duration: frame.duration,
                activity: FrameActivity::Redundant,
                coding,
            },
            &frame.indices,
            frame.pulses(),
        )?;
        Ok(())
    }
    fn insert(&mut self, input: &[i16]) {
        let start = 25 * self.rate.khz() as usize;
        self.history[start..start + input.len()].copy_from_slice(input);
    }
    fn advance(&mut self, length: usize) {
        let keep = 25 * self.rate.khz() as usize;
        self.history.copy_within(length..length + keep, 0);
    }
}
fn subframes(duration: FrameDuration) -> usize {
    if duration == FrameDuration::Ms10 {
        2
    } else {
        4
    }
}
fn decision_states(complexity: u8) -> u8 {
    match complexity {
        0 | 1 => 1,
        2..=5 => 2,
        6..=7 => 3,
        _ => 4,
    }
}
fn ltp_scale(signal: SignalType, gain: i32, c: FrameControls) -> u8 {
    if signal != SignalType::Voiced || c.coding != PitchCoding::Independent {
        return 0;
    }
    let mut loss = i32::from(c.packet_loss_percent) * i32::from(c.frames_per_packet);
    if c.redundancy {
        loss = 2 + loss * loss / 100;
    }
    let gain = gain * loss;
    u8::from(gain > log2lin(3796 - i32::from(c.snr_db_q7)))
        + u8::from(gain > log2lin(4796 - i32::from(c.snr_db_q7)))
}
#[derive(Clone)]
struct Trial {
    writer: RangeEncoder,
    entropy: FrameEncoderState,
    nsq: NoiseShapingQuantizer,
    gains: GainProcessState,
    indices: FrameIndices,
    pulses: [i8; 320],
    pcm: [i16; 320],
    bits: u32,
    encoded: bool,
    damaged: bool,
}
struct Selected {
    trial: Trial,
    iterations: u8,
    calls: u8,
}

struct SearchData<'a> {
    context: FrameContext,
    input: &'a [i16],
    parameters: NsqParameters,
    indices: FrameIndices,
    quantized: QuantizedGains,
    processed: &'a ProcessedGains,
    controls: FrameControls,
    warping: i16,
    packet_max_bits: u32,
}
impl ChannelFrameEncoder {
    fn search(
        &mut self,
        prefix: &RangeEncoder,
        mut data: SearchData<'_>,
    ) -> Result<Selected, FrameError> {
        let count = subframes(data.context.duration);
        let maximum = data.controls.max_bits;
        let margin = if data.controls.fill_budget {
            5
        } else {
            maximum / 4
        };
        let mut multiplier = 256i32;
        let mut lower: Option<(u32, i32, u32, Trial)> = None;
        let mut upper: Option<(u32, i32, u32)> = None;
        let mut current: Option<Trial> = None;
        let mut locks = [false; 4];
        let mut best_sum = [0i32; 4];
        let mut best_multiplier = [0i16; 4];
        let mut calls = 0;
        for iteration in 0..=6 {
            let id = gains_id(data.quantized.indices());
            let cached = lower
                .as_ref()
                .filter(|x| x.0 == id)
                .map(|x| x.2)
                .or_else(|| upper.filter(|x| x.0 == id).map(|x| x.2));
            let bits = if let Some(bits) = cached {
                bits
            } else {
                let mut trial = self.trial(prefix, &data)?;
                calls += 1;
                if iteration == 6 && lower.is_none() && trial.bits > maximum {
                    let held = self.gains.hold_previous(
                        data.processed,
                        data.controls.coding == PitchCoding::Conditional,
                    );
                    trial.gains = self.gains.clone();
                    trial.indices.gains[..count].copy_from_slice(held.indices());
                    trial.pulses[..data.input.len()].fill(0);
                    trial.writer = prefix.clone();
                    trial.entropy = self.entropy.clone();
                    trial.damaged = true;
                    encode_trial(&mut trial, data.context, data.input.len())?;
                }
                let bits = trial.bits;
                current = Some(trial);
                if !data.controls.fill_budget && iteration == 0 && bits <= maximum {
                    return select(current, iteration, calls, data.packet_max_bits);
                }
                bits
            };
            if iteration == 6 {
                if let Some((lower_id, _, _, trial)) = lower {
                    if id == lower_id || bits > maximum {
                        current = Some(trial);
                    }
                }
                return select(current, iteration, calls, data.packet_max_bits);
            }
            if bits > maximum {
                if lower.is_none() && iteration >= 2 {
                    data.parameters.lambda_q10 += data.parameters.lambda_q10 / 2;
                    upper = None;
                } else {
                    upper = Some((id, multiplier, bits));
                }
            } else if bits < maximum.saturating_sub(margin) {
                if lower.as_ref().is_none_or(|x| x.0 != id) {
                    lower = Some((
                        id,
                        multiplier,
                        bits,
                        current
                            .as_ref()
                            .ok_or(FrameError::BitBudgetExceeded)?
                            .clone(),
                    ));
                } else if let Some(previous) = lower.as_mut() {
                    previous.1 = multiplier;
                    previous.2 = bits;
                }
            } else {
                return select(current, iteration, calls, data.packet_max_bits);
            }
            if lower.is_none() && bits > maximum {
                let pulses = &current
                    .as_ref()
                    .ok_or(FrameError::BitBudgetExceeded)?
                    .pulses[..data.input.len()];
                let sf = data.input.len() / count;
                for k in 0..count {
                    let sum = pulses[k * sf..(k + 1) * sf]
                        .iter()
                        .map(|&p| i32::from(p).abs())
                        .sum();
                    if iteration == 0 || sum < best_sum[k] && !locks[k] {
                        best_sum[k] = sum;
                        best_multiplier[k] = multiplier as i16;
                    } else {
                        locks[k] = true;
                    }
                }
            }
            if let (
                Some((_, lower_multiplier, lower_bits, _)),
                Some((_, upper_multiplier, upper_bits)),
            ) = (&lower, upper)
            {
                let delta = upper_multiplier - lower_multiplier;
                multiplier = lower_multiplier
                    + delta * (maximum as i32 - *lower_bits as i32)
                        / (upper_bits as i32 - *lower_bits as i32);
                let high = lower_multiplier + (delta >> 2);
                let low = upper_multiplier - (delta >> 2);
                if multiplier > high {
                    multiplier = high;
                } else if multiplier < low {
                    multiplier = low;
                }
            } else if bits > maximum {
                multiplier = (multiplier * 3 / 2).min(1024);
            } else {
                multiplier = (multiplier * 4 / 5).max(64);
            }
            let mut multipliers = [multiplier as i16; 4];
            for k in 0..count {
                if locks[k] {
                    multipliers[k] = best_multiplier[k];
                }
            }
            data.quantized = self.gains.retry(
                data.processed,
                &multipliers,
                data.controls.coding == PitchCoding::Conditional,
            )?;
        }
        Err(FrameError::BitBudgetExceeded)
    }
    fn trial(&self, prefix: &RangeEncoder, data: &SearchData<'_>) -> Result<Trial, FrameError> {
        let count = subframes(data.context.duration);
        let length = data.input.len();
        let mut parameters = data.parameters.clone();
        parameters.gains_q16[..count].copy_from_slice(data.quantized.gains_q16());
        let states = decision_states(data.controls.complexity);
        let mut nsq = self.nsq.clone();
        let quantized = if states > 1 || data.warping > 0 {
            nsq.quantize_delayed(
                self.rate,
                data.context.duration,
                data.input,
                &parameters,
                states,
                data.warping,
            )?
        } else {
            nsq.quantize(self.rate, data.context.duration, data.input, &parameters)?
        };
        let mut indices = data.indices.clone();
        indices.gains[..count].copy_from_slice(data.quantized.indices());
        indices.seed = quantized.seed();
        let mut trial = Trial {
            writer: prefix.clone(),
            entropy: self.entropy.clone(),
            nsq,
            gains: self.gains.clone(),
            indices,
            pulses: [0; 320],
            pcm: [0; 320],
            bits: 0,
            encoded: false,
            damaged: false,
        };
        trial.pulses[..length].copy_from_slice(quantized.pulses());
        trial.pcm[..length].copy_from_slice(quantized.reconstructed_pcm());
        encode_trial(&mut trial, data.context, length)?;
        Ok(trial)
    }
}
fn encode_trial(trial: &mut Trial, context: FrameContext, length: usize) -> Result<(), FrameError> {
    match trial.entropy.encode(
        &mut trial.writer,
        context,
        &trial.indices,
        &trial.pulses[..length],
    ) {
        Ok(()) => {
            trial.bits = trial.writer.tell();
            trial.encoded = true;
        }
        Err(SilkEncodeError::Entropy(EncoderError::CapacityExhausted)) => {
            // The writer is transactional. A physically overflowing candidate
            // cannot be accepted; use a conservative over-capacity cost to keep
            // searching from the original prefix, without discarding valid lows.
            trial.bits = trial.writer.capacity() as u32 * 8 + 1;
            trial.encoded = false;
        }
        Err(e) => return Err(e.into()),
    }
    Ok(())
}
fn select(
    trial: Option<Trial>,
    iteration: u8,
    calls: u8,
    maximum: u32,
) -> Result<Selected, FrameError> {
    let trial = trial.ok_or(FrameError::BitBudgetExceeded)?;
    if !trial.encoded || trial.bits > maximum {
        return Err(FrameError::BitBudgetExceeded);
    }
    Ok(Selected {
        trial,
        iterations: iteration + 1,
        calls,
    })
}
fn gains_id(indices: &[u8]) -> u32 {
    indices.iter().fold(0u32, |id, &v| (id << 8) + u32::from(v))
}

#[cfg(test)]
#[path = "channel_tests.rs"]
mod tests;
