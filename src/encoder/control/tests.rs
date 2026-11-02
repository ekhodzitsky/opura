use super::*;
type TestResult = Result<(), Box<dyn std::error::Error>>;

fn mode(v: i32) -> Option<Mode> {
    match v {
        -1 => None,
        0 => Some(Mode::Silk),
        1 => Some(Mode::Hybrid),
        2 => Some(Mode::Celt),
        _ => panic!("fixture mode"),
    }
}
fn mode_number(v: Mode) -> i32 {
    match v {
        Mode::Silk => 0,
        Mode::Hybrid => 1,
        Mode::Celt => 2,
    }
}
fn rate(hz: i32) -> InputRate {
    match hz {
        8000 => InputRate::Khz8,
        12000 => InputRate::Khz12,
        16000 => InputRate::Khz16,
        24000 => InputRate::Khz24,
        48000 => InputRate::Khz48,
        _ => panic!("fixture rate"),
    }
}
#[test]
fn unchanged_native_decisions() -> TestResult {
    let mut count = 0;
    let mut modes = [0; 3];
    let mut transitions = [0; 4];
    for line in include_str!("../../../tests/opus-frontend-fixtures.txt")
        .lines()
        .filter(|l| !l.starts_with('#'))
    {
        let v: Vec<i32> = line
            .split_whitespace()
            .map(str::parse)
            .collect::<Result<_, _>>()?;
        assert_eq!(v.len(), 41);
        let frame = Frame {
            rate: rate(v[0]),
            channels: v[1] as usize,
            samples48: v[2] as usize,
            bitrate: v[3] as u32,
            packet_bytes: v[4] as usize,
            digital_silence: v[16] != 0,
            stereo_width_q15: v[17] as i16,
        };
        let controls = Controls {
            application: [
                Application::Audio,
                Application::Voip,
                Application::RestrictedLowDelay,
                Application::RestrictedSilk,
                Application::RestrictedCelt,
            ][v[5] as usize],
            signal: [Signal::Auto, Signal::Voice, Signal::Music][v[6] as usize],
            mode: mode(v[7]),
            channels: if v[8] == 0 { None } else { Some(v[8] as usize) },
            max_bandwidth: BANDS[v[9] as usize],
            bandwidth: if v[10] < 0 {
                None
            } else {
                Some(BANDS[v[10] as usize])
            },
            variable: v[11] != 0,
            complexity: v[12] as u8,
            loss_percent: v[13] as u8,
            fec: [Fec::Disabled, Fec::Enabled, Fec::ExceptMusic][v[14] as usize],
            dtx: v[15] != 0,
        };
        let history = History {
            first: v[18] != 0,
            previous_mode: mode(v[19]),
            previous_channels: v[20] as usize,
            channels: v[21] as usize,
            bandwidth: BANDS[v[22] as usize],
            auto_bandwidth: if v[23] < 0 {
                None
            } else {
                Some(BANDS[v[23] as usize])
            },
            allow_bandwidth_switch: v[24] != 0,
            wideband_ready: v[25] != 0,
            fec: v[26] != 0,
            to_mono: v[27] != 0,
        };
        let d = select(frame, controls, history)?;
        let actual = [
            mode_number(d.mode),
            d.channels as i32,
            band_index(d.bandwidth) as i32,
            d.auto_bandwidth.map_or(-1, |b| band_index(b) as i32),
            i32::from(d.fec),
            i32::from(d.to_mono),
            i32::from(d.redundancy),
            i32::from(d.celt_to_silk),
            i32::from(d.to_celt),
            i32::from(d.prefill_silk),
            d.equivalent_rate,
            i32::from(d.voice_q7),
            i32::from(d.silk_dtx),
        ];
        assert_eq!(
            actual,
            &v[28..],
            "row{count}: {frame:?} {controls:?} {history:?}"
        );
        modes[actual[0] as usize] += 1;
        for (counter, flag) in
            transitions
                .iter_mut()
                .zip([d.to_mono, d.celt_to_silk, d.to_celt, d.fec])
        {
            *counter += usize::from(flag);
        }
        count += 1;
    }
    assert_eq!(count, 16444);
    assert!(modes.iter().all(|&v| v > 100));
    assert!(transitions.iter().all(|&v| v > 100));
    Ok(())
}

#[test]
fn budget_and_domain_boundaries() -> TestResult {
    let frame = Frame {
        rate: InputRate::Khz48,
        channels: 2,
        samples48: 960,
        bitrate: 64000,
        packet_bytes: 160,
        digital_silence: false,
        stereo_width_q15: 0,
    };
    let c = Controls::default();
    let h = History::new(2)?;
    assert!(select(frame, c, h).is_ok());
    for f in [
        Frame {
            samples48: usize::MAX,
            ..frame
        },
        Frame {
            channels: 0,
            ..frame
        },
    ] {
        assert_eq!(select(f, c, h), Err(FrontendError::InvalidGeometry));
    }
    for f in [
        Frame {
            packet_bytes: 0,
            ..frame
        },
        Frame {
            packet_bytes: 61249,
            ..frame
        },
        Frame {
            bitrate: u32::MAX,
            ..frame
        },
        Frame {
            bitrate: 64001,
            ..frame
        },
        Frame {
            stereo_width_q15: -1,
            ..frame
        },
    ] {
        assert_eq!(select(f, c, h), Err(FrontendError::InvalidControl));
    }
    for c in [
        Controls {
            complexity: 11,
            ..c
        },
        Controls {
            loss_percent: 101,
            ..c
        },
        Controls {
            channels: Some(0),
            ..c
        },
        Controls {
            channels: Some(3),
            ..c
        },
    ] {
        assert_eq!(select(frame, c, h), Err(FrontendError::InvalidControl));
    }
    for h in [
        History { channels: 0, ..h },
        History { channels: 3, ..h },
        History {
            previous_channels: 3,
            ..h
        },
    ] {
        assert_eq!(select(frame, c, h), Err(FrontendError::InvalidControl));
    }
    assert!(History::new(0).is_err());
    assert!(History::new(3).is_err());
    assert_eq!(
        select(
            Frame {
                packet_bytes: 2,
                ..frame
            },
            c,
            h
        ),
        Err(FrontendError::InsufficientBudget)
    );
    for duration in [120, 240, 480, 960, 1920, 2880, 3840, 4800, 5760] {
        let fps = 48000 / duration;
        let bytes = if fps >= 50 {
            3
        } else {
            300_usize.div_ceil(fps)
        };
        let bitrate = (bytes * 8 * 48000 / duration) as u32;
        let frame = Frame {
            samples48: duration,
            bitrate,
            packet_bytes: bytes,
            ..frame
        };
        assert!(select(frame, c, h).is_ok());
        assert_eq!(
            select(
                Frame {
                    bitrate: 0,
                    ..frame
                },
                c,
                h
            ),
            Err(FrontendError::InsufficientBudget)
        );
    }
    Ok(())
}
