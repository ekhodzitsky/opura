# Changelog

Versions follow SemVer. Before 1.0, incompatible public API changes increment
the minor version; compatible additions and fixes increment the patch version.
Each version has an annotated Git tag. Entries describe implemented behavior
and validation, without implying full Opus compatibility before PCM decoding.

## [Unreleased]

## [0.1.45] - 2026-11-02

- Add fixed-storage CELT frame analysis over validated planar channel contexts,
  orchestrating long or interleaved short MDCTs and exposing bounded coded spectra.
- Match native stereo-to-mono separate-half rounding and low-rate spectral gain/
  cutoff at all four durations, five input rates and three channel layouts.
  Preserve input and return no partial output on invalid contexts.
- Match1080 independent original complete spectra, verify PCM/context/transform
  composition and malformed domains, and detect ten targeted mutations. New and
  previous input/transform corpora reproduce; all seven mandatory offline gates
  pass with no dependencies or unsafe code.

Transient/pitch-prefilter decisions, band analysis/quantization and complete audio
encoding remain under development.

## [0.1.44] - 2026-11-02

- Add safe allocation-free CELT signed16 input pre-emphasis at all five standard
  rates and four block sizes, with selected mono/stereo interleaved channels.
- Match Q12 conversion, low-rate zero insertion and Q15 filter memory across
  frames/rates; expose typed input rates, memory diagnostics and reset. Preserve
  complete output/state on errors and retain ignored output slots.
- Match512 independent original sample/memory cases, including both native
  fast/general paths, extreme PCM, rate/channel changes and resets. Verify
  duration chunking/channel isolation, detect eleven targeted mutations and
  reproduce optional fixtures. All seven mandatory offline gates pass.

Pitch prefilter/overlap staging, further analysis/quantization and complete audio
encoding remain under development; no Cargo dependency is introduced.

## [0.1.43] - 2026-11-02

- Add safe allocation-free standard CELT forward MDCT analysis for all four
  block sizes and ten valid interleaved size/stride combinations.
- Match original window/folding, pre-rotation/Q16 scale, signal headroom,
  staged FFT downshift and rounded spectral post-rotation using existing tables.
  Preserve input, ignored output slots and complete output on geometry errors.
- Match304 independent original spectra and observed headroom decisions, including
  signed extremes, impulses, random scales and sequential short blocks; cover
  headroom0..7 and detect eleven targeted mutations. Forward and prior inverse
  corpora reproduce; all seven mandatory offline checks pass with zero dependencies.

Complete audio encoding remains under development.

## [0.1.42] - 2026-11-02

- Add a safe fixed-capacity Opus range encoder: CDF/ICDF, probability bits,
  raw suffix bits, uniform integers and clipped classic Laplace values.
- Match native normalization/carry chains, whole/eighth-bit accounting, uniform
  prefix patching, raw-suffix relocation and minimal endpoint termination with
  partial-byte overlap. Expose finalized bytes, idempotent finish and reset.
- Preserve all state/bytes on invalid models, lifecycle, capacity and late write
  failures using a bounded write journal without per-symbol frame copies.
- Match607 independent original encoder cases and10969 intermediate states/full
  final bytes; verify mixed round trips, all Laplace zero-frequency extremes,
  resource rollback and twelve detected mutations. Optional fixtures reproduce;
  all mandatory offline gates pass with zero Cargo dependencies.

Audio analysis, quantization and complete packet encoding remain pending.

## [0.1.41] - 2026-11-02

- Add matched optional Rust/C complete-decoder replay benchmarks over all twelve
  official streams and all ten output configurations, with no Cargo dependency.
- Verify all 120 accepted PCM SHA256 hashes and full PCM/range fingerprints
  before 15 randomized CPU-pinned paired rounds; retain throughput, latency,
  decoder-state sizes and source/compiler/corpus provenance in a versioned report.
