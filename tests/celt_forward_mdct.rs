//! Forward CELT analysis geometry, atomicity and interleaved block composition.
use opura::celt::{
    FrameSize,
    mdct::{self, MdctError},
};
const SIZES: [FrameSize; 4] = [
    FrameSize::Ms2_5,
    FrameSize::Ms5,
    FrameSize::Ms10,
    FrameSize::Ms20,
];

#[test]
fn invalid_analysis_geometry_preserves_every_output_entry() {
    for size in SIZES {
        let n = size.samples();
        let mut output = [0x12345678; 961];
        let before = output;
        for length in [0, n, n + 119, n + 121, 2000] {
            let input = vec![0; length];
            assert_eq!(
                mdct::analyze(size, &input, &mut output[..960], 1),
                Err(MdctError::InvalidGeometry)
            );
            assert_eq!(output, before);
        }
        let input = vec![123; n + 120];
        for stride in [0, 3, 5, 16, usize::MAX] {
            assert_eq!(
                mdct::analyze(size, &input, &mut output[..960], stride),
                Err(MdctError::InvalidGeometry)
            );
            assert_eq!(output, before);
        }
        for length in [0, n - 1, 961] {
            assert_eq!(
                mdct::analyze(size, &input, &mut output[..length], 1),
                Err(MdctError::InvalidGeometry)
            );
            assert_eq!(output, before);
        }
        for stride in [2, 4, 8] {
            if n * stride > 960 {
                assert_eq!(
                    mdct::analyze(size, &input, &mut output[..960], stride),
                    Err(MdctError::InvalidGeometry)
                );
                assert_eq!(output, before);
            } else {
                assert_eq!(
                    mdct::analyze(size, &input, &mut output[..(n - 1) * stride], stride),
                    Err(MdctError::InvalidGeometry)
                );
                assert_eq!(output, before);
            }
        }
    }
}

#[test]
fn selected_slots_are_deterministic_and_short_blocks_compose() -> Result<(), MdctError> {
    for size in SIZES {
        let n = size.samples();
        let input: Vec<_> = (0..n + 120)
            .map(|i| ((i as i32).wrapping_mul(0x1234567)) >> 4)
            .collect();
        let before = input.clone();
        let mut contiguous = vec![0; n];
        mdct::analyze(size, &input, &mut contiguous, 1)?;
        for stride in [1, 2, 4, 8] {
            if n * stride > 960 {
                continue;
            }
            let mut output = [0x76543210; 960];
            mdct::analyze(size, &input, &mut output, stride)?;
            for (i, &value) in output.iter().enumerate() {
                assert_eq!(
                    value,
                    if i % stride == 0 && i / stride < n {
                        contiguous[i / stride]
                    } else {
                        0x76543210
                    }
                );
            }
            assert_eq!(input, before);
            let mut silence = [1; 960];
            mdct::analyze(size, &vec![0; n + 120], &mut silence[..n * stride], stride)?;
            assert!(
                silence[..n * stride]
                    .iter()
                    .step_by(stride)
                    .all(|&v| v == 0)
            );
        }
    }
    for blocks in [2, 4, 8] {
        let signal: Vec<_> = (0..120 * blocks + 120)
            .map(|i| (i as i32 * 76543) ^ 0x1234567)
            .collect();
        let mut interleaved = vec![0; 120 * blocks];
        for block in 0..blocks {
            let input = &signal[120 * block..120 * block + 240];
            let mut expected = [0; 120];
            mdct::analyze(FrameSize::Ms2_5, input, &mut expected, 1)?;
            mdct::analyze(FrameSize::Ms2_5, input, &mut interleaved[block..], blocks)?;
            assert!(
                expected
                    .iter()
                    .enumerate()
                    .all(|(i, &x)| interleaved[i * blocks + block] == x)
            );
        }
    }
    Ok(())
}
