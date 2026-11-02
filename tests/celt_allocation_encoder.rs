//! Native allocation encoder streams, effective budgets and field policies.
use opura::{
    celt::{
        FrameSize,
        allocation::{self, AllocationEncoding, AllocationError},
        energy::BandLayout,
    },
    range::{EncoderError, RangeDecoder, RangeEncoder},
};
const SIZES: [FrameSize; 4] = [
    FrameSize::Ms2_5,
    FrameSize::Ms5,
    FrameSize::Ms10,
    FrameSize::Ms20,
];
fn prefix(
    writer: &mut RangeEncoder,
    position: usize,
    target: u32,
    pattern: u32,
) -> Result<(), EncoderError> {
    if position == 1 {
        writer.encode_uint(13, 17)?;
    }
    if position == 2 {
        for i in 0..3 {
            writer.encode_bit_logp(i % 2 != 0, 2 + i)?;
        }
    }
    let mut random = 0x54464354u32 + pattern;
    while writer.tell() < target {
        let width = (target - writer.tell()).min(25);
        random = random.wrapping_mul(1664525).wrapping_add(1013904223);
        let value = match pattern {
            0 => 0,
            1 => u32::MAX,
            _ => random,
        } & ((1 << width) - 1);
        writer.encode_bits(value, width)?;
    }
    Ok(())
}
fn read_prefix(
    dec: &mut RangeDecoder<'_>,
    position: usize,
    target: u32,
    pattern: u32,
) -> Result<(), Box<dyn std::error::Error>> {
    if position == 1 {
        assert_eq!(dec.decode_uint(17)?, 13);
    }
    if position == 2 {
        for i in 0..3 {
            assert_eq!(dec.decode_bit_logp(2 + i)?, i % 2 != 0);
        }
    }
    let mut random = 0x54464354u32 + pattern;
    while dec.tell() < target {
        let width = (target - dec.tell()).min(25);
        random = random.wrapping_mul(1664525).wrapping_add(1013904223);
        let value = match pattern {
            0 => 0,
            1 => u32::MAX,
            _ => random,
        } & ((1 << width) - 1);
        assert_eq!(dec.decode_bits(width)?, value);
    }
    Ok(())
}
#[test]
fn original_allocation_bytes_effective_arrays_policies_and_counters_match()
-> Result<(), Box<dyn std::error::Error>> {
    let mut rows = 0;
    let mut layouts = std::collections::BTreeSet::new();
    let mut dual = 0;
    let mut anti = 0;
    let mut boosted = 0;
    let mut skipped = 0;
    for (line_index, line) in include_str!("celt-allocation-encoder-fixtures.txt")
        .lines()
        .enumerate()
    {
        if line.starts_with('#') {
            continue;
        }
        let c: Vec<_> = line.split_whitespace().collect();
        assert_eq!(c[0], "frame");
        let lm = c[1].parse::<usize>()?;
        let channels = c[2].parse::<usize>()?;
        let start = c[3].parse::<usize>()?;
        let end = c[4].parse::<usize>()?;
        let transient = c[5] == "1";
        let position = c[6].parse::<usize>()?;
        let remaining = c[7].parse::<u32>()?;
        let pattern = c[8].parse::<u32>()?;
        let data: Vec<u8> = (0..c[14].len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&c[14][i..i + 2], 16))
            .collect::<Result<_, _>>()?;
        let boosts: Vec<u8> = c[15..36]
            .iter()
            .map(|s| s.parse())
            .collect::<Result<_, _>>()?;
        let options = AllocationEncoding {
            boosts: std::array::from_fn(|i| boosts[i]),
            trim: c[9].parse()?,
            intensity: c[10].parse()?,
            dual_stereo: c[11] == "1",
            previous_coded_bands: c[12].parse()?,
            signal_bandwidth: c[13].parse()?,
        };
        let expected: Vec<i64> = c[36..]
            .iter()
            .map(|s| s.parse())
            .collect::<Result<_, _>>()?;
        assert_eq!(expected.len(), 118);
        let layout = BandLayout::new(channels, start, end)?;
        let size = SIZES[lm];
        let mut writer = RangeEncoder::new(data.len())?;
        let target = data.len() as u32 * 8 - remaining;
        prefix(&mut writer, position, target, pattern)?;
        assert_eq!(
            [
                i64::from(writer.range()),
                i64::from(writer.tell()),
                i64::from(writer.tell_frac())
            ],
            expected[115..118]
        );
        let result = allocation::encode_allocation(&mut writer, size, layout, transient, options)?;
        assert_eq!(
            [
                i64::from(result.transient()),
                i64::from(result.trim()),
                i64::from(result.anti_collapse_reserved()) * 8,
                result.coded_bands() as i64,
                result.intensity() as i64,
                i64::from(result.dual_stereo()),
                i64::from(result.balance()),
                i64::from(writer.range()),
                i64::from(writer.tell()),
                i64::from(writer.tell_frac())
            ],
            expected[..10],
            "state line{line_index}"
        );
        for i in 0..21 {
            assert_eq!(
                [
                    i64::from(result.caps()[i]),
                    i64::from(result.offsets()[i]),
                    i64::from(result.pvq_bits()[i]),
                    i64::from(result.fine_bits()[i]),
                    i64::from(result.fine_priority()[i])
                ],
                [
                    expected[10 + i],
                    expected[31 + i],
                    expected[52 + i],
                    expected[73 + i],
                    expected[94 + i]
                ],
                "budgets line{line_index} band{i}"
            );
        }
        assert_eq!(writer.finish()?, data, "bytes line{line_index}");
        let mut decoder = RangeDecoder::new(&data);
        read_prefix(&mut decoder, position, target, pattern)?;
        let decoded = allocation::decode_allocation(&mut decoder, size, layout, transient)?;
        assert_eq!(decoded, result);
        assert_eq!(
            [decoder.range(), decoder.tell(), decoder.tell_frac()],
            [writer.range(), writer.tell(), writer.tell_frac()]
        );
        layouts.insert((lm, channels, start, end, transient));
        dual += usize::from(result.dual_stereo());
        anti += usize::from(result.anti_collapse_reserved());
        boosted += usize::from(result.offsets().iter().any(|&b| b > 0));
        skipped += usize::from(result.coded_bands() < end);
        rows += 1;
    }
    eprintln!(
        "matched {rows} allocations, {} layouts, dual{dual}, anti{anti}, boosted{boosted}, skipped{skipped}",
        layouts.len()
    );
    assert_eq!(
        (rows, layouts.len(), dual, anti, boosted, skipped),
        (26480, 3696, 1101, 5318, 6579, 17829)
    );
    Ok(())
}
fn options() -> AllocationEncoding {
    AllocationEncoding {
        boosts: [0; 21],
        trim: 5,
        intensity: 21,
        dual_stereo: false,
        previous_coded_bands: 0,
        signal_bandwidth: 20,
    }
}
#[test]
fn malformed_layout_policy_payload_and_lifecycle_preserve_writer()
-> Result<(), Box<dyn std::error::Error>> {
    let mut writer = RangeEncoder::new(128)?;
    writer.encode_uint(13, 17)?;
    let before = writer.clone();
    let size = FrameSize::Ms20;
    let layout = BandLayout::new(2, 0, 21)?;
    for bad in [
        AllocationEncoding {
            trim: 11,
            ..options()
        },
        AllocationEncoding {
            trim: 255,
            ..options()
        },
        AllocationEncoding {
            intensity: 22,
            ..options()
        },
        AllocationEncoding {
            intensity: usize::MAX,
            ..options()
        },
        AllocationEncoding {
            previous_coded_bands: 22,
            ..options()
        },
        AllocationEncoding {
            signal_bandwidth: 21,
            ..options()
        },
        AllocationEncoding {
            signal_bandwidth: usize::MAX,
            ..options()
        },
    ] {
        assert_eq!(
            allocation::encode_allocation(&mut writer, size, layout, true, bad),
            Err(AllocationError::InvalidControls)
        );
        assert_eq!(writer, before);
    }
    assert_eq!(
        allocation::encode_allocation(
            &mut writer,
            size,
            BandLayout::new(2, 17, 21)?,
            false,
            AllocationEncoding {
                intensity: 16,
                ..options()
            }
        ),
        Err(AllocationError::InvalidControls)
    );
    assert_eq!(
        allocation::encode_allocation(
            &mut writer,
            size,
            BandLayout::new(2, 21, 21)?,
            false,
            options()
        ),
        Err(AllocationError::EmptyBands)
    );
    assert_eq!(writer, before);
    let mut one = RangeEncoder::new(1)?;
    let saved = one.clone();
    assert_eq!(
        allocation::encode_allocation(&mut one, size, layout, false, options()),
        Err(AllocationError::InvalidPayload)
    );
    assert_eq!(one, saved);
    let mut over = RangeEncoder::new(2)?;
    over.encode_bits(0, 16)?;
    let saved = over.clone();
    assert_eq!(
        allocation::encode_allocation(&mut over, size, layout, false, options()),
        Err(AllocationError::BitBudgetExceeded)
    );
    assert_eq!(over, saved);
    writer.finish()?;
    let saved = writer.clone();
    assert_eq!(
        allocation::encode_allocation(&mut writer, size, layout, false, options()),
        Err(AllocationError::Encoding(EncoderError::Finished))
    );
    assert_eq!(writer, saved);
    Ok(())
}

