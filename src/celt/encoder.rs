//! Stateful signed16 PCM to single-frame CELT-only Opus packets.
//! All-long default with optional tone/transient short-window selection. Further
//! adaptive decisions and SILK/Hybrid encoding remain separate work. See THIRD_PARTY_NOTICES.md for constituent algorithms.
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
    energy::{BandLayout, CoarseEncoding, EnergyEncoder, EnergyEncodingError, EnergyError},
    input::{InputError, InputRate, PreemphasisState},
    partition::{self, PartitionError, SpectrumEncoding, SpectrumInput},
    pvq::Spread,
};
use crate::range::{EncoderError as EntropyError, RangeEncoder};
use std::fmt;

/// Invalid PCM/geometry/budget or a failed constituent encoding stage.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum EncoderError {
    /// Input channel count must be one or two.
    InvalidChannels,
    /// Interleaved PCM length must exactly match frame duration/rate/channels.
    InvalidInput,
    /// Requested CELT payload must contain 2..1275 bytes, excluding the TOC byte.
    InvalidBudget,
    /// The selected frame cannot fit the requested physical budget.
    BudgetExhausted,
    /// PCM pre-emphasis failed.
    Input(InputError),
    /// Transform or band analysis failed.
    Analysis(AnalysisError),
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
            Self::InvalidInput => f.write_str("CELT PCM length does not match input geometry"),
            Self::InvalidBudget => f.write_str("CELT payload budget must be 2..1275 bytes"),
            Self::BudgetExhausted => f.write_str("CELT frame exceeds requested byte budget"),
            Self::Input(e) => write!(f, "CELT encoder input: {e}"),
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

