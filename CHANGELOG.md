# Changelog

Versions follow SemVer. Before 1.0, incompatible public API changes increment
the minor version; compatible additions and fixes increment the patch version.
Each version has an annotated Git tag. Entries describe implemented behavior
and validation, without implying full Opus compatibility before PCM decoding.

## [Unreleased]

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
