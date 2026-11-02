//! Fixed-point SILK encoder/decoder resampling with reference delay compensation.
//!
//! Adapted from Opus v1.6.1 silk/resampler*.c; see THIRD_PARTY_NOTICES.md.
//! This stage resamples internal PCM. Opus mono/stereo buffering is separate.
use super::{
    pitch::SilkRate,
    synthesis::{mul_wb, round_shift, saturate16},
};
use std::fmt;

#[path = "resampler_tables.rs"]
mod tables;
use tables::*;

/// Supported standard Opus PCM rates.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutputRate {
    /// 8000 samples per second.
    Khz8,
    /// 12000 samples per second.
    Khz12,
    /// 16000 samples per second.
    Khz16,
    /// 24000 samples per second.
    Khz24,
    /// 48000 samples per second.
    Khz48,
}
impl OutputRate {
    /// Sampling rate in Hz.
    pub fn hz(self) -> u32 {
        self.khz() as u32 * 1000
    }
    fn khz(self) -> usize {
        match self {
            Self::Khz8 => 8,
            Self::Khz12 => 12,
            Self::Khz16 => 16,
            Self::Khz24 => 24,
            Self::Khz48 => 48,
        }
    }
    fn index(self) -> usize {
        match self {
            Self::Khz8 => 0,
            Self::Khz12 => 1,
            Self::Khz16 => 2,
            Self::Khz24 => 3,
            Self::Khz48 => 4,
        }
    }
}

/// Invalid resampler geometry; neither history nor output is changed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ResampleError {
    /// Input must contain 1..=120 whole milliseconds at the supplied rate.
    InvalidInputLength,
    /// Output does not have capacity for this input's complete converted PCM.
    OutputTooSmall {
        /// Required sample capacity.
        required: usize,
        /// Actual sample capacity.
        available: usize,
    },
}
impl fmt::Display for ResampleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInputLength => {
                f.write_str("SILK resampling requires 1..120 whole milliseconds")
            }
            Self::OutputTooSmall {
                required,
                available,
            } => write!(
                f,
                "SILK resampling needs {required} output samples, got {available}"
            ),
        }
    }
}
impl std::error::Error for ResampleError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Method {
    Copy,
    Up2,
    UpFir,
    Down {
        order: usize,
        fractions: usize,
        coefficients: &'static [i16],
    },
}

/// Persistent decoder resampler. Rate changes clear filter/delay history.
/// All storage is fixed; output-rate buffering is separate.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResamplerState {
    core: ResamplerCore<16, 24, 184>,
}
impl ResamplerState {
    /// Start with empty filter/delay history at a fixed output rate.
    pub fn new(output: OutputRate) -> Self {
        Self {
            core: ResamplerCore::new(output, false),
        }
    }
    /// Output rate selected at construction.
    pub fn output_rate(&self) -> OutputRate {
        self.core.output
    }
    /// Clear input/filter/delay history, retaining the output rate.
    pub fn reset(&mut self) {
        self.core.reset();
    }
    /// Convert 1..=120 whole milliseconds of internal PCM into the output buffer.
    /// Geometry/capacity errors change neither history nor any output sample.
    /// Extra output tail is untouched; arbitrary whole-ms chunking is supported.
    pub fn process(
        &mut self,
        rate: SilkRate,
        input: &[i16],
        output: &mut [i16],
    ) -> Result<usize, ResampleError> {
        self.core.process(pcm_rate(rate), input, output)
    }
}

