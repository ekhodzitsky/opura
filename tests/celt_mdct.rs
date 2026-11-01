//! Chained original inverse-MDCT PCM and overlap, including pathological signals.
use opura::celt::{
    FrameSize,
    mdct::{MdctError, MdctState},
};
const SIZES: [FrameSize; 4] = [
    FrameSize::Ms2_5,
    FrameSize::Ms5,
    FrameSize::Ms10,
    FrameSize::Ms20,
];
#[test]
fn original_time_samples_and_overlap_match_through_sizes_strides_and_resets()
-> Result<(), Box<dyn std::error::Error>> {
    let mut state = MdctState::default();
    let mut count = 0;
    let mut geometry = [[false; 4]; 4];
    let mut pre = [false; 30];
    let mut post = [false; 30];
    let mut fft = [false; 30];
    for line in include_str!("celt-mdct-fixtures.txt")
        .lines()
        .filter(|l| !l.starts_with('#'))
    {
        let row: Vec<_> = line.split_whitespace().collect();
        assert_eq!(row[0], "frame");
        if row[1] == "1" {
            state.reset();
        }
        let lm = row[2].parse::<usize>()?;
        let stride = row[3].parse::<usize>()?;
        let length = row[4].parse::<usize>()?;
        pre[row[5].parse::<usize>()?] = true;
        post[row[6].parse::<usize>()?] = true;
        fft[row[7].parse::<usize>()?] = true;
        geometry[lm][stride.trailing_zeros() as usize] = true;
        let values: Vec<_> = row[8..]
            .iter()
            .map(|s| s.parse::<i32>())
            .collect::<Result<_, _>>()?;
        let input = &values[..length];
        let expected = &values[length..length + SIZES[lm].samples()];
        let overlap = &values[length + SIZES[lm].samples()..];
        let result = state.synthesize(SIZES[lm], input, stride)?;
        assert_eq!(
            result.samples(),
            expected,
            "time frame {count} size{lm} stride{stride}"
        );
        assert_eq!(state.overlap(), overlap, "overlap frame {count}");
        count += 1;
    }
    assert_eq!(count, 398);
    assert_eq!(
        geometry,
        [
            [true, true, true, true],
            [true, true, true, false],
            [true, true, false, false],
            [true, false, false, false]
        ]
    );
    assert!(pre[0] && pre[29]);
    assert!(post[0]);
    assert!(fft.iter().filter(|&&b| b).count() > 10);
    Ok(())
}
#[test]
fn invalid_geometry_preserves_overlap_and_reset_is_repeatable()
-> Result<(), Box<dyn std::error::Error>> {
    let mut state = MdctState::default();
    let good = vec![100000; 960];
    state.synthesize(FrameSize::Ms20, &good, 1)?;
    let before = state.clone();
    for size in SIZES {
        let n = size.samples();
        for stride in [0, 3, 5, 9, usize::MAX] {
            assert_eq!(
                state.synthesize(size, &good, stride),
                Err(MdctError::InvalidGeometry)
            );
            assert_eq!(state, before);
        }
        assert_eq!(
            state.synthesize(size, &good[..n - 1], 1),
            Err(MdctError::InvalidGeometry)
        );
        assert_eq!(state, before);
        assert_eq!(
            state.synthesize(size, &[0; 961], 1),
            Err(MdctError::InvalidGeometry)
        );
        assert_eq!(state, before);
    }
    assert_eq!(
        state.synthesize(FrameSize::Ms20, &good, 2),
        Err(MdctError::InvalidGeometry)
    );
    assert_eq!(state, before);
    let mut control = before;
    assert_eq!(
        state.synthesize(FrameSize::Ms2_5, &good, 8)?,
        control.synthesize(FrameSize::Ms2_5, &good, 8)?
    );
    assert_eq!(state, control);
    state.reset();
    assert_eq!(state, MdctState::default());
    assert_eq!(
        state.synthesize(FrameSize::Ms20, &good, 1)?,
        MdctState::default().synthesize(FrameSize::Ms20, &good, 1)?
    );
    state.synthesize(FrameSize::Ms2_5, &[0; 120], 1)?;
    assert_eq!(state.overlap(), &[0; 60]);
    assert_eq!(
        state.synthesize(FrameSize::Ms2_5, &[0; 120], 1)?.samples(),
        &[0; 120]
    );
    Ok(())
}