#[test]
fn pcm_control_energy_allocation_spectrum_and_public_packet_paths_compose()
-> Result<(), Box<dyn std::error::Error>> {
    use opura::{
        celt::{
            analysis, control,
            energy::{CoarseEncoding, EnergyEncoder, EnergyState},
            input::{InputRate, PreemphasisState},
            partition::{self, SpectrumEncoding, SpectrumInput},
            pvq::Spread,
        },
        decoder::OpusDecoder,
        silk::{decoder::PcmChannels, resampler::OutputRate},
    };
    let mut frames = 0;
    for (rate, end, config) in [
        (InputRate::Khz8, 13, 16),
        (InputRate::Khz12, 17, 20),
        (InputRate::Khz16, 17, 20),
        (InputRate::Khz24, 19, 24),
        (InputRate::Khz48, 21, 28),
    ] {
        for (lm, size) in SIZES.into_iter().enumerate() {
            let n = size.samples();
            let count = n / rate.factor();
            for channels in [1, 2] {
                let layout = BandLayout::new(channels, 0, end)?;
                let mut pre = [PreemphasisState::default(), PreemphasisState::default()];
                let mut overlap = [[0; 120]; 2];
                let mut energy = EnergyEncoder::default();
                let mut decoded_energy = EnergyState::default();
                let mut seed = 0;
                let mut backend = opura::celt::synthesis::CeltSynthesis::new(
                    opura::celt::output::Decimation::By1,
                    channels,
                )?;
                let mut decoder = OpusDecoder::new(
                    OutputRate::Khz48,
                    if channels == 1 {
                        PcmChannels::Mono
                    } else {
                        PcmChannels::Stereo
                    },
                )?;
                for frame in 0..4 {
                    let pcm: Vec<i16> = (0..count * channels)
                        .map(|i| match frame {
                            0 => 0,
                            1 => {
                                if i % 2 == 0 {
                                    32767
                                } else {
                                    -32768
                                }
                            }
                            2 => (i as i16).wrapping_mul(7659).wrapping_add(12345),
                            _ => {
                                if i % 17 == 0 {
                                    1
                                } else {
                                    0
                                }
                            }
                        })
                        .collect();
                    let mut context = vec![0; channels * (n + 120)];
                    for c in 0..channels {
                        let channel = &mut context[c * (n + 120)..(c + 1) * (n + 120)];
                        channel[..120].copy_from_slice(&overlap[c]);
                        pre[c].process(size, rate, &pcm, channels, c, &mut channel[120..])?;
                        overlap[c].copy_from_slice(&channel[n..n + 120]);
                    }
                    let transient = lm > 0 && frame % 2 != 0;
                    let spectrum = analysis::analyze_frame(
                        size, &context, channels, channels, rate, transient,
                    )?;
                    let bands =
                        analysis::analyze_bands(size, spectrum.values(), channels, end, end)?;
                    let amplitudes =
                        std::array::from_fn(|c| bands.amplitudes(c).copied().unwrap_or([0; 21]));
                    let target =
                        std::array::from_fn(|c| bands.log_energies(c).copied().unwrap_or([0; 21]));
                    let mut writer = RangeEncoder::new(256)?;
                    writer.encode_bit_logp(false, 15)?;
                    writer.encode_bit_logp(false, 1)?;
                    if lm > 0 {
                        writer.encode_bit_logp(transient, 3)?;
                    }
                    energy.coarse(
                        &mut writer,
                        &target,
                        CoarseEncoding {
                            layout,
                            size,
                            effective_end: end,
                            available_bytes: 256,
                            force_intra: frame == 0,
                            two_pass: true,
                            loss_rate: 0,
                            lfe: false,
                        },
                    )?;
                    let decisions = std::array::from_fn(|i| (i + frame) % 3 == 0);
                    let tf = control::encode_tf(
                        &mut writer,
                        size,
                        0..end,
                        transient,
                        &decisions,
                        frame % 2 != 0,
                    )?;
                    let spread = control::encode_spread(&mut writer, Spread::Normal)?;
                    let allocation = allocation::encode_allocation(
                        &mut writer,
                        size,
                        layout,
                        transient,
                        AllocationEncoding {
                            intensity: end,
                            ..options()
                        },
                    )?;
                    energy.fine(&mut writer, layout, allocation.fine_bits())?;
                    let encoded = partition::encode_spectrum(
                        &mut writer,
                        &allocation,
                        &tf,
                        spread,
                        SpectrumInput {
                            left: bands.normalized(0).ok_or("left")?,
                            right: bands.normalized(1),
                            amplitudes,
                        },
                        SpectrumEncoding {
                            seed,
                            disable_inversion: false,
                            theta_rdo: true,
                        },
                    )?;
                    if allocation.anti_collapse_reserved() {
                        writer.encode_bits(0, 1)?;
                    }
                    let bits_left = 256 * 8 - writer.tell();
                    energy.finalise(
                        &mut writer,
                        layout,
                        allocation.fine_bits(),
                        allocation.fine_priority(),
                        bits_left,
                    )?;
                    let payload = writer.finish()?.to_vec();
                    let mut read = RangeDecoder::new(&payload);
                    let header = control::decode_prefix(&mut read, size, 0)?;
                    assert_eq!(header.transient(), transient);
                    decoded_energy.coarse(&mut read, layout, size, header.intra())?;
                    assert_eq!(control::decode_tf(&mut read, size, 0..end, transient)?, tf);
                    assert_eq!(control::decode_spread(&mut read)?, spread);
                    assert_eq!(
                        allocation::decode_allocation(&mut read, size, layout, transient)?,
                        allocation
                    );
                    decoded_energy.fine(&mut read, layout, allocation.fine_bits())?;
                    assert_eq!(
                        partition::decode_spectrum(
                            &mut read,
                            &allocation,
                            &tf,
                            spread,
                            seed,
                            false
                        )?,
                        encoded
                    );
                    if allocation.anti_collapse_reserved() {
                        assert_eq!(read.decode_bits(1)?, 0);
                    }
                    let remaining = 256 * 8 - read.tell();
                    decoded_energy.finalise(
                        &mut read,
                        layout,
                        allocation.fine_bits(),
                        allocation.fine_priority(),
                        remaining,
                    )?;
                    assert_eq!(&decoded_energy, energy.energies());
                    assert_eq!(
                        [read.range(), read.tell(), read.tell_frac()],
                        [writer.range(), writer.tell(), writer.tell_frac()]
                    );
                    let mut packet =
                        vec![((config + lm) as u8) << 3 | if channels == 2 { 4 } else { 0 }];
                    packet.extend_from_slice(&payload);
                    let pcm = decoder.decode(&packet)?;
                    assert_eq!(pcm.samples_per_channel(), n);
                    assert_eq!(pcm.channels(), channels);
                    assert_eq!(pcm.final_range(), writer.range());
                    let energies = [
                        *energy.energies().channel(0).ok_or("energy left")?,
                        *energy.energies().channel(1).ok_or("energy right")?,
                    ];
                    let vectors: Vec<&[i32]> = (0..channels)
                        .map(|c| encoded.channel(c).ok_or("spectrum channel"))
                        .collect::<Result<_, _>>()?;
                    let reconstructed = backend.synthesize(
                        &vectors,
                        &energies[..channels],
                        &opura::celt::synthesis::SynthesisControls {
                            bands: 0..end,
                            transient,
                            ..opura::celt::synthesis::SynthesisControls::new(size)
                        },
                    )?;
                    assert_eq!(pcm.samples(), reconstructed.samples());
                    // Native packet state replaces the local folding seed with final range.
                    seed = writer.range();
                    frames += 1;
                }
            }
        }
    }
    assert_eq!(frames, 160);
    Ok(())
}
