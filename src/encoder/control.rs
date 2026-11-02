//! Native scalar mode/channel/bandwidth/FEC policy, without audio processing.
//! Copyright (c) 2010-2011 Xiph.Org Foundation, Skype Limited.
//! See THIRD_PARTY_NOTICES.md.
use super::{FrontendError, q15, valid_duration};
use crate::{Bandwidth, Mode, celt::input::InputRate};

/// Application constraints and native speech bias.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Application {
    /// General audio, with a scalar default voice estimate of48/127.
    Audio,
    /// Conversational voice, with a default estimate of115/127.
    Voip,
    /// CELT-only operation without the normal4ms input compensation delay.
    RestrictedLowDelay,
    /// Speech-only operation, at least10ms and at most wideband.
    RestrictedSilk,
    /// CELT-only operation.
    RestrictedCelt,
}
/// Scalar speech/music estimate. No floating-point classifier is used.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Signal {
    /// Use the application's scalar default.
    #[default]
    Auto,
    /// Speech estimate127/127.
    Voice,
    /// Speech estimate0/127.
    Music,
}
/// Requested FEC policy; actual selection also depends on rate, loss and mode.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Fec {
    /// Disable in-band recovery.
    #[default]
    Disabled,
    /// Prefer speech modes when loss justifies recovery.
    Enabled,
    /// Avoid forcing speech mode for confidently classified music.
    ExceptMusic,
}
/// User controls consumed by the normal-admission decision kernel.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Controls {
    /// Immutable stream application; changing it requires separate owner handling.
    pub application: Application,
    /// Speech/music hint.
    pub signal: Signal,
    /// Optional explicit mode, subject to application/frame/band constraints.
    pub mode: Option<Mode>,
    /// Encoded channel count override; must be1 or2 and no greater than input.
    pub channels: Option<usize>,
    /// Maximum automatic bandwidth.
    pub max_bandwidth: Bandwidth,
    /// Explicit bandwidth overrides the automatic maximum, as in native policy.
    pub bandwidth: Option<Bandwidth>,
    /// VBR or constrained VBR; false means CBR.
    pub variable: bool,
    /// Native complexity0..=10.
    pub complexity: u8,
    /// Expected packet loss0..=100 percent.
    pub loss_percent: u8,
    /// In-band recovery request.
    pub fec: Fec,
    /// Discontinuous transmission request.
    pub dtx: bool,
}
impl Default for Controls {
    fn default() -> Self {
        Self {
            application: Application::Audio,
            signal: Signal::Auto,
            mode: None,
            channels: None,
            max_bandwidth: Bandwidth::Fullband,
            bandwidth: None,
            variable: true,
            complexity: 9,
            loss_percent: 0,
            fec: Fec::Disabled,
            dtx: false,
        }
    }
}
/// Snapshot supplied by the actual packet/layer owner before selecting a frame.
///
/// These fields are not advanced by [`select`]. In particular, layer bandwidth
/// readiness, FEC history and transition completion must come from real encoding.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct History {
    /// True before the first successfully encoded frame.
    pub first: bool,
    /// Committed logical previous mode, including any outgoing CELT tail.
    pub previous_mode: Option<Mode>,
    /// Previous wire channels, or0 before a packet has been encoded.
    pub previous_channels: usize,
    /// Current wire channels, used for mono/stereo hysteresis.
    pub channels: usize,
    /// Current selected bandwidth.
    pub bandwidth: Bandwidth,
    /// Previous automatic bandwidth, absent before initial selection.
    pub auto_bandwidth: Option<Bandwidth>,
    /// SILK has granted bandwidth-switch permission.
    pub allow_bandwidth_switch: bool,
    /// SILK is wideband with its variable low-pass filter fully disabled.
    pub wideband_ready: bool,
    /// Previous selected in-band FEC state.
    pub fec: bool,
    /// Previous SILK stereo-to-mono request.
    pub to_mono: bool,
}
impl History {
    /// Initial native frontend snapshot for a mono/stereo stream.
    pub fn new(channels: usize) -> Result<Self, FrontendError> {
        if !matches!(channels, 1 | 2) {
            return Err(FrontendError::InvalidGeometry);
        }
        Ok(Self {
            first: true,
            previous_mode: None,
            previous_channels: 0,
            channels,
            bandwidth: Bandwidth::Fullband,
            auto_bandwidth: None,
            allow_bandwidth_switch: false,
            wideband_ready: false,
            fec: false,
            to_mono: false,
        })
    }
}
/// Input geometry, analysis and already-normalized packet budget.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Frame {
    /// Raw input rate.
    pub rate: InputRate,
    /// Raw input channels1 or2.
    pub channels: usize,
    /// Standard duration on the48kHz clock,2.5..=120ms.
    pub samples48: usize,
    /// TOC-inclusive bits/second, after physical-cap and CBR-byte rounding.
    /// Bounded to1500800 to include native rounding at the maximum user rate.
    pub bitrate: u32,
    /// Final TOC-inclusive byte cap,1..=61248. Normal policy needs at least3.
    pub packet_bytes: usize,
    /// Raw input is digital silence at the selected input depth.
    pub digital_silence: bool,
    /// Stereo width0..=32767, from raw input analysis (or0 for mono).
    pub stereo_width_q15: i16,
}
/// Pre-encode policy results, not proof that a packet/transition was encoded.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Decision {
    /// Primary mode; an outgoing transition may retain the previous speech mode.
    pub mode: Mode,
    /// Primary wire channels, including delayed stereo-to-mono handling.
    pub channels: usize,
    /// Selected band after FEC/application/Nyquist constraints.
    pub bandwidth: Bandwidth,
    /// Automatic-band hysteresis state before explicit/budget caps.
    pub auto_bandwidth: Option<Bandwidth>,
    /// In-band recovery selected for this primary mode and rate.
    pub fec: bool,
    /// Request native SILK smooth stereo-to-mono processing.
    pub to_mono: bool,
    /// Request transition redundancy; encoding may still reject it for budget.
    pub redundancy: bool,
    /// Incoming CELT-to-speech redundancy direction.
    pub celt_to_silk: bool,
    /// Outgoing speech-to-CELT tail; logical mode becomes CELT after success.
    pub to_celt: bool,
    /// Reset SILK and prefill its history before encoding the primary speech.
    pub prefill_silk: bool,
    /// Final effective rate after channel and mode corrections.
    pub equivalent_rate: i32,
    /// Speech estimate0..=127.
    pub voice_q7: u8,
    /// Use SILK's DTX path; generalized digital-silence DTX is handled outside.
    pub silk_dtx: bool,
}

