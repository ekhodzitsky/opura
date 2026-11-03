//! Stateful signed16 PCM to single-frame CELT-only Opus packets.
//! Long-window default with optional native transient, TF, spreading, allocation
//! and pitch-prefilter policies.
//! Further adaptive decisions and SILK/Hybrid encoding remain separate work.
//! See THIRD_PARTY_NOTICES.md for constituent algorithms.
//! Copyright (c) 2007-2008 CSIRO; 2007-2010 Xiph.Org Foundation;
//! 2008 Gregory Maxwell.
//!
//! ```
//! use opura::celt::{FrameSize, encoder::CeltEncoder, input::InputRate};
//! let mut encoder = CeltEncoder::new(InputRate::Khz48, 1)?;
//! let packet = encoder.encode(FrameSize::Ms20, &[0; 960], 128)?;
//! assert_eq!(packet.as_bytes().len(), 129); // TOC plus requested payload
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
use super::{
    FrameSize,
    allocation::{self, AllocationEncoding, AllocationError},
    analysis::{self, AnalysisError},
    control::{self, ControlError},
    control_tables::TAPSET,
    energy::{BandLayout, CoarseEncoding, EnergyEncoder, EnergyEncodingError, EnergyError},
    input::{InputError, InputRate, PreemphasisState},
    partition::{self, PartitionError, SpectrumEncoding, SpectrumInput},
    postfilter::{PostfilterParams, Tapset},
    prefilter::{PrefilterError, PrefilterOptions, PrefilterState},
    pvq::Spread,
};
use crate::range::{EncoderError as EntropyError, RangeEncoder};
use std::{cell::Cell, fmt};
#[path = "vbr.rs"]
mod vbr;
pub use vbr::VbrOptions;
use vbr::{VbrFrame, VbrRate, VbrState, VbrTarget};
#[path = "hybrid_vbr.rs"]
mod hybrid_vbr;
pub(crate) use hybrid_vbr::HybridVbr;
use hybrid_vbr::VbrBudget;
#[cfg(test)]
#[path = "channel_encoder_tests.rs"]
mod channel_encoder_tests;
#[cfg(test)]
#[path = "hybrid_dynalloc_tests.rs"]
mod hybrid_dynalloc_tests;
#[cfg(test)]
#[path = "vbr_tests.rs"]
mod vbr_tests;

/// Invalid PCM/geometry/budget or a failed constituent encoding stage.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum EncoderError {
    /// Input channel count must be one or two.
    InvalidChannels,
    /// CELT supports NB/WB/SWB/FB up to the input-rate default, excluding MB.
    InvalidBandwidth,
    /// Interleaved PCM length must exactly match frame duration/rate/channels.
    InvalidInput,
    /// Requested CELT payload must contain 2..1275 bytes, excluding the TOC byte.
    InvalidBudget,
    /// Nominal VBR rate must supply at least24 bits/frame and be at most1500000bps.
    InvalidRate,
    /// The selected frame cannot fit the requested physical budget.
    BudgetExhausted,
    /// PCM pre-emphasis failed.
    Input(InputError),
    /// Transform or band analysis failed.
    Analysis(AnalysisError),
    /// Native pitch prefilter failed.
    Prefilter(PrefilterError),
    /// Band layout failed.
    Layout(EnergyError),
    /// Energy quantization failed.
    Energy(EnergyEncodingError),
    /// TF/spreading controls failed.
    Control(ControlError),
    /// Dynamic/fine allocation failed.
    Allocation(AllocationError),
    /// Spectral shape encoding failed.
    Spectrum(PartitionError),
    /// Entropy writing/finalization failed.
    Entropy(EntropyError),
}
impl fmt::Display for EncoderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidChannels => f.write_str("CELT input must have one or two channels"),
            Self::InvalidBandwidth => f.write_str("unsupported CELT bandwidth for input rate"),
            Self::InvalidInput => f.write_str("CELT PCM length does not match input geometry"),
            Self::InvalidBudget => f.write_str("CELT payload budget must be 2..1275 bytes"),
            Self::InvalidRate => f.write_str("invalid CELT nominal VBR bitrate"),
            Self::BudgetExhausted => f.write_str("CELT frame exceeds requested byte budget"),
            Self::Input(e) => write!(f, "CELT encoder input: {e}"),
            Self::Prefilter(e) => write!(f, "CELT encoder prefilter: {e}"),
            Self::Analysis(e) => write!(f, "CELT encoder analysis: {e}"),
            Self::Layout(e) => write!(f, "CELT encoder layout: {e}"),
            Self::Energy(e) => write!(f, "CELT encoder energy: {e}"),
            Self::Control(e) => write!(f, "CELT encoder control: {e}"),
            Self::Allocation(e) => write!(f, "CELT encoder allocation: {e}"),
            Self::Spectrum(e) => write!(f, "CELT encoder spectrum: {e}"),
            Self::Entropy(e) => write!(f, "CELT encoder entropy: {e}"),
        }
    }
}
impl std::error::Error for EncoderError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Input(e) => Some(e),
            Self::Analysis(e) => Some(e),
            Self::Prefilter(e) => Some(e),
            Self::Layout(e) => Some(e),
            Self::Energy(e) => Some(e),
            Self::Control(e) => Some(e),
            Self::Allocation(e) => Some(e),
            Self::Spectrum(e) => Some(e),
            Self::Entropy(e) => Some(e),
            _ => None,
        }
    }
}
impl From<InputError> for EncoderError {
    fn from(e: InputError) -> Self {
        Self::Input(e)
    }
}
impl From<PrefilterError> for EncoderError {
    fn from(e: PrefilterError) -> Self {
        Self::Prefilter(e)
    }
}
impl From<AnalysisError> for EncoderError {
    fn from(e: AnalysisError) -> Self {
        Self::Analysis(e)
    }
}
impl From<EnergyError> for EncoderError {
    fn from(e: EnergyError) -> Self {
        Self::Layout(e)
    }
}
impl From<EnergyEncodingError> for EncoderError {
    fn from(e: EnergyEncodingError) -> Self {
        Self::Energy(e)
    }
}
impl From<ControlError> for EncoderError {
    fn from(e: ControlError) -> Self {
        Self::Control(e)
    }
}
impl From<AllocationError> for EncoderError {
    fn from(e: AllocationError) -> Self {
        Self::Allocation(e)
    }
}
impl From<PartitionError> for EncoderError {
    fn from(e: PartitionError) -> Self {
        Self::Spectrum(e)
    }
}
impl From<EntropyError> for EncoderError {
    fn from(e: EntropyError) -> Self {
        Self::Entropy(e)
    }
}

/// Complete single-frame Opus packet, including its TOC byte.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EncodedPacket {
    bytes: [u8; 1276],
    length: usize,
    size: FrameSize,
    channels: usize,
    range: u32,
}
impl EncodedPacket {
    /// Exactly one complete Opus packet, without container/transport framing.
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.length]
    }
    /// Transform duration on the internal 48kHz clock.
    pub fn frame_size(&self) -> FrameSize {
        self.size
    }
    /// Encoded channel count.
    pub fn channels(&self) -> usize {
        self.channels
    }
    /// Final range diagnostic for bitstream interoperability checks.
    pub fn final_range(&self) -> u32 {
        self.range
    }
}

