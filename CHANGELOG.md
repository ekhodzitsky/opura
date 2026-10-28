# Changelog

Versions follow SemVer. Before 1.0, incompatible public API changes increment
the minor version; compatible additions and fixes increment the patch version.
Each version has an annotated Git tag. Entries describe implemented behavior
and validation, without implying full Opus compatibility before PCM decoding.

## [Unreleased]

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
