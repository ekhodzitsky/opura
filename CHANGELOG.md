# Changelog

Versions follow SemVer. Before 1.0, incompatible public API changes increment
the minor version; compatible additions and fixes increment the patch version.
Each version has an annotated Git tag. Entries describe implemented behavior
and validation, without implying full Opus compatibility before PCM decoding.

## [Unreleased]

## [0.1.18] - 2026-11-01

- Add transactional CELT Laplace range symbols and persistent Q24 band-energy
  reconstruction for one/two channels, all four frame sizes and selected bands.
- Decode separate coarse, fine and final refinements, with exact budget
  fallbacks, priority ordering, inter-frame/inter-band prediction and clamps.
  Preserve reference signed-32 truncation before coarse energy saturation.
- Match all 42 energies and range/tell/tell_frac after each stage in 768 chained
  reference frames, plus 2688 standalone Laplace symbols. Exercise every two-byte
  prefix with three boundary Laplace models and verify late refinement rollback.
- Five mutations are detected; model tables and fixtures regenerate identically.
  All mandatory offline checks pass; no third-party dependencies are added.

CELT shape decoding/transforms, Hybrid and full Opus orchestration remain future work.

## [0.1.17] - 2026-11-01

- Start CELT with allocation-free fixed-point scalar math: exact integer square
  root, approximate square root/normalized reciprocal square root, periodic
  cosine, log2/exp2, reciprocal and fractional Q29/saturated-Q31 division.
- Expose Q formats, reference clamp/sentinel behavior and typed domain errors.
- Match 638975 exhaustively hashed results and 29164 scalar reference cases,
  including full normalized reciprocal-root, exponent and two cosine periods.
  Independently bracket every 16-bit square and 65536 deterministic wide inputs.
- Detect five rounding/refinement/normalization mutations; regenerate fixtures
  byte-identically and pass all mandatory offline checks without dependencies.

These are scalar primitives; CELT/Hybrid audio decoding remains incomplete.

## [0.1.16] - 2026-11-01

- Add bounded, atomic `SilkDecoder` for encoded mono/stereo and mono/interleaved
  stereo final PCM at every standard output rate, including loss and in-band FEC.
- Integrate stereo/LBRR field ordering, mid-only side suppression and restart,
  channel/rate transitions and separate left/right resampler histories.
- Match every PCM sample and public final range on 1738 normal/FEC/loss results
  plus 30 transactional transition rejections. Cover all 64 mid/side LBRR mask
  pairs per internal rate/output layout, all 15 rate pairs and 120 ms output.
- Retain 260 earlier mono oracle cases; verify reset, malformed requests, late
  internal/Opus-frame rollback and 1024 adversarial calls. Six targeted mutations
  fail the oracle tests; the new reference corpus regenerates byte-identically.
- Run all mandatory offline debug/release, lint, docs and dependency/tool checks.

CELT/Hybrid, implicit DTX and transition reconstruction remain incomplete.
Full Opus conformance and SOTA performance are not yet established.

## [0.1.15] - 2026-11-01

- Add transactional SILK stereo predictor and optional mid-only entropy decoding,
  with typed optimized Q13 predictions and strict primitive budget checks.
- Reconstruct buffered internal-rate left/right PCM from mid/side frames using
  reference 8 ms interpolation, signed-16 predictor-difference arithmetic,
  saturation and two-sample raw histories. Preserve history across rate changes.
- Match all 5625 quantizer pairs, mid-only flags and entropy counters against
  upstream; match both PCM channels and persistent reconstruction state on 192
  independent extreme, correlated/opposed, quiet, clipping and reset/rate cases.
- Verify short-prefix/geometry rollback and reset; mutations detect omitted
  predictor subtraction, incorrect interpolation arithmetic and predicted-side
  history retention. Reference tables and fixtures regenerate byte-identically.

Stereo packet/channel scheduling is not yet integrated. Existing final-rate mono
SILK remains supported; full Opus conformance/performance remain unestablished.

## [0.1.14] - 2026-11-01

- Add `MonoOutputDecoder` for final mono SILK PCM at 8/12/16/24/48 kHz,
  composing normal/loss/FEC decoding, one-sample mono buffering and resampling.
- Preserve the buffered channel sample across internal-rate switches while
  reinitializing the resampler; commit all packet/delay/filter history atomically.
