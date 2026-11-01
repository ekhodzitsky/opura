//! Independent native energy bitstreams, RDO histories and paired decoder states.
use opura::{
    celt::{
        FrameSize,
        energy::{BandLayout, CoarseEncoding, EnergyEncoder, EnergyEncodingError, EnergyState},
    },
    range::{EncoderError, RangeDecoder, RangeEncoder},
};
const SIZES: [FrameSize; 4] = [
    FrameSize::Ms2_5,
    FrameSize::Ms5,
    FrameSize::Ms10,
    FrameSize::Ms20,
];

fn check_encoded(
    energy: &EnergyEncoder,
    writer: &RangeEncoder,
    expected: &[i64],
) -> Result<(), Box<dyn std::error::Error>> {
    assert_eq!(expected.len(), 87);
    for c in 0..2 {
        for (i, &value) in energy
            .energies()
            .channel(c)
            .ok_or("missing energy channel")?
            .iter()
            .enumerate()
        {
            assert_eq!(i64::from(value), expected[c * 21 + i], "energy c{c} b{i}");
        }
        for (i, &value) in energy
            .residuals(c)
            .ok_or("missing residual channel")?
            .iter()
            .enumerate()
        {
            assert_eq!(
                i64::from(value),
                expected[42 + c * 21 + i],
                "residual c{c} b{i}"
            );
        }
    }
    assert_eq!(
        [
            i64::from(writer.range()),
            i64::from(writer.tell()),
            i64::from(writer.tell_frac())
        ],
        expected[84..]
    );
    Ok(())
}
fn check_decoded(
    energy: &EnergyState,
    decoder: &RangeDecoder<'_>,
    expected: &[i64],
) -> Result<(), Box<dyn std::error::Error>> {
    assert_eq!(expected.len(), 45);
    for c in 0..2 {
        for (i, &value) in energy
            .channel(c)
            .ok_or("missing decoded channel")?
            .iter()
            .enumerate()
        {
            assert_eq!(i64::from(value), expected[c * 21 + i], "decoded c{c} b{i}");
        }
    }
    assert_eq!(
        [
            i64::from(decoder.range()),
            i64::from(decoder.tell()),
            i64::from(decoder.tell_frac())
        ],
        expected[42..]
    );
    Ok(())
}