- Record every measured loss: Opura takes about 1.54–1.80 times the pinned scalar
  reference time and uses more decoder state. The scalar speed target fails;
  SIMD, other competitors, explicit PLC/FEC speed and encoder quality are unmeasured.
- Add fail-closed report/protocol/correctness fault tests and update stale
  full-codec gate diagnostics. All seven mandatory offline gates pass.

## [0.1.40] - 2026-11-02

- Validate the unified decoder against all120 official RFC8251 combinations:
  12 streams at five output rates and mono/stereo,200750 decoded packets and
  exact nonzero encoded final ranges, with original PCM conformance metrics.
- Add optional bounded vector-file decoding, archive/RFC file-hash verification,
  unchanged external comparator integration and atomic sanitized JSON reports.
- Reproduce complete PCM fingerprints/counts/metrics in two full runs. Prove live
  gates reject a corrupted range and zeroed PCM; add framing/endian/range/output
  and archive/metric/report fault tests. No decoder DSP fix was required.
- Record reproducible conformance evidence and update decoder capability docs;
  all mandatory offline gates and example/tool checks pass without Cargo deps.

Decoder performance validation and encoding remain pending; conformance does
not establish SOTA speed or encoder quality.

## [0.1.39] - 2026-11-02

- Add a transactional fixed-storage unified Opus decoder with shared SILK/CELT
  histories across all standard modes, rates and encoded/output channel layouts.
- Reconstruct both5ms redundant CELT tail directions, shrink primary entropy
  storage, apply native mode resets and2.5ms fades, and retain final-range XOR.
- Orchestrate all-mode explicit/implicit loss/DTX and requested FEC intervals,
  including initial silence, short SILK state advancement, prefix PLC/fallback,
  Hybrid-to-SILK fade-out, resets and native diagnostics.
- Match2440 public libopus PCM/range and independent complete CELT/PLC/context
  snapshots, including stale tails and48-frame/120ms packets. The unified API
  also matches3002 previous CELT/Hybrid loss/FEC cases unchanged.
- Verify512 corrupt mixed-mode sequences, retained deep/late transition failures,
  FEC-prefix rollback and22 detected mutations. Optional corpora reproduce and all
  mandatory offline checks pass.

Official conformance, full decoder performance validation and encoding remain
pending; SOTA performance is not yet established.

## [0.1.38] - 2026-11-01

- Add transactional Hybrid explicit/implicit loss, DTX, initial silence and
  first-frame in-band FEC with exact SILK/CELT concealment and recovery.
- Schedule120..5760 samples in2.5ms steps on the last packet clock; advance full
  SILK10ms state for shorter output and preserve Hybrid high-band noise context.
  Distinguish FEC persistent RNG diagnostics from zero loss final range.
- Fix SILK PLC tiny-gain LTP normalization: preserve original signed16 scale
  truncation and negative coefficients. Accept CNG's positive signed16 noise
  scale domain through32767; match192 additional original PCM/full-state cases.
- Match2130 public libopus PCM/range and independent complete CELT/PLC snapshots,
  including initial FEC, LBRR gaps, irregular losses, channel/bandwidth changes
  and resets. Verify128 corrupt mixed sequences, duration rollback and FEC
  first-frame behavior; thirteen targeted mutations are caught.
- Existing corpora remain unchanged and reproduce; all mandatory offline gates pass.

CELT transition tails, unified cross-mode orchestration, standalone SILK implicit
DTX, full conformance and encoding remain pending.

## [0.1.37] - 2026-11-01

- Add transactional fixed-storage normal Hybrid decoding: shared SILK/CELT
  entropy, 16kHz low band, SWB/FB high bands and exact signed16 accumulation.
- Support10/20ms frames, mono/stereo encoded and output channels, all five PCM
  rates, normal LBRR, padding and multi-frame packets up to120ms.
- Reuse positioned layer kernels without changing standalone SILK/CELT evidence;
  retain CELT start-band diagnostics and history clearing/reset semantics.