- Match every final PCM sample and public OPUS_GET_FINAL_RANGE on 255 successful
  public opus_decode results across all 15 rate pairs, frame durations, multi-frame
  120 ms packets, explicit loss, FEC/recovery and input-rate changes. Five real
  primary transitions reject without state updates.
- Verify reset, invalid requests, late FEC resource rollback and unchanged output
  histories on errors. Mutations detect missing mono delay and per-packet filter
  resets; the independent public-output fixture mode regenerates byte-identically.

This delivers final-rate PCM for the supported mono SILK subset. Stereo,
CELT/Hybrid, transition reconstruction and implicit DTX remain incomplete;
full Opus conformance and competitive performance are not established.

## [0.1.13] - 2026-11-01

- Add a fixed-storage SILK decoder resampler for all 15 combinations of
  8/12/16 kHz internal input and 8/12/16/24/48 kHz output rates.
- Match reference delay compensation, three-section allpass x2 upsampling,
  fractional FIR interpolation and AR/FIR downsampling with rounded-up Q16 ratios.
- Validate whole-ms input bounds (1–120 ms) and output capacity before modifying
  state or samples; retain filter history across calls and reset on rate changes.
- Match PCM and every active filter/delay/configuration field on 360 independent
  upstream cases, including impulses, DC extremes, alternating limits, random
  input, clipping, resets and rate transitions. Verify chunking invariance and
  invalid-buffer rollback; mutations detect allpass, FIR mirror and delay errors.

This component does not yet add Opus mono/stereo buffering or integrate output
resampling into packet decoding. Full Opus compatibility remains unestablished.

## [0.1.12] - 2026-11-01

- Add transactional mono in-band FEC recovery with `MonoDecoder::decode_fec`,
  recovering the preceding interval represented by the first Opus frame and
  leaving primary audio for a subsequent normal decode of the same packet.
- Decode LBRR in temporal order, using conditional coding only for adjacent
  redundant frames and PLC/CNG for gaps. Preserve reference gain history for
  FEC gaps, which differs from explicit packet-loss gain reset.
- Verify 111 FEC results and 215 subsequent/regular PCM results against the
  complete pinned upstream decoder, plus a rejected real transition and a
  late over-budget FEC regression; cover all LBRR masks, rates and durations.
- Test multi-frame first-frame semantics, reset, unsupported/malformed input and
  atomic failure recovery. Mutations detect gapped-context coding, premature
  commits and incorrect FEC gain reset. Earlier packet/loss fixtures are unchanged.

Output remains before mono buffering/resampling. CELT/Hybrid, stereo and implicit
DTX handling remain future work; full-codec conformance/performance are unestablished.

## [0.1.11] - 2026-11-01

- Add `MonoDecoder::conceal(duration_ms)` for explicit missing intervals of
  10–120 ms in 10 ms steps at the last successfully decoded SILK rate.
- Match upstream loss scheduling after 10 ms and longer Opus frames; apply
  channel PLC/CNG/recovery bookkeeping and report final range zero. Invalid
  durations and missing stream history preserve all state.
- Verify 288 real encoded packet-stream steps, including 120 scheduled losses
  covering every supported duration, burst loss, silence, FEC-bearing packets
  and recovery, against completed original decode_frame PCM and public final range.
- Retain the existing default raw-core fixture output unchanged; add an optional
  loss oracle mode, transactional error/reset tests and scheduling/history mutations.

PCM still precedes mono buffering/resampling. Empty/one-byte DTX packets require
explicit handling, and FEC recovery, stereo and CELT/Hybrid remain future work.

## [0.1.10] - 2026-11-01

- Add bounded internal-channel loss concealment with reference fixed-point
  PLC: random/voiced excitation, gain attenuation, pitch drift, rewhitening and
  LPC synthesis. Preserve shared prediction histories between losses and good frames.
- Mix CNG and apply recovery energy glue in upstream order; clear gain clamping
  after loss and apply post-loss LPC bandwidth expansion and voiced-to-unvoiced
  recovery prediction. Expose transactional `ChannelState::conceal` for 10/20 ms.
- Match all PCM samples, entropy counters, gain indices, shared synthesis history
  and complete PLC state against 240 pinned original decode_frame steps, including
  cold loss, bursts, recovery, rates/durations, inactive/unvoiced/voiced and clipping.