/// Fixed-input-rate/channel CELT-only encoder with bounded storage and no allocation.
///
/// Default policy uses long transforms (optional native transient detection),
/// disabled pitch comb (optional native prefilter)/silence suppression,
/// standard TF (optional native TF analysis)/Normal spreading, zero dynamic
/// boosts, trim five, full-band
/// intensity, no dual stereo, two-pass energy and stereo theta RDO. This supplies
/// valid audio packet coding, not adaptive Opus mode/quality selection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CeltEncoder {
    #[cfg(test)]
    pub(crate) call_trace: Vec<[u64; 5]>,
    rate: InputRate,
    channels: usize,
    encoded_channels: usize,
    bandwidth: crate::Bandwidth,
    preemphasis: [PreemphasisState; 2],
    overlap: [[i32; 120]; 2],
    energy: EnergyEncoder,
    range: u32,
    previous_coded: usize,
    first: bool,
    detect_transients: bool,
    analyze_tf: bool,
    analyze_dynamic: bool,
    analyze_spreading: bool,
    analyze_allocation: bool,
    pitch_prefilter: bool,
    prefilter: PrefilterState,
    intensity: analysis::IntensityState,
    trim: analysis::TrimState,
    spreading: analysis::SpreadingState,
    transient_streak: u8,
    vbr: VbrState,
    overlap_nonzero: bool,
    native_controls: Option<NativeControls>,
}
// The common Opus owner supplies controls independently of CELT's start band.
// Legacy fixed APIs retain their existing analysis switches and first-frame rule.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct NativeControls {
    pub complexity: u8,
    pub loss_percent: u8,
    pub prediction: u8,
}
// Only the enclosing Hybrid packet transaction calls this frame engine.
#[derive(Clone, Copy)]
pub(crate) struct HybridControls {
    pub bandwidth: crate::Bandwidth,
    pub complexity: u8,
    pub voiced: bool,
    pub loss_percent: u8,
    pub vbr: Option<HybridVbr>,
}
impl CeltEncoder {
    pub(crate) fn native_prediction(&self) -> u8 {
        self.native_controls.map_or(2, |c| c.prediction)
    }
    #[cfg(test)]
    pub(crate) fn input_fingerprint(&self, channel: usize) -> Option<u64> {
        let overlap = self.prefilter.overlap(channel)?;
        let mut hash = 14695981039346656037u64;
        for value in std::iter::once(&i32::from(self.vbr.count))
            .chain(std::iter::once(&self.preemphasis[channel].memory()))
            .chain(overlap)
        {
            for byte in value.to_le_bytes() {
                hash = (hash ^ u64::from(byte)).wrapping_mul(1099511628211);
            }
        }
        Some(hash)
    }
    pub(crate) fn set_native_controls(
        &mut self,
        controls: NativeControls,
    ) -> Result<(), EncoderError> {
        if controls.complexity > 10 || controls.loss_percent > 100 || controls.prediction > 2 {
            return Err(EncoderError::InvalidInput);
        }
        self.native_controls = Some(controls);
        self.detect_transients = controls.complexity >= 1;
        self.analyze_tf = controls.complexity >= 2;
        self.analyze_dynamic = true;
        self.analyze_spreading = controls.complexity >= 3;
        self.analyze_allocation = true;
        self.pitch_prefilter = controls.prediction > 1;
        Ok(())
    }
    /// Start a fresh one/two-channel stream at a standard PCM input rate.
    pub fn new(rate: InputRate, channels: usize) -> Result<Self, EncoderError> {
        if !(1..=2).contains(&channels) {
            return Err(EncoderError::InvalidChannels);
        }
        Ok(Self {
            #[cfg(test)]
            call_trace: Vec::new(),
            rate,
            channels,
            encoded_channels: channels,
            bandwidth: default_bandwidth(rate),
            preemphasis: [PreemphasisState::default(), PreemphasisState::default()],
            overlap: [[0; 120]; 2],
            energy: EnergyEncoder::default(),
            range: 0,
            previous_coded: 0,
            first: true,
            detect_transients: false,
            analyze_tf: false,
            analyze_dynamic: false,
            analyze_spreading: false,
            analyze_allocation: false,
            pitch_prefilter: false,
            prefilter: PrefilterState::new(channels)?,
            intensity: analysis::IntensityState::default(),
            trim: analysis::TrimState::default(),
            spreading: analysis::SpreadingState::default(),
            transient_streak: 0,
            vbr: VbrState::default(),
            overlap_nonzero: false,
            native_controls: None,
        })
    }
    /// Fixed PCM input rate.
    pub fn input_rate(&self) -> InputRate {
        self.rate
    }
    /// Fixed interleaved PCM input channel count.
    pub fn channels(&self) -> usize {
        self.channels
    }
    /// Selected encoded channel count, initially equal to the input layout.
    pub fn encoded_channels(&self) -> usize {
        self.encoded_channels
    }
    /// Selected CELT bandwidth, initially the input rate's default.
    /// The defaults are NB at8kHz, WB at12/16kHz, SWB at24kHz and FB at48kHz.
    pub fn bandwidth(&self) -> crate::Bandwidth {
        self.bandwidth
    }
    /// Select the next packet's spectral bandwidth and TOC without a reset.
    ///
    /// Supports NB/WB/SWB/FB no wider than the input-rate default. CELT cannot
    /// signal MB;12kHz input uses WB signalling with silent high frequencies.
    /// Input filters, active-band energies and rate feedback remain continuous;
    /// after a successful packet, inactive reconstructed energies are cleared
    /// in both input channels. Reset retains selection. Invalid requests
    /// preserve the entire encoder state.
    pub fn set_bandwidth(&mut self, bandwidth: crate::Bandwidth) -> Result<(), EncoderError> {
        if bandwidth_parameters(bandwidth)?.0
            > bandwidth_parameters(default_bandwidth(self.rate))?.0
        {
            return Err(EncoderError::InvalidBandwidth);
        }
        self.bandwidth = bandwidth;
        Ok(())
    }
    /// Select the next packet's wire layout, retaining both input histories.
    ///
    /// Stereo input can encode mono or stereo; mono input can encode mono only.
    /// Mono coding downmixes the MDCT spectrum rather than the raw PCM. Both
    /// input preemphasis, overlap and pitch histories advance throughout mono
    /// packets; reconstructed mono energies prime both coded channels. Reset
    /// retains this selection. Invalid selections preserve all state.
    pub fn set_encoded_channels(&mut self, channels: usize) -> Result<(), EncoderError> {
        if !(1..=self.channels).contains(&channels) {
            return Err(EncoderError::InvalidChannels);
        }
        self.encoded_channels = channels;
        Ok(())
    }
    /// Whether native tone/transient analysis selects short MDCT windows.
    pub fn transient_detection(&self) -> bool {
        self.detect_transients
    }
    /// Enable native transient detection (initially disabled for baseline policy).
    /// Histories remain continuous; reset retains this setting.
    pub fn set_transient_detection(&mut self, enabled: bool) {
        self.detect_transients = enabled;
    }
    /// Whether native per-band time/frequency analysis is enabled.
    pub fn tf_analysis(&self) -> bool {
        self.analyze_tf
    }
    /// Enable native TF analysis (initially disabled).
    /// Native budget/tone gating applies; histories and transient setting persist.
    /// Reset retains this setting. Importance is neutral unless dynamic-allocation
    /// analysis is enabled.
    pub fn set_tf_analysis(&mut self, enabled: bool) {
        self.analyze_tf = enabled;
    }
    /// Whether native perceptual dynamic-allocation analysis is enabled.
    pub fn dynamic_allocation(&self) -> bool {
        self.analyze_dynamic
    }
    /// Enable native spectral masking/boost analysis (initially disabled).
    /// Native long-window comparison is used for transients. TF, if enabled,
    /// consumes perceptual importance instead of neutral weights. Reset retains
    /// the setting. Spreading and trim analysis have independent settings.
    pub fn set_dynamic_allocation(&mut self, enabled: bool) {
        self.analyze_dynamic = enabled;
    }
    /// Whether native pulse-spreading analysis is enabled.
    pub fn spreading_analysis(&self) -> bool {
        self.analyze_spreading
    }
    /// Enable native sparsity/masking spreading choices (initially disabled).
    /// Histories remain continuous and reset retains this setting. Short windows,
    /// low payloads and scarce entropy use Normal without updating averages.
    pub fn set_spreading_analysis(&mut self, enabled: bool) {
        self.analyze_spreading = enabled;
    }
    /// Whether native trim, intensity and L1 stereo allocation analysis is enabled.
    pub fn allocation_analysis(&self) -> bool {
        self.analyze_allocation
    }
    /// Enable native allocation choices (initially disabled). Desired intensity
    /// history is retained before coded-band clipping; saving advances only when
    /// the trim symbol fits. Other settings are independent. Reset retains this
    /// setting and clears both histories; disabling preserves their last values.
    pub fn set_allocation_analysis(&mut self, enabled: bool) {
        self.analyze_allocation = enabled;
    }
    /// Whether native pitch prefilter selection and comb signaling is enabled.
    pub fn pitch_prefilter(&self) -> bool {
        self.pitch_prefilter
    }
    /// Enable native pitch filtering (initially disabled). Native byte/resource
    /// gates apply; raw/filtered histories remain continuous and old gain ramps
    /// out on disabling. Other settings are independent; reset retains this one.
    pub fn set_pitch_prefilter(&mut self, enabled: bool) {
        self.pitch_prefilter = enabled;
    }
    /// Last completed packet final range, initially zero.
    pub fn final_range(&self) -> u32 {
        self.range
    }
    /// Clear histories and diagnostics, retaining geometry and analysis settings.
    pub fn reset(&mut self) {
        self.preemphasis = [PreemphasisState::default(), PreemphasisState::default()];
        self.overlap = [[0; 120]; 2];
        self.energy.reset();
        self.range = 0;
        self.previous_coded = 0;
        self.first = true;
        self.transient_streak = 0;
        self.spreading.reset();
        self.intensity.reset();
        self.trim.reset();
        self.prefilter.reset();
        self.vbr = VbrState::default();
        self.overlap_nonzero = false;
    }
    /// Encode exactly one frame of interleaved signed16 PCM into a full packet.
    ///
    /// Input length is `size.samples()/input_rate().factor()*channels()`.
    /// The payload budget excludes the one-byte TOC; capacity is 2..1275.
    /// Small budgets may fail explicitly. All errors preserve the full encoder
    /// state and immutable input. Successful output has exactly budget+1 bytes.
    pub fn encode(
        &mut self,
        size: FrameSize,
        pcm: &[i16],
        payload_bytes: usize,
    ) -> Result<EncodedPacket, EncoderError> {
        self.encode_with(size, pcm, payload_bytes, RangeEncoder::finish)
    }
    /// Encode VBR/CVBR with a separate nominal TOC-inclusive rate and hard
    /// payload cap. Native per-channel/physical limits can reduce the target.
    /// Native zero-input silence signaling includes the preceding input overlap.
    /// Existing analysis/prefilter switches still apply. Errors preserve all
    /// stream histories, including variable-rate feedback and pending overlap.
    pub fn encode_vbr(
        &mut self,
        size: FrameSize,
        pcm: &[i16],
        options: VbrOptions,
    ) -> Result<EncodedPacket, EncoderError> {
        self.encode_with_rate(
            size,
            pcm,
            options.payload_bytes,
            Some(options),
            RangeEncoder::finish,
        )
    }
    pub(crate) fn encode_high_band(
        &mut self,
        size: FrameSize,
        pcm: &[i16],
        writer: &mut RangeEncoder,
        controls: HybridControls,
    ) -> Result<(), EncoderError> {
        self.encode_frame(size, pcm, writer, Some(controls), None)
    }
    // The Opus owner normalizes the complete rate. Embedded CELT has no TOC
    // signalling of its own, including during real dummy encodes.
    pub(crate) fn encode_stream(
        &mut self,
        size: FrameSize,
        pcm: &[i16],
        payload_bytes: usize,
        rate: Option<(u32, bool)>,
    ) -> Result<EncodedPacket, EncoderError> {
        if !(2..=1275).contains(&payload_bytes) {
            return Err(EncoderError::InvalidBudget);
        }
        if pcm.len() != size.samples() / self.rate.factor() * self.channels {
            return Err(EncoderError::InvalidInput);
        }
        if rate.is_some_and(|(bitrate, _)| !(500..=1500000).contains(&bitrate)) {
            return Err(EncoderError::InvalidRate);
        }
        let rate = rate.map(|(bitrate, constrained)| VbrRate {
            signalling: false,
            bitrate: bitrate as i32,
            constrained,
            capacity: payload_bytes,
        });
        self.encode_inner(size, pcm, payload_bytes, rate, RangeEncoder::finish)
    }
    pub(crate) fn prefill_high_band(
        &mut self,
        pcm: &[i16],
        controls: HybridControls,
    ) -> Result<(), EncoderError> {
        let mut writer = RangeEncoder::new(2)?;
        self.encode_frame(FrameSize::Ms2_5, pcm, &mut writer, Some(controls), None)?;
        writer.finish()?;
        Ok(())
    }
    fn tf_permitted(&self, diagnostics: analysis::TransientAnalysis, payload_bytes: usize) -> bool {
        self.analyze_tf && payload_bytes >= 15 * self.encoded_channels && !diagnostics.tf_tonal()
    }
    // Operates on the packet's staged stream; the outer transaction owns errors.
    fn encode_spreading(
        &mut self,
        writer: &mut RangeEncoder,
        bands: &analysis::BandAnalysis,
        transient: bool,
        perceptual: Option<&analysis::DynallocAnalysis>,
        active_prefilter: bool,
    ) -> Result<Spread, EncoderError> {
        let payload_bytes = writer.capacity();
        let budget = payload_bytes as u32 * 8;
        let size = bands.size();
        let end = bands.end();
        let requested_spread = if self.analyze_spreading
            && !transient
            && payload_bytes >= 10 * self.encoded_channels
            && writer.tell() + 4 <= budget
        {
            let weights = *perceptual
                .ok_or(EncoderError::Analysis(
                    AnalysisError::InvalidSpreadParameters,
                ))?
                .spread_weights();
            self.spreading.process(
                size,
                bands.normalized_values(),
                analysis::SpreadingOptions {
                    channels: self.encoded_channels,
                    end,
                    weights,
                    update_hf: active_prefilter,
                },
            )?
        } else {
            Spread::Normal
        };
        let spread = control::encode_spread(writer, requested_spread)?;
        self.spreading.effective_decision(spread);
        Ok(spread)
    }
    // Static finalization injection verifies rollback after every preceding stage.
    fn encode_with(
        &mut self,
        size: FrameSize,
        pcm: &[i16],
        payload_bytes: usize,
        finish: impl for<'a> FnOnce(&'a mut RangeEncoder) -> Result<&'a [u8], EntropyError>,
    ) -> Result<EncodedPacket, EncoderError> {
        self.encode_with_rate(size, pcm, payload_bytes, None, finish)
    }
    fn encode_with_rate(
        &mut self,
        size: FrameSize,
        pcm: &[i16],
        payload_bytes: usize,
        options: Option<VbrOptions>,
        finish: impl for<'a> FnOnce(&'a mut RangeEncoder) -> Result<&'a [u8], EntropyError>,
    ) -> Result<EncodedPacket, EncoderError> {
        if !(2..=1275).contains(&payload_bytes) {
            return Err(EncoderError::InvalidBudget);
        }
        if pcm.len() != size.samples() / self.rate.factor() * self.channels {
            return Err(EncoderError::InvalidInput);
        }
        let rate = if let Some(options) = options {
            let fps = (400 >> size.lm()) as u32;
            if !(24 * fps..=1500000).contains(&options.bitrate) {
                return Err(EncoderError::InvalidRate);
            }
            Some(VbrRate {
                signalling: true,
                bitrate: options
                    .bitrate
                    .min(750000 * self.channels as u32)
                    .min((payload_bytes as u32 + 1) * 8 * fps) as i32,
                constrained: options.constrained,
                capacity: payload_bytes,
            })
        } else {
            None
        };
        let mut next = self.clone();
        let packet = next.encode_inner(size, pcm, payload_bytes, rate, finish)?;
        *self = next;
        Ok(packet)
    }
    fn encode_inner(
        &mut self,
        size: FrameSize,
        pcm: &[i16],
        payload_bytes: usize,
        rate: Option<VbrRate>,
        finish: impl for<'a> FnOnce(&'a mut RangeEncoder) -> Result<&'a [u8], EntropyError>,
    ) -> Result<EncodedPacket, EncoderError> {
        let mut writer = RangeEncoder::new(payload_bytes)?;
        if let Some(rate) = rate {
            writer.shrink(self.vbr.capacity(size, rate, payload_bytes))?;
        }
        self.encode_frame(size, pcm, &mut writer, None, rate)?;
        let range = writer.range();
        let payload = finish(&mut writer)?;
        let config = bandwidth_parameters(self.bandwidth)?.1;
        let mut packet = EncodedPacket {
            bytes: [0; 1276],
            length: payload.len() + 1,
            size,
            channels: self.encoded_channels,
            range,
        };
        packet.bytes[0] =
            ((config + size.lm()) as u8) << 3 | if self.encoded_channels == 2 { 4 } else { 0 };
        packet.bytes[1..packet.length].copy_from_slice(payload);
        Ok(packet)
    }
    // Operates on staged state and a live entropy writer. The enclosing packet
    // owns entropy initialization/termination and commits only after finishing.
    fn encode_frame(
        &mut self,
        size: FrameSize,
        pcm: &[i16],
        writer: &mut RangeEncoder,
        hybrid: Option<HybridControls>,
        vbr_rate: Option<VbrRate>,
    ) -> Result<(), EncoderError> {
        #[cfg(test)]
        if let Some(controls) = self.native_controls {
            let hash = pcm
                .iter()
                .flat_map(|x| x.to_le_bytes())
                .fold(14695981039346656037u64, |h, b| {
                    (h ^ u64::from(b)).wrapping_mul(1099511628211)
                });
            self.call_trace.push([
                size.samples() as u64,
                if hybrid.is_some() { 17 } else { 0 },
                u64::from(controls.prediction),
                writer.capacity() as u64,
                hash,
            ]);
        }
        let silence = hybrid.is_none()
            && (vbr_rate.is_some() || self.native_controls.is_some())
            && !self.overlap_nonzero
            && pcm.iter().all(|&v| v == 0);
        let tail = 120 / self.rate.factor() * self.channels;
        self.overlap_nonzero = pcm[pcm.len() - tail..].iter().any(|&v| v != 0);
        if silence && vbr_rate.is_some() {
            writer.shrink(2)?;
        }
        let payload_bytes = writer.capacity();
        let initial_tell_q3 = writer.tell_frac();
        let start = if hybrid.is_some() { 17 } else { 0 };
        let available = if hybrid.is_some() {
            payload_bytes.saturating_sub(((writer.tell() + 4) >> 3) as usize)
        } else {
            payload_bytes
        };
        let effective_bytes = vbr_rate.map_or_else(
            || {
                hybrid
                    .and_then(|h| h.vbr)
                    .map_or(available, |v| v.analysis_bytes(size))
            },
            |rate| (rate.q3(size) as usize >> 6).clamp(2, 1275),
        );
        let n = size.samples();
        let stride = n + 120;
        let mut context = [0; 2160];
        for c in 0..self.channels {
            let channel = &mut context[c * stride..(c + 1) * stride];
            channel[..120].copy_from_slice(&self.overlap[c]);
            self.preemphasis[c].process(
                size,
                self.rate,
                pcm,
                self.channels,
                c,
                &mut channel[120..],
            )?;
            self.overlap[c].copy_from_slice(&channel[n..n + 120]);
        }
        let budget = payload_bytes as u32 * 8;
        if hybrid.is_none() {
            writer.encode_bit_logp(silence, 15)?;
            if silence && self.native_controls.is_some() {
                writer.consume_remaining_budget()?;
            }
        }
        let transient_candidate = size != FrameSize::Ms2_5 && writer.tell() + 3 <= budget;
        let diagnostics = if (self.detect_transients && transient_candidate)
            || self.analyze_tf
            || self.analyze_dynamic
            || self.analyze_spreading
            || self.analyze_allocation
            || self.pitch_prefilter
            || vbr_rate.is_some()
        {
            let analyzed = analysis::analyze_transients(
                size,
                &context[..self.channels * stride],
                self.channels,
                hybrid.is_some_and(|h| effective_bytes < 15 && !h.voiced),
            )?;
            Some(if self.native_controls.is_some_and(|c| c.complexity == 0) {
                analyzed.without_temporal_analysis()
            } else {
                analyzed
            })
        } else {
            None
        };
        let mut current = [0; 1920];
        for channel in 0..self.channels {
            current[channel * n..(channel + 1) * n]
                .copy_from_slice(&context[channel * stride + 120..(channel + 1) * stride]);
        }
        let filtered = self.prefilter.process(
            size,
            &current[..self.channels * n],
            PrefilterOptions {
                enabled: hybrid.is_none()
                    && self.pitch_prefilter
                    && !silence
                    && payload_bytes > 12 * self.encoded_channels
                    && 16 < budget,
                available_bytes: payload_bytes as u16,
                tf_estimate: diagnostics.map_or(0, |d| d.tf_estimate()),
                tone_frequency: diagnostics.map_or(-1, |d| d.tone_frequency()),
                toneishness: diagnostics.map_or(0, |d| d.adjusted_toneishness()),
                tapset: [Tapset::Set0, Tapset::Set1, Tapset::Set2]
                    [usize::from(self.spreading.tapset())],
                complexity: self.native_controls.map_or(10, |c| c.complexity),
                loss_rate: self.native_controls.map_or(0, |c| c.loss_percent),
            },
        )?;
        context[..self.channels * stride].copy_from_slice(filtered.values());
        if hybrid.is_none() && !(silence && self.native_controls.is_some()) {
            encode_comb(writer, filtered.parameters())?;
        }
        let transient_allowed = size != FrameSize::Ms2_5 && writer.tell() + 3 <= budget;
        let transient = self.detect_transients
            && transient_allowed
            && diagnostics.is_some_and(|d| d.transient());
        if transient_allowed {
            writer.encode_bit_logp(transient, 3)?;
        }
        let end = bandwidth_parameters(hybrid.map_or(self.bandwidth, |h| h.bandwidth))?.0;
        let layout = BandLayout::new(self.encoded_channels, start, end)?;
        let spectrum = analysis::analyze_frame(
            size,
            &context[..self.channels * stride],
            self.channels,
            self.encoded_channels,
            self.rate,
            transient,
        )?;
        let bands =
            analysis::analyze_bands(size, spectrum.values(), self.encoded_channels, end, end)?;
        let mut target = [[0; 21]; 2];
        let mut amplitudes = [[0; 21]; 2];
        for c in 0..self.encoded_channels {
            target[c] = *bands.log_energies(c).ok_or(EncoderError::InvalidChannels)?;
            amplitudes[c] = *bands.amplitudes(c).ok_or(EncoderError::InvalidChannels)?;
        }
        let temporal =
            self.vbr
                .temporal(&target, self.encoded_channels, start, end, size, transient);
        let perceptual = if (self.analyze_dynamic || self.analyze_spreading || vbr_rate.is_some())
            && (hybrid.is_none() || effective_bytes >= 2)
        {
            let d = diagnostics.ok_or(EncoderError::Analysis(
                AnalysisError::InvalidDynallocParameters,
            ))?;
            let mut comparison = target;
            if transient
                && self.analyze_dynamic
                && self.native_controls.is_none_or(|c| c.complexity >= 8)
            {
                let long = analysis::analyze_frame(
                    size,
                    &context[..self.channels * stride],
                    self.channels,
                    self.encoded_channels,
                    self.rate,
                    false,
                )?;
                let long_bands =
                    analysis::analyze_bands(size, long.values(), self.encoded_channels, end, end)?;
                for (c, logs) in comparison[..self.encoded_channels].iter_mut().enumerate() {
                    *logs = *long_bands
                        .log_energies(c)
                        .ok_or(EncoderError::InvalidChannels)?;
                    for value in &mut logs[..end] {
                        *value += (size.lm() as i32) << 23;
                    }
                }
            }
            Some(analyze_dynamic(
                size,
                layout,
                analysis::DynallocInput {
                    current: &target,
                    comparison: &comparison,
                    previous: self.energy.energies().values(),
                },
                analysis::DynallocOptions {
                    payload_bytes: effective_bytes as u16,
                    transient,
                    tone_frequency: d.tone_frequency(),
                    toneishness: if hybrid.is_some_and(|h| h.complexity == 0) {
                        d.toneishness()
                    } else {
                        d.adjusted_toneishness()
                    },
                    ..analysis::DynallocOptions::default()
                },
                hybrid,
                vbr_rate,
            )?)
        } else {
            None
        };
        for (c, target) in target[..self.encoded_channels].iter_mut().enumerate() {
            let previous = self
                .energy
                .energies()
                .channel(c)
                .ok_or(EncoderError::InvalidChannels)?;
            let residual = self
                .energy
                .residuals(c)
                .ok_or(EncoderError::InvalidChannels)?;
            for i in start..end {
                if (i64::from(target[i]) - i64::from(previous[i])).abs() < 2 << 24 {
                    target[i] -= residual[i].clamp(-(1 << 23), 1 << 23) >> 2;
                }
            }
        }
        self.energy.coarse(
            writer,
            &target,
            CoarseEncoding {
                layout,
                size,
                effective_end: end,
                available_bytes: available,
                force_intra: self
                    .native_controls
                    .map_or(hybrid.is_none() && self.first, |c| c.prediction == 0),
                two_pass: self
                    .native_controls
                    .map_or(hybrid.is_none_or(|h| h.complexity >= 4), |c| {
                        c.complexity >= 4
                    }),
                loss_rate: self
                    .native_controls
                    .map_or(hybrid.map_or(0, |h| h.loss_percent), |c| c.loss_percent),
                lfe: false,
            },
        )?;
        let mut decisions = [false; 21];
        decisions[start..end].fill(transient);
        let mut select = false;
        if let Some(h) = hybrid {
            if diagnostics.is_some_and(|d| d.weak_transient()) {
                decisions[start..end].fill(true);
            } else if effective_bytes < 15 && !h.voiced {
                decisions[start..end].fill(false);
                select = transient;
            }
        }
        if let Some(d) = diagnostics.filter(|&d| {
            hybrid.is_none()
                && self.tf_permitted(
                    d,
                    if vbr_rate.is_some() {
                        effective_bytes
                    } else {
                        payload_bytes
                    },
                )
        }) {
            let analyzed = analysis::analyze_tf(
                size,
                bands
                    .normalized(if self.encoded_channels == 1 {
                        0
                    } else {
                        d.channel()
                    })
                    .ok_or(EncoderError::InvalidChannels)?,
                end,
                analysis::TfAnalysisOptions {
                    transient,
                    lambda: (20480
                        / if vbr_rate.is_some() {
                            effective_bytes
                        } else {
                            payload_bytes
                        }
                        + 2)
                    .max(80) as u16,
                    estimate: d.tf_estimate(),
                    importance: perceptual
                        .as_ref()
                        .filter(|_| self.analyze_dynamic)
                        .map_or([13; 21], |d| *d.importance()),
                },
            )?;
            decisions = *analyzed.decisions();
            select = analyzed.select();
        }
        let tf = control::encode_tf(writer, size, start..end, transient, &decisions, select)?;
        let spread = if let Some(h) = hybrid {
            control::encode_spread(
                writer,
                if h.complexity == 0 {
                    Spread::None
                } else if transient {
                    Spread::Normal
                } else {
                    Spread::Aggressive
                },
            )?
        } else {
            self.encode_spreading(
                writer,
                &bands,
                transient,
                perceptual.as_ref(),
                filtered.active(),
            )?
        };
        let mut rate = analysis::equivalent_rate(size, self.encoded_channels, payload_bytes)?;
        if let Some(vbr) = hybrid.and_then(|h| h.vbr) {
            rate = rate.min(
                vbr.rate as i32
                    - (40 * self.encoded_channels as i32 + 20) * ((400 >> size.lm()) - 50),
            );
        }
        if let Some(vbr) = vbr_rate {
            rate = vbr.equivalent(size, self.encoded_channels);
        }
        let intensity = if self.analyze_allocation && self.encoded_channels == 2 {
            self.intensity.select((rate / 1000) as i16, layout)
        } else {
            end
        };
        let dual_stereo = self.analyze_allocation
            && self.encoded_channels == 2
            && size != FrameSize::Ms2_5
            && analysis::analyze_stereo(size, bands.normalized_values())?;
        let vbr = hybrid.and_then(|h| h.vbr);
        let vbr_budget = VbrBudget {
            size,
            channels: self.encoded_channels,
            capacity: payload_bytes,
            initial_tell_q3,
            tell_q3: 0,
            boost_q3: 0,
            temporal_q14: if hybrid.is_some_and(|h| h.complexity == 0) {
                0
            } else {
                diagnostics.map_or(0, |d| d.tf_estimate())
            },
        };
        let saving = Cell::new(self.trim.stereo_saving());
        let rate_target = VbrTarget {
            size,
            channels: self.encoded_channels,
            coded_bands: self.previous_coded,
            intensity,
            stereo_saving: 0,
            intended_boost: perceptual
                .as_ref()
                .filter(|_| self.analyze_dynamic)
                .map_or(0, |p| p.total_boost()),
            tf_estimate: diagnostics.map_or(0, |d| d.tf_estimate()),
            max_depth: perceptual.as_ref().map_or(0, |p| p.max_depth()),
            temporal,
            equivalent_rate: rate,
        };
        let native_vbr = self.native_controls.is_some();
        let rate_state = &mut self.vbr;
        let allocation = allocation::encode_analyzed_resized(
            writer,
            size,
            layout,
            transient,
            AllocationEncoding {
                boosts: perceptual
                    .as_ref()
                    .filter(|_| self.analyze_dynamic)
                    .map_or([0; 21], |d| *d.boosts()),
                trim: 5,
                intensity,
                dual_stereo,
                previous_coded_bands: self.previous_coded,
                signal_bandwidth: end - 1,
            },
            || {
                if !self.analyze_allocation || hybrid.is_some() {
                    return Ok(5);
                }
                self.trim
                    .process(
                        size,
                        bands.normalized_values(),
                        &target,
                        analysis::TrimOptions {
                            channels: self.encoded_channels,
                            end,
                            intensity,
                            equivalent_rate: rate,
                            tf_estimate: diagnostics.map_or(0, |d| d.tf_estimate()),
                            surround_trim: 0,
                        },
                    )
                    .map(|d| {
                        saving.set(self.trim.stereo_saving());
                        d.index()
                    })
                    .map_err(AllocationError::Analysis)
            },
            |writer, boost| {
                if let Some(vbr) = vbr {
                    vbr.resize(writer, vbr_budget, boost)?;
                    if native_vbr {
                        rate_state.count = (rate_state.count + 1).min(970);
                    }
                }
                if let Some(rate) = vbr_rate {
                    let capacity = rate_state
                        .finish(VbrFrame {
                            rate,
                            target: VbrTarget {
                                stereo_saving: saving.get(),
                                ..rate_target
                            },
                            capacity: writer.capacity(),
                            tell_q3: writer.tell_frac(),
                            actual_boost: boost,
                            silence,
                        })
                        .map_err(AllocationError::Analysis)?;
                    writer.shrink(capacity)?;
                }
                Ok(())
            },
        )?;
        let budget = writer.capacity() as u32 * 8;
        self.energy.fine(writer, layout, allocation.fine_bits())?;
        partition::encode_spectrum(
            writer,
            &allocation,
            &tf,
            spread,
            SpectrumInput {
                left: bands.normalized(0).ok_or(EncoderError::InvalidChannels)?,
                right: bands.normalized(1),
                amplitudes,
            },
            SpectrumEncoding {
                seed: self.range,
                disable_inversion: false,
                theta_rdo: self.native_controls.is_none_or(|c| c.complexity >= 8),
            },
        )?;
        if allocation.anti_collapse_reserved() {
            writer.encode_bits(u32::from(self.transient_streak < 2), 1)?;
        }
        let left = budget
            .checked_sub(writer.tell())
            .ok_or(EncoderError::BudgetExhausted)?;
        self.energy.finalise(
            writer,
            layout,
            allocation.fine_bits(),
            allocation.fine_priority(),
            left,
        )?;
        if silence {
            self.energy.silence(layout);
        }
        if self.channels == 2 && self.encoded_channels == 1 {
            self.energy.mirror_mono();
        }
        self.energy.clear_inactive_bands(layout, self.channels);
        let coded = allocation.coded_bands();
        self.previous_coded = if self.previous_coded == 0 {
            coded
        } else {
            (self.previous_coded + 1).min((self.previous_coded - 1).max(coded))
        };
        self.range = writer.range();
        // Native short/disabled-window streak only matters below threshold2.
        self.transient_streak = if transient || !transient_allowed {
            (self.transient_streak + 1).min(2)
        } else {
            0
        };
        self.first = false;
        Ok(())
    }
}

