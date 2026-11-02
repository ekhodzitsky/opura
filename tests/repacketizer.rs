//! Original public-native packet bytes, padding, flattening and atomic failures.
use opura::{
    Packet, PacketError,
    repacketizer::{Repacketizer, RepacketizerError},
};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[test]
fn original_public_repacketizer_and_padding_bytes_match() -> Result<()> {
    let boundaries = [0, 1, 2, 251, 252, 253, 254, 255, 256, 1274, 1275];
    let mut rows = 0;
    for line in include_str!("repacketizer-fixtures.txt")
        .lines()
        .filter(|l| !l.starts_with('#'))
    {
        let [toc, count, pattern, begin, end, extra, length, expected]: [u64; 8] = line
            .split_whitespace()
            .map(str::parse)
            .collect::<std::result::Result<Vec<_>, _>>()?
            .try_into()
            .map_err(|_| "invalid fixture row")?;
        let toc = toc as u8;
        let count = count as usize;
        let begin = begin as usize;
        let end = end as usize;
        let length = length as usize;
        let sources: Vec<Vec<u8>> = (0..count)
            .map(|frame| {
                let length = match pattern {
                    0..=4 => [0, 2, 251, 252, 1275][pattern as usize],
                    5 => [0, 1275][frame % 2],
                    6 => boundaries[(frame + usize::from(toc >> 3)) % boundaries.len()],
                    _ => (frame * 17 + usize::from(toc)) % 1276,
                };
                std::iter::once(toc)
                    .chain((0..length).map(|i| (i * 37 + frame * 19 + usize::from(toc) * 3) as u8))
                    .collect()
            })
            .collect();
        let mut rp = Repacketizer::new();
        for packet in &sources {
            rp.add_packet(packet)?;
        }
        let before = rp.clone();
        let mut output = vec![0xa5; 65544];
        let actual = if begin == 0 && end == count {
            if extra == 0 {
                rp.write(&mut output)?
            } else {
                rp.write_padded(&mut output, length)?
            }
        } else {
            rp.write_range(
                begin..end,
                &mut output,
                if extra == 0 { None } else { Some(length) },
            )?
        };
        assert_eq!(actual, length, "{line}");
        let fingerprint = output[..actual]
            .iter()
            .fold(14695981039346656037u64, |h, &b| {
                (h ^ u64::from(b)).wrapping_mul(1099511628211)
            });
        assert_eq!(fingerprint, expected, "{line}");
        assert_eq!(&output[actual..actual + 8], &[0xa5; 8]);
        assert_eq!(rp, before);
        let parsed = Packet::parse(&output[..actual])?;
        assert_eq!(parsed.frames(), &rp.frames()[begin..end]);
        assert_eq!(
            parsed.duration_samples(),
            parsed.frame_samples() * (end - begin) as u16
        );
        rows += 1;
    }
    assert_eq!(rows, 32816);
    Ok(())
}

#[test]
fn invalid_appends_ranges_and_output_limits_preserve_all_state() -> Result<()> {
    let mut rp = Repacketizer::new();
    let mut output = [0xa5; 128];
    assert_eq!(rp.write(&mut output), Err(RepacketizerError::NoFrames));
    assert_eq!(output, [0xa5; 128]);
    let frame = [0x80, 1, 2];
    for _ in 0..48 {
        rp.add_packet(&frame)?;
    }
    assert_eq!((rp.frame_count(), rp.duration_samples()), (48, 5760));
    let before = rp.clone();
    assert_eq!(
        rp.add_packet(&frame),
        Err(RepacketizerError::DurationTooLong)
    );
    assert_eq!(rp, before);
    for bytes in [&[][..], &[0x83, 0][..], &[0x83, 0x41, 255][..]] {
        assert!(matches!(
            rp.add_packet(bytes),
            Err(RepacketizerError::Packet(_))
        ));
        assert_eq!(rp, before);
    }
    let oversized = vec![0x80; 1277];
    assert_eq!(
        rp.add_packet(&oversized),
        Err(RepacketizerError::Packet(PacketError::FrameTooLarge))
    );
    assert_eq!(rp, before);
    assert_eq!(
        rp.add_packet(&[0x84, 1, 2]),
        Err(RepacketizerError::IncompatibleConfiguration)
    );
    assert_eq!(rp, before);
    for range in [
        0..0,
        std::ops::Range { start: 1, end: 0 },
        48..49,
        0..49,
        0..usize::MAX,
    ] {
        assert_eq!(
            rp.write_range(range, &mut output, None),
            Err(RepacketizerError::InvalidRange)
        );
        assert_eq!(output, [0xa5; 128]);
    }
    for capacity in [0, 1, 2, 97] {
        assert_eq!(
            rp.write(&mut output[..capacity]),
            Err(RepacketizerError::BufferTooSmall)
        );
        assert_eq!(output, [0xa5; 128]);
    }
    for target in [0, 1, 97, 129, usize::MAX] {
        assert_eq!(
            rp.write_padded(&mut output, target),
            Err(RepacketizerError::BufferTooSmall)
        );
        assert_eq!(output, [0xa5; 128]);
    }
    assert_eq!(rp, before);
    rp.reset();
    assert!(rp.is_empty());
    assert_eq!(rp.duration_samples(), 0);
    rp.add_packet(&[0x84, 1, 2])?;
    assert_eq!(rp.frame_count(), 1);
    // Duration limits must also reject fewer than48 frames; the maximum-count
    // 2.5ms case alone would not catch a missing combined-duration check.
    for (toc, limit) in [(0x18, 2), (0x68, 6), (0x98, 6)] {
        let frame = [toc, 1, 2];
        let mut bounded = Repacketizer::new();
        for _ in 0..limit {
            bounded.add_packet(&frame)?;
        }
        assert_eq!(bounded.duration_samples(), 5760);
        let before = bounded.clone();
        assert_eq!(
            bounded.add_packet(&frame),
            Err(RepacketizerError::DurationTooLong)
        );
        assert_eq!(bounded, before);
    }
    Ok(())
}

#[test]
fn already_grouped_packets_and_transport_padding_are_flattened() -> Result<()> {
    let a = [0x80, 1, 2];
    let b = [0x80, 3];
    let mut rp = Repacketizer::new();
    rp.add_packet(&a)?;
    rp.add_packet(&b)?;
    let mut output = [0; 1024];
    let length = rp.write_padded(&mut output, 512)?;
    let mut second = Repacketizer::new();
    second.add_packet(&output[..length])?;
    assert_eq!(second.frames(), rp.frames());
    let mut compact = [0xa5; 16];
    let length = second.write(&mut compact)?;
    assert_eq!(&compact[..length], &[0x82, 2, 1, 2, 3]);
    assert!(compact[length..].iter().all(|&v| v == 0xa5));
    second.add_packet(&a)?;
    assert_eq!(second.frame_count(), 3);
    assert_eq!(second.duration_samples(), 360);
    Ok(())
}
