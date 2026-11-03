//! Faults follow actual pulse decoding and Q24 output writes.
use super::*;
use crate::{
    Packet,
    celt::{
        decoder::CeltDecoder,
        output::Decimation,
        partition::{self, BandControls, PartitionControls, PartitionState},
    },
    decoder::OpusDecoder,
    hybrid::HybridDecoder,
    range::RangeEncoder,
    silk::{decoder::PcmChannels, resampler::OutputRate},
};
use std::cell::Cell;

thread_local! {
    static FAILURE: Cell<Option<usize>> = const { Cell::new(None) };
    static FIRED: Cell<bool> = const { Cell::new(false) };
}
struct Fault {
    previous: Option<usize>,
    fired: bool,
}
impl Fault {
    fn at(skip: usize) -> Self {
        Self {
            previous: FAILURE.replace(Some(skip)),
            fired: FIRED.replace(false),
        }
    }
    fn assert_fired(&self) {
        assert!(FIRED.get(), "post-write PVQ fault must be reached");
    }
}
impl Drop for Fault {
    fn drop(&mut self) {
        FAILURE.set(self.previous);
        FIRED.set(self.fired);
    }
}
pub(super) fn failure() -> Result<(), PvqError> {
    if let Some(skip) = FAILURE.get() {
        if skip == 0 {
            FIRED.set(true);
            return Err(PvqError::InvalidInput);
        }
        FAILURE.set(Some(skip - 1));
    }
    Ok(())
}

#[inline(never)]
fn rollback<S, O, E>(
    state: &mut S,
    skip: usize,
    call: impl Fn(&mut S) -> Result<O, E>,
) -> Result<(), Box<dyn std::error::Error>>
where
    S: Clone + Eq + std::fmt::Debug,
    O: Eq + std::fmt::Debug,
    E: std::error::Error + 'static,
{
    let before = Box::new(state.clone());
    {
        let fault = Fault::at(skip);
        assert!(call(state).is_err());
        fault.assert_fired();
    }
    assert_eq!(state, before.as_ref());
    let mut control = before;
    assert_eq!(call(state)?, call(control.as_mut())?);
    assert_eq!(state, control.as_ref());
    Ok(())
}

#[test]
fn bounded_output_wide_masks_and_real_postwrite_failure() -> Result<(), Box<dyn std::error::Error>>
{
    let book = PulseCodebook::new(16, 1)?;
    let mut pulses = [0; 16];
    pulses[15] = 1;
    let mut writer = RangeEncoder::new(64)?;
    book.encode(&mut writer, &pulses)?;
    let data = writer.finish()?;
    let mut entropy = RangeDecoder::new(data);
    let before = entropy.clone();
    let mut short = [42; 15];
    assert_eq!(
        decode_partition_band_into(&mut entropy, book, Spread::None, 16, 0, &mut short),
        Err(PvqError::InvalidInput)
    );
    assert_eq!(short, [42; 15]);
    assert_eq!(entropy, before);
    assert_eq!(
        decode_band(&mut entropy, book, Spread::None, 16, 0),
        Err(PvqError::InvalidBlocks)
    );
    assert_eq!(entropy, before);
    let mut output = [42; 16];
    assert_eq!(
        decode_partition_band_into(&mut entropy, book, Spread::None, 16, 0, &mut output)?,
        1 << 15
    );
    assert_eq!(output, [0; 16]);
    entropy = before.clone();
    {
        let fault = Fault::at(0);
        assert!(
            decode_partition_band_into(&mut entropy, book, Spread::None, 16, i32::MAX, &mut output)
                .is_err()
        );
        fault.assert_fired();
    }
    assert_ne!(entropy, before, "private entropy really advanced");
    assert_ne!(output, [0; 16], "private output really changed");
    entropy = before;
    rollback(&mut entropy, 0, |e| {
        decode_band(e, book, Spread::Normal, 4, i32::MAX)
    })?;
    Ok(())
}

#[test]
fn public_partition_and_band_restore_entropy_budget_and_resume()
-> Result<(), Box<dyn std::error::Error>> {
    let data = [0; 128];
    let controls = PartitionControls {
        band: 20,
        lm: 0,
        blocks: 1,
        bits: 128,
        gain: i32::MAX,
        spread: Spread::Normal,
        fill: 1,
    };
    let mut state = (RangeDecoder::new(&data), PartitionState::new(4096, 9)?);
    rollback(&mut state, 0, |s| {
        partition::decode_partition(&mut s.0, &mut s.1, controls, None)
    })?;
    rollback(&mut state, 0, |s| {
        partition::decode_band(
            &mut s.0,
            &mut s.1,
            BandControls {
                partition: controls,
                tf_change: 0,
                stereo: None,
            },
            None,
        )
    })?;
    Ok(())
}

fn packets(fixtures: &str) -> Result<Vec<Vec<u8>>, Box<dyn std::error::Error>> {
    let mut result = Vec::new();
    for row in fixtures.lines().filter(|v| v.starts_with("packet ")) {
        let hex = row.split_whitespace().nth(4).ok_or("fixture hex")?;
        let data = (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16))
            .collect::<Result<Vec<_>, _>>()?;
        if data.len() > 16 && Packet::parse(&data)?.frame_samples() == 960 {
            result.push(data);
            if result.len() == 8 {
                break;
            }
        }
    }
    assert_eq!(result.len(), 8);
    Ok(result)
}

#[test]
fn celt_owner_restores_histories_after_several_written_leaves()
-> Result<(), Box<dyn std::error::Error>> {
    let data = packets(include_str!("../../tests/celt-packet-fixtures.txt"))?;
    let last = data.last().ok_or("packet")?;
    let mut owner = CeltDecoder::new(Decimation::By1, 2)?;
    for packet in &data {
        owner.decode(packet)?;
    }
    rollback(&mut owner, 2, |s| s.decode(last))?;
    Ok(())
}

#[test]
fn hybrid_owner_restores_silk_and_celt_after_written_leaf() -> Result<(), Box<dyn std::error::Error>>
{
    let data = packets(include_str!("../../tests/hybrid-packet-fixtures.txt"))?;
    let last = data.last().ok_or("packet")?;
    let mut owner = HybridDecoder::new(OutputRate::Khz48, PcmChannels::Stereo)?;
    for packet in &data {
        owner.decode(packet)?;
    }
    rollback(&mut owner, 0, |s| s.decode(last))?;
    Ok(())
}

#[test]
fn opus_owner_restores_gain_and_transition_state_after_written_leaves()
-> Result<(), Box<dyn std::error::Error>> {
    let data = packets(include_str!("../../tests/hybrid-packet-fixtures.txt"))?;
    let celt = packets(include_str!("../../tests/celt-packet-fixtures.txt"))?;
    let last = data.last().ok_or("hybrid packet")?;
    let celt = celt.last().ok_or("celt packet")?;
    let mut owner = OpusDecoder::new(OutputRate::Khz48, PcmChannels::Stereo)?;
    owner.set_gain_q8(-1536);
    for packet in &data {
        owner.decode(packet)?;
    }
    rollback(&mut owner, 0, |s| s.decode(last))?;
    rollback(&mut owner, 2, |s| s.decode(celt))?;
    Ok(())
}
