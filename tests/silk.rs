//! SILK header ordering, frame scope and failure contracts.
use opura::{
    Packet,
    silk::{SilkError, decode_header},
};

#[test]
fn configuration_matrix() -> Result<(), Box<dyn std::error::Error>> {
    for config in 0..32 {
        for stereo in 0..=1 {
            let data = [config << 3 | stereo << 2, 0, 0, 0];
            let packet = Packet::parse(&data)?;
            if config >= 16 {
                assert_eq!(decode_header(&packet, 0), Err(SilkError::UnsupportedMode));
                continue;
            }
            let (header, decoder) = decode_header(&packet, 0)?;
            let frames = match packet.frame_samples() {
                1920 => 2,
                2880 => 3,
                _ => 1,
            };
            assert_eq!(header.channel_count(), usize::from(stereo + 1));
            assert_eq!(header.frame_count(), frames);
            assert_eq!(
                header.frame_duration_ms(),
                if packet.frame_samples() == 480 {
                    10
                } else {
                    20
                }
            );
            for channel in 0..header.channel_count() {
                assert_eq!(header.voice_activity(channel), Some(&[false; 3][..frames]));
                assert_eq!(header.redundancy(channel), Some(&[false; 3][..frames]));
            }
            assert_eq!(header.voice_activity(header.channel_count()), None);
            assert_eq!(header.redundancy(usize::MAX), None);
            assert_eq!(
                decoder.tell(),
                1 + ((frames + 1) * header.channel_count()) as u32
            );
        }
    }
    Ok(())
}

#[test]
fn rejects_missing_and_short_frames() -> Result<(), Box<dyn std::error::Error>> {
    let packet = Packet::parse(&[0])?;
    assert_eq!(decode_header(&packet, 0), Err(SilkError::MissingPayload));
    assert_eq!(decode_header(&packet, 1), Err(SilkError::InvalidFrameIndex));
    assert_eq!(
        decode_header(&packet, usize::MAX),
        Err(SilkError::InvalidFrameIndex)
    );
    let packet = Packet::parse(&[28, 0])?; // 60 ms stereo: 8 header bits + termination.
    assert_eq!(
        decode_header(&packet, 0),
        Err(SilkError::HeaderBudgetExceeded)
    );
    Ok(())
}

#[test]
fn uniform_header_bits_are_msb_first_per_channel() -> Result<(), Box<dyn std::error::Error>> {
    for config in [0u8, 1, 2, 3, 12, 13] {
        for stereo in 0..=1 {
            for first in 0..=255 {
                let bytes = [config << 3 | stereo << 2, first, 0x5a, 0xc3, 0];
                let packet = Packet::parse(&bytes)?;
                let (header, _) = decode_header(&packet, 0)?;
                let count = header.frame_count();
                for channel in 0..header.channel_count() {
                    let start = channel * (count + 1);
                    let vad = header.voice_activity(channel).ok_or("missing channel")?;
                    for (i, &flag) in vad.iter().enumerate() {
                        assert_eq!(flag, first & (0x80 >> (start + i)) != 0);
                    }
                    let any_lbrr = first & (0x80 >> (start + count)) != 0;
                    let lbrr = header.redundancy(channel).ok_or("missing channel")?;
                    assert_eq!(lbrr.iter().any(|&flag| flag), any_lbrr);
                }
            }
        }
    }
    Ok(())
}

