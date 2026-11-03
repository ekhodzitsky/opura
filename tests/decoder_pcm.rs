//! Pristine native gain/transition/long-packet/PLC-prefix PCM with packet-local offsets.
use opura::{
    decoder::OpusDecoder,
    silk::{decoder::PcmChannels, resampler::OutputRate},
};
use std::collections::BTreeSet;

#[test]
fn packet_offsets_gains_and_recovery_match_pristine_native()
-> Result<(), Box<dyn std::error::Error>> {
    let mut decoder = OpusDecoder::new(OutputRate::Khz48, PcmChannels::Mono)?;
    let (mut cases, mut operations, mut resets) = (0, [0; 3], 0);
    let mut longer = 0;
    let mut geometries = BTreeSet::new();
    for line in include_str!("decoder-pcm-fixtures.txt").lines() {
        if line.starts_with('#') {
            continue;
        }
        let row: Vec<_> = line.split_whitespace().collect();
        if row[0] == "case" {
            let rate = match row[1].parse::<u32>()? {
                8000 => OutputRate::Khz8,
                12000 => OutputRate::Khz12,
                16000 => OutputRate::Khz16,
                24000 => OutputRate::Khz24,
                48000 => OutputRate::Khz48,
                _ => return Err("invalid fixture rate".into()),
            };
            let channels = match row[2] {
                "1" => PcmChannels::Mono,
                "2" => PcmChannels::Stereo,
                _ => return Err("invalid fixture channels".into()),
            };
            decoder = OpusDecoder::new(rate, channels)?;
            let gain = row[3].parse()?;
            assert!(geometries.insert((rate.hz(), channels.count(), gain)));
            decoder.set_gain_q8(gain);
            cases += 1;
            continue;
        }
        assert_eq!(row.len(), 8);
        assert_eq!(row[0], "frame");
        let duration = row[2].parse()?;
        if row[3] == "1" {
            let gain = decoder.gain_q8();
            decoder.reset();
            assert_eq!(decoder.gain_q8(), gain);
            resets += 1;
        }
        let bytes = if row[4] == "-" {
            Vec::new()
        } else {
            (0..row[4].len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&row[4][i..i + 2], 16))
                .collect::<Result<Vec<_>, _>>()?
        };
        let pcm = match row[1] {
            "normal" => {
                operations[0] += 1;
                if duration > 960 {
                    longer += 1;
                }
                decoder.decode(&bytes)?
            }
            "plc" => {
                operations[1] += 1;
                decoder.conceal_samples(duration)?
            }
            "fec" => {
                operations[2] += 1;
                decoder.decode_fec(&bytes, duration)?
            }
            _ => return Err("invalid fixture operation".into()),
        };
        assert_eq!(pcm.samples_per_channel(), row[5].parse::<usize>()?);
        assert_eq!(pcm.final_range(), row[6].parse::<u32>()?);
        assert_eq!(decoder.final_range(), pcm.final_range());
        assert_eq!(decoder.last_packet_duration(), duration);
        let hash = pcm
            .samples()
            .iter()
            .flat_map(|x| x.to_le_bytes())
            .fold(14695981039346656037u64, |h, b| {
                (h ^ u64::from(b)).wrapping_mul(1099511628211)
            });
        assert_eq!(hash, row[7].parse::<u64>()?, "case{cases} op{operations:?}");
    }
    let expected: BTreeSet<_> = [8000, 12000, 16000, 24000, 48000]
        .into_iter()
        .flat_map(|rate| {
            [1, 2].into_iter().flat_map(move |channels| {
                [0, 1536, -1536, -1, 1, 32767, -32768]
                    .into_iter()
                    .map(move |gain| (rate, channels, gain))
            })
        })
        .collect();
    assert_eq!(geometries, expected);
    assert_eq!(cases, 70);
    assert_eq!(operations, [1330, 210, 140]);
    assert_eq!(resets, 70);
    assert_eq!(longer, 420);
    Ok(())
}
