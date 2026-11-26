//! Retained histories and transactional promotion for the compact decoder.
use opura::{
    Mode, Packet,
    decoder::{CompactOpusDecoder, DecoderError, OpusDecoder},
    silk::{decoder::PcmChannels, resampler::OutputRate},
};
use std::error::Error;

fn recorded_packet(mode: Mode) -> Result<Vec<u8>, Box<dyn Error>> {
    for row in include_str!("decoder-pcm-fixtures.txt").lines() {
        if let Some(frame) = row.strip_prefix("frame normal ") {
            let hex = frame.split_whitespace().nth(2).ok_or("missing packet")?;
            let bytes = (0..hex.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&hex[i..i + 2], 16))
                .collect::<Result<Vec<_>, _>>()?;
            if Packet::parse(&bytes)?.mode() == mode {
                return Ok(bytes);
            }
        }
    }
    Err("no recorded packet for requested mode".into())
}

#[test]
fn speech_stays_compact_until_celt_is_used_and_reset_releases_it() -> Result<(), Box<dyn Error>> {
    let silk = recorded_packet(Mode::Silk)?;
    for first_mode in [Mode::Celt, Mode::Hybrid] {
        let first = recorded_packet(first_mode)?;
        let mut fixed = OpusDecoder::new(OutputRate::Khz16, PcmChannels::Mono)?;
        let mut compact = CompactOpusDecoder::new(OutputRate::Khz16, PcmChannels::Mono)?;
        fixed.set_gain_q8(128);
        compact.set_gain_q8(128);

        assert_eq!(compact.decode(&silk)?, fixed.decode(&silk)?);
        assert_eq!(compact.conceal_samples(960)?, fixed.conceal_samples(960)?);
        assert!(compact.celt().is_none());

        let speech = compact.clone();
        assert_eq!(compact.decode(&first)?, fixed.decode(&first)?);
        assert_eq!(compact.celt(), Some(fixed.celt()));
        assert!(speech.celt().is_none());
        assert_eq!(compact.decode(&silk)?, fixed.decode(&silk)?);

        fixed.reset();
        compact.reset();
        assert!(compact.celt().is_none());
        assert_eq!(compact.gain_q8(), 128);
        assert_eq!(compact.decode(&first)?, fixed.decode(&first)?);
        assert_eq!(compact.celt(), Some(fixed.celt()));
    }
    Ok(())
}

#[test]
fn failed_first_transition_preserves_speech_state_for_retry() -> Result<(), Box<dyn Error>> {
    let silk = recorded_packet(Mode::Silk)?;
    let hybrid = recorded_packet(Mode::Hybrid)?;
    let mut fixed = OpusDecoder::new(OutputRate::Khz16, PcmChannels::Mono)?;
    let mut compact = CompactOpusDecoder::new(OutputRate::Khz16, PcmChannels::Mono)?;
    assert_eq!(compact.decode(&silk)?, fixed.decode(&silk)?);
    let before = compact.clone();

    // Use the malformed stereo Hybrid payload from the FEC rollback regression.
    let corrupt = [0x64, 0xff, 0xff];
    let err = fixed
        .decode_fec(&corrupt, 1200)
        .err()
        .ok_or("malformed FEC payload was accepted")?;
    assert!(matches!(err, DecoderError::Silk(_)));
    assert_eq!(compact.decode_fec(&corrupt, 1200), Err(err));
    assert_eq!(compact, before);
    assert!(compact.celt().is_none());

    let mut short = [123; 1];
    assert_eq!(
        compact.decode_into(&hybrid, &mut short),
        fixed.decode_into(&hybrid, &mut [123])
    );
    assert_eq!(short, [123]);
    assert_eq!(compact, before);
    let expected = fixed.decode(&hybrid)?;
    let mut buffer = vec![0; expected.samples().len()];
    assert_eq!(compact.decode_into(&hybrid, &mut buffer)?, buffer.len());
    assert_eq!(buffer, expected.samples());
    assert_eq!(compact.final_range(), fixed.final_range());
    assert_eq!(compact.celt(), Some(fixed.celt()));
    Ok(())
}

#[test]
fn cold_silk_falls_back_for_redundant_celt_after_narrowband_speech() -> Result<(), Box<dyn Error>> {
    // Two packets from the native mixed-mode fixtures. Both signal SILK in the TOC.
    let narrow = [
        0x00, 0x82, 0x87, 0x78, 0xce, 0x04, 0x6f, 0x0c, 0xf4, 0xd8, 0xfb, 0xaa, 0x4e, 0x5e, 0xfe,
        0x14, 0x57, 0x77, 0x3d, 0xcf, 0xd2, 0x85, 0x5e, 0x30, 0x6e, 0xb8, 0x18,
    ];
    let redundant = [
        0x40, 0xf5, 0x9a, 0x13, 0xdf, 0x21, 0xe3, 0xb4, 0x49, 0x68, 0x6e, 0x2d, 0x01, 0x4a, 0xfd,
        0x78, 0x3b, 0xa8, 0x3b, 0xa4, 0xc8, 0x93, 0xde, 0x09, 0x0b, 0x76, 0x43, 0xf3, 0x00, 0x68,
        0x8e, 0x2e, 0x1e, 0x88, 0x1b, 0x2c, 0x22, 0xe1, 0x82, 0xfc,
    ];
    let mut pair = narrow.to_vec();
    pair[0] |= 1;
    pair.extend_from_slice(&narrow[1..]);
    let mut compact = CompactOpusDecoder::new(OutputRate::Khz16, PcmChannels::Mono)?;
    let mut fixed = OpusDecoder::new(OutputRate::Khz16, PcmChannels::Mono)?;
    compact.set_gain_q8(192);
    fixed.set_gain_q8(192);
    for (packet, retained) in [
        (&narrow[..], false),
        (&[0][..], false), // A 10 ms DTX packet.
        (&pair[..], false),
        (&redundant[..], true),
        (&narrow[..], true),
    ] {
        let expected = fixed.decode(packet)?;
        let mut pcm = [12345; 321];
        let n = compact.decode_into(packet, &mut pcm)?;
        assert_eq!(&pcm[..n], expected.samples());
        assert_eq!(pcm[n], 12345);
        assert_eq!(compact.final_range(), fixed.final_range());
        assert_eq!(compact.previous_redundancy(), fixed.previous_redundancy());
        assert_eq!(compact.celt().is_some(), retained);
        if retained {
            assert_eq!(compact.celt(), Some(fixed.celt()));
        }
    }
    Ok(())
}
