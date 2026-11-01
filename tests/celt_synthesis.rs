//! Complete original normalized-spectrum CELT backend: PCM and all histories.
use opura::celt::{
    FrameSize,
    output::Decimation,
    postfilter::{PostfilterParams, Tapset},
    synthesis::{CeltSynthesis, SynthesisControls, SynthesisError},
};
const SIZES: [FrameSize; 4] = [
    FrameSize::Ms2_5,
    FrameSize::Ms5,
    FrameSize::Ms10,
    FrameSize::Ms20,
];
const RATES: [Decimation; 5] = [
    Decimation::By1,
    Decimation::By2,
    Decimation::By3,
    Decimation::By4,
    Decimation::By6,
];
fn tap(t: u16) -> Tapset {
    match t {
        0 => Tapset::Set0,
        1 => Tapset::Set1,
        2 => Tapset::Set2,
        _ => panic!("tap"),
    }
}
#[test]
fn original_complete_synthesis_pcm_and_channel_histories_match()
-> Result<(), Box<dyn std::error::Error>> {
    let mut state = CeltSynthesis::new(Decimation::By1, 1)?;
    let mut count = 0;
    let mut geometry = [[[[[false; 2]; 2]; 2]; 5]; 4];
    for line in include_str!("celt-synthesis-fixtures.txt")
        .lines()
        .filter(|l| !l.starts_with('#'))
    {
        let row: Vec<_> = line.split_whitespace().collect();
        assert_eq!(row[0], "frame");
        let factor = row[2].parse::<usize>()?;
        let rate_index = RATES
            .iter()
            .position(|r| r.factor() == factor)
            .ok_or("rate")?;
        let rate = RATES[rate_index];
        let api = row[3].parse::<usize>()?;
        let encoded = row[4].parse::<usize>()?;
        let lm = row[5].parse::<usize>()?;
        if state.channels() != api || state.sample_rate() != rate.sample_rate() {
            state = CeltSynthesis::new(rate, api)?;
        }
        if row[1] == "1" {
            state.reset();
        }
        let mut controls = SynthesisControls::new(SIZES[lm]);
        controls.bands = row[6].parse()?..row[7].parse()?;
        controls.transient = row[8] == "1";
        controls.silence = row[9] == "1";
        controls.postfilter =
            PostfilterParams::new(row[10].parse()?, row[11].parse()?, tap(row[12].parse()?))?;
        let values: Vec<_> = row[13..]
            .iter()
            .map(|s| s.parse::<i32>())
            .collect::<Result<_, _>>()?;
        let energy_count = 21 * encoded;
        let n = SIZES[lm].samples();
        let input_count = n * encoded;
        let pcm_count = n / factor * api;
        let energies: Vec<[i32; 21]> = values[..energy_count]
            .chunks_exact(21)
            .map(<[i32; 21]>::try_from)
            .collect::<Result<_, _>>()?;
        let channels: Vec<_> = values[energy_count..energy_count + input_count]
            .chunks_exact(n)
            .collect();
        let pcm = state.synthesize(&channels, &energies, &controls)?;
        let mut at = energy_count + input_count;
        let expected: Vec<_> = values[at..at + pcm_count]
            .iter()
            .map(|&v| i16::try_from(v))
            .collect::<Result<_, _>>()?;
        assert_eq!(
            pcm.samples(),
            expected,
            "PCM case {count} size{lm} api{api} wire{encoded} transient{}",
            controls.transient
        );
        assert_eq!(pcm.sample_rate(), rate.sample_rate());
        assert_eq!(pcm.channels(), api);
        assert_eq!(pcm.samples_per_channel(), n / factor);
        at += pcm_count;
        for channel in 0..api {
            assert_eq!(
                state.overlap(channel).ok_or("overlap")?,
                &values[at + 60 * channel..at + 60 * (channel + 1)],
                "overlap case{count} channel{channel}"
            );
        }
        at += 60 * api;
        for channel in 0..api {
            assert_eq!(
                state.postfilter(channel).ok_or("postfilter")?.history(),
                &values[at + 1026 * channel..at + 1026 * (channel + 1)],
                "processed history case{count} channel{channel}"
            );
        }
        at += 1026 * api;
        for channel in 0..api {
            let params = state.postfilter(channel).ok_or("parameters")?.parameters();
            for (i, actual) in params.iter().enumerate() {
                assert_eq!(
                    *actual,
                    PostfilterParams::new(
                        values[at + 3 * i] as u16,
                        values[at + 3 * i + 1] as u16,
                        tap(values[at + 3 * i + 2] as u16)
                    )?,
                    "parameters case {count} channel {channel}"
                );
            }
        }
        at += 6;
        assert_eq!(
            state.deemphasis_memory(),
            &values[at..at + 2],
            "deemphasis {count}"
        );
        assert_eq!(at + 2, values.len());
        assert!(state.overlap(api).is_none());
        assert!(state.postfilter(api).is_none());
        geometry[lm][rate_index][api - 1][encoded - 1][usize::from(controls.transient)] = true;
        count += 1;
    }
    assert_eq!(count, 480);
    assert_eq!(geometry, [[[[[true; 2]; 2]; 2]; 5]; 4]);
    Ok(())
}
#[test]
fn malformed_frames_roll_back_all_backend_state_and_reset_repeats()
-> Result<(), Box<dyn std::error::Error>> {
    assert_eq!(
        CeltSynthesis::new(Decimation::By1, 0),
        Err(SynthesisError::InvalidChannels)
    );
    assert_eq!(
        CeltSynthesis::new(Decimation::By1, 3),
        Err(SynthesisError::InvalidChannels)
    );
    let mut state = CeltSynthesis::new(Decimation::By2, 2)?;
    let mut controls = SynthesisControls::new(FrameSize::Ms20);
    controls.postfilter = PostfilterParams::new(60, 12288, Tapset::Set1)?;
    let left = [100000; 960];
    let right = [-200000; 960];
    let energies = [[0; 21]; 2];
    state.synthesize(&[&left, &right], &energies, &controls)?;
    let before = state.clone();
    assert!(state.synthesize(&[], &[], &controls).is_err());
    assert_eq!(state, before);
    assert!(
        state
            .synthesize(&[&left, &right, &left], &[[0; 21]; 3], &controls)
            .is_err()
    );
    assert_eq!(state, before);
    assert!(
        state
            .synthesize(&[&left, &right], &energies[..1], &controls)
            .is_err()
    );
    assert_eq!(state, before);
    assert!(
        state
            .synthesize(&[&left, &right[..959]], &energies, &controls)
            .is_err()
    );
    assert_eq!(state, before);
    controls.bands = 17..22;
    assert!(
        state
            .synthesize(&[&left, &right], &energies, &controls)
            .is_err()
    );
    assert_eq!(state, before);
    controls.bands = 0..21;
    controls.transient = true;
    let mut control = before;
    assert_eq!(
        state.synthesize(&[&left, &right], &energies, &controls)?,
        control.synthesize(&[&left, &right], &energies, &controls)?
    );
    assert_eq!(state, control);
    state.reset();
    assert_eq!(state, CeltSynthesis::new(Decimation::By2, 2)?);
    let zero = [0; 960];
    let mut silent = SynthesisControls::new(FrameSize::Ms20);
    silent.silence = true;
    assert!(
        state
            .synthesize(&[&zero], &energies[..1], &silent)?
            .samples()
            .iter()
            .all(|&s| s == 0)
    );
    Ok(())
}
