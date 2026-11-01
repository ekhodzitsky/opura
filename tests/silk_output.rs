//! End-to-end mono SILK output samples against the public opus_decode API.
use opura::silk::{
    mono::MonoError,
    output::{MonoOutputDecoder, OutputError},
    resampler::OutputRate,
};

fn bytes(hex: &str) -> Result<Vec<u8>, std::num::ParseIntError> {
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16))
        .collect()
}
fn output_rate(hz: u32) -> OutputRate {
    match hz {
        8000 => OutputRate::Khz8,
        12000 => OutputRate::Khz12,
        16000 => OutputRate::Khz16,
        24000 => OutputRate::Khz24,
        48000 => OutputRate::Khz48,
        _ => panic!("bad output rate"),
    }
}

#[test]
fn all_public_output_samples_and_ranges_match_opus_decode() -> Result<(), Box<dyn std::error::Error>>
{
    let mut decoder = MonoOutputDecoder::new(OutputRate::Khz8);
    let mut cases = 0;
    let mut pairs = [[false; 5]; 3];
    let mut kinds = [0; 4];
    let mut maximum = 0;
    let mut multi = false;
    for line in include_str!("silk-output-fixtures.txt")
        .lines()
        .filter(|l| !l.starts_with('#'))
    {
        let row: Vec<_> = line.split_whitespace().collect();
        let target: u32 = row[2].parse()?;
        let target = output_rate(target);
        if decoder.output_rate() != target {
            decoder = MonoOutputDecoder::new(target);
        }
        if row[1] == "1" {
            decoder.reset();
        }
        let duration: u16 = row[4].parse()?;
        let data = if row[5] == "-" {
            Vec::new()
        } else {
            bytes(row[5])?
        };
        let result = match row[0] {
            "loss" => {
                kinds[0] += 1;
                decoder.conceal(duration)?
            }
            "fec" => {
                kinds[1] += 1;
                decoder.decode_fec(&data)?
            }
            "packet" => {
                kinds[2] += 1;
                multi |= opura::Packet::parse(&data)?.frames().len() > 1;
                decoder.decode(&data)?
            }
            "transition" => {
                kinds[3] += 1;
                let before = decoder.clone();
                assert_eq!(
                    decoder.decode(&data),
                    Err(OutputError::Decode(MonoError::UnsupportedTransition))
                );
                assert_eq!(decoder, before);
                cases += 1;
                continue;
            }
            _ => panic!("unknown reference kind"),
        };
        let output_index = match target {
            OutputRate::Khz8 => 0,
            OutputRate::Khz12 => 1,
            OutputRate::Khz16 => 2,
            OutputRate::Khz24 => 3,
            OutputRate::Khz48 => 4,
        };
        pairs[match row[3] {
            "8" => 0,
            "12" => 1,
            "16" => 2,
            _ => panic!("bad input rate"),
        }][output_index] = true;
        let expected: Vec<_> = row[7..]
            .iter()
            .map(|s| s.parse::<i16>())
            .collect::<Result<_, _>>()?;
        assert_eq!(result.sample_rate(), target.hz());
        assert_eq!(
            result.samples().len(),
            duration as usize * target.hz() as usize / 1000
        );
        assert_eq!(
            result.final_range(),
            row[6].parse::<u32>()?,
            "range step {cases}"
        );
        assert_eq!(
            result.samples(),
            expected,
            "public PCM step {cases} {} {} kHz -> {:?}",
            row[0],
            row[3],
            target
        );
        maximum = maximum.max(result.samples().len());
        cases += 1;
    }
    assert_eq!(cases, 260);
    assert_eq!(pairs, [[true; 5]; 3]);
    assert!(kinds[..3].iter().all(|&count| count > 0));
    assert!(multi);
    assert_eq!(maximum, 5760);
    Ok(())
}

#[test]
fn output_delay_and_filter_state_are_transactional_and_resettable()
-> Result<(), Box<dyn std::error::Error>> {
    let line = include_str!("silk-output-fixtures.txt")
        .lines()
        .find(|l| l.starts_with("packet "))
        .ok_or("missing public fixture")?;
    let row: Vec<_> = line.split_whitespace().collect();
    let packet = bytes(row[5])?;
    for rate in [
        OutputRate::Khz8,
        OutputRate::Khz12,
        OutputRate::Khz16,
        OutputRate::Khz24,
        OutputRate::Khz48,
    ] {
        let mut decoder = MonoOutputDecoder::new(rate);
        assert_eq!(
            decoder.conceal(20),
            Err(OutputError::Decode(MonoError::MissingHistory))
        );
        decoder.decode(&packet)?;
        decoder.conceal(30)?;
        let before = decoder.clone();
        assert!(decoder.decode(&[]).is_err());
        assert_eq!(decoder, before);
        assert!(decoder.decode_fec(&[0x80, 0, 0]).is_err());
        assert_eq!(decoder, before);
        assert_eq!(
            decoder.conceal(15),
            Err(OutputError::Decode(MonoError::InvalidLossDuration))
        );
        assert_eq!(decoder, before);
        let malformed = include_str!("silk-fec-fixtures.txt")
            .lines()
            .find(|l| l.starts_with("budget "))
            .ok_or("missing late FEC failure")?;
        let encoded: Vec<_> = malformed.split_whitespace().collect();
        let budget = bytes(encoded[2])?;
        assert!(matches!(
            decoder.decode_fec(&budget),
            Err(OutputError::Decode(MonoError::Channel(_)))
        ));
        assert_eq!(decoder, before);
        let mut control = before;
        assert_eq!(decoder.decode(&packet)?, control.decode(&packet)?);
        assert_eq!(decoder, control);
        decoder.reset();
        assert_eq!(decoder, MonoOutputDecoder::new(rate));
        assert_eq!(
            decoder.decode(&packet)?,
            MonoOutputDecoder::new(rate).decode(&packet)?
        );
    }
    Ok(())
}