- Verify long loss bursts, reset and failed-recovery rollback; detect omitted PLC
  expansion, recovery fade, post-loss filter expansion and recovery prediction
  with targeted mutations. Reference regeneration is byte-identical.

This is an internal channel loss API. Packet loss scheduling, FEC recovery,
mono buffering/resampling, stereo and CELT/Hybrid still require integration.

## [0.1.9] - 2026-11-01

- Add fixed-storage SILK comfort-noise estimation from decoded inactive frames
  and CNG mixing into 10/20 ms concealed PCM at 8/12/16 kHz.
- Match reference spectral/gain smoothing, excitation-bank selection, wrapping
  PRNG, LPC filtering, PLC noise-energy compensation and signed saturation.
- Include CNG estimates in channel state transactions without changing existing
  loss-free mono PCM/final range; validate geometry and PLC noise context before
  changing state or output. Reset and rate changes follow upstream semantics.
- Compare every PCM sample and retained CNG state field on 192 independent
  upstream cases, covering cold loss, burst losses/recovery, rates/durations,
  gain branches and clipping. Typed tests verify error transactions and reset;
  targeted mutations detect mask, noise compensation and filter-reset errors.

CNG is a component for loss handling; a complete PLC implementation and packet
loss API are still required. Mono output remains before buffering/resampling;
stereo and CELT/Hybrid remain future work.

## [0.1.8] - 2026-11-01

- Decode loss-free mono SILK-only Opus packets to raw internal-rate PCM at
  8/12/16 kHz, including 10/20/40/60 ms Opus frames, packet codes 0–3, padding
  and multi-frame packets up to 120 ms with fixed output storage.
- Consume LBRR before regular audio with correct adjacent-frame conditional
  coding, isolated from regular gain, spectral and synthesis histories.
- Commit all packet histories and output atomically. Reject unsupported stereo,
  CELT/Hybrid, loss/DTX frames and CELT transition tails with typed errors.
- Handle legitimately stripped trailing zero bytes with a bounded packet-only
  virtual entropy budget; preserve strict public primitive budget contracts.
- Verify raw PCM samples and public final range against the pinned fixed-point
  libopus decoder on 222 real encoded periodic/noise, silence and FEC packets,
  including repacketized 120 ms inputs. Another 48 synthetic symbol packets cover
  all LBRR masks; six real transition packets verify unsupported-input rollback.
- Add rollback/retry, reset, malformed-input, resource-boundary and zero-trimming
  regressions and targeted mutations for gapped LBRR, premature commits and
  missing zero extension, with a reproducible external-reference generator.

Output precedes Opus mono delay buffering and resampling. Loss recovery,
CNG/PLC, stereo and CELT/Hybrid decoding remain unimplemented. Full Opus
conformance and competitive PCM performance are not established.

## [0.1.7] - 2026-11-01

- Add transactional single-channel SILK decoding from signal-type symbols through
  gains, NLSF, pitch/LTP, excitation and raw internal-rate PCM synthesis.
- Commit entropy and all channel histories together; failed frames leave them
  unchanged. Support reset, 10/20 ms duration changes and 8/12/16 kHz rate changes,
  including spectral-history reset for 8↔12 kHz and conditional-start rejection.
- Match every PCM sample and final range/tell/tell_frac across 144 consecutive
  synthetic upstream-reference frames, including all activity/coding modes.
- Verify rollback at each entropy stage, retry recovery, reset and rate changes;
  detect missing spectral resets and premature commits with targeted mutations.

This is a positioned-cursor channel API, before CNG/PLC, stereo and resampling;
complete Opus packet-to-PCM decoding and full-codec performance remain unverified.

## [0.1.6] - 2026-10-29

- Add loss-free one-channel SILK PCM synthesis at 8/12/16 kHz for 10/20 ms
  internal frames, consuming existing decoded gains, NLSFs, pitch and excitation.
- Apply reference fixed-point gain adjustment, LTP rewhitening/prediction, LPC
  filtering and signed-16 saturation with persistent histories and reset support.
- Reject incompatible geometry or voiced pitch before mutating state; define
  rate-switch history clearing and preserve allocation-free, safe Rust behavior.
- Match every PCM sample and persistent LPC/PCM history value across 192 pinned
  Opus 1.6.1 reference frames; exercise typed integration and state rollback.

