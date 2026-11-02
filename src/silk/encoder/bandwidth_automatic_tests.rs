use super::*;
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
fn rate(value: i32) -> SilkRate {
    match value {
        8 => SilkRate::Khz8,
        12 => SilkRate::Khz12,
        _ => SilkRate::Khz16,
    }
}
#[test]
fn unchanged_original_equivalent_rates_bandwidth_hysteresis_and_fec_match() -> Result<()> {
    let mut cases = 0;
    let mut lowered = 0;
    for line in include_str!("../../../tests/silk-encoder-automatic-policy-fixtures.txt")
        .lines()
        .filter(|s| !s.starts_with('#'))
    {
        let v: Vec<i32> = line
            .split_whitespace()
            .map(|s| s.parse())
            .collect::<std::result::Result<_, _>>()?;
        let input = match v[0] {
            8000 => OutputRate::Khz8,
            12000 => OutputRate::Khz12,
            16000 => OutputRate::Khz16,
            24000 => OutputRate::Khz24,
            _ => OutputRate::Khz48,
        };
        let duration = match v[1] {
            10 => PacketDuration::Ms10,
            20 => PacketDuration::Ms20,
            40 => PacketDuration::Ms40,
            _ => PacketDuration::Ms60,
        };
        let options = MonoOptions {
            complexity: v[4] as u8,
            variable_bitrate: v[5] != 0,
            packet_loss_percent: v[6] as u8,
        };
        let mut state = AutomaticBandwidth {
            initialized: v[11] != 0,
            auto_bandwidth: v[12] as u8,
            bandwidth: v[13] as u8,
            fec: v[14] != 0,
        };
        let (desired, maximum) = state.select(AutomaticControls {
            input,
            duration,
            packet_bytes: v[3] as u32,
            channels: v[2] as u8,
            options,
            fec: v[7] != 0,
            allowed: v[8] != 0,
            in_wb: v[9] != 0,
            voice_q7: v[10] as u8,
        });
        assert_eq!(
            equivalent_rate(v[3] * 8000 / v[1], v[2] as u8, duration, options),
            v[15],
            "equivalent rate case{cases}"
        );
        assert_eq!(
            (desired, maximum),
            (rate(v[16]), rate(v[17])),
            "target/cap case{cases}"
        );
        assert_eq!(
            (state.auto_bandwidth, state.bandwidth, state.fec),
            (v[18] as u8, v[19] as u8, v[20] != 0),
            "state case{cases}"
        );
        assert!(state.initialized);
        lowered += usize::from(state.fec && state.bandwidth < state.auto_bandwidth.min(2));
        cases += 1;
    }
    assert_eq!(cases, 12480);
    assert!(lowered > 20);
    Ok(())
}
