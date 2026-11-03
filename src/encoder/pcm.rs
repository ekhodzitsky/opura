//! Common unmodified input history, before any CELT high-band/stereo fades.
//!
//! Filtering and history completion are separate: native SILK DTX returns
//! after filtering but before the outer history advances. Packet owners stage
//! this state together with both layers, including on FEC budget retries.
use super::FrontendError;
use crate::silk::{resampler::OutputRate, synthesis::round_shift};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PcmState {
    rate: OutputRate,
    channels: usize,
    dc: [i32; 2],
    voip: [i32; 4],
    history: [i16; 960],
}
impl PcmState {
    pub fn has_geometry(&self, rate: OutputRate, channels: usize) -> bool {
        self.rate == rate && self.channels == channels
    }
    pub fn mono(rate: OutputRate) -> Self {
        Self {
            rate,
            channels: 1,
            dc: [0; 2],
            voip: [0; 4],
            history: [0; 960],
        }
    }
    pub fn stereo(rate: OutputRate) -> Self {
        Self {
            channels: 2,
            ..Self::mono(rate)
        }
    }
    fn valid(&self, input: &[i16]) -> bool {
        !input.is_empty()
            && input.len() % self.channels == 0
            && input.len() / self.channels <= self.rate.hz() as usize * 120 / 1000
    }
    /// Native AUDIO rejection on immutable API channels, before downmix or
    /// resampling. The output may have spare capacity, which remains untouched.
    pub fn filter(&mut self, input: &[i16], output: &mut [i16]) -> Result<(), FrontendError> {
        if !self.valid(input) || output.len() < input.len() {
            return Err(FrontendError::InvalidGeometry);
        }
        for c in 0..self.channels {
            let shift = 31 - (self.rate.hz() / 12).leading_zeros();
            for i in (c..input.len()).step_by(self.channels) {
                output[i] = reject_sample(&mut self.dc[c], shift, input[i]);
            }
        }
        Ok(())
    }
    pub fn filter_voip(
        &mut self,
        input: &[i16],
        output: &mut [i16],
        cutoff: i32,
    ) -> Result<(), FrontendError> {
        if !self.valid(input) || output.len() < input.len() || !(60..=100).contains(&cutoff) {
            return Err(FrontendError::InvalidGeometry);
        }
        crate::silk::encoder::highpass::filter(
            input,
            output,
            &mut self.voip,
            self.rate.hz(),
            self.channels,
            cutoff,
        );
        Ok(())
    }
    /// Produce the normal 4ms-compensated CELT input from the previous committed
    /// history and the current filtered frame. Call before `commit`, and before
    /// CELT-only gain/stereo fades. Supports frames shorter than compensation.
    pub fn aligned(&self, filtered: &[i16], output: &mut [i16]) -> Result<(), FrontendError> {
        self.aligned_with_delay(filtered, output, true)
    }
    pub fn aligned_with_delay(
        &self,
        filtered: &[i16],
        output: &mut [i16],
        delayed: bool,
    ) -> Result<(), FrontendError> {
        if !self.valid(filtered) || output.len() < filtered.len() {
            return Err(FrontendError::InvalidGeometry);
        }
        let end = self.rate.hz() as usize / 100 * self.channels;
        let delay = if delayed {
            self.rate.hz() as usize / 250 * self.channels
        } else {
            0
        };
        let old = delay.min(filtered.len());
        output[..old].copy_from_slice(&self.history[end - delay..end - delay + old]);
        if filtered.len() > delay {
            output[delay..filtered.len()].copy_from_slice(&filtered[..filtered.len() - delay]);
        }
        Ok(())
    }
    /// Retain the latest native 10ms of unmodified filtered API-channel input.
    /// Short frames shift older input; long frames keep only the final window.
    /// Omit this call for the SILK early-DTX return, including in SILK-only mode.
    pub fn commit(&mut self, filtered: &[i16]) -> Result<(), FrontendError> {
        if !self.valid(filtered) {
            return Err(FrontendError::InvalidGeometry);
        }
        let end = self.rate.hz() as usize / 100 * self.channels;
        if filtered.len() >= end {
            self.history[..end].copy_from_slice(&filtered[filtered.len() - end..]);
        } else {
            self.history.copy_within(filtered.len()..end, 0);
            self.history[end - filtered.len()..end].copy_from_slice(filtered);
        }
        Ok(())
    }
    pub fn history(&self) -> &[i16] {
        &self.history[..self.rate.hz() as usize / 100 * self.channels]
    }
    pub fn history_mut(&mut self) -> &mut [i16] {
        &mut self.history[..self.rate.hz() as usize / 100 * self.channels]
    }
}

fn reject_sample(state: &mut i32, shift: u32, sample: i16) -> i16 {
    let y = (i32::from(sample) << 14) - *state;
    *state += round_shift(y, shift);
    round_shift(y, 14).clamp(-32767, 32767) as i16
}

// Preserve the original planar native fixture independently of the interleaved
// owner tests. The production filter above uses precisely this sample kernel.
#[cfg(test)]
pub(crate) fn dc_reject(state: &mut i32, rate: OutputRate, input: &[i16], output: &mut [i16]) {
    let shift = 31 - (rate.hz() / 12).leading_zeros();
    for (out, &sample) in output.iter_mut().zip(input) {
        *out = reject_sample(state, shift, sample);
    }
}

#[cfg(test)]
mod tests;
