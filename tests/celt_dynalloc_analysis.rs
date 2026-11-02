//! Native perceptual dynamic analysis and boost-count wire bounds.
use opura::{
    celt::{
        FrameSize,
        allocation::{self, AllocationEncoding},
        analysis::{DynallocInput, DynallocOptions, analyze_dynalloc},
        energy::BandLayout,
    },
    range::RangeEncoder,
};
const SIZES: [FrameSize; 4] = [
    FrameSize::Ms2_5,
    FrameSize::Ms5,
    FrameSize::Ms10,
    FrameSize::Ms20,
];
#[test]
fn original_masking_followers_weights_requests_and_depth_match()
-> Result<(), Box<dyn std::error::Error>> {
    let mut lines = include_str!("celt-dynalloc-analysis-fixtures.txt")
        .lines()
        .filter(|l| !l.starts_with('#'));
    let mut frames = 0;
    let mut analyses = 0;
    let mut clipped = 0;
    let mut boosted = 0;
    let mut weighted = 0;
    let mut cap_cells = 0;
    let mut max_symbols = 0;
    let bands = [
        0, 1, 2, 3, 4, 5, 6, 7, 8, 10, 12, 14, 16, 20, 24, 28, 34, 40, 48, 60, 78, 100,
    ];
    while let Some(line) = lines.next() {
        let f: Vec<_> = line.split_whitespace().collect();
        if f[0] == "caps" {
            let lm = f[1].parse::<usize>()?;
            let channels = f[2].parse::<usize>()?;
            let native: Vec<i32> = f[3..].iter().map(|s| s.parse()).collect::<Result<_, _>>()?;
            let mut writer = RangeEncoder::new(1275)?;
            let allocation = allocation::encode_allocation(
                &mut writer,
                SIZES[lm],
                BandLayout::new(channels, 0, 21)?,
                false,
                AllocationEncoding {
                    boosts: [255; 21],
                    trim: 5,
                    intensity: 21,
                    dual_stereo: false,
                    previous_coded_bands: 0,
                    signal_bandwidth: 20,
                },
            )?;
            assert_eq!(allocation.caps(), native.as_slice());
            for i in 0..21 {
                let width = (channels as i32 * (bands[i + 1] - bands[i])) << lm;
                let quantum = (width * 8).min(width.max(48));
                let count = (native[i] + quantum - 1) / quantum;
                assert!(count < 255);
                max_symbols = max_symbols.max(count);
                cap_cells += 1;
            }
            continue;
        }
        assert_eq!(f[0], "frame");
        let size = SIZES[f[1].parse::<usize>()?];
        let channels = f[2].parse::<usize>()?;
        let start = f[3].parse::<usize>()?;
        let end = f[4].parse::<usize>()?;
        let cases = f[5].parse::<usize>()?;
        let logs: Vec<i32> = f[6..].iter().map(|s| s.parse()).collect::<Result<_, _>>()?;
        assert_eq!(logs.len(), 126);
        let mut arrays = [[[0; 21]; 2]; 3];
        for k in 0..3 {
            for c in 0..2 {
                arrays[k][c].copy_from_slice(&logs[k * 42 + c * 21..k * 42 + (c + 1) * 21]);
            }
        }
        let original = arrays;
        let input = DynallocInput {
            current: &arrays[0],
            comparison: &arrays[1],
            previous: &arrays[2],
        };
        let layout = BandLayout::new(channels, start, end)?;
        for case in 0..cases {
            let d: Vec<i32> = lines
                .next()
                .ok_or("dynamic row")?
                .split_whitespace()
                .skip(1)
                .map(|s| s.parse())
                .collect::<Result<_, _>>()?;
            assert_eq!(d.len(), 91);
            let flags = d[2];
            let mut surround = [0; 21];
            surround.copy_from_slice(&d[5..26]);
            let options = DynallocOptions {
                payload_bytes: u16::try_from(d[0])?,
                lsb_depth: u8::try_from(d[1])?,
                transient: flags & 1 != 0,
                vbr: flags & 2 != 0,
                constrained_vbr: flags & 4 != 0,
                lfe: flags & 8 != 0,
                surround,
                tone_frequency: i16::try_from(d[3])?,
                toneishness: d[4],
            };
            let result = analyze_dynalloc(size, layout, input, options)?;
            assert_eq!(
                [result.max_depth(), result.total_boost()],
                d[26..28],
                "depth/total frame{frames} case{case}"
            );
            assert_eq!(
                result.requested_boosts().as_slice(),
                &d[28..49],
                "requests frame{frames} case{case}"
            );
            for i in 0..21 {
                assert_eq!(
                    i32::from(result.importance()[i]),
                    d[49 + i],
                    "importance frame{frames} case{case} band{i}"
                );
                assert_eq!(
                    i32::from(result.spread_weights()[i]),
                    d[70 + i],
                    "spread frame{frames} case{case} band{i}"
                );
                assert_eq!(i32::from(result.boosts()[i]), d[28 + i].clamp(0, 255));
                clipped += usize::from(d[28 + i] > 255);
                boosted += usize::from(d[28 + i] > 0);
                weighted += usize::from(d[49 + i] > 13);
            }
            assert_eq!(arrays, original);
            analyses += 1;
        }
        frames += 1;
    }
    eprintln!(
        "dynamic frames{frames} analyses{analyses} clipped{clipped} boosted{boosted} weighted{weighted}"
    );
    assert_eq!(
        (frames, analyses, cap_cells, max_symbols),
        (2737, 21889, 168, 66)
    );
    assert!(clipped > 0 && boosted > 0 && weighted > 0);
    Ok(())
}