/// Fixed-rate/channel CELT-only encoder with bounded storage and no allocation.
///
/// Default policy uses long transforms (optional native transient detection),
/// disabled pitch comb/silence suppression,
/// standard TF/Normal spreading, zero dynamic boosts, trim five, full-band
/// intensity, no dual stereo, two-pass energy and stereo theta RDO. This supplies
/// valid audio packet coding, not adaptive Opus mode/quality selection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CeltEncoder {
    rate: InputRate,
    channels: usize,
    preemphasis: [PreemphasisState; 2],
    overlap: [[i32; 120]; 2],
    energy: EnergyEncoder,
    range: u32,
    previous_coded: usize,
    first: bool,
    detect_transients: bool,
    transient_streak: u8,
}
impl CeltEncoder {
    /// Start a fresh one/two-channel stream at a standard PCM input rate.
    pub fn new(rate: InputRate, channels: usize) -> Result<Self, EncoderError> {
        if !(1..=2).contains(&channels) {
            return Err(EncoderError::InvalidChannels);
        }
        Ok(Self {
            rate,
            channels,
            preemphasis: [PreemphasisState::default(), PreemphasisState::default()],
            overlap: [[0; 120]; 2],
            energy: EnergyEncoder::default(),
            range: 0,
            previous_coded: 0,
            first: true,
            detect_transients: false,
            transient_streak: 0,
        })
    }
    /// Fixed PCM input rate.
    pub fn input_rate(&self) -> InputRate {
        self.rate
    }
    /// Fixed interleaved input/encoded channel count.
    pub fn channels(&self) -> usize {
        self.channels
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
    /// Last completed packet final range, initially zero.
    pub fn final_range(&self) -> u32 {
        self.range
    }
    /// Clear histories and diagnostics, retaining geometry and transient setting.
    pub fn reset(&mut self) {
        self.preemphasis = [PreemphasisState::default(), PreemphasisState::default()];
        self.overlap = [[0; 120]; 2];
        self.energy.reset();
        self.range = 0;
        self.previous_coded = 0;
        self.first = true;
        self.transient_streak = 0;
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
    // Static finalization injection verifies rollback after every preceding stage.
    fn encode_with(
        &mut self,
        size: FrameSize,
        pcm: &[i16],
        payload_bytes: usize,
        finish: impl for<'a> FnOnce(&'a mut RangeEncoder) -> Result<&'a [u8], EntropyError>,
    ) -> Result<EncodedPacket, EncoderError> {
        if !(2..=1275).contains(&payload_bytes) {
            return Err(EncoderError::InvalidBudget);
        }
        if pcm.len() != size.samples() / self.rate.factor() * self.channels {
            return Err(EncoderError::InvalidInput);
        }
        let mut next = self.clone();
        let packet = next.encode_inner(size, pcm, payload_bytes, finish)?;
        *self = next;
        Ok(packet)
    }
    fn encode_inner(
        &mut self,
        size: FrameSize,
        pcm: &[i16],
        payload_bytes: usize,
        finish: impl for<'a> FnOnce(&'a mut RangeEncoder) -> Result<&'a [u8], EntropyError>,
    ) -> Result<EncodedPacket, EncoderError> {
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
        let mut writer = RangeEncoder::new(payload_bytes)?;
        let budget = payload_bytes as u32 * 8;
        writer.encode_bit_logp(false, 15)?;
        // Comb presence uses the decoder's cached initial tell=1.
        if 16 < budget {
            writer.encode_bit_logp(false, 1)?;
        }
        let transient_allowed = size != FrameSize::Ms2_5 && writer.tell() + 3 <= budget;
        let transient = self.detect_transients
            && transient_allowed
            && analysis::analyze_transients(
                size,
                &context[..self.channels * stride],
                self.channels,
                false,
            )?
            .transient();
        if transient_allowed {
            writer.encode_bit_logp(transient, 3)?;
        }
        let (end, config) = match self.rate {
            InputRate::Khz8 => (13, 16),
            InputRate::Khz12 | InputRate::Khz16 => (17, 20),
            InputRate::Khz24 => (19, 24),
            InputRate::Khz48 => (21, 28),
        };
        let layout = BandLayout::new(self.channels, 0, end)?;
        let spectrum = analysis::analyze_frame(
            size,
            &context[..self.channels * stride],
            self.channels,
            self.channels,
            self.rate,
            transient,
        )?;
        let bands = analysis::analyze_bands(size, spectrum.values(), self.channels, end, end)?;
        let mut target = [[0; 21]; 2];
        let mut amplitudes = [[0; 21]; 2];
        for c in 0..self.channels {
            target[c] = *bands.log_energies(c).ok_or(EncoderError::InvalidChannels)?;
            amplitudes[c] = *bands.amplitudes(c).ok_or(EncoderError::InvalidChannels)?;
            let previous = self
                .energy
                .energies()
                .channel(c)
                .ok_or(EncoderError::InvalidChannels)?;
            let residual = self
                .energy
                .residuals(c)
                .ok_or(EncoderError::InvalidChannels)?;
            for i in 0..end {
                if (i64::from(target[c][i]) - i64::from(previous[i])).abs() < 2 << 24 {
                    target[c][i] -= residual[i].clamp(-(1 << 23), 1 << 23) >> 2;
                }
            }
        }
        self.energy.coarse(
            &mut writer,
            &target,
            CoarseEncoding {
                layout,
                size,
                effective_end: end,
                available_bytes: payload_bytes,
                force_intra: self.first,
                two_pass: true,
                loss_rate: 0,
                lfe: false,
            },
        )?;
        let tf = control::encode_tf(
            &mut writer,
            size,
            0..end,
            transient,
            &[transient; 21],
            false,
        )?;
        let spread = control::encode_spread(&mut writer, Spread::Normal)?;
        let allocation = allocation::encode_allocation(
            &mut writer,
            size,
            layout,
            transient,
            AllocationEncoding {
                boosts: [0; 21],
                trim: 5,
                intensity: end,
                dual_stereo: false,
                previous_coded_bands: self.previous_coded,
                signal_bandwidth: end - 1,
            },
        )?;
        self.energy
            .fine(&mut writer, layout, allocation.fine_bits())?;
        partition::encode_spectrum(
            &mut writer,
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
                theta_rdo: true,
            },
        )?;
        if allocation.anti_collapse_reserved() {
            writer.encode_bits(u32::from(self.transient_streak < 2), 1)?;
        }
        let left = budget
            .checked_sub(writer.tell())
            .ok_or(EncoderError::BudgetExhausted)?;
        self.energy.finalise(
            &mut writer,
            layout,
            allocation.fine_bits(),
            allocation.fine_priority(),
            left,
        )?;
        let range = writer.range();
        let payload = finish(&mut writer)?;
        let mut packet = EncodedPacket {
            bytes: [0; 1276],
            length: payload_bytes + 1,
            size,
            channels: self.channels,
            range,
        };
        packet.bytes[0] =
            ((config + size.lm()) as u8) << 3 | if self.channels == 2 { 4 } else { 0 };
        packet.bytes[1..packet.length].copy_from_slice(payload);
        let coded = allocation.coded_bands();
        self.previous_coded = if self.previous_coded == 0 {
            coded
        } else {
            (self.previous_coded + 1).min((self.previous_coded - 1).max(coded))
        };
        self.range = range;
        // Native short/disabled-window streak only matters below threshold2.
        self.transient_streak = if transient || !transient_allowed {
            (self.transient_streak + 1).min(2)
        } else {
            0
        };
        self.first = false;
        Ok(packet)
    }
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
    fn injected_finalization_errors_preserve_complete_stream_state()
    -> Result<(), Box<dyn std::error::Error>> {
        for detect in [false, true] {
            verify_finalization_failure(detect)?;
        }
        Ok(())
    }
    fn verify_finalization_failure(detect: bool) -> Result<(), Box<dyn std::error::Error>> {
        let mut encoder = CeltEncoder::new(InputRate::Khz48, 2)?;
        encoder.set_transient_detection(detect);
        encoder.encode(FrameSize::Ms10, &[0; 960], 128)?;
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
    fn original_complete_packets_histories_and_accounting_match()
    -> Result<(), Box<dyn std::error::Error>> {
        verify_packets(include_str!("../../tests/celt-encoder-fixtures.txt"), false)
    }
    #[test]
    fn original_transient_packets_histories_and_accounting_match()
    -> Result<(), Box<dyn std::error::Error>> {
        verify_packets(
            include_str!("../../tests/celt-encoder-transient-fixtures.txt"),
            true,
        )
    }
    fn verify_packets(fixtures: &str, detect: bool) -> Result<(), Box<dyn std::error::Error>> {
        use crate::{celt::energy::EnergyState, range::RangeDecoder};
        let sizes = [
            FrameSize::Ms2_5,
            FrameSize::Ms5,
            FrameSize::Ms10,
            FrameSize::Ms20,
        ];
        let mut encoder = CeltEncoder::new(InputRate::Khz48, 1)?;
        encoder.set_transient_detection(detect);
        let mut decoded_energy = EnergyState::default();
        let mut lines = fixtures.lines().filter(|l| !l.starts_with('#'));
        let mut rows = 0;
        while let Some(line) = lines.next() {
            let f: Vec<_> = line.split_whitespace().collect();
            assert_eq!(f[0], "frame");
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
            assert_eq!(state.len(), if detect { 334 } else { 332 });
            if encoder.rate != rate || encoder.channels != channels {
                encoder = CeltEncoder::new(rate, channels)?;
                encoder.set_transient_detection(detect);
                decoded_energy.reset();
            } else if f[1] == "1" {
                encoder.reset();
                decoded_energy.reset();
            }
            let seed = encoder.range;
            let streak = encoder.transient_streak;
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
            if detect {
                assert_eq!(
                    i64::from(encoder.transient_streak),
                    state[333],
                    "streak frame{rows}"
                );
            }
            assert_eq!(encoder.transient_detection(), detect);
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
            assert_eq!(prefix.transient(), detect && state[332] != 0);
            decoded_energy.coarse(&mut read, layout, size, prefix.intra())?;
            let tf = control::decode_tf(&mut read, size, 0..end, prefix.transient())?;
            let spread = control::decode_spread(&mut read)?;
            let allocation =
                allocation::decode_allocation(&mut read, size, layout, prefix.transient())?;

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
        assert_eq!(rows, if detect { 960 } else { 720 });
        Ok(())
    }
}