Full frame orchestration, stereo, CNG/PLC, resampling and CELT/Hybrid mixing are
still absent. This milestone does not establish full-codec conformance or speed.

## [0.1.5] - 2026-10-29

- Share the validated one/two-frame paths across all packet codes, including
  padding; avoid the general length buffer for these cases.
- Build multi-frame CBR views without an intermediate length array. Store VBR
  lengths in u16 only after validating the final frame size; retain usize sums.
- Add eight-frame inline storage to reduce initialization/copying for shorter
  multi-frame packets, preserving the public API, no allocation and safe Rust.
- Expand exact-span regression coverage to all counts 1–48, CBR/VBR, padding
  and frame-length boundaries, including oversized lengths that could truncate.
- Record fresh paired benchmarks against the unchanged corpus and thresholds.
  Full packet and full-codec superiority remain unachieved; see the results.

## [0.1.4] - 2026-10-29

- Reduce packet parsing overhead with one/two-frame storage, short-form fast
  paths and cross-crate inlining, preserving the public API and validation.
- Add a machine-readable registry of 25 pinned competitor repositories and an
  offline packet-framing benchmark against an explicitly supplied libopus.
- Record paired distributions, corpus and binary hashes, compiler/CPU details,
  and before/after measurements; keep full-codec comparisons visibly missing.
- Add a competitive gate requiring a 10% aggregate win and no >5% critical-case
  regression at the bootstrap upper bound, plus negative tests for false wins.
  The current full packet gate still FAILS; PCM superiority is unestablished.
- Preserve local-only development until the owner resumes GitHub activity.

## [0.1.3] - 2026-10-28

- Decode SILK pulse rate levels, bounded LSB escapes, shell trees and signs,
  including the padded 120-sample internal frame.
- Reconstruct deterministic Q14 excitation from pulses, quantization offset
  and seed; reject invalid context and preserve entropy on failure.
- Match upstream pulse/excitation fingerprints and entropy counters across
  1584 rate/duration/signal/offset/seed cases; test malformed inputs and four
  targeted arithmetic, padding and state mutations.
- Document 25 GitHub comparison baselines with pinned evidence and measurable
  compatibility, speed, memory, audio-quality and usability targets. No
  comparative performance or overall leadership claim is made.

LTP/LPC synthesis, complete frame orchestration and PCM output remain unimplemented.

## [0.1.2] - 2026-10-28

- Decode SILK pitch, five-tap LTP coefficients, scaling and excitation seed for
  8/12/16 kHz and 10/20 ms internal frames.
- Support conditional lag history, absolute lag escapes and independent coding
  with or without transmitted LTP scaling. Reset history on rate changes.
- Preserve entropy and pitch history together on errors; clamp reconstructed
  sample lags and bound malformed conditional chains.
- Match 3618 upstream reference steps, covering every pitch contour and LTP
  codebook entry; exercise all two-byte inputs across 18 configurations.

Excitation decoding, post-loss processing and PCM synthesis remain unimplemented.

## [0.1.1] - 2026-10-28

- Add fixed-point NLSF-to-LPC conversion for both SILK filter orders and frame
  halves, including coefficient fitting and bounded stability correction.
- Expose prediction filters through `FrameNlsf::to_lpc()` and `FrameLpc`.
- Verify exact coefficients and inverse prediction gains against 2281 spectra
  from Opus 1.6.1, plus integration with the NLSF decoding reference sequences.
- Preserve zero dependencies and allocation-free decoder primitives.

PCM synthesis and post-loss bandwidth expansion remain unimplemented.

## [0.1.0] - 2026-10-28

Initial development baseline, not yet an audio decoder.

- Parse Opus packet framing and expose borrowed frames and TOC metadata.
- Decode range-coded symbols, raw bits and bounded unsigned integers.
- Decode SILK VAD/LBRR headers, signal types and subframe gains.
- Decode and reconstruct SILK NLSF spectra for orders 10 and 16, including
  stabilization, interpolation and transactional history updates.
- Validate primitives against pinned Opus 1.6.1 reference fixtures, exhaustive
  short-input cases and targeted arithmetic/state mutations.
- Enforce pure Rust, zero Cargo dependencies, no unsafe code and Rust 1.87.
- Add offline tests, formatting, Clippy, rustdoc and dependency checks to CI.

PCM synthesis, full SILK/CELT decoding and encoding are not implemented.