#[test]
fn invalid_domains_reject_and_bounded_contexts_keep_native_ranges()
-> Result<(), Box<dyn std::error::Error>> {
    use opura::celt::analysis::AnalysisError;
    let arrays = [[[0; 21]; 2]; 3];
    let input = DynallocInput {
        current: &arrays[0],
        comparison: &arrays[1],
        previous: &arrays[2],
    };
    let options = DynallocOptions::default();
    for size in SIZES {
        for (start, end) in [(0, 0), (0, 1), (0, 2), (21, 21)] {
            assert_eq!(
                analyze_dynalloc(size, BandLayout::new(1, start, end)?, input, options),
                Err(AnalysisError::InvalidBands)
            );
        }
        let layout = BandLayout::new(2, 0, 21)?;
        let mut invalid = Vec::new();
        for payload_bytes in [0, 1, 1276, u16::MAX] {
            invalid.push(DynallocOptions {
                payload_bytes,
                ..options
            });
        }
        for lsb_depth in [0, 7, 25, u8::MAX] {
            invalid.push(DynallocOptions {
                lsb_depth,
                ..options
            });
        }
        for tone_frequency in [i16::MIN, -2, 25737, i16::MAX] {
            invalid.push(DynallocOptions {
                tone_frequency,
                ..options
            });
        }
        for toneishness in [-1, (1 << 29) + 1, i32::MAX] {
            invalid.push(DynallocOptions {
                toneishness,
                ..options
            });
        }
        for value in [-1, (4 << 24) + 1, i32::MIN, i32::MAX] {
            let mut surround = [0; 21];
            surround[20] = value;
            invalid.push(DynallocOptions {
                surround,
                ..options
            });
        }
        if size == FrameSize::Ms2_5 {
            invalid.push(DynallocOptions {
                transient: true,
                ..options
            });
        }
        for controls in invalid {
            assert_eq!(
                analyze_dynalloc(size, layout, input, controls),
                Err(AnalysisError::InvalidDynallocParameters)
            );
        }
        for kind in 0..3 {
            for c in 0..2 {
                for value in [i32::MIN, -(32 << 24) - 1, (32 << 24) + 1, i32::MAX] {
                    let mut bad = arrays;
                    bad[kind][c][20] = value;
                    let before = bad;
                    assert_eq!(
                        analyze_dynalloc(
                            size,
                            layout,
                            DynallocInput {
                                current: &bad[0],
                                comparison: &bad[1],
                                previous: &bad[2]
                            },
                            options
                        ),
                        Err(AnalysisError::ArithmeticOverflow)
                    );
                    assert_eq!(bad, before);
                }
            }
        }
        for value in [-(32 << 24), 32 << 24] {
            let boundary = [[[value; 21]; 2]; 3];
            for lsb_depth in [8, 24] {
                analyze_dynalloc(
                    size,
                    layout,
                    DynallocInput {
                        current: &boundary[0],
                        comparison: &boundary[1],
                        previous: &boundary[2],
                    },
                    DynallocOptions {
                        lsb_depth,
                        ..options
                    },
                )?;
            }
        }
    }
    let mut seed = 0x44594e41u32;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed
    };
    for size in SIZES {
        for channels in [1, 2] {
            for _ in 0..128 {
                let end = 3 + next() as usize % 19;
                let start = next() as usize % end;
                let mut logs = [[[i32::MAX; 21]; 2]; 3];
                for array in &mut logs {
                    for c in &mut array[..channels] {
                        for value in &mut c[..end] {
                            *value = (next() % ((64 << 24) + 1)) as i32 - (32 << 24);
                        }
                    }
                }
                let mut surround = [i32::MAX; 21];
                for value in &mut surround[start..end] {
                    *value = (next() % ((4 << 24) + 1)) as i32;
                }
                let options = DynallocOptions {
                    payload_bytes: (2 + next() % 1274) as u16,
                    lsb_depth: (8 + next() % 17) as u8,
                    transient: size != FrameSize::Ms2_5 && next() % 2 != 0,
                    vbr: next() % 2 != 0,
                    constrained_vbr: next() % 2 != 0,
                    lfe: next() % 8 == 0,
                    surround,
                    tone_frequency: (next() % 25738) as i16 - 1,
                    toneishness: (next() % ((1 << 29) + 1)) as i32,
                };
                let input = DynallocInput {
                    current: &logs[0],
                    comparison: &logs[1],
                    previous: &logs[2],
                };
                let original = logs;
                let result =
                    analyze_dynalloc(size, BandLayout::new(channels, start, end)?, input, options)?;
                assert_eq!(logs, original);
                for i in 0..21 {
                    if (start..end).contains(&i) {
                        assert!((13..=208).contains(&result.importance()[i]));
                        assert!((1..=32).contains(&result.spread_weights()[i]));
                        assert!(result.requested_boosts()[i] >= -63);
                    } else {
                        assert_eq!(
                            (
                                result.requested_boosts()[i],
                                result.importance()[i],
                                result.spread_weights()[i]
                            ),
                            (0, 0, 0)
                        );
                    }
                }
                assert!(result.total_boost() >= 0);
                if !options.vbr || (options.constrained_vbr && !options.transient) {
                    assert!(result.total_boost() >> 6 <= 2 * i32::from(options.payload_bytes) / 3);
                }
            }
        }
    }
    Ok(())
}
