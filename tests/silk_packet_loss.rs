//! Real mono packet losses and recovery compared with complete upstream frames.
use opura::silk::mono::{MonoDecoder, MonoError};

fn bytes(hex: &str) -> Result<Vec<u8>, std::num::ParseIntError> {
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16))
        .collect()
}

#[test]
fn real_packet_loss_and_recovery_pcm_match_completed_reference_frames()
-> Result<(), Box<dyn std::error::Error>> {
    let mut decoder = MonoDecoder::default();
    let mut count = 0;
    let mut losses = 0;
    let mut durations = [false; 12];
    let mut rates = [false; 3];
    for line in include_str!("silk-packet-loss-fixtures.txt")
        .lines()
        .filter(|l| !l.starts_with('#'))
    {
        let row: Vec<_> = line.split_whitespace().collect();
        if row[1] == "1" {
            decoder.reset();
        }
        let duration: u16 = row[2].parse()?;
        let pcm = if row[0] == "loss" {
            losses += 1;
            durations[(duration / 10 - 1) as usize] = true;
            decoder.conceal(duration)?
        } else {
            let data = bytes(row[3])?;
            if row[0] == "transition" {
                let before = decoder.clone();
                assert_eq!(decoder.decode(&data), Err(MonoError::UnsupportedTransition));
                assert_eq!(decoder, before);
                count += 1;
                continue;
            }
            assert_eq!(row[0], "packet");
            decoder.decode(&data)?
        };
        let rate = row[4].parse::<u32>()? * 1000;
        rates[match rate {
            8000 => 0,
            12000 => 1,
            16000 => 2,
            _ => panic!("invalid reference rate"),
        }] = true;
        let expected: Vec<_> = row[6..]
            .iter()
            .map(|s| s.parse::<i16>())
            .collect::<Result<_, _>>()?;
        assert_eq!(pcm.sample_rate(), rate);
        assert_eq!(
            pcm.samples().len(),
            duration as usize * rate as usize / 1000
        );
        assert_eq!(
            pcm.final_range(),
            row[5].parse::<u32>()?,
            "range step {count}"
        );
        assert_eq!(pcm.samples(), expected, "PCM step {count} {}", row[0]);
        count += 1;
    }
    assert_eq!(count, 288);
    assert_eq!(losses, 120);
    assert_eq!(durations, [true; 12]);
    assert_eq!(rates, [true; 3]);
    Ok(())
}

#[test]
fn invalid_loss_requests_and_failed_recovery_preserve_packet_state()
-> Result<(), Box<dyn std::error::Error>> {
    let mut decoder = MonoDecoder::default();
    let clean = decoder.clone();
    assert_eq!(decoder.conceal(20), Err(MonoError::MissingHistory));
    assert_eq!(decoder, clean);
    let line = include_str!("silk-packet-loss-fixtures.txt")
        .lines()
        .find(|l| l.starts_with("packet "))
        .ok_or("no reference packet")?;
    let row: Vec<_> = line.split_whitespace().collect();
    let packet = bytes(row[3])?;
    decoder.decode(&packet)?;
    for duration in [0, 1, 5, 9, 11, 125, 130, u16::MAX] {
        let before = decoder.clone();
        assert_eq!(
            decoder.conceal(duration),
            Err(MonoError::InvalidLossDuration)
        );
        assert_eq!(decoder, before);
    }
    decoder.conceal(120)?;
    let before = decoder.clone();
    assert!(decoder.decode(&[]).is_err());
    assert_eq!(decoder, before);
    let mut control = before;
    assert_eq!(decoder.decode(&packet)?, control.decode(&packet)?);
    assert_eq!(decoder, control);
    decoder.reset();
    assert_eq!(decoder, clean);
    assert_eq!(decoder.conceal(10), Err(MonoError::MissingHistory));
    Ok(())
}
