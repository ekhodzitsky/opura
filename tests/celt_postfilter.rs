//! Original comb output, full processed history and decoder parameter scheduling.
use opura::celt::postfilter::{PostfilterError, PostfilterParams, PostfilterState, Tapset};
fn tap(value: u16) -> Tapset {
    match value {
        0 => Tapset::Set0,
        1 => Tapset::Set1,
        2 => Tapset::Set2,
        _ => panic!("fixture tap"),
    }
}
#[test]
fn original_in_place_filter_and_parameter_generations_match()
-> Result<(), Box<dyn std::error::Error>> {
    let mut state = PostfilterState::default();
    let mut count = 0;
    let mut tapsets = [false; 3];
    let mut sizes = [false; 4];
    let mut gains = [false; 5];
    for line in include_str!("celt-postfilter-fixtures.txt")
        .lines()
        .filter(|l| !l.starts_with('#'))
    {
        let row: Vec<_> = line.split_whitespace().collect();
        assert_eq!(row[0], "frame");
        if row[1] == "1" {
            state.reset();
        }
        let n = row[2].parse::<usize>()?;
        let period = row[3].parse()?;
        let gain = row[4].parse()?;
        let t = row[5].parse::<u16>()?;
        let params = PostfilterParams::new(period, gain, tap(t))?;
        let values: Vec<_> = row[6..]
            .iter()
            .map(|s| s.parse::<i32>())
            .collect::<Result<_, _>>()?;
        let result = state.process(&values[..n], params)?;
        assert_eq!(
            result.samples(),
            &values[n..2 * n],
            "output frame {count} len{n} period{period} gain{gain} tap{t}"
        );
        let metadata = &values[2 * n..2 * n + 6];
        for (i, actual) in state.parameters().into_iter().enumerate() {
            assert_eq!(
                actual,
                PostfilterParams::new(
                    metadata[3 * i] as u16,
                    metadata[3 * i + 1] as u16,
                    tap(metadata[3 * i + 2] as u16)
                )?,
                "parameters frame {count}"
            );
        }
        assert_eq!(
            state.history(),
            &values[2 * n + 6..],
            "history frame {count}"
        );
        tapsets[t as usize] = true;
        sizes[match n {
            120 => 0,
            240 => 1,
            480 => 2,
            960 => 3,
            _ => panic!("size"),
        }] = true;
        gains[match gain {
            0 => 0,
            3072 => 1,
            6144 => 2,
            12288 => 3,
            24576 => 4,
            _ => panic!("gain"),
        }] = true;
        count += 1;
    }
    assert_eq!(count, 576);
    assert_eq!(tapsets, [true; 3]);
    assert_eq!(sizes, [true; 4]);
    assert_eq!(gains, [true; 5]);
    Ok(())
}
#[test]
fn validation_delayed_short_parameters_and_disable_tail_are_explicit()
-> Result<(), Box<dyn std::error::Error>> {
    for (period, gain) in [
        (0, 1),
        (14, 0),
        (1024, 0),
        (15, 24577),
        (u16::MAX, u16::MAX),
    ] {
        assert_eq!(
            PostfilterParams::new(period, gain, Tapset::Set0),
            Err(PostfilterError::InvalidParameters)
        );
    }
    let on = PostfilterParams::new(15, 24576, Tapset::Set2)?;
    let off = PostfilterParams::default();
    let mut state = PostfilterState::default();
    assert_eq!(state.process(&[100000; 120], on)?.samples(), &[100000; 120]);
    assert_eq!(state.parameters()[1], on);
    let before = state.clone();
    for len in [0, 1, 119, 121, 961, 1026] {
        assert_eq!(
            state.process(&vec![0; len], on),
            Err(PostfilterError::InvalidSignal)
        );
        assert_eq!(state, before);
    }
    let mut control = before;
    assert_eq!(
        state.process(&[100000; 960], on)?,
        control.process(&[100000; 960], on)?
    );
    assert_eq!(state, control);
    assert_eq!(state.parameters(), [on, on]);
    let result = state.process(&[123456; 960], off)?;
    assert_eq!(&result.samples()[240..], &[123456; 720]);
    assert_eq!(state.parameters(), [off, off]);
    assert_eq!(
        state.process(&[i32::MAX; 240], off)?.samples(),
        &[i32::MAX; 240]
    );
    state.reset();
    assert_eq!(state, PostfilterState::default());
    assert_eq!(state.process(&[0; 960], off)?.samples(), &[0; 960]);
    Ok(())
}