#[test]
fn upstream_header_fixtures() -> Result<(), Box<dyn std::error::Error>> {
    let mut cases = 0;
    let mut masks_seen = [[false; 8]; 3];
    for (line_number, line) in include_str!("silk-header-fixtures.txt").lines().enumerate() {
        if line.starts_with('#') {
            continue;
        }
        let columns: Vec<_> = line.split_whitespace().collect();
        let mut input = Vec::new();
        for i in (0..columns[0].len()).step_by(2) {
            input.push(u8::from_str_radix(&columns[0][i..i + 2], 16)?);
        }
        let packet = Packet::parse(&input)?;
        let result = decode_header(&packet, 0);
        match columns[1] {
            "missing" => assert_eq!(result, Err(SilkError::MissingPayload)),
            "budget" => assert_eq!(result, Err(SilkError::HeaderBudgetExceeded)),
            "ok" => {
                let (header, state) = result?;
                let mut vad_mask = 0u32;
                let mut lbrr_mask = 0u32;
                for channel in 0..header.channel_count() {
                    let vad = header.voice_activity(channel).ok_or("missing VAD")?;
                    let lbrr = header.redundancy(channel).ok_or("missing LBRR")?;
                    let mut mask = 0;
                    for i in 0..header.frame_count() {
                        vad_mask |= u32::from(vad[i]) << (channel * 3 + i);
                        lbrr_mask |= u32::from(lbrr[i]) << (channel * 3 + i);
                        mask |= usize::from(lbrr[i]) << i;
                    }
                    masks_seen[header.frame_count() - 1][mask] = true;
                }
                let actual = [
                    vad_mask,
                    lbrr_mask,
                    state.range(),
                    state.tell(),
                    state.tell_frac(),
                ];
                let expected: Vec<u32> = columns[2..]
                    .iter()
                    .map(|s| s.parse())
                    .collect::<Result<_, _>>()?;
                assert_eq!(
                    actual.as_slice(),
                    expected,
                    "fixture line {}",
                    line_number + 1
                );
            }
            _ => return Err("unexpected fixture status".into()),
        }
        cases += 1;
    }
    assert_eq!(cases, 640);
    for (count, seen) in masks_seen.iter().enumerate() {
        assert!(
            seen[..1 << (count + 1)].iter().all(|v| *v),
            "missing LBRR masks for {} frames",
            count + 1
        );
    }
    Ok(())
}

#[test]
fn header_entropy_state_is_ready_and_frame_local() -> Result<(), Box<dyn std::error::Error>> {
    for adjacent in [0u8, 255] {
        let bytes = [2, 2, 0x80, 0x12, adjacent, adjacent];
        let packet = Packet::parse(&bytes)?;
        let (header, mut state) = decode_header(&packet, 0)?;
        assert_eq!(header.voice_activity(0), Some(&[true][..]));
        assert_eq!(header.redundancy(0), Some(&[false][..]));
        let mut expected = opura::range::RangeDecoder::new(&[0x80, 0x12]);
        expected.decode_bit_logp(1)?;
        expected.decode_bit_logp(1)?;
        assert_eq!(state, expected);
        for _ in 0..32 {
            assert_eq!(state.decode_uint(251)?, expected.decode_uint(251)?);
        }
        let (second, _) = decode_header(&packet, 1)?;
        assert_eq!(second.voice_activity(0), Some(&[adjacent != 0][..]));
    }
    Ok(())
}

#[test]
fn short_payloads_preserve_header_bounds() -> Result<(), Box<dyn std::error::Error>> {
    for config in [0u8, 1, 2, 3, 12, 13] {
        for stereo in 0..=1 {
            for word in 0..=u16::MAX {
                let [first, second] = word.to_be_bytes();
                let bytes = [config << 3 | stereo << 2, first, second];
                let packet = Packet::parse(&bytes)?;
                match decode_header(&packet, 0) {
                    Ok((header, state)) => {
                        assert!(state.tell() <= 16);
                        assert_eq!(header.channel_count(), usize::from(stereo + 1));
                        for channel in 0..header.channel_count() {
                            assert_eq!(
                                header.voice_activity(channel).map(<[bool]>::len),
                                Some(header.frame_count())
                            );
                            assert_eq!(
                                header.redundancy(channel).map(<[bool]>::len),
                                Some(header.frame_count())
                            );
                        }
                    }
                    Err(error) => assert_eq!(error, SilkError::HeaderBudgetExceeded),
                }
            }
        }
    }
    Ok(())
}
