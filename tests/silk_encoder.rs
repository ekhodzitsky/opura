//! Original native frame bytes, pulse reconstruction and public packet decoding.
use opura::{
    decoder::OpusDecoder,
    range::{EncoderError, RangeDecoder, RangeEncoder},
    silk::{
        decoder::PcmChannels,
        encoder::{FrameContext, FrameEncoderState, FrameIndices, SilkEncodeError},
        excitation::decode_excitation,
        gains::{FrameActivity, FrameDuration, GainCoding, GainState, SignalType},
        nlsf::{NlsfOrder, NlsfState},
        pitch::{PitchCoding, PitchState, SilkRate},
        resampler::OutputRate,
    },
};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
fn parse_indices(values: &[i16]) -> FrameIndices {
    let mut gains = [0; 4];
    let mut nlsf = [0; 17];
    let mut ltp = [0; 4];
    for j in 0..4 {
        gains[j] = values[2 + j] as u8;
        ltp[j] = values[27 + j] as u8;
    }
    for j in 0..17 {
        nlsf[j] = values[6 + j] as i8;
    }
    FrameIndices {
        signal: match values[0] {
            0 => SignalType::Inactive,
            1 => SignalType::Unvoiced,
            _ => SignalType::Voiced,
        },
        offset: values[1] as u8,
        gains,
        nlsf,
        interpolation_q2: values[23] as u8,
        lag: values[24] as u16,
        contour: values[25] as u8,
        periodicity: values[26] as u8,
        ltp,
        ltp_scale: values[31] as u8,
        seed: values[32] as u8,
    }
}
fn fingerprint(samples: &[i16]) -> u64 {
    samples
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .fold(14695981039346656037, |h, b| {
            (h ^ u64::from(b)).wrapping_mul(1099511628211)
        })
}
#[test]
fn native_bytes_parameters_pulses_and_public_pcm_match() -> Result<()> {
    let mut packets = 0;
    let mut raw = 0;
    let mut rejected = 0;
    for (line_number, line) in include_str!("silk-encoder-fixtures.txt")
        .lines()
        .enumerate()
    {
        if line.starts_with('#') {
            continue;
        }
        let columns: Vec<_> = line.split_whitespace().collect();
        let meta: Vec<u64> = columns[..11]
            .iter()
            .map(|s| s.parse())
            .collect::<std::result::Result<_, _>>()?;
        let kind = meta[0];
        let khz = meta[1] as usize;
        let subframes = meta[2] as usize;
        let frames = meta[3] as usize;
        let n = khz * subframes * 5;
        let fields: Vec<i16> = columns[11..11 + frames * 33]
            .iter()
            .map(|s| s.parse())
            .collect::<std::result::Result<_, _>>()?;
        let indices: Vec<_> = fields.chunks_exact(33).map(parse_indices).collect();
        let pulses: Vec<i8> = columns[11 + frames * 33..11 + frames * (33 + n)]
            .iter()
            .map(|s| s.parse())
            .collect::<std::result::Result<_, _>>()?;
        let rate = match khz {
            8 => SilkRate::Khz8,
            12 => SilkRate::Khz12,
            _ => SilkRate::Khz16,
        };
        let duration = if subframes == 2 {
            FrameDuration::Ms10
        } else {
            FrameDuration::Ms20
        };
        let mut encoder = RangeEncoder::new(1275)?;
        if kind != 1 {
            for i in &indices {
                encoder.encode_bit_logp(i.signal != SignalType::Inactive, 1)?;
            }
            encoder.encode_bit_logp(false, 1)?;
        }
        let mut state = FrameEncoderState::default();
        let mut contexts = Vec::new();
        let mut failed = false;
        for (frame, i) in indices.iter().enumerate() {
            let context = FrameContext {
                rate,
                duration,
                activity: if i.signal == SignalType::Inactive {
                    FrameActivity::Inactive
                } else {
                    FrameActivity::Active
                },
                coding: if kind == 1 {
                    PitchCoding::IndependentWithoutScaling
                } else if frame == 0 {
                    PitchCoding::Independent
                } else {
                    PitchCoding::Conditional
                },
            };
            contexts.push(context);
            let saved_encoder = encoder.clone();
            let saved_state = state.clone();
            if let Err(e) = state.encode(
                &mut encoder,
                context,
                i,
                &pulses[frame * n..(frame + 1) * n],
            ) {
                assert_eq!(kind, 2, "line {}: {e}", line_number + 1);
                assert_eq!(e, SilkEncodeError::Entropy(EncoderError::CapacityExhausted));
                assert_eq!(encoder, saved_encoder);
                assert_eq!(state, saved_state);
                failed = true;
                break;
            }
        }
        if kind == 2 {
            assert!(failed, "native overflow line {}", line_number + 1);
            rejected += 1;
            continue;
        }
        assert!(!failed);
        assert_eq!(
            [
                u64::from(encoder.range()),
                u64::from(encoder.tell()),
                u64::from(encoder.tell_frac())
            ],
            meta[4..7],
            "line {}",
            line_number + 1
        );
        let hex = columns.last().ok_or("fixture payload absent")?;
        let bytes: Vec<u8> = (0..hex.len())
            .step_by(2)
            .map(|p| u8::from_str_radix(&hex[p..p + 2], 16))
            .collect::<std::result::Result<_, _>>()?;
        encoder.shrink(meta[7] as usize)?;
        assert_eq!(
            encoder.finish()?,
            bytes,
            "native bytes line {}",
            line_number + 1
        );
        let mut decoder = RangeDecoder::new(&bytes);
        if kind != 1 {
            for i in &indices {
                assert_eq!(
                    decoder.decode_bit_logp(1)?,
                    i.signal != SignalType::Inactive
                );
            }
            assert!(!decoder.decode_bit_logp(1)?);
        }
        let mut gains = GainState::default();
        let mut nlsf = NlsfState::default();
        let mut pitch = PitchState::default();
        for (frame, i) in indices.iter().enumerate() {
            let c = contexts[frame];
            let order = if khz == 16 {
                NlsfOrder::Order16
            } else {
                NlsfOrder::Order10
            };
            let decoded_gains = gains.decode(
                &mut decoder,
                duration,
                c.activity,
                if c.coding == PitchCoding::Conditional {
                    GainCoding::Conditional
                } else {
                    GainCoding::Independent
                },
            )?;
            assert_eq!(decoded_gains.signal_type(), i.signal);
            assert_eq!(decoded_gains.offset_type(), i.offset);
            assert_eq!(decoded_gains.indices(), &i.gains[..subframes]);
            let decoded_nlsf = nlsf.decode(&mut decoder, order, duration, i.signal)?;
            assert_eq!(
                decoded_nlsf.indices(),
                &i.nlsf[..if khz == 16 { 17 } else { 11 }]
            );
            assert_eq!(decoded_nlsf.coded_interpolation_q2(), i.interpolation_q2);
            let decoded_pitch = pitch.decode(&mut decoder, rate, duration, i.signal, c.coding)?;
            assert_eq!(decoded_pitch.seed(), i.seed);
            if i.signal == SignalType::Voiced {
                assert_eq!(decoded_pitch.lag_index(), Some(i.lag as i16));
                assert_eq!(decoded_pitch.contour_index(), i.contour);
                assert_eq!(decoded_pitch.periodicity_index(), i.periodicity);
                assert_eq!(decoded_pitch.ltp_indices(), &i.ltp[..subframes]);
                assert_eq!(decoded_pitch.scale_index(), i.ltp_scale);
            }
            let decoded =
                decode_excitation(&mut decoder, rate, duration, i.signal, i.offset, i.seed)?;
            assert_eq!(
                decoded.pulses(),
                &pulses[frame * n..(frame + 1) * n]
                    .iter()
                    .map(|&v| i16::from(v))
                    .collect::<Vec<_>>(),
                "line {}, frame {frame}",
                line_number + 1
            );
        }
        assert_eq!(u64::from(decoder.range()), meta[4]);
        assert_eq!(u64::from(decoder.tell()), meta[5]);
        if kind == 1 {
            raw += 1;
            continue;
        }
        let config = (if khz == 8 {
            0
        } else if khz == 12 {
            4
        } else {
            8
        }) + if subframes == 2 { 0 } else { frames };
        let mut packet = vec![(config as u8) << 3];
        packet.extend_from_slice(&bytes);
        let mut opus = OpusDecoder::new(OutputRate::Khz48, PcmChannels::Mono)?;
        let pcm = opus.decode(&packet)?;
        assert_eq!(pcm.samples().len(), meta[8] as usize);
        assert_eq!(u64::from(pcm.final_range()), meta[9]);
        assert_eq!(
            fingerprint(pcm.samples()),
            meta[10],
            "public PCM line {}",
            line_number + 1
        );
        packets += 1;
    }
    assert_eq!(packets + rejected, 72);
    assert_eq!(raw, 18);
    assert!(rejected > 0);
    Ok(())
}
fn simple() -> (FrameContext, FrameIndices) {
    (
        FrameContext {
            rate: SilkRate::Khz16,
            duration: FrameDuration::Ms20,
            activity: FrameActivity::Active,
            coding: PitchCoding::Independent,
        },
        FrameIndices {
            signal: SignalType::Unvoiced,
            offset: 0,
            gains: [20, 4, 4, 4],
            nlsf: [0; 17],
            interpolation_q2: 4,
            lag: 0,
            contour: 0,
            periodicity: 0,
            ltp: [0; 4],
            ltp_scale: 0,
            seed: 0,
        },
    )
}
#[test]
fn invalid_inputs_late_exhaustion_reset_and_finalized_writer_are_atomic() -> Result<()> {
    let (context, indices) = simple();
    let mut state = FrameEncoderState::default();
    let mut encoder = RangeEncoder::new(1275)?;
    let conditional = FrameContext {
        coding: PitchCoding::Conditional,
        ..context
    };
    assert_eq!(
        state.encode(&mut encoder, conditional, &indices, &[0; 320]),
        Err(SilkEncodeError::MissingHistory)
    );
    state.encode(&mut encoder, context, &indices, &[0; 320])?;
    let saved_state = state.clone();
    let saved_encoder = encoder.clone();
    for case in 0..8 {
        let mut invalid = indices.clone();
        match case {
            0 => invalid.offset = 2,
            1 => invalid.seed = 4,
            2 => invalid.gains[0] = 64,
            3 => invalid.gains[1] = 41,
            4 => invalid.nlsf[0] = -1,
            5 => invalid.nlsf[1] = 11,
            6 => invalid.interpolation_q2 = 5,
            _ => invalid.signal = SignalType::Inactive,
        }
        assert_eq!(
            state.encode(&mut encoder, context, &invalid, &[0; 320]),
            Err(SilkEncodeError::InvalidIndices)
        );
        assert_eq!(state, saved_state);
        assert_eq!(encoder, saved_encoder);
    }
    assert_eq!(
        state.encode(&mut encoder, context, &indices, &[0; 319]),
        Err(SilkEncodeError::InvalidPulseLength)
    );
    let changed = FrameContext {
        rate: SilkRate::Khz8,
        ..conditional
    };
    assert_eq!(
        state.encode(&mut encoder, changed, &indices, &[0; 160]),
        Err(SilkEncodeError::MissingHistory)
    );
    let mut small = RangeEncoder::new(32)?;
    small.encode_bit_logp(true, 1)?;
    let before = small.clone();
    assert_eq!(
        state.encode(&mut small, context, &indices, &[127; 320]),
        Err(SilkEncodeError::Entropy(EncoderError::CapacityExhausted))
    );
    assert_eq!(small, before);
    assert_eq!(state, saved_state);
    let mut finished = RangeEncoder::new(100)?;
    finished.finish()?;
    let before = finished.clone();
    assert_eq!(
        state.encode(&mut finished, context, &indices, &[0; 320]),
        Err(SilkEncodeError::Entropy(EncoderError::Finished))
    );
    assert_eq!(finished, before);
    assert_eq!(state, saved_state);
    state.reset();
    assert_eq!(state, FrameEncoderState::default());
    assert_eq!(
        state.encode(&mut encoder, conditional, &indices, &[0; 320]),
        Err(SilkEncodeError::MissingHistory)
    );
    Ok(())
}
