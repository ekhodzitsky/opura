# Shared encoder frontend controls

Version0.1.82 adds input analysis and a normal-admission policy kernel, not a
unified automatic PCM encoder. Fixed SILK/CELT/Hybrid encoders and CLI defaults
are unchanged. Shared input/layer ownership, native transition audio and a
usable adaptive file path still require integration and end-to-end validation.

## Contracts

`encoder::stereo_width::StereoWidthState` consumes raw interleaved signed16
stereo PCM before DC filtering/delay. Its energy/correlation smoothing,
fixed-point square roots/division, one-second width average and peak follower
match scalar fixed-point Opus. It retains state across calls, allocates nothing
and preserves all state on invalid input or arithmetic-domain errors. At
12kHz/2.5ms it deliberately ignores the last two sample pairs, as native does.

`encoder::control::select` consumes typed user controls, a frame with an
already-normalized TOC-inclusive rate/byte cap, measured width/digital silence
and an explicit pre-encode history snapshot. It returns primary mode, wire
channels, band, automatic-band history, selected FEC/DTX, effective rate and
requests for delayed downmix/reset/prefill/transition redundancy.

Selection repeats the native effective-rate calculation before channel choice,
after channel choice and after mode/downmix choice. The mode-unknown loss
penalty differs from SILK's penalty; CELT uses a different complexity penalty.
Threshold interpolation retains separate Q15 truncations. FEC can reduce the
selected band, with hysteresis and restoration if no band can support it.
Application constraints, input Nyquist limits, SILK bandwidth readiness and the
outgoing speech packet before a CELT tail follow native ordering.

The kernel is pure and never commits history. The real owner must supply
layer-derived readiness, perform requested signal processing and commit only
after successful packet encoding. Transition flags are requests, not evidence
of an encoded redundant tail. Controlled history snapshots need not describe
a reachable full-stream state. No classifier, LFE, DRED or multistream is
implemented here. Tiny budgets return `InsufficientBudget`; the real owner
must generate native PLC signalling rather than treating it as audio policy.

## Independent reference evidence

Reference: original Opus1.6.1 revision
`22244de5a79bd1d6d623c32e72bf1954b56235be`, scalar fixed-point,
`DISABLE_FLOAT_API`, fast64 Q24. `tools/opus_frontend_fixtures.py` checks the
reference revision/clean worktree and compiles its original manifests. It
includes original `opus_encoder.c`, invokes original width/math functions and
extracts the contiguous unchanged normal-admission block, ending after the
SILK/Hybrid band correction. Original initialized encoder contexts supply the
block's SILK reset/CELT control side effects. No C implementation is translated
into the oracle. Ordinary tests consume checked-in integers and need no native
code, network or third-party dependency.

Reproduce with an existing clean checkout of that revision:

```sh
python3 tools/opus_frontend_fixtures.py target/reference/opus target/frontend.txt --width-output target/width.txt
cmp tests/opus-frontend-fixtures.txt target/frontend.txt
cmp tests/opus-stereo-width-fixtures.txt target/width.txt
cargo test --offline --locked --lib encoder::control::
cargo test --offline --locked --lib encoder::stereo_width::
```

The deterministic seed is `0x4f505553`.16444 policy snapshots cover five rates,
nine durations, both input/wire layouts, five applications, all signal hints,
automatic/forced modes/channels/bands, CBR/VBR, complexity/loss/FEC/DTX and
previous-mode/band/readiness/downmix state. Results contain7156 SILK,250 Hybrid
and9038 CELT primary modes; all five bands occur. They include971 selected FEC,
310 delayed downmix,767 incoming and904 outgoing transition requests. These
are policy cases, not actual encoded transitions or encoded quality evidence.
Sixty targeted10ms stereo-input/forced-mono cases span width/rate boundaries.
They were added after random snapshots failed to detect a missing intermediate
channel-rate recalculation.

4320 width snapshots cover96 consecutive frames for each of45 rate/duration
geometries. Signals include initial/final silence, identical/inverted stereo,
level imbalance, one-sided signals, full-scale endpoints, deterministic noise
and near-zero signals. All five internal histories and output width are checked
after every call, without resetting between patterns.

Fixture SHA256:

- Policy: `9a736463a827c9ee8a5f210df432f6268a5ac7ac6e3da966d41d9dad8d154365`.
- Width: `54cd32aab21a80b2f3e17b4a278baeceaa8906d7482210fc47841342c0376f03`.

Six isolated source mutations are detected by assertion failures: using SILK's
loss penalty before mode selection, omitting the channel-rate recalculation,
omitting the outgoing speech packet, omitting delayed stereo downmix, reversing
FEC hysteresis and changing signed width smoothing from truncating division to
floor division. Each mutation was tested separately and the source restored
before final verification. The [mutation record](results/frontend-mutations-0.1.82.json)
names the tests and expected failing exit codes. It is evidence of sensitivity
to those errors, not exhaustive proof.

Final verification passed all seven repository gates: formatting, debug and
release offline/locked tests, all-targets Clippy with warnings denied, rustdoc
with warnings denied, the zero-dependency check and23 Python tests. Existing
encoder/decoder and audio CLI regressions remain in the normal test suites;
no existing PCM engine was changed for this milestone.

These checks establish the stated control arithmetic and snapshots only.
Complete adaptive encoder interoperability, waveform continuity, delay,
bitrate behavior and competitive quality/performance remain unproven.