- Match960 public libopus PCM/range results and independent complete CELT state
  snapshots, including clipping and channel/bandwidth changes. Verify packet
  versus individual-frame equivalence, deep/late failure rollback,512 corrupt
  packets and twelve detected mutations. Optional corpora reproduce; all offline gates pass.

Hybrid loss/FEC/DTX, transition tails, unified cross-mode orchestration, full
conformance and encoding remain pending.

## [0.1.36] - 2026-11-01

- Add transactional CELT periodic/noise PLC, explicit48kHz-clock loss scheduling,
  initial silence, empty/one-byte DTX frames and normal recovery across mixed
  frame/channel/bandwidth and multi-frame packet contexts.
- Preserve full signal/future/LPC history; connect pitch/excitation decay, bounded
  synthesis stabilization/energy guards, negative comb prefilter/TDAC, noise RNG
  and background/recovery updates. Public range is zero for loss; RNG persists.
- Expose read-only concealment diagnostics and retain native reset band-limit
  semantics. Every error rolls back the complete frame/packet/loss operation.
- Match public libopus PCM, range and complete state on872 scenarios, including
  long bursts, mixed implicit loss, resets and210x120ms stress reaching caps.
- Match208 original bandwidth and232 energy-guard cases; verify256 adversarial
  normal/loss/recovery sequences, guard/duration rollback and a retained late
  corrupt-uniform regression. Fifteen mutations are caught.
- Existing normal corpus remains unchanged; all optional corpora reproduce and
  mandatory offline gates pass.

Hybrid, cross-mode transitions, full conformance and encoding remain pending.
Standalone SILK implicit DTX also awaits unified packet orchestration.

## [0.1.35] - 2026-11-01

- Add fixed-storage classic CELT PLC pitch analysis from2048 mono/stereo internal
  history samples: factor2 lowpass/LPC4/FIR5 whitening, coarse quarter-rate and
  fine candidate correlation, normalized pitch ranking and pseudo interpolation.
- Validate decoder signal bounds and return both the period and full1024-sample
  whitened signal without mutating input or allocating.
- Match all samples and periods on2202 original cases covering every target
  period100..720, channel combinations, phase/noise/silence and amplitude bounds.
- Domain/silence checks and seven meaningful mutations pass; optional corpus
  reproduces and all mandatory offline gates pass.

Full CELT periodic/noise loss synthesis and recovery, Hybrid, conformance and
encoding remain pending; normal CELT packets already decode to final PCM.

## [0.1.34] - 2026-11-01

- Add bounded allocation-free CELT autocorrelation/window/scaling, fast64 LPC
  analysis and Q25→Q12 coefficient fitting, Q12 FIR and canonical in-place IIR
  primitives for upcoming pitch search and periodic loss concealment.
- Validate supported geometry/correlation/reflection domains and preserve all
  outputs on errors; retain original FIR saturation and IIR memory semantics.
- Match 1601 correlations, 2680 LPC analyses, 832 FIR and 336 IIR original cases.
  Add power-of-two estimator boundaries and clustered positive covariance cases
  after mutation testing exposed missing scale/fitting coverage.
- Independent zero-filter/correlation bounds, invalid/unstable rollback and seven
  arithmetic mutations pass; corpus reproduces and mandatory offline gates pass.

Normal CELT PCM remains supported; periodic/noise PLC, Hybrid and complete Opus
conformance/encoding are still pending.

## [0.1.33] - 2026-11-01

- Add stateful allocation-free normal CELT-only frame/packet decoding to final
  mono/stereo signed-16 PCM at 8/12/16/24/48kHz, including silence, all encoded
  frame sizes/bandwidths and multi-frame packets up to 48 frames/120ms.
- Connect every verified entropy/spectral/anti-collapse/synthesis stage, preserve
  mono/stereo energy/log/background/RNG and synthesis histories, and apply
  current reference mono-API inversion policy. Frame/packet errors roll back.
- Match public pinned opus_decode PCM/final range and complete independently
  observed energy/filter/overlap/de-emphasis state on 960 real encoder packets.
