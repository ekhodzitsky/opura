//! Native SILK inactivity hangover and fixed-point outer digital-silence DTX.
//! Adapted from Opus v1.6.1; see THIRD_PARTY_NOTICES.md.
use super::mono::PacketDuration;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Decision {
    Transmit,
    Silk,
    DigitalSilence,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct DtxState {
    pub enabled: bool,
    inactive: [u8; 2],
    packet: [bool; 2],
    quiet_ms_q1: u16,
    pub suppressed: bool,
}
impl DtxState {
    pub fn restart_side(&mut self) {
        self.inactive[1] = 0;
    }
    pub fn begin(&mut self, digital_silence: bool) {
        self.packet = [self.enabled && !digital_silence; 2];
    }
    // Called only for actually analyzed channels, including when DTX is off.
    // Omitted side channels retain their counter and packet's initial flag.
    pub fn frame(&mut self, channel: usize, active: bool) {
        if active {
            self.inactive[channel] = 0;
            self.packet[channel] = false;
        } else {
            self.inactive[channel] += 1;
            if self.inactive[channel] <= 10 {
                self.packet[channel] = false;
            } else if self.inactive[channel] > 30 {
                self.inactive[channel] = 10;
                self.packet[channel] = false;
            }
        }
    }
    pub fn finish(
        &mut self,
        duration: PacketDuration,
        channels: usize,
        digital_silence: bool,
    ) -> Decision {
        let decision = if self.packet[..channels].iter().all(|&flag| flag) {
            // Native SILK returns before the outer DTX clock is updated.
            Decision::Silk
        } else if self.enabled && digital_silence {
            self.quiet_ms_q1 += 2 * duration.milliseconds() as u16;
            if self.quiet_ms_q1 > 400 && self.quiet_ms_q1 <= 1200 {
                Decision::DigitalSilence
            } else {
                if self.quiet_ms_q1 > 1200 {
                    self.quiet_ms_q1 = 400;
                }
                Decision::Transmit
            }
        } else {
            self.quiet_ms_q1 = 0;
            Decision::Transmit
        };
        self.suppressed = decision != Decision::Transmit;
        decision
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::silk::{
        encoder::{
            mono::{MonoEncoder, MonoOptions},
            stereo::StereoEncoder,
        },
        pitch::SilkRate,
        resampler::OutputRate,
    };
    type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
    fn duration(ms: u32) -> PacketDuration {
        match ms {
            10 => PacketDuration::Ms10,
            20 => PacketDuration::Ms20,
            40 => PacketDuration::Ms40,
            _ => PacketDuration::Ms60,
        }
    }
    #[test]
    fn unchanged_native_vad_and_outer_dtx_clock_match() -> Result<()> {
        let mut vad = 0;
        let mut clock = 0;
        let mut refresh = 0;
        for line in include_str!("../../../tests/silk-encoder-dtx-fixtures.txt").lines() {
            let fields: Vec<_> = line.split_whitespace().collect();
            if fields[0] == "#" || fields[0] == "public" || fields[0] == "burg" {
                continue;
            }
            let v: Vec<u16> = fields[1..]
                .iter()
                .map(|s| s.parse())
                .collect::<std::result::Result<_, _>>()?;
            if fields[0] == "vad" {
                let mut s = DtxState::default();
                s.inactive[0] = v[0] as u8;
                s.packet[0] = v[1] != 0;
                s.frame(0, v[2] != 0);
                assert_eq!(u16::from(s.inactive[0]), v[3]);
                assert_eq!(s.packet[0], v[4] != 0);
                vad += 1;
                refresh += usize::from(v[0] == 30 && v[2] == 0);
            } else {
                assert_eq!(fields[0], "clock");
                let mut s = DtxState {
                    enabled: true,
                    quiet_ms_q1: v[0],
                    ..DtxState::default()
                };
                s.begin(v[2] == 0);
                s.frame(0, true); // Ensure this checks the outer path alone.
                let d = s.finish(duration(u32::from(v[1])), 1, v[2] == 0);
                assert_eq!(s.quiet_ms_q1, v[3]);
                assert_eq!(d != Decision::Transmit, v[4] != 0);
                clock += 1;
            }
        }
        assert_eq!(vad, 4220);
        assert_eq!(clock, 400);
        assert!(refresh > 20);
        Ok(())
    }
    #[test]
    fn public_native_suppression_schedule_matches_mono_and_stereo() -> Result<()> {
        let mut mono = MonoEncoder::new(OutputRate::Khz48, SilkRate::Khz16);
        let mut stereo = StereoEncoder::new(OutputRate::Khz48, SilkRate::Khz16);
        let mut calls = 0;
        let mut suppressed = 0;
        for line in include_str!("../../../tests/silk-encoder-dtx-fixtures.txt").lines() {
            let Some(rest) = line.strip_prefix("public ") else {
                continue;
            };
            let v: Vec<usize> = rest
                .split_whitespace()
                .map(|s| s.parse())
                .collect::<std::result::Result<_, _>>()?;
            let [channels, ms, f, enabled, expected]: [usize; 5] =
                v.try_into().map_err(|_| "invalid public row")?;
            if f == 0 {
                mono = MonoEncoder::new(OutputRate::Khz48, SilkRate::Khz16);
                stereo = StereoEncoder::new(OutputRate::Khz48, SilkRate::Khz16);
            }
            mono.set_dtx(enabled != 0);
            stereo.set_dtx(enabled != 0);
            let pcm: Vec<i16> = (0..48 * ms)
                .flat_map(|i| {
                    (0..channels).map(move |c| {
                        let phase = (f * 48 * ms + i) % 240;
                        if !(5..110).contains(&f) {
                            ((if phase < 120 { phase } else { 240 - phase }) as i32 - 60) as i16
                                * 200
                        } else if f < 55 {
                            0
                        } else {
                            ((i + c) % 2) as i16
                        }
                    })
                })
                .collect();
            let options = MonoOptions {
                complexity: 0,
                ..MonoOptions::default()
            };
            let actual = if channels == 1 {
                let packet = mono.encode(duration(ms as u32), &pcm, ms * 3, options)?;
                assert_eq!(mono.in_dtx(), packet.as_bytes().len() == 1);
                mono.in_dtx()
            } else {
                let packet = stereo.encode(duration(ms as u32), &pcm, ms * 6, options)?;
                assert_eq!(stereo.in_dtx(), packet.as_bytes().len() == 1);
                stereo.in_dtx()
            };
            assert_eq!(
                actual,
                expected != 0,
                "native {channels} channels/{ms}ms/frame{f}"
            );
            calls += 1;
            suppressed += usize::from(actual);
        }
        assert_eq!(calls, 2400);
        assert!(suppressed > 300);
        Ok(())
    }
    #[test]
    fn dtx_errors_preserve_state_and_reset_retains_settings() -> Result<()> {
        let mut mono = MonoEncoder::new(OutputRate::Khz48, SilkRate::Khz16);
        let mut stereo = StereoEncoder::new(OutputRate::Khz48, SilkRate::Khz16);
        mono.set_dtx(true);
        stereo.set_dtx(true);
        let options = MonoOptions::default();
        for _ in 0..30 {
            mono.encode(PacketDuration::Ms20, &[0; 960], 80, options)?;
            stereo.encode(PacketDuration::Ms20, &[0; 1920], 160, options)?;
        }
        assert!(mono.in_dtx() && stereo.in_dtx());
        let before_mono = mono.clone();
        let before_stereo = stereo.clone();
        assert!(
            mono.encode(PacketDuration::Ms20, &[0; 959], 80, options)
                .is_err()
        );
        assert!(
            stereo
                .encode(PacketDuration::Ms20, &[0; 1919], 160, options)
                .is_err()
        );
        assert!(
            mono.encode(PacketDuration::Ms20, &[i16::MAX; 960], 3, options)
                .is_err()
        );
        assert!(
            stereo
                .encode(PacketDuration::Ms20, &[i16::MAX; 1920], 3, options)
                .is_err()
        );
        assert_eq!(mono, before_mono);
        assert_eq!(stereo, before_stereo);
        mono.reset();
        stereo.reset();
        let mut fresh_mono = MonoEncoder::new(OutputRate::Khz48, SilkRate::Khz16);
        let mut fresh_stereo = StereoEncoder::new(OutputRate::Khz48, SilkRate::Khz16);
        fresh_mono.set_dtx(true);
        fresh_stereo.set_dtx(true);
        assert_eq!(mono, fresh_mono);
        assert_eq!(stereo, fresh_stereo);
        assert!(!mono.in_dtx() && !stereo.in_dtx());
        Ok(())
    }
    #[test]
    fn native_burg_preserves_signed_near_silence_residual() -> Result<()> {
        use crate::silk::{encoder::analysis::analyze_lpc, gains::FrameDuration};
        let row = include_str!("../../../tests/silk-encoder-dtx-fixtures.txt")
            .lines()
            .find_map(|line| line.strip_prefix("burg "))
            .ok_or("missing Burg probe")?;
        let v: Vec<i32> = row
            .split_whitespace()
            .map(|s| s.parse())
            .collect::<std::result::Result<_, _>>()?;
        let mut input = [0i16; 100];
        input[..3].fill(-1);
        let result = analyze_lpc(SilkRate::Khz8, FrameDuration::Ms10, &input, 63388)?;
        assert_eq!(result.residual_energy(), (v[0], v[1]));
        assert_eq!(result.coefficients_q16(), &v[2..]);
        assert!(v[0] < 0);
        Ok(())
    }
}
