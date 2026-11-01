//! Classic periodic/noise concealment and recovery scheduling.
//! Copyright (c) 2007-2008 CSIRO; 2007-2010 Xiph.Org Foundation;
//! 2008 Gregory Maxwell. See THIRD_PARTY_NOTICES.md.
use super::{CeltDecoder, DecodedPcm, DecoderError, FrameConfig};
use crate::celt::{
    FrameSize,
    bands::{self, BandError},
    lpc::{self, LpcError},
    math::{self, MathError},
    mdct_tables::WINDOW,
    output::PcmFrame,
    output_tables::BANDS,
    pitch::{self, PitchError},
    postfilter::{self, PostfilterParams},
    synthesis::{SynthesisControls, SynthesisError},
};
use std::fmt;
/// Latest CELT synthesis kind (neural/QEXT paths are not enabled).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConcealmentMode {
    /// No audio packet has established a stream yet.
    None,
    /// Successfully decoded normal frame.
    Normal,
    /// Pitch/excitation based periodic concealment.
    Periodic,
    /// Background/noise based concealment.
    Noise,
}
/// Concealment validation or DSP failure, with no partial state committed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ConcealmentError {
    /// Requested duration must be 120..5760 on a 48kHz clock, divisible by120.
    InvalidDuration,
    /// Internal history/context is inconsistent.
    InvalidState,
    /// Bounded LPC bandwidth expansion did not reach its safe synthesis domain.
    UnstableFilter,
    /// Pitch analysis failed.
    Pitch(PitchError),
    /// LPC analysis/filtering failed.
    Lpc(LpcError),
    /// Scalar decay/gain arithmetic failed.
    Math(MathError),
    /// Noise-band normalization failed.
    Band(BandError),
    /// PCM synthesis failed.
    Synthesis(SynthesisError),
}
impl fmt::Display for ConcealmentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidDuration => {
                f.write_str("CELT loss duration must be 120..5760 samples in 120-sample steps")
            }
            Self::InvalidState => f.write_str("inconsistent CELT concealment state"),
            Self::UnstableFilter => f.write_str("CELT concealment LPC filter did not stabilize"),
            Self::Pitch(e) => write!(f, "CELT concealment pitch: {e}"),
            Self::Lpc(e) => write!(f, "CELT concealment LPC: {e}"),
            Self::Math(e) => write!(f, "CELT concealment math: {e}"),
            Self::Band(e) => write!(f, "CELT concealment noise: {e}"),
            Self::Synthesis(e) => write!(f, "CELT concealment synthesis: {e}"),
        }
    }
}
impl std::error::Error for ConcealmentError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Pitch(e) => Some(e),
            Self::Lpc(e) => Some(e),
            Self::Math(e) => Some(e),
            Self::Band(e) => Some(e),
            Self::Synthesis(e) => Some(e),
            _ => None,
        }
    }
}
impl From<PitchError> for ConcealmentError {
    fn from(e: PitchError) -> Self {
        Self::Pitch(e)
    }
}
impl From<LpcError> for ConcealmentError {
    fn from(e: LpcError) -> Self {
        Self::Lpc(e)
    }
}
impl From<MathError> for ConcealmentError {
    fn from(e: MathError) -> Self {
        Self::Math(e)
    }
}
impl From<BandError> for ConcealmentError {
    fn from(e: BandError) -> Self {
        Self::Band(e)
    }
}
impl From<SynthesisError> for ConcealmentError {
    fn from(e: SynthesisError) -> Self {
        Self::Synthesis(e)
    }
}
/// Read-only fixed-storage concealment history and scheduling diagnostics.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConcealmentState {
    history: [[i32; 2048]; 2],
    future: [[i32; 120]; 2],
    coefficients: [[i16; 24]; 2],
    pitch: u16,
    pub(super) duration: u16,
    plc_duration: u16,
    skip: bool,
    prefilter: bool,
    mode: ConcealmentMode,
    pub(super) initialized: bool,
    pub(super) last_size: FrameSize,
    start: usize,
    end: usize,
}
impl Default for ConcealmentState {
    fn default() -> Self {
        Self {
            history: [[0; 2048]; 2],
            future: [[0; 120]; 2],
            coefficients: [[0; 24]; 2],
            pitch: 0,
            duration: 0,
            plc_duration: 0,
            skip: true,
            prefilter: false,
            mode: ConcealmentMode::None,
            initialized: false,
            last_size: FrameSize::Ms2_5,
            start: 0,
            end: 21,
        }
    }
}
impl ConcealmentState {
    /// Last normal frame's inclusive start (zero for CELT,17 for Hybrid).
    pub fn start_band(&self) -> usize {
        self.start
    }
    /// Last normal frame's exclusive band endpoint, retained across loss headers.
    pub fn end_band(&self) -> usize {
        self.end
    }
    /// Latest normal/periodic/noise synthesis kind.
    pub fn mode(&self) -> ConcealmentMode {
        self.mode
    }
    /// Consecutive lost duration in 2.5ms units, capped at 10000.
    pub fn loss_duration(&self) -> u16 {
        self.duration
    }
    /// Concealment duration in 2.5ms units, capped at 10000.
    pub fn plc_duration(&self) -> u16 {
        self.plc_duration
    }
    /// Current 48kHz pitch period, zero until periodic PLC first runs.
    pub fn pitch_period(&self) -> u16 {
        self.pitch
    }
    /// Whether periodic concealment waits for consecutive normal frames.
    pub fn skips_periodic(&self) -> bool {
        self.skip
    }
    /// Pending negative prefilter/TDAC needed before the next MDCT synthesis.
    pub fn prefilter_pending(&self) -> bool {
        self.prefilter
    }
    /// Processed 2048-sample signal history; channel indices0/1 are always available.
    pub fn history(&self, channel: usize) -> Option<&[i32; 2048]> {
        self.history.get(channel)
    }
    /// 120-sample pending time extension; only first 60 are MDCT overlap after normal frames.
    pub fn future(&self, channel: usize) -> Option<&[i32; 120]> {
        self.future.get(channel)
    }
    /// Periodic PLC's persistent 24 Q12 prediction coefficients.
    pub fn coefficients(&self, channel: usize) -> Option<&[i16; 24]> {
        self.coefficients.get(channel)
    }
    pub(super) fn reset(&mut self) {
        let start = self.start;
        let end = self.end;
        *self = Self::default();
        self.start = start;
        self.end = end;
    }
    pub(super) fn begin_normal(&mut self) {
        if self.duration == 0 {
            self.skip = false;
        }
    }
}
impl CeltDecoder {
    /// Inspect fixed concealment history and scheduling state.
    pub fn concealment(&self) -> &ConcealmentState {
        &self.loss
    }
    /// Persistent CELT RNG, distinct from public final_range (zero for loss).
    pub fn rng_state(&self) -> u32 {
        self.rng
    }
    /// Conceal an explicit missing interval on a 48kHz clock,120..5760 samples in
    /// 120-sample steps. Scheduling follows the last packet's frame size and valid
    /// CELT subframes. Before the first audio packet this returns silence.
    pub fn conceal_samples(&mut self, duration: u16) -> Result<DecodedPcm, DecoderError> {
        if !(120..=5760).contains(&duration) || duration % 120 != 0 {
            return Err(ConcealmentError::InvalidDuration.into());
        }
        let mut next = self.clone();
        let mut result = DecodedPcm {
            values: [0; 11520],
            length: 0,
            rate: self.rate,
            channels: self.channels(),
            range: 0,
        };
        let mut remaining = usize::from(duration);
        while remaining > 0 {
            let limit = remaining.min(next.loss.last_size.samples()).min(960);
            let size = if limit >= 960 {
                FrameSize::Ms20
            } else if limit >= 480 {
                FrameSize::Ms10
            } else if limit >= 240 {
                FrameSize::Ms5
            } else {
                FrameSize::Ms2_5
            };
            let frame = next.loss_frame(size)?;
            let end = result.length + frame.samples().len();
            result.values[result.length..end].copy_from_slice(frame.samples());
            result.length = end;
            remaining -= size.samples();
        }
        *self = next;
        Ok(result)
    }
    pub(super) fn frame_or_loss(
        &mut self,
        payload: &[u8],
        config: FrameConfig,
    ) -> Result<PcmFrame, DecoderError> {
        Self::configuration_end(config)?;
        self.loss.last_size = config.size;
        if payload.len() <= 1 {
            Ok(self.loss_frame(config.size)?)
        } else {
            self.frame(payload, config)
        }
    }
    pub(super) fn recover_energy(
        &mut self,
        size: FrameSize,
        intra: bool,
        start: usize,
        end: usize,
    ) {
        if intra || self.loss.duration == 0 {
            return;
        }
        let missing = (i32::from(self.loss.duration) >> size.lm()).min(10);
        let safety = match size {
            FrameSize::Ms2_5 => 3 << 23,
            FrameSize::Ms5 => 1 << 23,
            _ => 0,
        };
        for (c, channel) in self.energy.values_mut().iter_mut().enumerate() {
            for (i, value) in channel.iter_mut().enumerate().take(end).skip(start) {
                let p1 = self.previous[c][i];
                let p2 = self.previous2[c][i];
                if *value < p1.max(p2) {
                    let slope = (p1 - *value).max((p2 - *value) >> 1).min(2 << 24);
                    *value = (-(20 << 24)).max(*value - ((1 + missing) * slope).max(0));
                } else {
                    *value = (*value).min(p1).min(p2);
                }
                *value -= safety;
            }
        }
    }
    pub(super) fn prepare_overlap(&mut self) -> Result<(), ConcealmentError> {
        if !self.loss.prefilter {
            return Ok(());
        }
        let mut overlap = [[0i32; 60]; 2];
        for (c, channel) in overlap.iter_mut().enumerate().take(self.channels()) {
            let post = self
                .backend
                .postfilter(c)
                .ok_or(ConcealmentError::InvalidState)?;
            let filtered =
                postfilter::prefilter(post.history(), &self.loss.future[c], post.parameters()[1]);
            for i in 0..60 {
                channel[i] = mul15(i32::from(WINDOW[i]), filtered[119 - i])
                    .wrapping_add(mul15(i32::from(WINDOW[119 - i]), filtered[i]));
            }
        }
        self.backend.set_overlap(&overlap);
        Ok(())
    }
    pub(super) fn record_normal(
        &mut self,
        size: FrameSize,
        start: usize,
        end: usize,
    ) -> Result<(), ConcealmentError> {
        self.record_signal(size)?;
        self.loss.duration = 0;
        self.loss.plc_duration = 0;
        self.loss.mode = ConcealmentMode::Normal;
        self.loss.prefilter = false;
        self.loss.initialized = true;
        self.loss.start = start;
        self.loss.end = end;
        Ok(())
    }
    fn record_signal(&mut self, size: FrameSize) -> Result<(), ConcealmentError> {
        let n = size.samples();
        for c in 0..self.channels() {
            let post = self
                .backend
                .postfilter(c)
                .ok_or(ConcealmentError::InvalidState)?;
            self.loss.history[c].copy_within(n..2048, 0);
            self.loss.history[c][2048 - n..].copy_from_slice(&post.history()[1026 - n..]);
            self.loss.future[c][..60].copy_from_slice(
                self.backend
                    .overlap(c)
                    .ok_or(ConcealmentError::InvalidState)?,
            );
        }
        Ok(())
    }
    // A configured outer Hybrid frame calls the native CELT loss path even on
    // first-packet FEC; initial Opus no-mode silence is handled by the caller.
    pub(crate) fn conceal_layer(
        &mut self,
        size: FrameSize,
        end: usize,
    ) -> Result<PcmFrame, DecoderError> {
        if !matches!(end, 19 | 21) {
            return Err(DecoderError::InvalidConfiguration);
        }
        self.loss.start = 17;
        self.loss.end = end;
        self.loss.initialized = true;
        Ok(self.loss_frame(size)?)
    }
    fn loss_frame(&mut self, size: FrameSize) -> Result<PcmFrame, ConcealmentError> {
        if !self.loss.initialized {
            self.range = 0;
            return PcmFrame::silence(size, self.rate, self.channels())
                .map_err(SynthesisError::from)
                .map_err(ConcealmentError::from);
        }
        let noise = self.loss.plc_duration >= 40 || self.loss.skip || self.loss.start != 0;
        let pcm = if noise {
            self.noise_loss(size)?
        } else {
            self.periodic_loss(size)?
        };
        self.loss.duration = (self.loss.duration + (1u16 << size.lm())).min(10000);
        self.loss.plc_duration = (self.loss.plc_duration + (1u16 << size.lm())).min(10000);
        self.loss.mode = if noise {
            ConcealmentMode::Noise
        } else {
            ConcealmentMode::Periodic
        };
        self.range = 0;
        Ok(pcm)
    }
    fn noise_loss(&mut self, size: FrameSize) -> Result<PcmFrame, ConcealmentError> {
        self.prepare_overlap()?;
        let channels = self.channels();
        let end = self.loss.end;
        let decay = if self.loss.duration == 0 {
            3 << 23
        } else {
            1 << 23
        };
        let mut spectra = [[0i32; 960]; 2];
        for (c, spectrum) in spectra.iter_mut().enumerate().take(channels) {
            for i in self.loss.start..end {
                self.energy.values_mut()[c][i] =
                    self.background[c][i].max(self.energy.values()[c][i] - decay);
                let first = usize::from(BANDS[i]) << size.lm();
                let last = usize::from(BANDS[i + 1]) << size.lm();
                for value in &mut spectrum[first..last] {
                    self.rng = self.rng.wrapping_mul(1664525).wrapping_add(1013904223);
                    *value = ((self.rng as i32) >> 20) << 10;
                }
                bands::renormalize(&mut spectrum[first..last], i32::MAX)?;
            }
        }
        let current = self
            .backend
            .postfilter(0)
            .ok_or(ConcealmentError::InvalidState)?
            .parameters()[1];
        let params =
            PostfilterParams::new(current.period().max(15), current.gain(), current.tapset())
                .map_err(SynthesisError::from)?;
        let controls = SynthesisControls {
            size,
            bands: self.loss.start..end,
            transient: false,
            silence: false,
            postfilter: params,
        };
        let energy = *self.energy.values();
        let n = size.samples();
        let pcm = if channels == 2 {
            self.backend
                .synthesize(&[&spectra[0][..n], &spectra[1][..n]], &energy, &controls)?
        } else {
            self.backend
                .synthesize(&[&spectra[0][..n]], &energy[..1], &controls)?
        };
        self.record_signal(size)?;
        self.loss.prefilter = false;
        self.loss.skip = true;
        Ok(pcm)
    }
    fn periodic_loss(&mut self, size: FrameSize) -> Result<PcmFrame, ConcealmentError> {
        let first = self.loss.mode != ConcealmentMode::Periodic;
        let channels = self.channels();
        if first {
            self.loss.pitch = pitch::plc_pitch(
                &self.loss.history[0],
                if channels == 2 {
                    Some(&self.loss.history[1])
                } else {
                    None
                },
            )?
            .period();
        }
        let period = usize::from(self.loss.pitch);
        let exc_length = (2 * period).min(1024);
        let n = size.samples();
        let count = n + 120;
        let fade = if first { 32767 } else { 26214 };
        for c in 0..channels {
            let mut buffer = [0i32; 2168];
            buffer[..2048].copy_from_slice(&self.loss.history[c]);
            buffer[2048..].copy_from_slice(&self.loss.future[c]);
            let mut exc = [0i16; 1048];
            for (i, value) in exc.iter_mut().enumerate() {
                *value = round_signal(buffer[1000 + i]);
            }
            let coeff = &mut self.loss.coefficients[c];
            if first {
                let ac = lpc::autocorrelation(&exc[24..], 24, true)?;
                let mut correlation = [0i32; 25];
                correlation.copy_from_slice(ac.values());
                correlation[0] += correlation[0] >> 13;
                for (i, value) in correlation.iter_mut().enumerate().skip(1) {
                    *value -= mul15((2 * i * i) as i32, *value);
                }
                lpc::analyse(&correlation, coeff)?;
                stabilize(coeff)?;
            }
            let mut residual = [0i16; 1024];
            lpc::fir(
                &exc[1024 - exc_length..],
                coeff,
                &mut residual[..exc_length],
            )?;
            exc[1048 - exc_length..].copy_from_slice(&residual[..exc_length]);
            let max = exc[1048 - exc_length..]
                .iter()
                .fold(0i32, |m, &v| m.max(i32::from(v).abs()));
            let log = if max > 0 {
                31 - max.leading_zeros() as i32
            } else {
                0
            };
            let shift = (2 * log - 20).max(0);
            let half = exc_length / 2;
            let mut e1 = 1i32;
            let mut e2 = 1i32;
            for i in 0..half {
                let a = i32::from(exc[1048 - half + i]);
                let b = i32::from(exc[1048 - 2 * half + i]);
                e1 += (a * a) >> shift;
                e2 += (b * b) >> shift;
            }
            e1 = e1.min(e2);
            let decay = math::sqrt(math::fractional_divide(e1 >> 1, e2)?)?;
            buffer.copy_within(n..2048, 0);
            let offset = 1024 - period;
            let mut attenuation = mul15(fade, decay);
            let mut cursor = 0;
            let mut s1 = 0i32;
            for i in 0..count {
                if cursor >= period {
                    cursor -= period;
                    attenuation = mul15(attenuation, decay);
                }
                buffer[2048 - n + i] =
                    mul15(attenuation, i32::from(exc[24 + offset + cursor])) << 12;
                let sample = i32::from(round_signal(buffer[2048 - n - period + cursor]));
                s1 += (sample * sample) >> 11;
                cursor += 1;
            }
            let mut memory = [0i16; 24];
            for (i, v) in memory.iter_mut().enumerate() {
                *v = round_signal(buffer[2048 - n - 1 - i]);
            }
            lpc::iir(&mut buffer[2048 - n..], coeff, &mut memory)?;
            for value in &mut buffer[2048 - n..] {
                *value = (*value).clamp(-536870911, 536870911);
            }
            guard_signal(&mut buffer[2048 - n..], s1)?;
            self.loss.history[c].copy_from_slice(&buffer[..2048]);
            self.loss.future[c].copy_from_slice(&buffer[2048..]);
        }
        let pcm = self
            .backend
            .render_concealed(&self.loss.history, &self.loss.future, size)?;
        self.loss.prefilter = true;
        Ok(pcm)
    }
}
fn stabilize(coefficients: &mut [i16; 24]) -> Result<(), ConcealmentError> {
    let mut next = *coefficients;
    // Including rounding, each magnitude satisfies next<=0.99*old+1.
    // After512 iterations even an initial32768 is below300;24 such terms
    // plus4096 fit the sum bound. Commit only after the finite guard succeeds.
    for _ in 0..512 {
        if 4096 + next.iter().map(|&v| i32::from(v).abs()).sum::<i32>() < 65535 {
            break;
        }
        let mut tmp = 32767;
        for value in &mut next {
            tmp = mul15(32440, tmp);
            *value = mul15(i32::from(*value), tmp) as i16;
        }
    }
    if 4096 + next.iter().map(|&v| i32::from(v).abs()).sum::<i32>() >= 65535 {
        return Err(ConcealmentError::UnstableFilter);
    }
    *coefficients = next;
    Ok(())
}
fn guard_signal(signal: &mut [i32], previous_energy: i32) -> Result<(), ConcealmentError> {
    let length = signal.len();
    // Saturated rounded samples contribute at most(32767^2)>>11=524256
    // each, so both energy accumulators fit positive i32 for these lengths.
    if !matches!(length, 240 | 360 | 600 | 1080)
        || previous_energy < 0
        || previous_energy > length as i32 * 524256
        || signal.iter().any(|v| !(-536870911..=536870911).contains(v))
    {
        return Err(ConcealmentError::InvalidState);
    }
    let mut energy = 0i32;
    for &value in signal.iter() {
        let sample = i32::from(round_signal(value));
        energy += (sample * sample) >> 11;
    }
    if previous_energy <= energy >> 2 {
        signal.fill(0);
    } else if previous_energy < energy {
        let ratio = math::sqrt(math::fractional_divide(
            (previous_energy >> 1) + 1,
            energy + 1,
        )?)?;
        for (i, value) in signal.iter_mut().enumerate() {
            let gain = if i < 120 {
                32767 - mul15(i32::from(WINDOW[i]), 32767 - ratio)
            } else {
                ratio
            };
            *value = mul15(gain, *value);
        }
    }
    Ok(())
}
fn round_signal(x: i32) -> i16 {
    (x.wrapping_add(2048) >> 12).clamp(-32767, 32767) as i16
}
fn mul15(a: i32, b: i32) -> i32 {
    ((i64::from(a as i16) * i64::from(b)) >> 15) as i32
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_bandwidth_and_energy_guards_match() -> Result<(), Box<dyn std::error::Error>> {
        let mut counts = [0; 2];
        for (index, line) in include_str!("../../../tests/celt-plc-filter-fixtures.txt")
            .lines()
            .enumerate()
        {
            if line.starts_with('#') {
                continue;
            }
            let row: Vec<_> = line.split_whitespace().collect();
            if row[0] == "filter" {
                let input: Vec<i16> = row[1..25]
                    .iter()
                    .map(|v| v.parse())
                    .collect::<Result<_, _>>()?;
                let expected: Vec<i16> = row[25..]
                    .iter()
                    .map(|v| v.parse())
                    .collect::<Result<_, _>>()?;
                let mut coefficients: [i16; 24] =
                    input.try_into().map_err(|_| "coefficient length")?;
                stabilize(&mut coefficients)?;
                assert_eq!(coefficients.as_slice(), expected, "bandwidth row{index}");
                assert!(
                    4096 + coefficients
                        .iter()
                        .map(|&v| i32::from(v).abs())
                        .sum::<i32>()
                        < 65535
                );
                counts[0] += 1;
            } else {
                assert_eq!(row[0], "guard");
                let n: usize = row[1].parse()?;
                let previous = row[2].parse()?;
                let length = n + 120;
                let mut values: Vec<i32> = row[3..3 + length]
                    .iter()
                    .map(|v| v.parse())
                    .collect::<Result<_, _>>()?;
                let before = values.clone();
                let expected: Vec<i32> = row[3 + length..]
                    .iter()
                    .map(|v| v.parse())
                    .collect::<Result<_, _>>()?;
                guard_signal(&mut values, previous)?;
                assert_eq!(values, expected, "energy guard row{index}");
                assert!(
                    values
                        .iter()
                        .zip(before)
                        .all(|(&a, b)| i64::from(a).abs() <= i64::from(b).abs())
                );
                counts[1] += 1;
            }
        }
        assert_eq!(counts, [208, 232]);
        Ok(())
    }
    #[test]
    fn invalid_energy_guard_preserves_complete_signal() {
        for (mut values, energy) in [
            (vec![], 0),
            (vec![0; 239], 0),
            (vec![0; 1081], 0),
            (vec![0; 240], -1),
            (vec![0; 240], 240 * 524256 + 1),
            (vec![536870912; 240], 0),
            (vec![i32::MIN; 240], 0),
        ] {
            let before = values.clone();
            assert_eq!(
                guard_signal(&mut values, energy),
                Err(ConcealmentError::InvalidState)
            );
            assert_eq!(values, before);
        }
    }
}