- Verify packet/frame equivalence, reset, late-frame rollback, 512 adversarial
  payloads and eight integration mutations. Optional full-reference corpus
  reproduces; all mandatory offline checks pass.

CELT PLC/DTX, Hybrid, cross-mode transitions, full conformance and encoding remain
pending. End-to-end normal CELT evidence does not establish SOTA performance.

## [0.1.32] - 2026-11-01

- Add atomic transient anti-collapse on normalized spectra: bit-depth thresholds,
  current/two prior energy attenuation, mono history max, 20ms amplitude scaling,
  interleaved per-block noise and legacy vector renormalization.
- Validate matching transient 10/20ms allocation/reservation and bounded Q24
  energies; preserve the complete spectrum on errors and retain the stored seed
  because anti-collapse consumes a local RNG copy.
- Match 1850 original complete-stage cases through reserved control/final energy
  and optional anti-collapse, including 395 enabled restorations (135 changing
  coefficients); reject 166 corrupt streams across 2016 total cases.
- Check invalid context/energy rollback and seven targeted mutations; all prior
  spectral evidence regenerates unchanged and mandatory offline gates pass.

Stateful compressed CELT PCM integration, loss recovery and Hybrid remain pending.

## [0.1.31] - 2026-11-01

- Assemble complete normalized CELT mono/stereo spectra from immutable
  allocation/TF contexts, with all-band budget carry, folding-source/mask
  selection, Hybrid fold tails and dual-to-intensity folding conversion.
- Return fixed-storage Q24 spectra, channel masks and folding seed; inactive
  channels/bins stay zero and failures preserve entropy. Accept real CELT/Hybrid
  starts 0/17 and validate the TF/frame context and physical payload budget.
- Match selected spectra, masks, seed and entropy on 3779 original full-stage
  sequences from coarse energy through all bands, including 1008 prefix cases;
  reject 253 corrupt streams atomically across 4032 total cases.
- Verify invalid TF/start/payload/cursor rollback and eight mutations. Reference
  regeneration and all mandatory offline gates pass.

Anti-collapse, frame energy history and compressed CELT PCM integration remain
pending; Hybrid and complete Opus conformance still require further work.

## [0.1.30] - 2026-11-01

- Add transactional mono/stereo CELT band decoding over recursive partitions:
  one-coordinate signs, TF Haar/Hadamard reorganization and collapse masks,
  scaled folding output, stereo theta/intensity/inversion, N2 orthogonal-side
  signs and general stereo allocation/merge.
- Preserve the existing partition API and fixed-storage bounds; validate TF,
  initial blocks, stereo gain/intensity and fill domains before decoding.
- Match both channels, folding output, masks, budget/seed and entropy on 15680
  original band cases covering every frame/band/TF/channel context; reject 28
  corrupt uniform cases atomically. Domain rollback and nine mutations pass.
- Break a correlated oracle parameter combination exposed by mutation testing
  to verify inversion override independently. Fixtures/tables reproduce and all
  mandatory offline gates pass.

All-band folding/budget orchestration, anti-collapse and complete CELT/Hybrid
packet decoding remain required before full Opus conformance.

## [0.1.29] - 2026-11-01

- Add transactional classic CELT mono partition decoding: standard pulse-cache
  lookup, uniform/triangular theta, recursive gains/transient allocation and
  rebalancing, pulse budget fallback, deterministic noise/folding and masks.
- Support sixteen blocks internally with a u16 partition mask, preserving the
  existing public eight-block PVQ contract. Bound recursion to four split levels.
- Match complete coordinates, masks, seed/budget and entropy on 9409 original
  cases covering every valid standard geometry and fresh/positioned cursors;
  reject 29 corrupt uniform cases atomically. Check malformed domains, negative
  budget continuation and eight targeted mutations.
- Reference tables/fixtures reproduce; all mandatory offline checks pass.

