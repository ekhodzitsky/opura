//! Classic per-channel CELT comb postfilter with decoder frame scheduling.
//! Processed history and delayed old/current parameters follow pinned decoder.
//! Copyright (c) 2007-2008 CSIRO; 2007-2010 Xiph.Org Foundation;
//! 2008 Gregory Maxwell. See THIRD_PARTY_NOTICES.md.
use super::mdct_tables::WINDOW;
use std::fmt;

/// One of the three standard five-tap comb shapes.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Tapset {
    /// Standard tapset zero.
    #[default]
    Set0,
    /// Standard tapset one.
    Set1,
    /// Standard tapset two.
    Set2,
}
impl Tapset {
    fn gains(self) -> [i32; 3] {
        match self {
            Self::Set0 => [10048, 7112, 4248],
            Self::Set1 => [15200, 8784, 0],
            Self::Set2 => [26208, 3280, 0],
        }
    }
}
/// Invalid postfilter parameters or signal length; state remains unchanged.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum PostfilterError {
    /// Gain must be Q15 0..24576 and period 15..1023 (or zero for zero gain).
    InvalidParameters,
    /// Signal length must be 120/240/480/960 internal-rate samples.
    InvalidSignal,
}
impl fmt::Display for PostfilterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidParameters => "invalid classic CELT postfilter parameters",
            Self::InvalidSignal => "invalid CELT postfilter signal length",
        })
    }
}
impl std::error::Error for PostfilterError {}
/// Validated classic decoder comb parameters; default disables filtering.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PostfilterParams {
    period: u16,
    gain: u16,
    tapset: Tapset,
}
impl PostfilterParams {
    /// Accept a period 15..1023 and Q15 gain 0..24576; period zero is also
    /// accepted when gain is zero, matching the absent-postfilter packet field.
    pub fn new(period: u16, gain: u16, tapset: Tapset) -> Result<Self, PostfilterError> {
        if gain > 24576 || !((15..=1023).contains(&period) || period == 0 && gain == 0) {
            return Err(PostfilterError::InvalidParameters);
        }
        Ok(Self {
            period,
            gain,
            tapset,
        })
    }
    /// Pitch period in 48 kHz internal-rate samples, zero when absent.
    pub fn period(self) -> u16 {
        self.period
    }
    /// Nonnegative Q15 gain.
    pub fn gain(self) -> u16 {
        self.gain
    }
    /// Standard tap shape.
    pub fn tapset(self) -> Tapset {
        self.tapset
    }
    fn clamped(self) -> Self {
        Self {
            period: self.period.max(15),
            ..self
        }
    }
    fn coefficients(self) -> [i32; 3] {
        self.tapset
            .gains()
            .map(|g| (i32::from(self.gain) * g + 16384) >> 15)
    }
}
/// Postfiltered internal signal, before de-emphasis and signed-16 conversion.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FilteredFrame {
    samples: [i32; 960],
    length: usize,
}
impl FilteredFrame {
    /// Complete 120/240/480/960-sample postfiltered signal.
    pub fn samples(&self) -> &[i32] {
        &self.samples[..self.length]
    }
}
/// Per-channel processed history and normal decoder parameter scheduling.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PostfilterState {
    history: [i32; 1026],
    old: PostfilterParams,
    current: PostfilterParams,
}
impl Default for PostfilterState {
    fn default() -> Self {
        Self {
            history: [0; 1026],
            old: PostfilterParams::default(),
            current: PostfilterParams::default(),
        }
    }
}
impl PostfilterState {
    /// Clear signal history and both parameter generations.
    pub fn reset(&mut self) {
        *self = Self::default();
    }
    /// Old/current parameter generations used by the next frame's first 120 samples.
    pub fn parameters(&self) -> [PostfilterParams; 2] {
        [self.old, self.current]
    }
    /// Last 1026 processed signal values, oldest first.
    pub fn history(&self) -> &[i32; 1026] {
        &self.history
    }
    /// Filter a normal decoded frame using the newly decoded packet parameters.
    ///
    /// First 120 samples transition old->current. For longer frames the rest
    /// transitions current->new and then uses new constantly. A 120-sample call
    /// delays new parameters until the next frame. Validation precedes mutation.
    pub fn process(
        &mut self,
        input: &[i32],
        new: PostfilterParams,
    ) -> Result<FilteredFrame, PostfilterError> {
        let n = input.len();
        if !matches!(n, 120 | 240 | 480 | 960) {
            return Err(PostfilterError::InvalidSignal);
        }
        let old = self.old.clamped();
        let current = self.current.clamped();
        let mut buffer = [0i32; 1986];
        buffer[..1026].copy_from_slice(&self.history);
        buffer[1026..1026 + n].copy_from_slice(input);
        filter(&mut buffer, 1026, 120, old, current);
        if n > 120 {
            filter(&mut buffer, 1146, n - 120, current, new);
        }
        let mut result = FilteredFrame {
            samples: [0; 960],
            length: n,
        };
        result.samples[..n].copy_from_slice(&buffer[1026..1026 + n]);
        self.history.copy_from_slice(&buffer[n..n + 1026]);
        self.old = if n > 120 { new } else { current };
        self.current = new;
        Ok(result)
    }
}
fn filter(
    buffer: &mut [i32; 1986],
    base: usize,
    n: usize,
    old: PostfilterParams,
    new: PostfilterParams,
) {
    if old.gain == 0 && new.gain == 0 {
        return;
    }
    let old = old.clamped();
    let new = new.clamped();
    let overlap = if old == new { 0 } else { 120 };
    let g0 = old.coefficients();
    let g1 = new.coefficients();
    for (i, &window) in WINDOW.iter().enumerate().take(overlap) {
        let weight = (i32::from(window) * i32::from(window)) >> 15;
        let at = base + i;
        let mut value = buffer[at];
        for (params, gains, w) in [(old, g0, 32767 - weight), (new, g1, weight)] {
            let at_delay = at - usize::from(params.period);
            let terms = [
                buffer[at_delay],
                buffer[at_delay + 1].wrapping_add(buffer[at_delay - 1]),
                buffer[at_delay + 2].wrapping_add(buffer[at_delay - 2]),
            ];
            for j in 0..3 {
                value = value.wrapping_add(mul((w * gains[j]) >> 15, terms[j]));
            }
        }
        buffer[at] = value.wrapping_sub(3).clamp(-536870911, 536870911);
    }
    if new.gain == 0 {
        return;
    }
    for i in overlap..n {
        let at = base + i;
        let delayed = at - usize::from(new.period);
        let terms = [
            buffer[delayed],
            buffer[delayed + 1].wrapping_add(buffer[delayed - 1]),
            buffer[delayed + 2].wrapping_add(buffer[delayed - 2]),
        ];
        let mut value = buffer[at];
        for j in 0..3 {
            value = value.wrapping_add(mul(g1[j], terms[j]));
        }
        buffer[at] = value.wrapping_sub(1).clamp(-536870911, 536870911);
    }
}
fn mul(coefficient: i32, value: i32) -> i32 {
    ((i64::from(coefficient as i16) * i64::from(value)) >> 15) as i32
}
