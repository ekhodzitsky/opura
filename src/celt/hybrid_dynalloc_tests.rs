use super::*;

#[test]
fn connected_hybrid_dynamic_policy_matches_original_requests()
-> Result<(), Box<dyn std::error::Error>> {
    let mut lines = include_str!("../../tests/hybrid-dynalloc-analysis-fixtures.txt")
        .lines()
        .filter(|l| !l.starts_with('#'));
    let mut frames = 0;
    let mut analyses = 0;
    let mut different = 0;
    while let Some(line) = lines.next() {
        let row: Vec<_> = line.split_whitespace().collect();
        assert_eq!(row[0], "frame");
        let lm = row[1].parse::<usize>()?;
        let channels = row[2].parse::<usize>()?;
        let start = row[3].parse::<usize>()?;
        let end = row[4].parse::<usize>()?;
        let cases = row[5].parse::<usize>()?;
        let logs = row[6..]
            .iter()
            .map(|v| v.parse())
            .collect::<Result<Vec<i32>, _>>()?;
        let mut arrays = [[[0; 21]; 2]; 3];
        for (kind, array) in arrays.iter_mut().enumerate() {
            for (channel, logs_out) in array.iter_mut().enumerate() {
                let offset = kind * 42 + channel * 21;
                logs_out.copy_from_slice(&logs[offset..offset + 21]);
            }
        }
        let size = if lm == 2 {
            FrameSize::Ms10
        } else {
            FrameSize::Ms20
        };
        let mut previous = None;
        for _ in 0..cases {
            let row = lines
                .next()
                .ok_or("missing native dynamic row")?
                .split_whitespace()
                .skip(1)
                .map(str::parse)
                .collect::<Result<Vec<i32>, _>>()?;
            assert_eq!(row.len(), 91);
            let variable = row[2] & 2 != 0;
            let result = analyze_dynamic(
                size,
                BandLayout::new(channels, start, end)?,
                analysis::DynallocInput {
                    current: &arrays[0],
                    comparison: &arrays[1],
                    previous: &arrays[2],
                },
                analysis::DynallocOptions {
                    payload_bytes: row[0] as u16,
                    lsb_depth: row[1] as u8,
                    transient: row[2] & 1 != 0,
                    tone_frequency: row[3] as i16,
                    toneishness: row[4],
                    ..analysis::DynallocOptions::default()
                },
                Some(HybridControls {
                    bandwidth: if end == 19 {
                        crate::Bandwidth::SuperWideband
                    } else {
                        crate::Bandwidth::Fullband
                    },
                    complexity: 10,
                    voiced: true,
                    loss_percent: 0,
                    vbr: variable.then_some(HybridVbr {
                        rate: 16000,
                        offset: 100,
                    }),
                }),
                None,
            )?;
            assert_eq!(
                (result.max_depth(), result.total_boost()),
                (row[26], row[27]),
                "frame{frames} flags{} bytes{}",
                row[2],
                row[0]
            );
            assert_eq!(result.requested_boosts().as_slice(), &row[28..49]);
            for i in 0..21 {
                assert_eq!(i32::from(result.importance()[i]), row[49 + i]);
                assert_eq!(i32::from(result.spread_weights()[i]), row[70 + i]);
            }
            if variable {
                different += usize::from(previous.as_ref() != Some(&result));
            }
            previous = Some(result);
            analyses += 1;
        }
        frames += 1;
    }
    assert_eq!((frames, analyses), (32, 768));
    assert!(different > 0);
    Ok(())
}
