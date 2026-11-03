//! Faults occur after real overlap/filter/history mutations, not just validation.
use super::*;
use crate::{
    Packet,
    celt::decoder::CeltDecoder,
    decoder::OpusDecoder,
    hybrid::HybridDecoder,
    silk::{decoder::PcmChannels, resampler::OutputRate},
};
use std::cell::Cell;

thread_local! {
    static FAILURE: Cell<(u8, usize)> = const { Cell::new((0, 0)) };
    static FIRED: Cell<bool> = const { Cell::new(false) };
}
struct Fault {
    previous: (u8, usize),
    fired: bool,
}
impl Fault {
    fn at(stage: u8, skip: usize) -> Self {
        let previous = FAILURE.replace((stage, skip));
        let fired = FIRED.replace(false);
        Self { previous, fired }
    }
    fn assert_fired(&self) {
        assert!(FIRED.get(), "late synthesis fault must be exercised");
    }
}
impl Drop for Fault {
    fn drop(&mut self) {
        FAILURE.set(self.previous);
        FIRED.set(self.fired);
    }
}
pub(super) fn failure(stage: u8) -> Result<(), SynthesisError> {
    let (selected, skip) = FAILURE.get();
    if selected == stage {
        if skip == 0 {
            FIRED.set(true);
            return Err(SynthesisError::InvalidSpectrum);
        }
        FAILURE.set((selected, skip - 1));
    }
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

#[inline(never)]
fn rollback<S, O, E>(
    state: &mut S,
    stage: u8,
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
        let fault = Fault::at(stage, skip);
        assert!(call(state).is_err());
        fault.assert_fired();
    }
    assert_eq!(state, before.as_ref());
    let mut control = before;
    let actual = call(state)?;
    let expected = call(control.as_mut())?;
    assert_eq!(actual, expected);
    assert_eq!(state, control.as_ref());
    Ok(())
}

#[test]
fn public_backend_rolls_back_real_late_mutations_and_resumes()
-> Result<(), Box<dyn std::error::Error>> {
    let warm = [100000; 960];
    let left: [i32; 960] = std::array::from_fn(|i| if i % 13 < 6 { 1 << 20 } else { -(1 << 20) });
    let right: [i32; 960] = std::array::from_fn(|i| if i % 17 < 8 { -(1 << 20) } else { 1 << 20 });
    let energy = [[10 << 24; 21]; 2];
    let mut controls = SynthesisControls::new(FrameSize::Ms20);
    controls.postfilter = PostfilterParams::new(60, 12288, super::super::postfilter::Tapset::Set1)?;
    let mut backend = CeltSynthesis::new(Decimation::By1, 2)?;
    backend.synthesize(&[&warm, &warm], &[[0; 21]; 2], &controls)?;
    for stage in 1..=3 {
        let before = backend.clone();
        // The private path actually mutates an unpublished owner before failing.
        let mut staged = before.clone();
        {
            let fault = Fault::at(stage, 0);
            assert!(
                staged
                    .synthesize_staged(&[&left, &right], &energy, &controls)
                    .is_err()
            );
            fault.assert_fired();
        }
        assert_ne!(staged, before, "stage{stage} must follow real mutation");
        {
            let fault = Fault::at(stage, 0);
            assert!(
                backend
                    .synthesize(&[&left, &right], &energy, &controls)
                    .is_err()
            );
            fault.assert_fired();
        }
        assert_eq!(backend, before, "public rollback stage{stage}");
        let mut control = before;
        assert_eq!(
            backend.synthesize(&[&left, &right], &energy, &controls)?,
            control.synthesize(&[&left, &right], &energy, &controls)?
        );
        assert_eq!(backend, control);
    }
    Ok(())
}

#[test]
fn celt_normal_noise_and_pitch_public_owners_roll_back_and_resume()
-> Result<(), Box<dyn std::error::Error>> {
    let data = packets(include_str!("../../tests/celt-packet-fixtures.txt"))?;
    let last = data.last().ok_or("packet")?;
    let mut owner = CeltDecoder::new(Decimation::By1, 2)?;
    for packet in &data {
        owner.decode(packet)?;
    }
    for stage in 1..=3 {
        rollback(&mut owner, stage, 0, |s| s.decode(last))?;
    }
    rollback(&mut owner, 4, 0, |s| s.conceal_samples(960))?;
    owner.conceal_samples(5760)?;
    for stage in 1..=3 {
        rollback(&mut owner, stage, 0, |s| s.conceal_samples(960))?;
    }
    Ok(())
}

#[test]
fn hybrid_normal_and_noise_public_owners_remain_atomic() -> Result<(), Box<dyn std::error::Error>> {
    let data = packets(include_str!("../../tests/hybrid-packet-fixtures.txt"))?;
    let last = data.last().ok_or("packet")?;
    let mut owner = HybridDecoder::new(OutputRate::Khz48, PcmChannels::Stereo)?;
    for packet in &data {
        owner.decode(packet)?;
    }
    for stage in 1..=3 {
        rollback(&mut owner, stage, 0, |s| s.decode(last))?;
        rollback(&mut owner, stage, 0, |s| s.conceal_samples(960))?;
        owner.decode(last)?;
    }
    Ok(())
}

#[test]
fn opus_prefixed_fec_and_transition_owners_remain_atomic() -> Result<(), Box<dyn std::error::Error>>
{
    let data = packets(include_str!("../../tests/hybrid-packet-fixtures.txt"))?;
    let celt = packets(include_str!("../../tests/celt-packet-fixtures.txt"))?;
    let last = data.last().ok_or("hybrid")?;
    let celt = celt.last().ok_or("celt")?;
    let mut owner = OpusDecoder::new(OutputRate::Khz48, PcmChannels::Stereo)?;
    owner.set_gain_q8(1536);
    for packet in &data {
        owner.decode(packet)?;
    }
    for stage in 1..=3 {
        // Five noise-PLC blocks in the100ms prefix precede recovered CELT.
        let skip = if stage == 1 { 10 } else { 5 };
        rollback(&mut owner, stage, skip, |s| s.decode_fec(last, 5760))?;
        owner.decode(last)?;
    }
    rollback(&mut owner, 3, 0, |s| s.decode(celt))?;
    Ok(())
}