fn default_bandwidth(rate: InputRate) -> crate::Bandwidth {
    use crate::Bandwidth::*;
    match rate {
        InputRate::Khz8 => Narrowband,
        InputRate::Khz12 | InputRate::Khz16 => Wideband,
        InputRate::Khz24 => SuperWideband,
        InputRate::Khz48 => Fullband,
    }
}
fn bandwidth_parameters(bandwidth: crate::Bandwidth) -> Result<(usize, usize), EncoderError> {
    use crate::Bandwidth::*;
    match bandwidth {
        Narrowband => Ok((13, 16)),
        Wideband => Ok((17, 20)),
        SuperWideband => Ok((19, 24)),
        Fullband => Ok((21, 28)),
        Mediumband => Err(EncoderError::InvalidBandwidth),
    }
}

// Both packet modes share analysis, but Hybrid never enables constrained VBR.
fn analyze_dynamic(
    size: FrameSize,
    layout: BandLayout,
    input: analysis::DynallocInput<'_>,
    mut options: analysis::DynallocOptions,
    hybrid: Option<HybridControls>,
    vbr: Option<VbrRate>,
) -> Result<analysis::DynallocAnalysis, AnalysisError> {
    options.vbr = vbr.is_some() || hybrid.is_some_and(|h| h.vbr.is_some());
    options.constrained_vbr = vbr.is_some_and(|v| v.constrained);
    analysis::analyze_dynalloc(size, layout, input, options)
}