#[test]
fn original_complete_energy_packets_and_encoder_decoder_histories_match()
-> Result<(), Box<dyn std::error::Error>> {
    let mut state = EnergyEncoder::default();
    let mut decoded = EnergyState::default();
    let mut rows = 0;
    let mut intra_count = 0;
    let mut inter_count = 0;
    let mut histories_differ = 0;
    let mut starts = [false; 22];
    let mut sizes = [false; 4];
    let mut fine_values = [false; 9];
    for (index, line) in include_str!("celt-energy-encoder-fixtures.txt")
        .lines()
        .enumerate()
    {
        if line.starts_with('#') {
            continue;
        }
        let cols: Vec<_> = line.split_whitespace().collect();
        assert_eq!(cols[0], "packet");
        let values: Vec<i64> = cols[1..cols.len() - 1]
            .iter()
            .map(|s| s.parse())
            .collect::<Result<_, _>>()?;
        assert_eq!(values.len(), 499);
        let p = &values[..15];
        if p[0] != 0 {
            state.reset();
            decoded.reset();
        }
        let lm = p[1] as usize;
        let channels = p[2] as usize;
        let start = p[3] as usize;
        let end = p[4] as usize;
        let layout = BandLayout::new(channels, start, end)?;
        let mut bits = [0u8; 21];
        let mut priority = [false; 21];
        let mut targets = [[0i32; 21]; 2];
        for i in 0..21 {
            bits[i] = values[15 + i] as u8;
            priority[i] = values[36 + i] != 0;
            fine_values[bits[i] as usize] = true;
        }
        for c in 0..2 {
            for i in 0..21 {
                targets[c][i] = values[57 + c * 21 + i] as i32;
            }
        }
        let config = CoarseEncoding {
            layout,
            size: SIZES[lm],
            effective_end: p[5] as usize,
            available_bytes: p[7] as usize,
            force_intra: p[8] != 0,
            two_pass: p[9] != 0,
            loss_rate: p[10] as u8,
            lfe: p[11] != 0,
        };
        let mut writer = RangeEncoder::new(p[6] as usize)?;
        if p[12] != 0 {
            writer.encode_uint(p[13] as u32, 1 << p[12])?;
        }
        let intra = state.coarse(&mut writer, &targets, config)?;
        assert_eq!(intra, values[361] != 0, "intra row{index}");
        assert_eq!(
            state.delayed_intra(),
            values[360] as i32,
            "delayed row{index}"
        );
        check_encoded(&state, &writer, &values[99..186])?;
        state.fine(&mut writer, layout, &bits)?;
        check_encoded(&state, &writer, &values[186..273])?;
        let left = values[362] as u32;
        assert_eq!(
            state.finalise(&mut writer, layout, &bits, &priority, left)?,
            values[363] as u32
        );
        check_encoded(&state, &writer, &values[273..360])?;
        let hex = cols[cols.len() - 1];
        let expected: Vec<u8> = (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16))
            .collect::<Result<_, _>>()?;
        assert_eq!(writer.finish()?, expected, "bytes row{index}");
        let mut decoder = RangeDecoder::new(&expected);
        if p[12] != 0 {
            assert_eq!(decoder.decode_uint(1 << p[12])?, p[13] as u32);
        }
        let read_intra = if decoder.tell() + 3 <= p[6] as u32 * 8 {
            decoder.decode_bit_logp(3)?
        } else {
            false
        };
        assert_eq!(read_intra, intra);
        decoded.coarse(&mut decoder, layout, SIZES[lm], read_intra)?;
        check_decoded(&decoded, &decoder, &values[364..409])?;
        decoded.fine(&mut decoder, layout, &bits)?;
        check_decoded(&decoded, &decoder, &values[409..454])?;
        assert_eq!(
            decoded.finalise(&mut decoder, layout, &bits, &priority, left)?,
            values[363] as u32
        );
        check_decoded(&decoded, &decoder, &values[454..499])?;
        if state.energies() != &decoded {
            histories_differ += 1;
        }
        if intra {
            intra_count += 1;
        } else {
            inter_count += 1;
        }
        starts[start] = true;
        sizes[lm] = true;
        rows += 1;
    }
    assert_eq!(rows, 388);
    assert_eq!((intra_count, inter_count), (212, 176));
    assert_eq!(histories_differ, 11);
    assert!(sizes.iter().all(|&v| v));
    assert!(fine_values.iter().all(|&v| v));
    assert!(starts[0] && starts[5] && starts[17] && starts[21]);
    assert_eq!(state.residuals(2), None);
    Ok(())
}

fn options(layout: BandLayout) -> CoarseEncoding {
    CoarseEncoding {
        layout,
        size: FrameSize::Ms20,
        effective_end: layout.bands().end,
        available_bytes: 64,
        force_intra: false,
        two_pass: true,
        loss_rate: 10,
        lfe: false,
    }
}
#[test]
fn invalid_options_targets_and_lifecycle_leave_both_states_unchanged()
-> Result<(), Box<dyn std::error::Error>> {
    let layout = BandLayout::new(2, 0, 21)?;
    let mut state = EnergyEncoder::default();
    let mut writer = RangeEncoder::new(64)?;
    state.coarse(&mut writer, &[[-10 << 24; 21]; 2], options(layout))?;
    let before = state.clone();
    let original = writer.clone();
    for config in [
        CoarseEncoding {
            effective_end: 22,
            ..options(layout)
        },
        CoarseEncoding {
            available_bytes: 65,
            ..options(layout)
        },
        CoarseEncoding {
            loss_rate: 101,
            ..options(layout)
        },
    ] {
        assert_eq!(
            state.coarse(&mut writer, &[[0; 21]; 2], config),
            Err(EnergyEncodingError::InvalidConfiguration)
        );
        assert_eq!(state, before);
        assert_eq!(writer, original);
    }
    for value in [i32::MIN, (-(28 << 24)) - 1, (16 << 24) + 1, i32::MAX] {
        let mut targets = [[0; 21]; 2];
        targets[1][20] = value;
        assert_eq!(
            state.coarse(&mut writer, &targets, options(layout)),
            Err(EnergyEncodingError::InvalidEnergy)
        );
        assert_eq!(state, before);
        assert_eq!(writer, original);
    }
    let mut bad = [0; 21];
    bad[20] = 9;
    assert_eq!(
        state.fine(&mut writer, layout, &bad),
        Err(EnergyEncodingError::InvalidFineBits)
    );
    assert_eq!(
        state.finalise(&mut writer, layout, &bad, &[false; 21], 0),
        Err(EnergyEncodingError::InvalidFineBits)
    );
    assert_eq!(
        state.finalise(&mut writer, layout, &[0; 21], &[false; 21], 513),
        Err(EnergyEncodingError::InvalidBudget)
    );
    assert_eq!(state, before);
    assert_eq!(writer, original);
    writer.finish()?;
    let finished = writer.clone();
    assert_eq!(
        state.coarse(&mut writer, &[[0; 21]; 2], options(layout)),
        Err(EnergyEncodingError::Entropy(EncoderError::Finished))
    );
    assert_eq!(
        state.fine(&mut writer, layout, &[0; 21]),
        Err(EnergyEncodingError::Entropy(EncoderError::Finished))
    );
    assert_eq!(
        state.finalise(&mut writer, layout, &[0; 21], &[false; 21], 0),
        Err(EnergyEncodingError::Entropy(EncoderError::Finished))
    );
    assert_eq!(state, before);
    assert_eq!(writer, finished);
    state.reset();
    assert_eq!(state, EnergyEncoder::default());
    Ok(())
}

