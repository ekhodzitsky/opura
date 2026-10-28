//! Packet framing contracts and malformed-input regression tests.

use opura::{Bandwidth, Mode, Packet, PacketError};

#[test]
fn toc_configurations() -> Result<(), PacketError> {
    let groups = [
        (
            0,
            4,
            Mode::Silk,
            Bandwidth::Narrowband,
            &[480, 960, 1920, 2880][..],
        ),
        (
            4,
            4,
            Mode::Silk,
            Bandwidth::Mediumband,
            &[480, 960, 1920, 2880][..],
        ),
        (
            8,
            4,
            Mode::Silk,
            Bandwidth::Wideband,
            &[480, 960, 1920, 2880][..],
        ),
        (
            12,
            2,
            Mode::Hybrid,
            Bandwidth::SuperWideband,
            &[480, 960][..],
        ),
        (14, 2, Mode::Hybrid, Bandwidth::Fullband, &[480, 960][..]),
        (
            16,
            4,
            Mode::Celt,
            Bandwidth::Narrowband,
            &[120, 240, 480, 960][..],
        ),
        (
            20,
            4,
            Mode::Celt,
            Bandwidth::Wideband,
            &[120, 240, 480, 960][..],
        ),
        (
            24,
            4,
            Mode::Celt,
            Bandwidth::SuperWideband,
            &[120, 240, 480, 960][..],
        ),
        (
            28,
            4,
            Mode::Celt,
            Bandwidth::Fullband,
            &[120, 240, 480, 960][..],
        ),
    ];
    for (start, count, mode, bandwidth, durations) in groups {
        for index in 0..count {
            for stereo in 0..=1 {
                let bytes = [(start + index) * 8 + stereo * 4];
                let p = Packet::parse(&bytes)?;
                assert_eq!(p.configuration(), start + index);
                assert_eq!(p.mode(), mode);
                assert_eq!(p.bandwidth(), bandwidth);
                assert_eq!(p.channels(), stereo + 1);
                assert_eq!(p.frame_samples(), durations[usize::from(index)]);
                assert_eq!(p.duration_samples(), p.frame_samples());
                assert_eq!(p.frames(), &[&[][..]]);
            }
        }
    }
    Ok(())
}

#[test]
fn packing_and_padding() -> Result<(), PacketError> {
    for (input, frames) in [
        (&[0, 11, 12][..], vec![&[11, 12][..]]),
        (&[1, 11, 12][..], vec![&[11][..], &[12][..]]),
        (&[2, 1, 11, 12, 13][..], vec![&[11][..], &[12, 13][..]]),
        (
            &[3, 3, 11, 12, 13][..],
            vec![&[11][..], &[12][..], &[13][..]],
        ),
        (
            &[3, 0x83, 0, 1, 11, 12, 13][..],
            vec![&[][..], &[11][..], &[12, 13][..]],
        ),
        (
            &[3, 0xc2, 2, 1, 11, 12, 99, 98][..],
            vec![&[11][..], &[12][..]],
        ),
        (&[3, 0x41, 0, 11][..], vec![&[11][..]]),
        (&[3, 0x81, 11][..], vec![&[11][..]]),
        (&[1][..], vec![&[][..], &[][..]]),
        (&[2, 0][..], vec![&[][..], &[][..]]),
    ] {
        assert_eq!(Packet::parse(input)?.frames(), frames);
    }
    let mut chained = vec![3, 0x41, 255, 0, 42];
    chained.extend([123; 254]);
    assert_eq!(Packet::parse(&chained)?.frames(), &[&[42][..]]);
    Ok(())
}

#[test]
fn malformed_packets() {
    for (input, error) in [
        (&[][..], PacketError::EmptyPacket),
        (&[1, 0][..], PacketError::InvalidFrameLength),
        (&[2][..], PacketError::TruncatedPacket),
        (&[2, 252][..], PacketError::TruncatedPacket),
        (&[2, 1][..], PacketError::TruncatedPacket),
        (&[3][..], PacketError::TruncatedPacket),
        (&[3, 0][..], PacketError::InvalidFrameCount),
        (&[0x83, 49][..], PacketError::InvalidFrameCount),
        (&[27, 3][..], PacketError::DurationTooLong),
        (&[3, 2, 11][..], PacketError::InvalidFrameLength),
        (&[3, 0x41][..], PacketError::TruncatedPacket),
        (&[3, 0x41, 255][..], PacketError::TruncatedPacket),
        (&[3, 0x41, 2, 0][..], PacketError::TruncatedPacket),
        (&[3, 0x82][..], PacketError::TruncatedPacket),
        (&[3, 0x82, 252][..], PacketError::TruncatedPacket),
        (&[3, 0x83, 2, 2, 0][..], PacketError::TruncatedPacket),
        (&[3, 0xc2, 1, 252, 0][..], PacketError::TruncatedPacket),
    ] {
        assert_eq!(Packet::parse(input).err(), Some(error), "{input:?}");
    }
}