// Fresh CELT prefix after silence=false; cached initial tell is one.
// Whole-frame staging owns every entropy error. Native positive gain is possible
// only beyond the outer >12*C byte gate, so the complete tap field fits.
fn encode_comb(
    writer: &mut RangeEncoder,
    parameters: PostfilterParams,
) -> Result<(), EncoderError> {
    if parameters.gain() != 0 && (parameters.period() > 1022 || parameters.gain() % 3072 != 0) {
        return Err(ControlError::InvalidContext.into());
    }
    if 16 < writer.capacity() as u32 * 8 {
        writer.encode_bit_logp(parameters.gain() != 0, 1)?;
        if parameters.gain() != 0 {
            let period = u32::from(parameters.period()) + 1;
            let octave = (32 - period.leading_zeros()) - 5;
            writer.encode_uint(octave, 6)?;
            writer.encode_bits(period - (16 << octave), 4 + octave)?;
            writer.encode_bits(u32::from(parameters.gain()) / 3072 - 1, 3)?;
            writer.encode_icdf(parameters.tapset() as usize, &TAPSET, 2)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn numbers(line: &str) -> Result<Vec<i64>, Box<dyn std::error::Error>> {
        Ok(line
            .split_whitespace()
            .skip(1)
            .map(|s| s.parse())
            .collect::<Result<_, _>>()?)
    }
    #[test]
    fn original_comb_field_bytes_and_accounting_match() -> Result<(), Box<dyn std::error::Error>> {
        use crate::range::RangeDecoder;
        let taps = [Tapset::Set0, Tapset::Set1, Tapset::Set2];
        let mut count = 0;
        for line in include_str!("../../tests/celt-comb-encoder-fixtures.txt")
            .lines()
            .filter(|l| !l.starts_with('#'))
        {
            let f: Vec<_> = line.split_whitespace().collect();
            assert_eq!(f[0], "comb");
            assert_eq!(f.len(), 9);
            let v: Vec<u32> = f[1..8]
                .iter()
                .map(|s| s.parse())
                .collect::<Result<_, _>>()?;
            let params = PostfilterParams::new(
                u16::try_from(v[1])?,
                u16::try_from(v[2])?,
                taps[v[3] as usize],
            )?;
            let mut writer = RangeEncoder::new(v[0] as usize)?;
            writer.encode_bit_logp(false, 15)?;
            encode_comb(&mut writer, params)?;
            assert_eq!(
                [writer.range(), writer.tell(), writer.tell_frac()],
                v[4..7],
                "{line}"
            );
            let expected: Vec<u8> = (0..f[8].len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&f[8][i..i + 2], 16))
                .collect::<Result<_, _>>()?;
            assert_eq!(writer.finish()?, expected, "{line}");
            let mut read = RangeDecoder::new(&expected);
            assert!(!read.decode_bit_logp(15)?);
            let mut decoded = PostfilterParams::default();
            if 16 < v[0] * 8 && read.decode_bit_logp(1)? {
                let octave = read.decode_uint(6)?;
                let period = (16 << octave) + read.decode_bits(4 + octave)? - 1;
                let gain = (read.decode_bits(3)? + 1) * 3072;
                assert!(read.tell() + 2 <= v[0] * 8);
                let tap = read.decode_icdf(&TAPSET, 2)?;
                decoded = PostfilterParams::new(period as u16, gain as u16, taps[tap])?;
            }
            assert_eq!(
                decoded,
                if params.gain() == 0 {
                    PostfilterParams::default()
                } else {
                    params
                },
                "{line}"
            );
            assert_eq!(
                [read.range(), read.tell(), read.tell_frac()],
                v[4..7],
                "paired {line}"
            );
            count += 1;
        }
        assert_eq!(count, 96828);
        let mut writer = RangeEncoder::new(32)?;
        writer.encode_bit_logp(false, 15)?;
        let before = writer.clone();
        for params in [
            PostfilterParams::new(1023, 3072, Tapset::Set0)?,
            PostfilterParams::new(15, 1, Tapset::Set0)?,
        ] {
            assert_eq!(
                encode_comb(&mut writer, params),
                Err(EncoderError::Control(ControlError::InvalidContext))
            );
            assert_eq!(writer, before);
        }
        Ok(())
    }
    #[test]
    fn injected_finalization_errors_preserve_complete_stream_state()
    -> Result<(), Box<dyn std::error::Error>> {
        for detect in [false, true] {
            for tf in [false, true] {
                for dynamic in [false, true] {
                    for spreading in [false, true] {
                        for allocation in [false, true] {
                            for pitch in [false, true] {
                                verify_finalization_failure(
                                    detect, tf, dynamic, spreading, allocation, pitch,
                                )?;
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }
    fn verify_finalization_failure(
        detect: bool,
        tf: bool,
        dynamic: bool,
        spreading: bool,
        allocation: bool,
        pitch: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut encoder = CeltEncoder::new(InputRate::Khz48, 2)?;
        encoder.set_transient_detection(detect);
        encoder.set_tf_analysis(tf);
        encoder.set_dynamic_allocation(dynamic);
        encoder.set_spreading_analysis(spreading);
        encoder.set_allocation_analysis(allocation);
        encoder.set_pitch_prefilter(pitch);
        if pitch {
            let warm: Vec<i16> = (0..480)
                .flat_map(|i| {
                    let value = ((i32::from(super::super::math::cos_norm(i * 131072 / 240))
                        * 12000)
                        >> 15) as i16;
                    [value, value]
                })
                .collect();
            for _ in 0..3 {
                encoder.encode(FrameSize::Ms10, &warm, 128)?;
            }
            assert!(encoder.prefilter.parameters().gain() > 0);
        } else {
            encoder.encode(FrameSize::Ms10, &[0; 960], 128)?;
        }
        let before = encoder.clone();
        let mut input = vec![0; 1920];
        input[960] = 32767;
        input[961] = -32768;
        let original = input.clone();
        for after_finish in [false, true] {
            let mut called = false;
            assert_eq!(
                encoder.encode_with(FrameSize::Ms20, &input, 128, |writer| {
                    called = true;
                    assert!(writer.tell() > 32);
                    if after_finish {
                        assert_eq!(writer.finish()?.len(), 128);
                    }
                    Err(EntropyError::CapacityExhausted)
                }),
                Err(EncoderError::Entropy(EntropyError::CapacityExhausted))
            );
            assert!(called);
            assert_eq!(encoder, before);
            assert_eq!(input, original);
        }
        let packet = encoder.encode(FrameSize::Ms20, &input, 128)?;
        let mut reference = before;
        assert_eq!(reference.encode(FrameSize::Ms20, &input, 128)?, packet);
        assert_eq!(encoder, reference);
        Ok(())
    }
    #[test]
    fn original_entropy_tails_preserve_history_when_spread_cannot_be_signalled()
    -> Result<(), Box<dyn std::error::Error>> {
        let sizes = [
            FrameSize::Ms2_5,
            FrameSize::Ms5,
            FrameSize::Ms10,
            FrameSize::Ms20,
        ];
        let spreads = [
            Spread::None,
            Spread::Light,
            Spread::Normal,
            Spread::Aggressive,
        ];
        let mut cases = 0;
        for line in include_str!("../../tests/celt-spreading-tail-fixtures.txt").lines() {
            if line.starts_with('#') {
                continue;
            }
            let f: Vec<_> = line.split_whitespace().collect();
            assert_eq!(f[0], "tail");
            assert_eq!(f.len(), 13);
            let size = sizes[f[1].parse::<usize>()?];
            let channels = f[2].parse::<usize>()?;
            let bytes = f[3].parse::<usize>()?;
            let remaining = f[4].parse::<usize>()?;
            let native: Vec<i64> = f[5..12]
                .iter()
                .map(|s| s.parse())
                .collect::<Result<_, _>>()?;
            let expected: Vec<u8> = (0..f[12].len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&f[12][i..i + 2], 16))
                .collect::<Result<_, _>>()?;
            let mut encoder = CeltEncoder::new(InputRate::Khz48, channels)?;
            encoder.set_spreading_analysis(true);
            let mut reference = encoder.clone();
            reference.spreading = analysis::SpreadingState::from_history(
                u16::try_from(native[1])?,
                u8::try_from(native[2])?,
                u8::try_from(native[3])?,
                spreads[usize::try_from(native[0])?],
            )?;
            let input = vec![0; channels * size.samples()];
            let bands = analysis::analyze_bands(size, &input, channels, 21, 21)?;
            let mut logs = [[0; 21]; 2];
            for (c, l) in logs[..channels].iter_mut().enumerate() {
                *l = *bands.log_energies(c).ok_or("logs")?;
            }
            let perceptual = analysis::analyze_dynalloc(
                size,
                BandLayout::new(channels, 0, 21)?,
                analysis::DynallocInput {
                    current: &logs,
                    comparison: &logs,
                    previous: &[[0; 21]; 2],
                },
                analysis::DynallocOptions {
                    payload_bytes: bytes as u16,
                    ..analysis::DynallocOptions::default()
                },
            )?;
            let mut writer = RangeEncoder::new(bytes)?;
            let mut left = bytes * 8 - 1 - remaining;
            while left > 0 {
                let bits = left.min(25) as u32;
                writer.encode_bits((1u32 << bits) - 1, bits)?;
                left -= bits as usize;
            }
            assert_eq!(writer.tell() as usize, bytes * 8 - remaining);
            let decision =
                encoder.encode_spreading(&mut writer, &bands, false, Some(&perceptual), false)?;
            assert_eq!(decision as i64, native[0], "decision case{cases}");
            assert_eq!(
                encoder, reference,
                "full stream history case{cases} remaining{remaining}"
            );
            assert_eq!(
                [
                    i64::from(writer.range()),
                    i64::from(writer.tell()),
                    i64::from(writer.tell_frac())
                ],
                native[4..7],
                "entropy case{cases}"
            );
            assert_eq!(writer.finish()?, expected, "payload case{cases}");
            cases += 1;
        }
        assert_eq!(cases, 72);
        Ok(())
    }
    #[test]
    fn original_complete_packets_histories_and_accounting_match()
    -> Result<(), Box<dyn std::error::Error>> {
        verify_packets(
            include_str!("../../tests/celt-encoder-fixtures.txt"),
            false,
            false,
            false,
            false,
        )
    }
    #[test]
    fn original_transient_packets_histories_and_accounting_match()
    -> Result<(), Box<dyn std::error::Error>> {
        verify_packets(
            include_str!("../../tests/celt-encoder-transient-fixtures.txt"),
            true,
            false,
            false,
            false,
        )
    }
    #[test]
    fn original_tf_packets_histories_and_accounting_match() -> Result<(), Box<dyn std::error::Error>>
    {
        verify_packets(
            include_str!("../../tests/celt-encoder-tf-fixtures.txt"),
            true,
            true,
            false,
            false,
        )
    }
    #[test]
    fn original_dynamic_packets_histories_and_accounting_match()
    -> Result<(), Box<dyn std::error::Error>> {
        verify_packets(
            include_str!("../../tests/celt-encoder-dynalloc-fixtures.txt"),
            true,
            true,
            true,
            false,
        )
    }
    #[test]
    fn original_spreading_packets_histories_and_accounting_match()
    -> Result<(), Box<dyn std::error::Error>> {
        verify_packets(
            include_str!("../../tests/celt-encoder-spreading-fixtures.txt"),
            true,
            true,
            true,
            true,
        )
    }
    fn verify_packets(
        fixtures: &str,
        detect: bool,
        tf_analysis: bool,
        dynamic_analysis: bool,
        spreading_corpus: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        verify_packet_policies(
            fixtures,
            detect,
            tf_analysis,
            dynamic_analysis,
            spreading_corpus,
            false,
        )
    }
    #[test]
    fn original_allocation_packets_histories_and_accounting_match()
    -> Result<(), Box<dyn std::error::Error>> {
        verify_packet_policies(
            include_str!("../../tests/celt-encoder-allocation-fixtures.txt"),
            true,
            true,
            true,
            true,
            true,
        )
    }
    fn verify_packet_policies(
        fixtures: &str,
        detect: bool,
        tf_analysis: bool,
        dynamic_analysis: bool,
        spreading_corpus: bool,
        allocation_corpus: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        verify_full_packet_policies(
            fixtures,
            detect,
            tf_analysis,
            dynamic_analysis,
            spreading_corpus,
            allocation_corpus,
            false,
        )
    }
    #[test]
    fn original_pitch_packets_histories_and_accounting_match()
    -> Result<(), Box<dyn std::error::Error>> {
        verify_full_packet_policies(
            include_str!("../../tests/celt-encoder-pitch-packets-fixtures.txt"),
            true,
            true,
            true,
            true,
            true,
            true,
        )
    }
    fn verify_full_packet_policies(
        fixtures: &str,
        detect: bool,
        tf_analysis: bool,
        dynamic_analysis: bool,
        spreading_corpus: bool,
        allocation_corpus: bool,
        pitch_corpus: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        use crate::{celt::energy::EnergyState, range::RangeDecoder};
        let sizes = [
            FrameSize::Ms2_5,
            FrameSize::Ms5,
            FrameSize::Ms10,
            FrameSize::Ms20,
        ];
        let mut encoder = CeltEncoder::new(InputRate::Khz48, 1)?;
        encoder.set_transient_detection(detect);
        encoder.set_tf_analysis(tf_analysis);
        encoder.set_dynamic_allocation(dynamic_analysis);
        encoder.set_spreading_analysis(spreading_corpus);
        let mut decoded_energy = EnergyState::default();
        let mut lines = fixtures.lines().filter(|l| !l.starts_with('#'));
        let mut rows = 0;
        while let Some(line) = lines.next() {
            let f: Vec<_> = line.split_whitespace().collect();
            assert_eq!(f[0], "frame");
            let (
                detect,
                tf_analysis,
                dynamic_analysis,
                spreading_enabled,
                allocation_enabled,
                pitch_enabled,
            ) = if spreading_corpus || allocation_corpus || pitch_corpus {
                let flags = f[7].parse::<u8>()?;
                (
                    flags & 1 != 0,
                    flags & 2 != 0,
                    flags & 4 != 0,
                    flags & 8 != 0,
                    flags & 16 != 0,
                    flags & 32 != 0,
                )
            } else {
                (detect, tf_analysis, dynamic_analysis, false, false, false)
            };

            let rate = match f[2] {
                "8000" => InputRate::Khz8,
                "12000" => InputRate::Khz12,
                "16000" => InputRate::Khz16,
                "24000" => InputRate::Khz24,
                "48000" => InputRate::Khz48,
                _ => return Err("rate".into()),
            };
            let channels = f[3].parse::<usize>()?;
            let size = sizes[f[4].parse::<usize>()?];
            let bytes = f[5].parse::<usize>()?;
            let expected: Vec<u8> = (0..f[6].len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&f[6][i..i + 2], 16))
                .collect::<Result<_, _>>()?;
            let input: Vec<i16> = numbers(lines.next().ok_or("input")?)?
                .into_iter()
                .map(|x| x as i16)
                .collect();
            let state = numbers(lines.next().ok_or("state")?)?;
            assert_eq!(
                state.len(),
                if pitch_corpus {
                    468
                } else if allocation_corpus {
                    455
                } else if spreading_corpus {
                    447
                } else if dynamic_analysis {
                    443
                } else if tf_analysis {
                    378
                } else if detect {
                    334
                } else {
                    332
                }
            );
            if encoder.rate != rate || encoder.channels != channels {
                encoder = CeltEncoder::new(rate, channels)?;
                encoder.set_transient_detection(detect);
                encoder.set_tf_analysis(tf_analysis);
                encoder.set_dynamic_allocation(dynamic_analysis);
                encoder.set_spreading_analysis(spreading_corpus);
                decoded_energy.reset();
            } else if f[1] == "1" {
                encoder.reset();
                decoded_energy.reset();
            }
            encoder.set_transient_detection(detect);
            encoder.set_tf_analysis(tf_analysis);
            encoder.set_dynamic_allocation(dynamic_analysis);
            encoder.set_spreading_analysis(spreading_enabled);
            encoder.set_allocation_analysis(allocation_enabled);
            encoder.set_pitch_prefilter(pitch_enabled);
            let seed = encoder.range;
            let streak = encoder.transient_streak;
            if (tf_analysis || spreading_corpus || allocation_corpus) && !pitch_corpus {
                let stride = size.samples() + 120;
                let mut context = vec![0; channels * stride];
                for c in 0..channels {
                    context[c * stride..c * stride + 120].copy_from_slice(&encoder.overlap[c]);
                    let mut preemphasis = encoder.preemphasis[c].clone();
                    preemphasis.process(
                        size,
                        rate,
                        &input,
                        channels,
                        c,
                        &mut context[c * stride + 120..(c + 1) * stride],
                    )?;
                }
                let diagnostics = analysis::analyze_transients(size, &context, channels, false)?;
                assert_eq!(
                    i64::from(encoder.tf_permitted(diagnostics, bytes)),
                    state[334],
                    "TF gate frame{rows}"
                );
                if dynamic_analysis || spreading_enabled {
                    let transient = state[332] != 0;
                    let frame = analysis::analyze_frame(
                        size, &context, channels, channels, rate, transient,
                    )?;
                    let end = match rate {
                        InputRate::Khz8 => 13,
                        InputRate::Khz12 | InputRate::Khz16 => 17,
                        InputRate::Khz24 => 19,
                        InputRate::Khz48 => 21,
                    };
                    let measured =
                        analysis::analyze_bands(size, frame.values(), channels, end, end)?;
                    let mut current = [[0; 21]; 2];
                    for (c, logs) in current[..channels].iter_mut().enumerate() {
                        *logs = *measured.log_energies(c).ok_or("current logs")?;
                    }
                    let mut comparison = current;
                    if transient && dynamic_analysis {
                        let long = analysis::analyze_frame(
                            size, &context, channels, channels, rate, false,
                        )?;
                        let long_bands =
                            analysis::analyze_bands(size, long.values(), channels, end, end)?;
                        for (c, logs) in comparison[..channels].iter_mut().enumerate() {
                            *logs = *long_bands.log_energies(c).ok_or("long logs")?;
                            for value in &mut logs[..end] {
                                *value += (size.lm() as i32) << 23;
                            }
                        }
                    }
                    let analyzed = analysis::analyze_dynalloc(
                        size,
                        BandLayout::new(channels, 0, end)?,
                        analysis::DynallocInput {
                            current: &current,
                            comparison: &comparison,
                            previous: encoder.energy.energies().values(),
                        },
                        analysis::DynallocOptions {
                            payload_bytes: bytes as u16,
                            transient,
                            tone_frequency: diagnostics.tone_frequency(),
                            toneishness: diagnostics.adjusted_toneishness(),
                            ..analysis::DynallocOptions::default()
                        },
                    )?;
                    assert_eq!(
                        [
                            i64::from(analyzed.max_depth()),
                            i64::from(analyzed.total_boost())
                        ],
                        state[378..380],
                        "dynamic depth/total frame{rows}"
                    );
                    for i in 0..21 {
                        assert_eq!(
                            i64::from(analyzed.requested_boosts()[i]),
                            state[380 + i],
                            "dynamic requests frame{rows} band{i}"
                        );
                        assert_eq!(
                            i64::from(analyzed.importance()[i]),
                            state[401 + i],
                            "dynamic importance frame{rows} band{i}"
                        );
                        assert_eq!(
                            i64::from(analyzed.spread_weights()[i]),
                            state[422 + i],
                            "dynamic spread frame{rows} band{i}"
                        );
                    }
                }
            }
            let packet = encoder.encode(size, &input, bytes)?;
            assert_eq!(packet.as_bytes(), expected, "packet frame{rows}");
            assert_eq!(
                [
                    i64::from(encoder.range),
                    encoder.previous_coded as i64,
                    i64::from(encoder.first),
                    i64::from(encoder.energy.delayed_intra()),
                    i64::from(encoder.preemphasis[0].memory()),
                    i64::from(encoder.preemphasis[1].memory())
                ],
                state[..6],
                "state frame{rows}"
            );
            for c in 0..2 {
                for i in 0..120 {
                    assert_eq!(
                        i64::from(encoder.overlap[c][i]),
                        state[6 + c * 120 + i],
                        "overlap frame{rows}"
                    );
                }
                let energy = encoder.energy.energies().channel(c).ok_or("energy")?;
                let residual = encoder.energy.residuals(c).ok_or("residual")?;
                for i in 0..21 {
                    assert_eq!(
                        i64::from(energy[i]),
                        state[246 + c * 21 + i],
                        "energy frame{rows}"
                    );
                    assert_eq!(
                        i64::from(residual[i]),
                        state[288 + c * 21 + i],
                        "residual frame{rows} channel{c} band{i}"
                    );
                }
            }
            if detect || tf_analysis || dynamic_analysis || spreading_corpus || allocation_corpus {
                assert_eq!(
                    i64::from(encoder.transient_streak),
                    state[333],
                    "streak frame{rows}"
                );
            }
            assert_eq!(encoder.transient_detection(), detect);
            assert_eq!(encoder.tf_analysis(), tf_analysis);
            assert_eq!(encoder.dynamic_allocation(), dynamic_analysis);
            assert_eq!(encoder.spreading_analysis(), spreading_enabled);
            if spreading_corpus || allocation_corpus {
                assert_eq!(
                    [
                        encoder.spreading.decision() as i64,
                        i64::from(encoder.spreading.tonal_average()),
                        i64::from(encoder.spreading.hf_average()),
                        i64::from(encoder.spreading.tapset())
                    ],
                    state[443..447],
                    "spreading state frame{rows}"
                );
            }

            assert_eq!(encoder.pitch_prefilter(), pitch_enabled);
            if pitch_corpus {
                let params = encoder.prefilter.parameters();
                assert_eq!(
                    [
                        i64::from(params.gain() != 0),
                        i64::from(params.period()),
                        i64::from(params.gain()),
                        if params.gain() == 0 {
                            0
                        } else {
                            i64::from(params.gain() / 3072 - 1)
                        },
                        params.tapset() as i64
                    ],
                    state[455..460],
                    "prefilter parameters frame{rows}"
                );
                for channel in 0..2 {
                    let hash = |input: Option<&[i32]>| -> u64 {
                        input.map_or(0, |input| {
                            let mut h = 14695981039346656037u64;
                            for value in input {
                                for byte in value.to_le_bytes() {
                                    h = (h ^ u64::from(byte)).wrapping_mul(1099511628211);
                                }
                            }
                            h
                        })
                    };
                    let raw = hash(encoder.prefilter.raw_history(channel).map(|a| a.as_slice()));
                    let overlap = hash(encoder.prefilter.overlap(channel).map(|a| a.as_slice()));
                    let base = 460 + channel * 4;
                    assert_eq!(
                        [raw, overlap],
                        [
                            state[base] as u64 | ((state[base + 1] as u64) << 32),
                            state[base + 2] as u64 | ((state[base + 3] as u64) << 32)
                        ],
                        "prefilter histories frame{rows} C{channel}"
                    );
                }
            }
            assert_eq!(encoder.allocation_analysis(), allocation_enabled);
            if allocation_corpus {
                assert_eq!(
                    [
                        i64::from(encoder.trim.stereo_saving()),
                        encoder.intensity.band() as i64
                    ],
                    state[447..449],
                    "allocation history frame{rows}"
                );
                assert_eq!(
                    i64::from(analysis::equivalent_rate(size, channels, bytes)?),
                    state[454]
                );
            }
            let end = match rate {
                InputRate::Khz8 => 13,
                InputRate::Khz12 | InputRate::Khz16 => 17,
                InputRate::Khz24 => 19,
                InputRate::Khz48 => 21,
            };
            let layout = BandLayout::new(channels, 0, end)?;
            let mut read = RangeDecoder::new(&expected[1..]);
            let prefix = control::decode_prefix(&mut read, size, 0)?;
            assert!(!prefix.silence());
            if pitch_corpus {
                assert_eq!(
                    prefix.postfilter(),
                    if state[455] != 0 {
                        encoder.prefilter.parameters()
                    } else {
                        PostfilterParams::default()
                    },
                    "comb prefix frame{rows}"
                );
            }
            assert_eq!(prefix.transient(), detect && state[332] != 0);
            decoded_energy.coarse(&mut read, layout, size, prefix.intra())?;
            let tf = control::decode_tf(&mut read, size, 0..end, prefix.transient())?;
            if tf_analysis || spreading_corpus || allocation_corpus {
                for i in 0..21 {
                    assert_eq!(
                        i64::from(tf.values()[i]),
                        state[357 + i],
                        "TF frame{rows} band{i}"
                    );
                }
            }
            let spread = control::decode_spread(&mut read)?;
            assert_eq!(
                spread,
                encoder.spreading.decision(),
                "effective spreading frame{rows}"
            );
            let allocation =
                allocation::decode_allocation(&mut read, size, layout, prefix.transient())?;

            if allocation_corpus {
                assert_eq!(
                    [
                        i64::from(allocation.trim()),
                        allocation.intensity() as i64,
                        i64::from(allocation.dual_stereo())
                    ],
                    state[449..452],
                    "effective allocation frame{rows}"
                );
            }
            decoded_energy.fine(&mut read, layout, allocation.fine_bits())?;
            partition::decode_spectrum(&mut read, &allocation, &tf, spread, seed, false)?;
            if allocation.anti_collapse_reserved() {
                assert_eq!(
                    read.decode_bits(1)?,
                    u32::from(streak < 2),
                    "anti control frame{rows}"
                );
            }
            let left = bytes as u32 * 8 - read.tell();
            decoded_energy.finalise(
                &mut read,
                layout,
                allocation.fine_bits(),
                allocation.fine_priority(),
                left,
            )?;
            for c in 0..channels {
                assert_eq!(
                    decoded_energy.channel(c),
                    encoder.energy.energies().channel(c),
                    "paired energy frame{rows}"
                );
            }
            assert_eq!(
                [i64::from(read.tell()), i64::from(read.tell_frac())],
                state[330..332],
                "accounting frame{rows}"
            );
            assert_eq!(read.range(), encoder.range);
            for _ in 0..10 {
                assert!(lines.next().ok_or("decoded")?.starts_with("decoded "));
            }
            rows += 1;
        }
        assert_eq!(
            rows,
            if pitch_corpus {
                1920
            } else if allocation_corpus {
                1440
            } else if detect {
                960
            } else {
                720
            }
        );
        Ok(())
    }
}
