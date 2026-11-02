// Original tone/transient constants, pinned Opus v1.6.1. See THIRD_PARTY_NOTICES.md.
pub(super) const INV: [u8; 128] = [
    255, 255, 156, 110, 86, 70, 59, 51, 45, 40, 37, 33, 31, 28, 26, 25, 23, 22, 21, 20, 19, 18, 17,
    16, 16, 15, 15, 14, 13, 13, 12, 12, 12, 12, 11, 11, 11, 10, 10, 10, 9, 9, 9, 9, 9, 9, 8, 8, 8,
    8, 8, 7, 7, 7, 7, 7, 7, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 5, 5, 5, 5, 5, 5, 5, 5,
    5, 5, 5, 5, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 3, 3, 3,
    3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 2,
];
pub(super) const TONE_LIMIT: i32 = 1073741312;
pub(super) const COMPLEX_SCALE: i32 = 2147483136;
pub(super) const TONE_GUARD: i32 = 526133504;
pub(super) const TONE_GUARD_FREQ: i32 = 213;
pub(super) const TF_SCALE: i32 = 113;
pub(super) const TF_OFFSET: i32 = 37312528;
