// Original CELT trim/stereo constants, pinned Opus v1.6.1.
// See THIRD_PARTY_NOTICES.md.
// Native initial saving/intensity: 0/0; guard logQ10: 6145.
pub(super) const MODEL_EPSILON: i32 = 1;
pub(super) const MID_SCALE_Q15: i32 = 23170;
pub(super) const CORRELATION_GUARD_Q20: i32 = 1049625;
pub(super) const INTENSITY_THRESHOLDS: [i16; 21] = [
    1, 2, 3, 4, 5, 6, 7, 8, 16, 24, 36, 44, 50, 56, 62, 67, 72, 79, 88, 106, 134,
];
pub(super) const INTENSITY_HYSTERESIS: [i16; 21] = [
    1, 1, 1, 1, 1, 1, 1, 2, 2, 2, 2, 2, 2, 2, 3, 3, 4, 5, 6, 8, 8,
];