const BANDS: [Bandwidth; 5] = [
    Bandwidth::Narrowband,
    Bandwidth::Mediumband,
    Bandwidth::Wideband,
    Bandwidth::SuperWideband,
    Bandwidth::Fullband,
];
fn band_index(band: Bandwidth) -> usize {
    match band {
        Bandwidth::Narrowband => 0,
        Bandwidth::Mediumband => 1,
        Bandwidth::Wideband => 2,
        Bandwidth::SuperWideband => 3,
        Bandwidth::Fullband => 4,
    }
}
fn equivalent_rate(frame: Frame, controls: Controls, channels: usize, mode: Option<Mode>) -> i32 {
    let mut rate = frame.bitrate as i32;
    let fps = 48000 / frame.samples48 as i32;
    if fps > 50 {
        rate -= (40 * channels as i32 + 20) * (fps - 50);
    }
    if !controls.variable {
        rate -= rate / 12;
    }
    rate = rate * (90 + i32::from(controls.complexity)) / 100;
    let loss = i32::from(controls.loss_percent);
    match mode {
        Some(Mode::Silk | Mode::Hybrid) => {
            if controls.complexity < 2 {
                rate = rate * 4 / 5;
            }
            rate -= rate * loss / (6 * loss + 10);
        }
        Some(Mode::Celt) => {
            if controls.complexity < 5 {
                rate = rate * 9 / 10;
            }
        }
        None => rate -= rate * loss / (12 * loss + 20),
    }
    rate
}