/// Encoder PCM conversion from any standard external rate to internal SILK PCM.
/// Uses the native encoder delay matrix, including same-rate compensation.
/// One state handles one channel; packet/channel orchestration is separate.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EncoderResamplerState {
    input: OutputRate,
    core: ResamplerCore<48, 36, 516>,
}
impl EncoderResamplerState {
    /// Start a channel converter at fixed external and internal sampling rates.
    pub fn new(input: OutputRate, internal: SilkRate) -> Self {
        Self {
            input,
            core: ResamplerCore::new(pcm_rate(internal), true),
        }
    }
    /// Clear filter/delay history, retaining the configured rates.
    pub fn reset(&mut self) {
        self.core.reset();
    }
    /// Convert 1..=120 whole milliseconds of external one-channel PCM.
    /// All input is consumed. Geometry/capacity errors change neither history
    /// nor output samples, and any output tail remains untouched. This does not
    /// add the encoder's separate analysis/lookahead buffering.
    pub fn process(&mut self, input: &[i16], output: &mut [i16]) -> Result<usize, ResampleError> {
        self.core.process(self.input, input, output)
    }
}
fn pcm_rate(rate: SilkRate) -> OutputRate {
    match rate {
        SilkRate::Khz8 => OutputRate::Khz8,
        SilkRate::Khz12 => OutputRate::Khz12,
        SilkRate::Khz16 => OutputRate::Khz16,
    }
}
// Const storage keeps decoder history arrays and its maximum work buffer unchanged.
#[derive(Clone, Debug, Eq, PartialEq)]
struct ResamplerCore<const DELAY: usize, const FIR: usize, const WORK: usize> {
    input: Option<OutputRate>,
    output: OutputRate,
    for_encoder: bool,
    iir: [i32; 6],
    fir16: [i16; 8],
    fir32: [i32; FIR],
    delay: [i16; DELAY],
    method: Method,
    ratio: u32,
    input_delay: usize,
}
impl<const DELAY: usize, const FIR: usize, const WORK: usize> ResamplerCore<DELAY, FIR, WORK> {
    /// Start with empty filter/delay history at a fixed output rate.
    fn new(output: OutputRate, for_encoder: bool) -> Self {
        Self {
            input: None,
            for_encoder,
            output,
            iir: [0; 6],
            fir16: [0; 8],
            fir32: [0; FIR],
            delay: [0; DELAY],
            method: Method::Copy,
            ratio: 0,
            input_delay: 0,
        }
    }
    /// Clear all input/filter/delay history while retaining the output rate.
    pub fn reset(&mut self) {
        *self = Self::new(self.output, self.for_encoder);
    }

    /// Resample 1..=120 whole milliseconds of internal SILK PCM.
    ///
    /// Validates geometry and output capacity before changing history or any
    /// output sample. Returns the number written; any extra output tail is left
    /// unchanged. Input can use any whole-ms chunking; filter history persists.
    /// This method does not add the separate Opus mono/stereo buffering delay.
    pub fn process(
        &mut self,
        rate: OutputRate,
        input: &[i16],
        output: &mut [i16],
    ) -> Result<usize, ResampleError> {
        let khz = rate.khz();
        if input.len() < khz || input.len() > khz * 120 || input.len() % khz != 0 {
            return Err(ResampleError::InvalidInputLength);
        }
        let needed = input.len() / khz * self.output.khz();
        if output.len() < needed {
            return Err(ResampleError::OutputTooSmall {
                required: needed,
                available: output.len(),
            });
        }
        self.configure(rate);
        let fresh = khz - self.input_delay;
        self.delay[self.input_delay..khz].copy_from_slice(&input[..fresh]);
        let delayed = self.delay;
        let split = self.output.khz();
        self.filter(rate, &delayed[..khz], &mut output[..split]);
        self.filter(
            rate,
            &input[fresh..input.len() - self.input_delay],
            &mut output[split..needed],
        );
        self.delay[..self.input_delay].copy_from_slice(&input[input.len() - self.input_delay..]);
        Ok(needed)
    }

    fn configure(&mut self, rate: OutputRate) {
        if self.input == Some(rate) {
            return;
        }
        self.reset();
        self.input = Some(rate);
        let khz = rate.khz();
        let output = self.output.khz();
        self.input_delay = if self.for_encoder {
            [[6, 0, 3], [0, 7, 3], [0, 1, 10], [0, 2, 6], [18, 10, 12]][rate.index()]
                [self.output.index()]
        } else {
            [[4, 0, 2, 0, 0], [0, 9, 4, 7, 4], [0, 3, 12, 7, 7]][rate.index()][self.output.index()]
        };
        self.method = if output == khz {
            Method::Copy
        } else if output == 2 * khz {
            Method::Up2
        } else if output > khz {
            Method::UpFir
        } else {
            match (khz, output) {
                (12, 8) => Method::Down {
                    order: 18,
                    fractions: 2,
                    coefficients: &DOWN_2_3,
                },
                (16, 12) => Method::Down {
                    order: 18,
                    fractions: 3,
                    coefficients: &DOWN_3_4,
                },
                (16, 8) | (24, 12) => Method::Down {
                    order: 24,
                    fractions: 1,
                    coefficients: &DOWN_1_2,
                },
                (24, 16) => Method::Down {
                    order: 18,
                    fractions: 2,
                    coefficients: &DOWN_2_3,
                },
                (24, 8) | (48, 16) => Method::Down {
                    order: 36,
                    fractions: 1,
                    coefficients: &DOWN_1_3,
                },
                (48, 12) => Method::Down {
                    order: 36,
                    fractions: 1,
                    coefficients: &DOWN_1_4,
                },
                _ => Method::Down {
                    order: 36,
                    fractions: 1,
                    coefficients: &DOWN_1_6,
                }, // 48 -> 8 only.
            }
        };
        let up = u32::from(self.method == Method::UpFir);
        // Same minimal rounded-up Q16 ratio as resampler_init's refinement loop.
        self.ratio = ((khz as u32) << (16 + up)).div_ceil(output as u32);
    }

