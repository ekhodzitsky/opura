# Preserve Q24 precision in surround masking

Status: accepted

## Decision

Surround spectral masking uses signed32 Q24 logarithmic energies throughout,
including the spatial log-sum result and the channel-count offset. Convert the
native Q10 channel-count logarithm to Q24 before adding it to spatial masks.
Use the original integer approximations and constants, with explicit reference
provenance; do not replace them with floating-point approximations.

## Evidence

The pinned Opus1.6.1 revision
`22244de5a79bd1d6d623c32e72bf1954b56235be` defines `celt_glog` as signed32 with
`DB_SHIFT=24`. In `src/opus_multistream_encoder.c`, the original `logSum`
function nevertheless returns `opus_val16`. The original `channel_offset`
also uses `opus_val16` and adds a Q10 result directly to Q24 spatial masks.

The optional oracle builds two separate temporary translation units. A direct
probe returns0 for pristine `logSum(0,0)` and8388608 (0.5 in Q24) when its return
retains `celt_glog`. Corrected analysis differs from pristine masks in all2160
captured cases, while every preemphasis and overlap history remains identical.
Two independent complete generations produce identical analysis records.

The corrected temporary source makes exactly three asserted replacements:

1. Change `logSum`'s result from `opus_val16` to `celt_glog`.
2. Change `channel_offset`'s type from `opus_val16` to `celt_glog`.
3. Convert its Q10 logarithm to Q24 and use a32-bit half shift.

Original component algorithms, spectral coefficients and reference checkout
remain unchanged. The native decoder is never patched. Per-stream CELT mask
reduction matches another2816 records from the unmodified original block;
pristine/corrected builds give identical reduction outputs for identical masks.

## Consequences

Rust's surround analysis matches the explicitly corrected oracle rather than
the pristine build's narrowed masks. This is a stated arithmetic choice, not a
claim of pristine surround encoder byte identity or perceptual improvement.
Public documentation and any later encoder comparison must retain this scope.

These analysis APIs alone do not activate surround energy masking or LFE coding
in the generic mapped encoder. Actual common-owner/CELT integration, native
surround allocation and LFE policies remain required. Validate those behaviors
independently before advertising complete surround encoding.