Time-frequency/stereo band orchestration and complete CELT/Hybrid remain pending.

## [0.1.28] - 2026-11-01

- Add bounded allocation-free CELT coded-theta gains/log2tan, Haar and
  Hadamard/interleave operations, legacy folded/noise vector normalization and
  normalized stereo merge. Invalid geometry/gain/vector domains preserve inputs.
- Match 16259 coded angles, 68644 positive log-ratio pairs, 1531 vector transforms,
  420 normalizations and 400 stereo merges from unchanged pinned reference code.
- Check permutation inverses, invalid-input rollback and seven arithmetic/order
  mutations. Constants/fixtures reproduce and all mandatory offline gates pass.

Recursive spectral entropy decoding and full CELT/Hybrid remain incomplete.

## [0.1.27] - 2026-11-01

- Add bounded transactional classic CELT allocation: caps, dynamic boosts, trim,
  anti-collapse reservation, interpolated curves, skip/intensity/dual fields,
  PVQ eighth-bit budgets, fine-energy bits/priorities and excess balancing.
- Preserve encoded frame/layout/transient context in immutable allocation results
  and expose fine arrays directly to existing energy stages.
- Match every result/entropy field and following fine-energy output on 16296
  original cases, including every nonempty band interval at every LM/channel.
- Verify budget conservation, malformed rollback and 512 deterministic adversarial
  cases; six mutations are detected. Tables/fixtures reproduce and all gates pass.

Recursive CELT band/shape decoding is still required to feed compressed audio
through the verified synthesis backend; full Opus remains incomplete.

## [0.1.26] - 2026-11-01

- Add transactional CELT control prefix decoding with typed silence/postfilter/
  transient/intra fields, positioned start0/17 contexts and CELT Packet wrapper.
- Preserve cached-tell decisions and physical silence accounting; add exact
  differential TF/select reservation and spreading models/budget fallbacks.
- Match fields and range/tell/tell_frac on 3360 original prefix/coarse/TF/spread
  sequences and 1824 standalone TF boundary cases, including all postfilter
  octave/pitch/gain/tap boundaries and 2..1275-byte payload budgets.
- Exhaust every two-byte prefix at all frame sizes/start contexts; validate
  malformed rollback and silence accounting overflow. Six mutations are caught;
  fixtures/tables reproduce and all mandatory offline gates pass.

These control stages still need CELT allocation and recursive spectral decoding
before compressed frames can use the verified PCM backend.

## [0.1.25] - 2026-11-01

- Add atomic fixed-rate/channel `CeltSynthesis` from decoded normalized spectra
  and Q24 energies through scaling, mono/stereo projection, long/transient MDCT,
  synthesis clipping, comb postfilter and final de-emphasis/decimation to PCM.
- Preserve independent output-channel overlap/filter histories across encoded
  channel changes; match separate-half downmix rounding and short-block stride.
- Match final PCM and all overlap/postfilter/de-emphasis state on 480 original
  complete-backend cases across every rate/frame/channel/transient combination.
- Verify malformed rollback/reset and five integration mutations. Oracle data
  reproduces byte-identically; all mandatory offline checks pass.

Input is already decoded spectra. CELT entropy/allocation/recursive band decoding,
loss concealment, Hybrid and complete Opus conformance remain incomplete.

## [0.1.24] - 2026-11-01

- Add bounded per-channel classic CELT comb postfilter with validated periods,
  Q15 gains/tapsets, processed history and old/current parameter generations.
- Match constant/windowed in-place arithmetic, distinct transition/constant
  biases, clipping and 120-sample versus long-frame parameter scheduling.
- Match every output, all 1026 history entries and six parameter fields on 576
  chained original cases across tapsets, gains, periods, sizes, resets/extremes.
- Verify delayed short parameters, disable-tail passthrough, malformed rollback,
  five detected mutations and reproducible fixtures. All offline gates pass.

End-to-end CELT synthesis integration, allocation/recursive bands and full Opus remain incomplete.