#[test]
fn matching_energy_stage_contexts_round_trip_through_normal_packet_budgets()
-> Result<(), Box<dyn std::error::Error>> {
    let mut state = EnergyEncoder::default();
    let mut decoded = EnergyState::default();
    for frame in 0..96 {
        if frame % 17 == 0 {
            state.reset();
            decoded.reset();
        }
        let channels = 1 + (frame / 3) % 2;
        let start = if frame % 5 == 0 { 17 } else { 0 };
        let layout = BandLayout::new(channels, start, 21)?;
        let size = SIZES[frame % 4];
        let mut writer = RangeEncoder::new([32, 64, 128][frame % 3])?;
        let mut target = [[i32::MIN; 21]; 2];
        for (channel, values) in target.iter_mut().enumerate().take(channels) {
            for (band, value) in values.iter_mut().enumerate().skip(start) {
                *value = (-24 + ((band * 7 + frame + channel * 11) % 37) as i32) << 24;
            }
        }
        let config = CoarseEncoding {
            layout,
            size,
            effective_end: 21,
            available_bytes: writer.capacity(),
            force_intra: frame % 11 == 0,
            two_pass: frame % 3 != 0,
            loss_rate: [0, 10, 30, 100][frame % 4],
            lfe: frame % 13 == 0,
        };
        let intra = state.coarse(&mut writer, &target, config)?;
        let coarse = state.energies().clone();
        let bits = std::array::from_fn(|i| ((i + frame) % 9) as u8);
        let priority = std::array::from_fn(|i| (i + frame) % 2 != 0);
        state.fine(&mut writer, layout, &bits)?;
        let fine = state.energies().clone();
        let left = (writer.capacity() as u32 * 8)
            .saturating_sub(writer.tell())
            .min(20);
        let unused = state.finalise(&mut writer, layout, &bits, &priority, left)?;
        let range = writer.range();
        let tell = writer.tell();
        let fraction = writer.tell_frac();
        let data = writer.finish()?;
        let mut entropy = RangeDecoder::new(data);
        let flag = entropy.decode_bit_logp(3)?;
        assert_eq!(flag, intra);
        decoded.coarse(&mut entropy, layout, size, intra)?;
        assert_eq!(decoded, coarse, "coarse frame{frame}");
        decoded.fine(&mut entropy, layout, &bits)?;
        assert_eq!(decoded, fine, "fine frame{frame}");
        assert_eq!(
            decoded.finalise(&mut entropy, layout, &bits, &priority, left)?,
            unused
        );
        assert_eq!(&decoded, state.energies(), "final frame{frame}");
        assert_eq!(
            (entropy.range(), entropy.tell(), entropy.tell_frac()),
            (range, tell, fraction)
        );
    }
    Ok(())
}