#[test]
fn length_and_duration_limits() -> Result<(), PacketError> {
    for size in [0, 1, 251, 252, 253, 254, 255, 256, 1274, 1275, 1276] {
        for code in [0, 1, 2, 3] {
            let mut input = match code {
                0 | 1 => vec![code],
                2 => vec![2, 0],
                _ => vec![3, 1],
            };
            input.resize(input.len() + size * if code == 1 { 2 } else { 1 }, 42);
            let result = Packet::parse(&input);
            if size <= 1275 {
                assert_eq!(result?.frames().last().map(|f| f.len()), Some(size));
            } else {
                assert_eq!(result.err(), Some(PacketError::FrameTooLarge));
            }
        }
    }
    for size in [252usize, 253, 254, 255, 256, 1275] {
        let mut input = vec![2, (252 + size % 4) as u8, ((size - 252) / 4) as u8];
        input.resize(3 + size, 42);
        assert_eq!(Packet::parse(&input)?.frames(), &[&input[3..], &[][..]]);
    }
    for config in 0..32 {
        let toc = [config << 3];
        let duration = Packet::parse(&toc)?.frame_samples();
        let max = (5760 / duration) as u8;
        let bytes = [(config << 3) | 3, max];
        assert_eq!(Packet::parse(&bytes)?.duration_samples(), 5760);
        let invalid = [(config << 3) | 3, max + 1];
        assert!(Packet::parse(&invalid).is_err());
    }
    Ok(())
}

#[test]
fn all_signaled_counts_obey_duration_limit() -> Result<(), PacketError> {
    for config in 0..32 {
        let toc = [config << 3];
        let samples = u32::from(Packet::parse(&toc)?.frame_samples());
        for count in 0..64 {
            for flags in [0, 0x80] {
                let mut input = vec![(config << 3) | 3, count | flags];
                if flags != 0 && count > 0 {
                    input.resize(2 + usize::from(count - 1), 0);
                }
                assert_eq!(
                    Packet::parse(&input).is_ok(),
                    count > 0 && samples * u32::from(count) <= 5760,
                    "config={config} count={count} flags={flags}"
                );
            }
        }
    }
    Ok(())
}

#[test]
fn libopus_reference_fixtures() -> Result<(), Box<dyn std::error::Error>> {
    for (line_number, line) in include_str!("packet-fixtures.txt").lines().enumerate() {
        if line.starts_with('#') {
            continue;
        }
        let (encoded, expected) = line.split_once(' ').ok_or("invalid fixture line")?;
        let mut input = Vec::new();
        if encoded != "-" {
            for chunk in encoded.split(',') {
                if let Some((byte, count)) = chunk.split_once('*') {
                    input.extend(std::iter::repeat_n(
                        u8::from_str_radix(byte, 16)?,
                        count.parse()?,
                    ));
                } else {
                    for i in (0..chunk.len()).step_by(2) {
                        input.push(u8::from_str_radix(&chunk[i..i + 2], 16)?);
                    }
                }
            }
        }
        let result = Packet::parse(&input);
        if expected == "error" {
            assert!(result.is_err(), "fixture line {}", line_number + 1);
            continue;
        }
        let packet = result?;
        let bandwidth = match packet.bandwidth() {
            Bandwidth::Narrowband => 1101,
            Bandwidth::Mediumband => 1102,
            Bandwidth::Wideband => 1103,
            Bandwidth::SuperWideband => 1104,
            Bandwidth::Fullband => 1105,
        };
        let mut actual = format!(
            "{},{},{bandwidth}",
            packet.duration_samples(),
            packet.channels()
        );
        for frame in packet.frames() {
            let offset = frame.as_ptr() as usize - input.as_ptr() as usize;
            actual.push_str(&format!(",{offset}:{}", frame.len()));
        }
        assert_eq!(actual, expected, "fixture line {}", line_number + 1);
    }
    Ok(())
}

fn check_invariants(input: &[u8]) {
    if let Ok(packet) = Packet::parse(input) {
        assert!((1..=48).contains(&packet.frames().len()));
        assert!((120..=5760).contains(&packet.duration_samples()));
        let base = input.as_ptr() as usize;
        let mut previous_end = base + 1;
        for frame in packet.frames() {
            let start = frame.as_ptr() as usize;
            assert!(start >= previous_end);
            assert!(start + frame.len() <= base + input.len());
            assert!(frame.len() <= 1275);
            previous_end = start + frame.len();
        }
    }
}

#[test]
fn arbitrary_input_has_bounded_valid_views_or_errors() {
    check_invariants(&[]);
    for first in 0..=255 {
        check_invariants(&[first]);
        for second in 0..=255 {
            check_invariants(&[first, second]);
        }
    }
    // Fixed seed and deterministic bytes preserve failures across platforms.
    let mut state = 0x91a2_b3c4_u32;
    for case in 0..30_000 {
        let len = case % 4096;
        let mut input = Vec::with_capacity(len);
        for _ in 0..len {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            input.push(state as u8);
        }
        check_invariants(&input);
    }
    // Long padding must not consume stack proportional to the header length.
    let mut padded = vec![3, 0x41];
    padded.extend(std::iter::repeat_n(255, 4096));
    padded.push(0);
    padded.extend(std::iter::repeat_n(123, 254 * 4096));
    assert!(Packet::parse(&padded).is_ok());
    padded.pop();
    assert!(Packet::parse(&padded).is_err());
}