## [0.1.23] - 2026-11-01

- Add fixed-storage CELT spectral denormalization using Q24 band/mean energies,
  reference mantissa rounding/gain caps, selected bands, silence and rate cutoff.
- Add canonical per-channel output de-emphasis and signed-16 PCM conversion at
  8/12/16/24/48 kHz. Filter every internal sample before decimation, retain both
  memories across layout/rate changes and validate geometry before mutation.
- Match every scaled spectrum on 640 original cases and PCM/memories on 384
  chained de-emphasis cases across all frame/rate/channel combinations.
- Check malformed rollback/reset, five detected mutations and byte-identical
  table/fixture regeneration; all mandatory offline checks pass.

Comb postfilter, CELT recursive bands/allocation and complete packet output remain incomplete.

## [0.1.22] - 2026-11-01

- Add per-channel inverse CELT MDCT and standard 120-sample TDAC window with
  persistent 60-sample overlap across 120/240/480/960-sample block changes.
- Support validated strided spectra for interleaved short blocks; preserve exact
  headroom selection, rotations, FFT scaling and pathological signed wrapping.
  Return internal signed-32 signal, before postfilter/de-emphasis/final PCM.
- Match all 198360 time samples and overlap values in 398 original chained cases,
  including every size/stride geometry, resets, silence, impulses and extremes.
- Verify invalid-input rollback/reset, five detected mutations and byte-identical
  table/fixture regeneration. All mandatory offline checks pass.

CELT allocation/recursive bands, postfiltering and full packet decoding remain incomplete.

## [0.1.21] - 2026-11-01

- Add allocation-free standard CELT complex FFT plans for 60/120/240/480 points,
  with exact radix 2/3/4/5 arithmetic, bit reversal and shared Q15 twiddles.
- Expose forward transforms with 0..31 per-stage downshifts, canonical scaled
  forward and unscaled inverse wrappers. Validate buffers before any write and
  preserve output tails; intentional reference wrapping is explicit.
- Match every complex output on 1632 original FFT results across all plans,
  shifts, impulses, DC/Nyquist tones, full-range extremes and deterministic input.
  Independently check order/sign by DFT and 1/N scaling with exact impulses.
- Detect five mutations; extracted plans and fixtures regenerate identically.
  All mandatory offline gates pass; transform synthesis still needs inverse MDCT.

## [0.1.20] - 2026-11-01

- Add atomic normalized CELT PVQ bands from positive-pulse codebooks: Q31 gain,
  high-precision Q24 normalization, all spreading modes and 1/2/4/8-block masks.
- Implement refined Q31-input/Q29-output reciprocal square root with explicit
  normalized-domain validation and reference Newton arithmetic.
- Match every coordinate, mask and entropy diagnostic in 28464 original
  alg_unquant results across 4116 streams; match 196608 refined-root inputs.
- Verify invalid gains/geometry and raw-bit rollback; five mutations detect root
  refinement, rotation order/rounding/stride coefficients and collapse mask.
  Reference corpus reproduces and all required offline checks pass.

Recursive CELT bands, allocation, transforms and complete Opus decoding remain incomplete.

## [0.1.19] - 2026-11-01

- Add fixed-storage CELT algebraic pulse codebooks with compile-time symmetric U
  counts, checked 32-bit cardinality and exact CWRS index/entropy decoding.
- Support 2..176 coordinates and 0..128 pulses when representable; zero pulses
  consumes no bits. Expose pulse vectors and exact squared norms.
- Match all 22575 configuration outcomes, 70423 exhaustive small-codebook indices,
  13401 boundary/random vectors and 566 entropy steps across 73 streams against
  original generic cwrs.c; independently verify cardinalities combinatorially.
- Verify invalid indices/geometries, raw-suffix rollback and pulse invariants;
  four targeted mutations fail. Fixtures reproduce and all mandatory gates pass.

CELT band normalization/rotation, allocation and transforms remain incomplete.

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
