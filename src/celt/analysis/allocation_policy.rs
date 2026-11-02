//! Native stereo L1 modelling, intensity hysteresis and spectral trim analysis.
use super::{AnalysisError, FrameSize, allocation_tables::*};
use crate::celt::{energy::BandLayout, math, output_tables::BANDS};

/// Byte-derived equivalent bitrate with native short-frame coding overhead.
/// Channels must be 1/2 and payload 2..1275 bytes, excluding TOC. No outer bitrate
/// cap is applied; low-budget short frames can have a negative equivalent rate.
pub fn equivalent_rate(
    size: FrameSize,
    channels: usize,
    payload_bytes: usize,
) -> Result<i32, AnalysisError> {
    if !(1..=2).contains(&channels) {
        return Err(AnalysisError::InvalidGeometry);
    }
    if !(2..=1275).contains(&payload_bytes) {
        return Err(AnalysisError::InvalidAllocationParameters);
    }
    let lm = size.lm();
    Ok(((payload_bytes as i32 * 8 * 50) << (3 - lm))
        - (40 * channels as i32 + 20) * ((400 >> lm) - 50))
}
/// Original L1 choice of independent L/R coding rather than M/S coding.
/// Input is exactly two planar normalized Q24 channels. Only the first 13 bands
/// are inspected, with squared norm <=5/4 Q48 per band/channel. Remaining bins
/// are ignored. LM0 diagnostics are allowed; packet policy forces M/S there.
/// No allocation or input mutation occurs, including on errors.
pub fn analyze_stereo(size: FrameSize, normalized: &[i32]) -> Result<bool, AnalysisError> {
    if normalized.len() != 2 * size.samples() {
        return Err(AnalysisError::InvalidGeometry);
    }
    validate_norm(size, normalized, 2, 13)?;
    let (lr, ms) = stereo_costs(size, normalized);
    Ok(ms > lr)
}
fn stereo_costs(size: FrameSize, values: &[i32]) -> (i32, i32) {
    let n = size.samples();
    let lm = size.lm();
    let stop = usize::from(BANDS[13]) << lm;
    let mut lr = MODEL_EPSILON;
    let mut ms = MODEL_EPSILON;
    for i in 0..stop {
        let l = values[i] >> 10;
        let r = values[n + i] >> 10;
        lr += l.abs() + r.abs();
        ms += (l + r).abs() + (l - r).abs();
    }
    ms = ((i64::from(MID_SCALE_Q15) * i64::from(ms)) >> 15) as i32;
    let base = i32::from(BANDS[13]) << (lm + 1);
    let theta = if lm <= 1 { 5 } else { 13 };
    (
        ((i64::from(base) * i64::from(lr)) >> 15) as i32,
        ((i64::from(base + theta) * i64::from(ms)) >> 15) as i32,
    )
}
/// Checked persistent intensity-stereo boundary before retained-band clipping.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct IntensityState {
    band: u8,
}
impl IntensityState {
    /// Restore a native boundary in 0..21.
    pub fn from_band(band: u8) -> Result<Self, AnalysisError> {
        if band > 21 {
            return Err(AnalysisError::InvalidAllocationParameters);
        }
        Ok(Self { band })
    }
    /// Last selected boundary before allocation's retained-band clipping.
    pub fn band(self) -> usize {
        usize::from(self.band)
    }
    /// Restore native initial boundary zero.
    pub fn reset(&mut self) {
        *self = Self::default();
    }
    /// Select from equivalent kb/s with native strict hysteresis, then clamp to
    /// the validated layout interval. Empty intervals are permitted. Channel
    /// count does not affect this rate-only model; packet policy calls it for stereo.
    pub fn select(&mut self, rate_kbps: i16, layout: BandLayout) -> usize {
        let previous = self.band();
        let mut band = 0;
        while band < 21 && rate_kbps >= INTENSITY_THRESHOLDS[band] {
            band += 1;
        }
        if band > previous
            && rate_kbps < INTENSITY_THRESHOLDS[previous] + INTENSITY_HYSTERESIS[previous]
        {
            band = previous;
        }
        if band < previous
            && rate_kbps > INTENSITY_THRESHOLDS[previous - 1] - INTENSITY_HYSTERESIS[previous - 1]
        {
            band = previous;
        }
        self.band = band.clamp(layout.bands().start, layout.bands().end) as u8;
        self.band()
    }
}
/// Immutable native trim inputs besides normalized spectra and Q24 band logs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TrimOptions {
    /// One/two planar channels.
    pub channels: usize,
    /// Exclusive band end: 2..21 for mono, 8..21 for stereo.
    pub end: usize,
    /// Desired intensity boundary in 0..end; mono ignores the value.
    pub intensity: usize,
    /// Equivalent bits/second, including native short-frame overhead adjustment.
    pub equivalent_rate: i32,
    /// Native temporal estimate in Q14, restricted to 0..16384.
    pub tf_estimate: i16,
    /// Surround tilt adjustment in Q24, restricted to [-4,4].
    pub surround_trim: i32,
}
impl Default for TrimOptions {
    fn default() -> Self {
        Self {
            channels: 1,
            end: 21,
            intensity: 21,
            equivalent_rate: 80000,
            tf_estimate: 0,
            surround_trim: 0,
        }
    }
}
/// Native quantized trim and fixed-point diagnostics before final clamping.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TrimAnalysis {
    index: u8,
    trim: i16,
    tilt: i32,
}
impl TrimAnalysis {
    /// Allocation trim index in 0..10.
    pub fn index(self) -> u8 {
        self.index
    }
    /// Native trim before final rounding/clamping, in Q8.
    pub fn trim_q8(self) -> i16 {
        self.trim
    }
    /// Averaged native spectral tilt, in Q19.
    pub fn spectral_tilt_q19(self) -> i32 {
        self.tilt
    }
}
/// Checked native mid/side saving history in Q8 (initially zero).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TrimState {
    saving: u16,
}
impl TrimState {
    /// Restore checked saving history in 0..1280 Q8.
    pub fn from_saving(saving: u16) -> Result<Self, AnalysisError> {
        if saving > 1280 {
            return Err(AnalysisError::InvalidAllocationParameters);
        }
        Ok(Self { saving })
    }
    /// Current Q8 mid/side saving estimate.
    pub fn stereo_saving(self) -> u16 {
        self.saving
    }
    /// Clear the saving history for a fresh stream.
    pub fn reset(&mut self) {
        *self = Self::default();
    }
    /// Analyze trim and commit saving only on success. Call when a trim symbol
    /// is actually available; packet integration must preserve history otherwise.
    ///
    /// Normalized input length is channels*size.samples(). Stereo inspects the
    /// first max(8,intensity) bands, each squared norm <=5/4 Q48; mono ignores
    /// coefficient values. Logs inspect active channels and bands `0..end-1` only,
    /// restricted to [-32,32] Q24. The mathematical final weighted tilt sum after
    /// input>>5 must fit signed32. Unsupported magnitudes/accumulations reject
    /// without changing state or inputs. Other fields use the documented bounds.
    /// No allocation occurs; inactive channels/bins/logs are ignored.
    pub fn process(
        &mut self,
        size: FrameSize,
        normalized: &[i32],
        logs: &[[i32; 21]; 2],
        options: TrimOptions,
    ) -> Result<TrimAnalysis, AnalysisError> {
        let c = options.channels;
        let end = options.end;
        if !(1..=2).contains(&c) || normalized.len() != c * size.samples() {
            return Err(AnalysisError::InvalidGeometry);
        }
        if !(2..=21).contains(&end) || (c == 2 && end < 8) {
            return Err(AnalysisError::InvalidBands);
        }
        if options.intensity > end
            || !(0..=16384).contains(&options.tf_estimate)
            || !(-(4 << 24)..=4 << 24).contains(&options.surround_trim)
        {
            return Err(AnalysisError::InvalidAllocationParameters);
        }
        for channel in &logs[..c] {
            if channel[..end - 1]
                .iter()
                .any(|&x| !(-(32 << 24)..=32 << 24).contains(&x))
            {
                return Err(AnalysisError::ArithmeticOverflow);
            }
        }
        if c == 2 {
            validate_norm(size, normalized, 2, 8.max(options.intensity))?;
        }
        let mut next = *self;
        let mut trim = if options.equivalent_rate < 64000 {
            4 << 8
        } else if options.equivalent_rate < 80000 {
            (4 << 8) + 16 * ((options.equivalent_rate - 64000) >> 10)
        } else {
            5 << 8
        };
        if c == 2 {
            let mut sum = 0;
            for i in 0..8 {
                sum += correlation(size, normalized, i) >> 18;
            }
            sum = ((4096 * sum) >> 15).abs().min(1024);
            let mut minimum = sum;
            for i in 8..options.intensity {
                minimum = minimum.min((correlation(size, normalized, i) >> 18).abs());
            }
            let (log, log2) = correlation_logs(sum, minimum)?;
            trim += (-1024).max((24576 * log) >> 15);
            next.saving = u16::try_from((i32::from(self.saving) + 64).min(-(log2 >> 1)))
                .map_err(|_| AnalysisError::ArithmeticOverflow)?;
        }
        let mut total = 0i64;
        for channel in &logs[..c] {
            for (i, &value) in channel[..end - 1].iter().enumerate() {
                total += i64::from(value >> 5) * (2 + 2 * i as i64 - end as i64);
            }
        }
        let tilt = i32::try_from(total).map_err(|_| AnalysisError::ArithmeticOverflow)?
            / (c * (end - 1)) as i32;
        trim -= (((tilt + (1 << 19)) >> 11) / 6).clamp(-512, 512);
        trim -= options.surround_trim >> 16;
        trim -= 2 * (i32::from(options.tf_estimate) >> 6);
        let raw = i16::try_from(trim).map_err(|_| AnalysisError::ArithmeticOverflow)?;
        let result = TrimAnalysis {
            index: ((trim + 128) >> 8).clamp(0, 10) as u8,
            trim: raw,
            tilt,
        };
        *self = next;
        Ok(result)
    }
}
fn correlation(size: FrameSize, values: &[i32], band: usize) -> i32 {
    let n = size.samples();
    let start = usize::from(BANDS[band]) << size.lm();
    let stop = usize::from(BANDS[band + 1]) << size.lm();
    let sum: i64 = values[start..stop]
        .iter()
        .zip(&values[n + start..n + stop])
        .map(|(&l, &r)| i64::from(l) * i64::from(r))
        .sum();
    (sum >> 20) as i32
}
fn correlation_logs(sum: i32, minimum: i32) -> Result<(i32, i32), AnalysisError> {
    let raw = i32::from(math::log2(CORRELATION_GUARD_Q20 - sum * sum)?);
    let raw2 = (raw >> 1).max(i32::from(math::log2(
        CORRELATION_GUARD_Q20 - minimum * minimum,
    )?));
    Ok(((raw - (6 << 10) + 2) >> 2, (raw2 - (6 << 10) + 2) >> 2))
}
fn validate_norm(
    size: FrameSize,
    values: &[i32],
    channels: usize,
    end: usize,
) -> Result<(), AnalysisError> {
    let n = size.samples();
    for c in 0..channels {
        for i in 0..end {
            let input = &values[c * n + (usize::from(BANDS[i]) << size.lm())
                ..c * n + (usize::from(BANDS[i + 1]) << size.lm())];
            let energy: i128 = input.iter().map(|&x| i128::from(x) * i128::from(x)).sum();
            if energy > 5 * (1i128 << 46) {
                return Err(AnalysisError::ArithmeticOverflow);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn hash(values: impl IntoIterator<Item = i32>) -> u64 {
        let mut hash = 14695981039346656037u64;
        for value in values {
            for byte in value.to_le_bytes() {
                hash = (hash ^ u64::from(byte)).wrapping_mul(1099511628211);
            }
        }
        hash
    }
    #[test]
    fn original_correlation_logs_equivalent_rates_and_intensity_states_match()
    -> Result<(), Box<dyn std::error::Error>> {
        let sizes = [
            FrameSize::Ms2_5,
            FrameSize::Ms5,
            FrameSize::Ms10,
            FrameSize::Ms20,
        ];
        let layout = BandLayout::new(2, 0, 21)?;
        let mut counts = [0; 3];
        let mut headers = [false; 4];
        for line in
            include_str!("../../../tests/celt-allocation-analysis-math-fixtures.txt").lines()
        {
            if line.starts_with('#') {
                continue;
            }
            let f: Vec<_> = line.split_whitespace().collect();
            match f[0] {
                "constants" => {
                    let values: Vec<i32> =
                        f[1..].iter().map(|s| s.parse()).collect::<Result<_, _>>()?;
                    assert_eq!(
                        values,
                        [
                            MODEL_EPSILON,
                            MID_SCALE_Q15,
                            CORRELATION_GUARD_Q20,
                            i32::from(math::log2(CORRELATION_GUARD_Q20)?)
                        ]
                    );
                    headers[0] = true;
                }
                "defaults" => {
                    let values: Vec<usize> =
                        f[1..].iter().map(|s| s.parse()).collect::<Result<_, _>>()?;
                    assert_eq!(
                        values,
                        [
                            usize::from(TrimState::default().stereo_saving()),
                            IntensityState::default().band()
                        ]
                    );
                    headers[1] = true;
                }
                "thresholds" | "hysteresis" => {
                    let values: Vec<i16> =
                        f[1..].iter().map(|s| s.parse()).collect::<Result<_, _>>()?;
                    let which = usize::from(f[0] == "hysteresis");
                    assert_eq!(values, [INTENSITY_THRESHOLDS, INTENSITY_HYSTERESIS][which]);
                    headers[2 + which] = true;
                }
                "correlation" => {
                    let sum = f[1].parse::<i32>()?;
                    let count = f[2].parse::<i32>()?;
                    let mut values = Vec::new();
                    for minimum in 0..count {
                        let (l, r) = correlation_logs(sum, minimum)?;
                        values.extend([l, r, -(r >> 1)]);
                    }
                    assert_eq!(hash(values), f[3].parse::<u64>()?, "correlation sum{sum}");
                    counts[0] += count as usize;
                }
                "equivalent" => {
                    let size = sizes[f[1].parse::<usize>()?];
                    let channels = f[2].parse::<usize>()?;
                    let start = f[3].parse::<usize>()?;
                    let count = f[4].parse::<usize>()?;
                    let values: Vec<i32> = (start..start + count)
                        .map(|b| equivalent_rate(size, channels, b))
                        .collect::<Result<_, _>>()?;
                    assert_eq!(hash(values), f[5].parse::<u64>()?);
                    counts[1] += count;
                }
                "intensity" => {
                    let previous = f[1].parse::<u8>()?;
                    let start = f[2].parse::<i32>()?;
                    let count = f[3].parse::<i32>()?;
                    let mut values = Vec::new();
                    for rate in start..start + count {
                        let mut state = IntensityState::from_band(previous)?;
                        let band = state.select(i16::try_from(rate)?, layout);
                        assert_eq!(band, state.band());
                        values.push(band as i32);
                    }
                    assert_eq!(
                        hash(values),
                        f[4].parse::<u64>()?,
                        "intensity previous{previous} rate{start}"
                    );
                    counts[2] += count as usize;
                }
                _ => return Err("unknown allocation scalar row".into()),
            }
        }
        assert_eq!(headers, [true; 4]);
        assert_eq!(counts, [525825, 10192, 1441792]);
        Ok(())
    }
}

#[cfg(test)]
mod stereo_tests {
    use super::*;
    #[test]
    fn original_stereo_l1_decisions_and_costs_match() -> Result<(), Box<dyn std::error::Error>> {
        let sizes = [
            FrameSize::Ms2_5,
            FrameSize::Ms5,
            FrameSize::Ms10,
            FrameSize::Ms20,
        ];
        let mut lines = include_str!("../../../tests/celt-stereo-analysis-fixtures.txt")
            .lines()
            .filter(|l| !l.starts_with('#'));
        let mut frames = 0;
        let mut dual = 0;
        let mut ties = 0;
        while let Some(line) = lines.next() {
            let f: Vec<_> = line.split_whitespace().collect();
            assert_eq!(f[0], "frame");
            let size = sizes[f[1].parse::<usize>()?];
            let input: Vec<i32> = f[2..].iter().map(|s| s.parse()).collect::<Result<_, _>>()?;
            let original = input.clone();
            let output: Vec<i32> = lines
                .next()
                .ok_or("stereo row")?
                .split_whitespace()
                .skip(1)
                .map(|s| s.parse())
                .collect::<Result<_, _>>()?;
            assert_eq!(output.len(), 3);
            assert_eq!(
                analyze_stereo(size, &input)?,
                output[0] != 0,
                "decision frame{frames}"
            );
            let (lr, ms) = stereo_costs(size, &input);
            assert_eq!([lr, ms], output[1..], "costs frame{frames}");
            assert_eq!(input, original);
            dual += usize::from(output[0] != 0);
            ties += usize::from(lr == ms);
            frames += 1;
        }
        eprintln!("stereo frames{frames} dual{dual} ties{ties}");
        assert_eq!(frames, 2016);
        assert!(dual > 100 && ties > 100);
        Ok(())
    }
}