    // process validates complete-ms geometry; every output slice has the exact
    // expected length. At most 10 ms enter a filter batch, keeping stack bounded.
    fn filter(&mut self, rate: OutputRate, input: &[i16], output: &mut [i16]) {
        if input.is_empty() {
            return;
        }
        match self.method {
            Method::Copy => output.copy_from_slice(input),
            Method::Up2 => upsample2(&mut self.iir, input, output),
            Method::UpFir => {
                let mut buffer = [0i16; 328];
                buffer[..8].copy_from_slice(&self.fir16);
                let mut written = 0;
                let batch_size = rate.khz() * 10;
                for chunk in input.chunks(batch_size) {
                    upsample2(&mut self.iir, chunk, &mut buffer[8..8 + 2 * chunk.len()]);
                    let maximum = (chunk.len() as u32) << 17;
                    let mut index = 0;
                    while index < maximum {
                        let phase = (((index & 65535) * 12) >> 16) as usize;
                        let at = (index >> 16) as usize;
                        let mut sum = 0i32;
                        for j in 0..4 {
                            sum = sum.wrapping_add(
                                i32::from(buffer[at + j]) * i32::from(FRACTIONAL[phase * 4 + j]),
                            );
                            sum = sum.wrapping_add(
                                i32::from(buffer[at + 7 - j])
                                    * i32::from(FRACTIONAL[(11 - phase) * 4 + j]),
                            );
                        }
                        output[written] = saturate16(round_shift(sum, 15));
                        written += 1;
                        index += self.ratio;
                    }
                    buffer.copy_within(2 * chunk.len()..2 * chunk.len() + 8, 0);
                }
                self.fir16.copy_from_slice(&buffer[..8]);
            }
            Method::Down {
                order,
                fractions,
                coefficients,
            } => {
                let mut buffer = [0i32; WORK];
                buffer[..order].copy_from_slice(&self.fir32[..order]);
                let batch_size = rate.khz() * 10;
                let mut written = 0;
                for chunk in input.chunks(batch_size) {
                    for (i, &sample) in chunk.iter().enumerate() {
                        let value = self.iir[0].wrapping_add(i32::from(sample) << 8);
                        buffer[order + i] = value;
                        let scaled = value.wrapping_shl(2);
                        self.iir[0] =
                            self.iir[1].wrapping_add(mul_wb(scaled, i32::from(coefficients[0])));
                        self.iir[1] = mul_wb(scaled, i32::from(coefficients[1]));
                    }
                    let maximum = (chunk.len() as u32) << 16;
                    let mut index = 0;
                    while index < maximum {
                        let at = (index >> 16) as usize;
                        let mut sum = 0i32;
                        if order == 18 {
                            let phase = (((index & 65535) * fractions as u32) >> 16) as usize;
                            for j in 0..9 {
                                sum = sum.wrapping_add(mul_wb(
                                    buffer[at + j],
                                    i32::from(coefficients[2 + phase * 9 + j]),
                                ));
                                sum = sum.wrapping_add(mul_wb(
                                    buffer[at + 17 - j],
                                    i32::from(coefficients[2 + (fractions - 1 - phase) * 9 + j]),
                                ));
                            }
                        } else {
                            for j in 0..order / 2 {
                                sum = sum.wrapping_add(mul_wb(
                                    buffer[at + j].wrapping_add(buffer[at + order - 1 - j]),
                                    i32::from(coefficients[2 + j]),
                                ));
                            }
                        }
                        output[written] = saturate16(round_shift(sum, 6));
                        written += 1;
                        index += self.ratio;
                    }
                    buffer.copy_within(chunk.len()..chunk.len() + order, 0);
                }
                self.fir32[..order].copy_from_slice(&buffer[..order]);
            }
        }
    }
}

// Three allpass sections per phase. The final negative coefficient has an
// implicit leading one; omission changes the transfer function completely.
fn upsample2(state: &mut [i32; 6], input: &[i16], output: &mut [i16]) {
    for (i, &sample) in input.iter().enumerate() {
        for (phase, coefficients) in [UP_0, UP_1].iter().enumerate() {
            let mut value = i32::from(sample) << 10;
            for (section, &coefficient) in coefficients.iter().enumerate() {
                let at = phase * 3 + section;
                let difference = value.wrapping_sub(state[at]);
                let mut product = mul_wb(difference, i32::from(coefficient));
                if section == 2 {
                    product = product.wrapping_add(difference);
                }
                let filtered = state[at].wrapping_add(product);
                state[at] = value.wrapping_add(product);
                value = filtered;
            }
            output[i * 2 + phase] = saturate16(round_shift(value, 10));
        }
    }
}

#[cfg(test)]
#[path = "resampler_tests.rs"]
mod tests;
