use super::*;

#[test]
fn complete_reference_loss_recovery_pcm_and_histories_match()
-> Result<(), Box<dyn std::error::Error>> {
    let mut state = ChannelState::default();
    let mut lines = include_str!("../../tests/silk-plc-fixtures.txt")
        .lines()
        .filter(|l| !l.starts_with('#'));
    let mut cases = 0;
    let mut voiced = false;
    let mut unvoiced = false;
    let mut inactive = false;
    let mut clipped = 0;
    while let Some(line) = lines.next() {
        let header: Vec<_> = line.split_whitespace().collect();
        if header[0] == "1" {
            state.reset();
        }
        let rate = match header[1] {
            "8" => SilkRate::Khz8,
            "12" => SilkRate::Khz12,
            _ => SilkRate::Khz16,
        };
        let duration = if header[2] == "2" {
            FrameDuration::Ms10
        } else {
            FrameDuration::Ms20
        };
        let row: Vec<i64> = lines
            .next()
            .ok_or("missing oracle result")?
            .split_whitespace()
            .map(str::parse)
            .collect::<Result<_, _>>()?;
        let khz = rate.khz() as usize;
        let length = khz * 5 * header[2].parse::<usize>()?;
        let pcm = if header[5] == "1" {
            state.conceal(rate, duration)?
        } else {
            let hex = header[6];
            let input: Vec<_> = (0..hex.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&hex[i..i + 2], 16))
                .collect::<Result<_, _>>()?;
            let mut entropy = RangeDecoder::new(&input);
            let pcm = state.decode(
                &mut entropy,
                rate,
                duration,
                if header[3] == "0" {
                    FrameActivity::Inactive
                } else {
                    FrameActivity::Active
                },
                if header[4] == "0" {
                    PitchCoding::Independent
                } else {
                    PitchCoding::Conditional
                },
            )?;
            assert_eq!(
                (entropy.range(), entropy.tell(), entropy.tell_frac()),
                (
                    row[length] as u32,
                    row[length + 1] as u32,
                    row[length + 2] as u32
                ),
                "entropy {cases}"
            );
            voiced |= row[length + 4] == 2;
            unvoiced |= row[length + 4] == 1;
            inactive |= row[length + 4] == 0;
            pcm
        };
        let expected: Vec<_> = row[..length].iter().map(|&v| v as i16).collect();
        assert_eq!(pcm.samples(), expected, "PCM case {cases}");
        assert_eq!(
            state.gains.previous_index(),
            Some(row[length + 3] as u8),
            "gain case {cases}"
        );
        let lpc: Vec<_> = row[length + 5..length + 21]
            .iter()
            .map(|&v| v as i32)
            .collect();
        assert_eq!(
            state.synthesis.lpc.as_slice(),
            lpc,
            "LPC history case {cases}"
        );
        let history: Vec<_> = row[length + 21..length + 21 + khz * 20]
            .iter()
            .map(|&v| v as i16)
            .collect();
        assert_eq!(
            &state.synthesis.history[..khz * 20],
            history,
            "PCM history case {cases}"
        );
        assert_eq!(
            state.synthesis.previous_gain,
            row[length + 341] as i32,
            "previous gain case {cases}"
        );
        let expected_plc: Vec<_> = row[length + 342..].iter().map(|&v| v as i32).collect();
        assert_eq!(
            state.plc.reference_state(),
            expected_plc,
            "PLC history case {cases}"
        );
        clipped += pcm
            .samples()
            .iter()
            .filter(|&&s| s == i16::MIN || s == i16::MAX)
            .count();
        cases += 1;
    }
    assert_eq!(cases, 240);
    assert!(voiced && unvoiced && inactive);
    assert!(clipped > 0);
    Ok(())
}