/// Select native scalar normal-admission controls, without mutating history.
///
/// The outer encoder must first normalize user rate/CBR bytes, analyze raw
/// stereo input, and handle tiny-budget PLC. It must then actually process the
/// requested layer resets/prefills/redundancy and commit real post-encode state.
/// This function alone does not implement automatic PCM encoding. No float
/// classifier, LFE, DRED, multistream or low-bit-depth analysis is included.
pub fn select(
    frame: Frame,
    controls: Controls,
    history: History,
) -> Result<Decision, FrontendError> {
    if !valid_duration(frame.samples48) || !matches!(frame.channels, 1 | 2) {
        return Err(FrontendError::InvalidGeometry);
    }
    if controls.complexity > 10
        || controls.loss_percent > 100
        || frame.stereo_width_q15 < 0
        || controls
            .channels
            .is_some_and(|c| c == 0 || c > frame.channels)
        || !(1..=frame.channels).contains(&history.channels)
        || history.previous_channels > frame.channels
        || frame.bitrate > 1500800
        || frame.packet_bytes == 0
        || frame.packet_bytes > 61248
        || (controls.application == Application::RestrictedSilk && frame.samples48 < 480)
    {
        return Err(FrontendError::InvalidControl);
    }
    let fps = 48000 / frame.samples48;
    if frame.packet_bytes < 3
        || frame.bitrate < (3 * fps * 8) as u32
        || (fps < 50 && (frame.packet_bytes * fps < 300 || frame.bitrate < 2400))
    {
        return Err(FrontendError::InsufficientBudget);
    }
    let max_rate = frame.packet_bytes as u64 * 8 * 48000 / frame.samples48 as u64;
    if u64::from(frame.bitrate) > max_rate {
        return Err(FrontendError::InvalidControl);
    }
    let voice = match controls.signal {
        Signal::Voice => 127,
        Signal::Music => 0,
        Signal::Auto if controls.application == Application::Voip => 115,
        Signal::Auto => 48,
    };
    let voice_squared = voice * voice;
    let initial_rate = equivalent_rate(frame, controls, frame.channels, None);
    let mut channels = if let Some(channels) = controls.channels {
        channels
    } else if frame.channels == 2 {
        let threshold = 17000
            + ((voice_squared * 2000) >> 14)
            + if history.channels == 2 { -1000 } else { 1000 };
        if initial_rate > threshold { 2 } else { 1 }
    } else {
        1
    };
    let channel_rate = equivalent_rate(frame, controls, channels, None);
    let silk_dtx = controls.dtx && !frame.digital_silence;
    let mut mode = match controls.application {
        Application::RestrictedSilk => Mode::Silk,
        Application::RestrictedLowDelay | Application::RestrictedCelt => Mode::Celt,
        _ => {
            if let Some(mode) = controls.mode {
                mode
            } else {
                let width = i32::from(frame.stereo_width_q15);
                // Retain each native Q15 truncation, including the music threshold.
                let mode_voice = q15(32767 - width, 64000) + q15(width, 44000);
                let mode_music = q15(32767 - width, 10000) + q15(width, 10000);
                let mut threshold =
                    mode_music + ((voice_squared * (mode_voice - mode_music)) >> 14);
                if controls.application == Application::Voip {
                    threshold += 8000;
                }
                threshold += match history.previous_mode {
                    Some(Mode::Celt) => -4000,
                    Some(_) => 4000,
                    None => 0,
                };
                let mut mode = if channel_rate >= threshold {
                    Mode::Celt
                } else {
                    Mode::Silk
                };
                if (controls.fec != Fec::Disabled
                    && i32::from(controls.loss_percent) > ((128 - voice) >> 4)
                    && (controls.fec != Fec::ExceptMusic || voice > 25))
                    || (silk_dtx && voice > 100)
                {
                    mode = Mode::Silk;
                }
                let threshold_bits = (if fps > 50 { 9000 } else { 6000 }) * frame.samples48 / 48000;
                if frame.packet_bytes < threshold_bits / 8 {
                    mode = Mode::Celt;
                }
                mode
            }
        }
    };
    if frame.samples48 < 480 {
        mode = Mode::Celt;
    }
    let mut redundancy = false;
    let mut celt_to_silk = false;
    let mut to_celt = false;
    if let Some(previous) = history.previous_mode {
        if (mode == Mode::Celt) != (previous == Mode::Celt) {
            redundancy = true;
            celt_to_silk = mode != Mode::Celt;
            if !celt_to_silk {
                if frame.samples48 >= 480 {
                    mode = previous;
                    to_celt = true;
                } else {
                    redundancy = false;
                }
            }
        }
    }
    let to_mono = channels == 1
        && history.previous_channels == 2
        && !history.to_mono
        && mode != Mode::Celt
        && history.previous_mode != Some(Mode::Celt);
    if to_mono {
        channels = 2;
    }
    let rate = equivalent_rate(frame, controls, channels, Some(mode));
    let prefill_silk = mode != Mode::Celt && history.previous_mode == Some(Mode::Celt);
    let mut band = band_index(history.bandwidth);
    let mut auto_bandwidth = history.auto_bandwidth;
    if mode == Mode::Celt || history.first || history.allow_bandwidth_switch {
        const MUSIC: [i32; 8] = [9000, 700, 9000, 700, 11000, 1000, 12000, 2000];
        const VOICE: [i32; 8] = [9000, 700, 9000, 700, 13500, 1000, 14000, 2000];
        band = 4;
        while band > 0 {
            let index = 2 * (band - 1);
            let mut threshold =
                MUSIC[index] + ((voice_squared * (VOICE[index] - MUSIC[index])) >> 14);
            let hysteresis =
                MUSIC[index + 1] + ((voice_squared * (VOICE[index + 1] - MUSIC[index + 1])) >> 14);
            if !history.first {
                threshold += if history
                    .auto_bandwidth
                    .is_some_and(|b| band_index(b) >= band)
                {
                    -hysteresis
                } else {
                    hysteresis
                };
            }
            if rate >= threshold {
                break;
            }
            band -= 1;
        }
        if band == 1 {
            band = 2;
        }
        auto_bandwidth = Some(BANDS[band]);
        if !history.first && mode != Mode::Celt && !history.wideband_ready {
            band = band.min(2);
        }
    }
    band = band.min(band_index(controls.max_bandwidth));
    if let Some(explicit) = controls.bandwidth {
        band = band_index(explicit);
    }
    if mode != Mode::Celt && max_rate < 15000 {
        band = band.min(2);
    }
    band = band.min(match frame.rate {
        InputRate::Khz8 => 0,
        InputRate::Khz12 => 1,
        InputRate::Khz16 => 2,
        InputRate::Khz24 => 3,
        InputRate::Khz48 => 4,
    });
    let mut fec = false;
    if controls.fec != Fec::Disabled && controls.loss_percent > 0 && mode != Mode::Celt {
        const THRESHOLDS: [i32; 5] = [12000, 14000, 16000, 20000, 22000];
        let original = band;
        loop {
            let base = THRESHOLDS[band] + if history.fec { -1000 } else { 1000 };
            let threshold = (base * (125 - i32::from(controls.loss_percent.min(25))) * 655) >> 16;
            if rate > threshold {
                fec = true;
                break;
            }
            if controls.loss_percent <= 5 {
                break;
            }
            if band == 0 {
                band = original;
                break;
            }
            band -= 1;
        }
    }
    if mode == Mode::Celt && band == 1 {
        band = 2;
    }
    if controls.application == Application::RestrictedSilk {
        band = band.min(2);
    }
    if mode == Mode::Silk && band > 2 {
        mode = Mode::Hybrid;
    }
    if mode == Mode::Hybrid && band <= 2 {
        mode = Mode::Silk;
    }
    Ok(Decision {
        mode,
        channels,
        bandwidth: BANDS[band],
        auto_bandwidth,
        fec,
        to_mono,
        redundancy,
        celt_to_silk,
        to_celt,
        prefill_silk,
        equivalent_rate: rate,
        voice_q7: voice as u8,
        silk_dtx,
    })
}

#[cfg(test)]
mod tests;
