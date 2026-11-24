# Range and CELT encoder implementation

Historical notes from the milestones leading to v0.2.5. Commands assume the
repository root. For the integrated codec, see the [current support and evidence](../validation/codec-readiness.md).

## Range encoding foundation

`opura::range::RangeEncoder` owns fixed storage for one 1..=1275-byte frame.
It supports validated CDFs, 8-bit inverse CDFs, log-probability bits, raw suffix
bits, split uniform integers and classic CELT Laplace deltas. Laplace returns
the actually coded value when the finite tail clips a large input. Range and
whole/eighth-bit counters match the native entropy coder. This is an encoder
primitive; audio analysis, quantization and complete packet encoding remain work.

```rust
use opura::range::{RangeDecoder, RangeEncoder};
let mut encoder = RangeEncoder::new(8)?;
encoder.encode_uint(123, 257)?;
encoder.encode_bits(5, 3)?;
let frame = encoder.finish()?;
let mut decoder = RangeDecoder::new(frame);
assert_eq!(decoder.decode_uint(257)?, 123);
assert_eq!(decoder.decode_bits(3)?, 5);
# Ok::<(), Box<dyn std::error::Error>>(())
```

Successful finalization returns exactly the current capacity, including zero
padding. `shrink(bytes)` moves the written raw suffix to a smaller logical end;
`patch_initial_bits(value, bits)` changes up to eight reserved uniform prefix
bits. The caller must ensure that the prefix model permits patching. `finish()`
is idempotent, `bytes()` is unavailable before success, and encoding/patching/
shrinking after finish fails. `reset()` clears bytes and state while retaining
current capacity. Every error preserves all bytes and state, including late raw
or termination failures. A small copied scalar state and bounded write journal
avoid copying the whole frame per symbol; the implementation allocates nothing.

Callers reserve a codec-specific bit budget and check finalization: a successful
symbol operation does not guarantee termination will fit. Conversely, `tell()`
includes conservative termination overhead; eight exact uniform bits can fit in
one byte while reporting nine bits. Native partial raw/termination overlap is
retained. Invalid model/value/precision, capacity and lifecycle failures use
`EncoderError`; errors are recoverable rather than sticky. Capacity cannot grow.

The independent pinned original encoder corpus covers 607 cases and 10,969
intermediate states, complete final bytes, carry runs exceeding 1,199 bytes,
all prefix patch locations, raw relocation, tight overlap and expected finish
failures. Mixed-model round trips preserve range and both counters; extreme
Laplace tails round-trip at all 32,736 zero frequencies and both decay endpoints.
Twelve targeted mutations are detected. Regeneration is optional:

```sh
python3 tools/range_encoder_fixtures.py /path/to/opus-v1.6.1 > /tmp/range-encoder-fixtures.txt
diff -u tests/range-encoder-fixtures.txt /tmp/range-encoder-fixtures.txt
```

The optional oracle compiles unchanged external entropy sources temporarily;
normal Cargo builds/tests use the retained fixtures and have no foreign dependency.

## CELT forward transform foundation

`opura::celt::mdct::analyze` converts a pre-emphasized internal-rate signal
block to standard CELT MDCT bins using the canonical 120-sample window. It
supports all four block sizes and the ten valid size/stride combinations.
Input contains exactly `size.samples() + 120` signed-32 values, including
context, and remains unchanged. Output bin i is written at i*stride; all other
entries remain intact. Invalid input/output geometry preserves the whole output.
The transform uses fixed storage, safe Rust and the existing canonical FFT.

Window/folding, Q16 scale, signal-dependent headroom, FFT stage downshift and
rounded post-rotation match the pinned scalar fixed-point reference. The
independent corpus retains complete spectra and observed headroom for 304 cases:
silence, boundary impulses, full signed extremes, random scales and interleaved
short blocks. It covers every headroom value from zero through seven. Eleven
targeted mutations are detected; existing inverse fixtures reproduce unchanged.

```sh
python3 tools/celt_mdct_fixtures.py /path/to/opus-v1.6.1 --forward > /tmp/celt-forward-mdct-fixtures.txt
diff -u tests/celt-forward-mdct-fixtures.txt /tmp/celt-forward-mdct-fixtures.txt
```

This is transform analysis, not a complete audio encoder. Pre-emphasis, encoder
band analysis/quantization and packet orchestration remain under development.

## CELT signed16 input foundation

`opura::celt::input::PreemphasisState` converts one selected mono/stereo channel
to pre-emphasized Q12 signal at CELT's 48 kHz internal clock. `InputRate` supports
8/12/16/24/48 kHz. Input is exactly one interleaved frame at the selected rate;
output holds 120/240/480/960 internal samples. Trailing output slots within a
960-sample buffer are preserved. Each channel uses its own state.

```rust
use opura::celt::{FrameSize, input::{InputRate, PreemphasisState}};
let mut filter = PreemphasisState::default();
let mut signal = [0; 120];
filter.process(FrameSize::Ms2_5, InputRate::Khz48, &[0; 120], 1, 0, &mut signal)?;
assert_eq!(signal, [0; 120]);
# Ok::<(), opura::celt::input::InputError>(())
```

The stage converts signed16 to Q12, inserts zeros for low-rate input and applies
the standard Q15 coefficient 27853. It keeps native memory across rate/frame
changes; `reset()` clears it for an independent stream, and `memory()` exposes
the pending Q12 term. Invalid layout/channel/capacity preserves the complete
output and memory. No allocation, intermediate signal buffer or foreign code
is used. Signed16 input does not need the reference's wider-input clipping path.

All samples and both channel memories match 512 independent original cases over
every rate/frame/channel combination, signed extremes, rate/layout changes,
resets and both native fast/general paths. Duration chunking and channel
isolation pass; eleven targeted mutations are detected. Optional regeneration:

```sh
python3 tools/celt_input_fixtures.py /path/to/opus-v1.6.1 > /tmp/celt-input-fixtures.txt
diff -u tests/celt-input-fixtures.txt /tmp/celt-input-fixtures.txt
```

This primitive does not retain MDCT overlap or perform pitch prefiltering. Low-rate
spectra still need native scaling/cutoff after transformation. Those stages,
encoder analysis/quantization and complete packet encoding remain work.

## CELT channel and subframe analysis

`opura::celt::analysis::analyze_frame` wraps the forward transform for complete
channel contexts. Input is exactly one or two planar channels, each containing
`size.samples() + 120` pre-emphasized/prefiltered values with caller-supplied
history. A frame can use one long transform or `2^LM` interleaved 120-sample
short transforms. Stereo input can produce stereo or mono coded spectra;
mono input produces mono. The fixed `SpectralFrame` exposes planar bins and
bounded per-channel views without allocation or input mutation.

Stereo-to-mono mixing halves each channel separately before addition, retaining
native negative/odd rounding. For low-rate input, bins below
`size.samples()/rate.factor()` receive the native gain and higher bins become
zero. The sequence is transform, channel mixing, then rate scaling/cutoff.
Extreme synthetic signals use explicit signed wrapping, matching the reference.

All complete spectra match 1,080 independent original `compute_mdcts` cases
across four durations, five input rates, three input/coded channel layouts and
both long/short modes. Signed extremes, low-amplitude rounding and random scales
are retained. PCM preprocessing/context/transform composition and malformed
geometry tests pass; all ten targeted mutations are detected. Optional oracle:

```sh
python3 tools/celt_analysis_fixtures.py /path/to/opus-v1.6.1 > /tmp/celt-analysis-fixtures.txt
diff -u tests/celt-analysis-fixtures.txt /tmp/celt-analysis-fixtures.txt
```

The caller selects the block mode and supplies the complete signal context.
Transient detection, pitch-prefilter/overlap orchestration, band analysis,
quantization and complete audio packet encoding remain under development.

## CELT band analysis foundation

`opura::celt::analysis::analyze_bands` measures and normalizes a planar mono/stereo
spectrum at any standard CELT block size. `BandAnalysis` owns fixed normalized
Q24 channel vectors, native amplitudes and mean-adjusted Q24 log energies.
Band ends satisfy `1 <= effective_end <= end <= 21`: measured bands end at
`effective_end`, later log energies through `end` are -14, and inactive storage
is zero. Bounded per-channel views expose each result without allocation.

Headroom depends on native canonical band widths. Energy uses the refined
square-root estimate and maximum-coefficient floor; normalization applies the
native extra unit for amplitudes below ten and a refined reciprocal. The math
module now exposes `sqrt_extended` (QX to Q(X/2+16), zero/clamp semantics) and
`reciprocal_normalized_q31` (normalized [2^30,2^31) to Q30). Existing reciprocal
results remain unchanged after sharing its Q15 initialization.

Input is never modified. Only active coefficients are inspected; unused bins
may contain arbitrary values. Active `i32::MIN`, unrepresentable headroom sums
and overflowing signed32 squared-energy accumulations return
`AnalysisError::ArithmeticOverflow`, with no partial result. This explicitly
bounds the reference's implicit arithmetic domain. Ordinary signed16 PCM paths
through preprocessing and long/short/channel/rate transforms pass these checks.
The primitive does not quantize energies or normalized shapes.

All amplitudes, normalized samples and log energies match 640 independent
original cases. Scalar evidence covers 69,834 wide-square-root and 34,877
normalized-reciprocal inputs through dense hashes and boundary cases. Tests
include silence, tiny-energy protection, active-end tails, wide bands, random
scales, PCM composition and rejected extreme energy domains. Fourteen targeted
mutations are detected; earlier scalar math results reproduce unchanged.

```sh
python3 tools/celt_band_analysis_fixtures.py /path/to/opus-v1.6.1 > /tmp/celt-band-analysis-fixtures.txt
python3 tools/celt_band_analysis_fixtures.py /path/to/opus-v1.6.1 --math > /tmp/celt-analysis-math-fixtures.txt
diff -u tests/celt-band-analysis-fixtures.txt /tmp/celt-band-analysis-fixtures.txt
diff -u tests/celt-analysis-math-fixtures.txt /tmp/celt-analysis-math-fixtures.txt
```

Transient/pitch-prefilter decisions, quantization and complete audio packet
encoding remain under development.

## CELT energy encoding foundation

`opura::celt::energy::EnergyEncoder` keeps reconstructed energies, current Q24
residuals and native delayed-intra distortion. `CoarseEncoding` supplies the
band/channel layout, duration, effective distortion end, available bytes,
force/two-pass choices, loss percentage and LFE policy. Coarse encoding matches
native temporal/inter-band prediction, decay limits, bit-scarcity fallbacks and
inter/intra RDO. It returns the selected flag and writes it only when it fits.
Fine and final methods insert the selected raw refinements in native order.

Each complete stage commits both energy and entropy state together; errors leave
both unchanged. Inputs are selected Q24 log energies in [-28,16], covering the
standard signed16 analysis domain; inactive targets are ignored. Reconstructed
histories outside the layout persist, while residuals outside a new coarse
layout become zero. Accessors expose histories, residuals and delayed distortion;
reset clears all three. Fine quantizers are 0..8 bits. Final refinement returns
unused budget and rejects budgets beyond conservative remaining storage.
Stages are separate so allocation and shape coding can occur between them.

All finalized bytes, stage ranges/whole/eighth-bit counters, active energies,
residuals, delayed distortion and chosen flags match 388 independent original
packets (212 intra,176 inter). The corpus retains both original encoder and
decoder histories. Eleven artificial prefix-scarcity cases reproduce native
history differences, including the first-band one-bit path; they are not treated
as encoder/decoder history agreement. Callers must reserve an appropriate layer
budget. A separate 96-frame test verifies full matching energy/range/counter
round trips at ordinary budgets across frame/layout changes and resets.

Tests verify late coarse/fine/final arithmetic failures after staged writes,
invalid options/targets/refinements and finished writers. All sixteen targeted
mutations are detected. The original RDO routine leaves unused residual scratch
uninitialized; the optional oracle clears only unselected entries before reading
them, matching Opura's deterministic contract without changing active DSP.

```sh
python3 tools/celt_energy_encoder_fixtures.py /path/to/opus-v1.6.1 > /tmp/celt-energy-encoder-fixtures.txt
diff -u tests/celt-energy-encoder-fixtures.txt /tmp/celt-energy-encoder-fixtures.txt
```

These are energy bitstreams, not complete compressed audio packets. Shape
quantization, frame controls, transient/pitch decisions and full packet encoding
remain under development. All stages use fixed storage, safe Rust and zero Cargo
dependencies; original C is confined to optional external probes.

## CWRS pulse encoding foundation

`PulseCodebook::index_of(values)` ranks a signed integer vector in native CWRS
order; `encode(writer, values)` writes the corresponding uniform index with
native range/raw splitting. Vector length and L1 pulse sum must match the
validated book. Both methods allocate nothing and preserve input; encoding
errors preserve the whole writer. Zero pulses accepts only the all-zero vector,
consumes no bits and still requires an unfinished writer. This maps already
selected integer pulses; it does not choose them from a spectral signal.

Independent original `icwrs`/`encode_pulses` fixtures cover all 1,834 representable
books in the 2..176 dimension/0..128 pulse domain: 13,395 vectors with full streams,
ranked indices and range/whole/eighth-bit counters. Tests also invert all 70,423
indices of the retained small books plus deterministic wide samples, verify
malformed length/L1/signed extremes and capacity/lifecycle rollback, and detect
nine targeted mutations. Prior pulse-decoder fixtures reproduce unchanged.

```sh
python3 tools/celt_pulse_fixtures.py /path/to/opus-v1.6.1 --encode > /tmp/celt-pulse-encoder-fixtures.txt
diff -u tests/celt-pulse-encoder-fixtures.txt /tmp/celt-pulse-encoder-fixtures.txt
```

The optional oracle uses unchanged generic SMALL_FOOTPRINT CWRS code to cover
all representable geometries. Normal Cargo builds/tests use retained fixtures
and have zero dependencies. PVQ pulse search and recursive band encoding remain
under development before these primitives form complete encoded audio.

## PVQ pulse-search encoding foundation

`opura::celt::pvq::encode_band` selects and encodes integer pulses from normalized
Q24 input using an existing positive-pulse `PulseCodebook`. It returns the same
`NormalizedBand` shape/collapse representation as decoding, resynthesized at a
nonnegative Q31 gain. Public block counts are 1/2/4/8 and must divide dimension;
the internal partition path also supports sixteen blocks and the wider mask.
The gain affects returned reconstruction, independently of pulse selection.

Input length matches the book and its squared Q24 norm is at most two
(`sum(x*x) <= 2^49`). This includes rounded unit vectors; arbitrary large signal
vectors are rejected explicitly. Forward spreading, adaptive Q14 headroom,
pyramid projection, tiny/silent fallback, greedy strict-tie decisions and sign
restoration match the native fixed-point algorithm. Input is unchanged and no
allocation occurs. Reconstruction is staged before the single atomic CWRS
write, so model, arithmetic, capacity and lifecycle errors preserve the writer.

All 16,368 independent original cases match full bytes/range/accounting, chosen
decoded integer pulses, resynthesized coordinates and collapse masks. They span
representative books, every spread/block/gain setting, raw/range prefixes,
zero/impulse/tie/tiny/random/energy-boundary vectors and the exact pyramid fallback
boundary. Original `alg_unquant` independently confirms the native resynthesis.
Mixed public band streams round-trip with identical output and counters; boundary,
invalid-domain and rollback tests pass, and fourteen targeted mutations are
caught. Prior normalized-band decoder fixtures reproduce unchanged.

```sh
python3 tools/celt_pvq_fixtures.py /path/to/opus-v1.6.1 --encode > /tmp/celt-pvq-encoder-fixtures.txt
diff -u tests/celt-pvq-encoder-fixtures.txt /tmp/celt-pvq-encoder-fixtures.txt
```

QEXT, angle/recursive-band encoding, frame controls and complete audio packet
encoding remain separate work. The optional original oracle stays outside Cargo.

## Recursive mono CELT partition encoding

`opura::celt::partition::encode_partition` uses the existing `PartitionControls`
and `PartitionState` to encode a normalized mono partition. It measures the angle
between subvector energies, quantizes it with native triangular/time-uniform
models, applies optional split-noise avoidance and performs native bit division,
rebalance and PVQ leaves. No-pulse paths retain the decoder's exact folding,
dither, LCG seed and normalization behavior. It returns a reconstructed Q24
`Partition` with up to sixteen collapse bits.

Input length matches the standard band/LM dimension and squared Q24 norm is at
most two. Existing geometry, nonnegative gain, fold and 2..1275-byte initial
storage domains apply. Input/fold buffers stay unchanged, no allocation occurs,
and the full entropy writer plus shared remaining budget/seed commit together.
Malformed models, excessive signal energy, finished writers and late physical
write failures preserve both caller states. Conservative physical budget and
requested/shared eighth-bit budgets remain distinct caller-controlled values.

`math::atan_norm` adds the reference normalized Q30 arctangent on [-1,1], with
exact +/-0.5 endpoints. `math::atan2_norm` measures a Q30 quarter-circle angle
from nonnegative same-scale magnitudes, including the native both-zero result.
Independent dense/wide evidence covers 69,833 atan and 4,321 atan2 inputs.
A finite table check verifies every one of the 64 split-capable standard geometry
cells can code an angle at its minimum split budget.

All bytes, range/whole/eighth-bit counters, reconstructed coordinates, masks and
remaining budget/seeds match 9,440 independent original encoder cases. Native
and Rust decoders reproduce each encoded partition. The set covers 286
geometries, split/pulse thresholds, endpoints/interior angles, zero/tiny/random
vectors, gain/fill/fold/prefix settings and two minimized rebalance regressions.
Those eight-coordinate cases distinguish the native three-bit reserve from an
incorrect one-bit reserve in both allocation-order branches. Twenty targeted
mutations and late capacity/invalid-domain tests are detected; earlier partition
outputs remain unchanged after extracting shared no-pulse reconstruction.

```sh
python3 tools/celt_partition_fixtures.py /path/to/opus-v1.6.1 --encode > /tmp/celt-partition-encoder-fixtures.txt
python3 tools/celt_partition_fixtures.py /path/to/opus-v1.6.1 --angles > /tmp/celt-angle-fixtures.txt
diff -u tests/celt-partition-encoder-fixtures.txt /tmp/celt-partition-encoder-fixtures.txt
diff -u tests/celt-angle-fixtures.txt /tmp/celt-angle-fixtures.txt
```

TF/stereo and full-spectrum encoding, frame controls and complete audio packet
encoding remain under development. Original probes remain external to Cargo.


## CELT mono/stereo TF band encoding

`opura::celt::partition::encode_band` accepts immutable normalized mono/left and
optional right vectors, existing `BandControls`/`PartitionState`, optional folding
input and `BandEncoding` decisions. Right input is present exactly for stereo.
The result is the existing `Band`: reconstructed channels in frequency-interleaved
order, scaled folding basis and combined collapse mask. No allocation occurs;
the full entropy writer and shared budget/seed commit together only on success.

Each input squared Q24 norm is at most 5/4 (`sum(x*x)<=5*2^46`), including rounding
margin for native band analysis. Options supply nonnegative linear amplitudes,
mono split-noise protection and stereo theta rounding (-1 biased down, 0 nearest,
+1 biased up). The caller chooses frame-wide controls and candidates. Band
geometry, gain, fold, TF and physical/requested/shared budgets follow decoding.
The public PVQ norm bound of two and mono partition bound of two are unchanged;
private stereo/TF mixed vectors allow squared norm up to three to accommodate
mixing while preserving bounded adaptive Q14 search arithmetic.

Encoding handles one-coordinate signs, forward TF, native stereo angle models,
small-amplitude channel copying, amplitude-weighted intensity, conditional
inversion, N=2 orientation and stronger-basis coding, larger mid/side budgets and
three-bit rebalance reserves. Both entropy directions use shared TF validation,
inverse transforms, fold scaling and collapse reconstruction. Inputs and borrowed
folds stay unchanged, including invalid domains and late capacity failures.

All 22,176 original complete encodings match bytes, range/tell/tell_frac,
remaining budget/seed, reconstructed channels, folding output and masks across
924 band/LM/block/TF/channel configurations. Native and Rust paired decoding
agree. An additional 4,352 native split pairs and 1,024 intensity mixtures check
intermediate fixed-point rounding that later pulse quantization can hide.
All 23 targeted mutations are detected. PCM-derived analysis bands round-trip in
6,720 cases across every input rate, duration, long/short and channel layout;
previous mono partition and band decoder evidence stays unchanged.

```sh
python3 tools/celt_partition_fixtures.py /path/to/opus-v1.6.1 --encode-bands > /tmp/celt-band-encoder-fixtures.txt
python3 tools/celt_partition_fixtures.py /path/to/opus-v1.6.1 --stereo-math > /tmp/celt-stereo-math-fixtures.txt
diff -u tests/celt-band-encoder-fixtures.txt /tmp/celt-band-encoder-fixtures.txt
diff -u tests/celt-stereo-math-fixtures.txt /tmp/celt-stereo-math-fixtures.txt
```

These are spectral encoding components. Full-spectrum allocation/folding,
stereo candidate selection, frame controls, complete audio packets and encoder
quality evaluation remain under development. Original C probes are optional
external evidence; ordinary Rust builds/tests do not compile or link them.


## Complete CELT normalized-spectrum encoding

`opura::celt::partition::encode_spectrum` consumes an existing immutable
`Allocation`, matching `TfResolution`, spread policy, `SpectrumInput` and
`SpectrumEncoding`. Input borrows full-length normalized left/optional-right
spectra with per-band linear amplitudes. Options supply the folding seed,
inversion suppression and stereo theta RDO policy. It returns the existing
`Spectrum` with reconstructed channels, collapse masks and final seed, before
anti-collapse and final energy. No allocation occurs; the complete writer commits
on success only. An allocation/TF encoder and complete frame assembly are still
required for audio encoding.

Each selected band has squared Q24 norm <=5/4; selected amplitudes are nonnegative.
Input lengths/channel presence match the allocation. Inactive coefficients and
amplitudes are ignored; returned inactive bins/masks are zero. Stereo RDO is
eligible only for initially non-dual stereo and bands below intensity. Its adjusted
amplitude plus one must fit positive i32; overflow rejects before encoding.
The native adjustment is one third of the smaller amplitude. Candidate scores use
exact Q15 weighted source/reconstruction correlations; biased down wins ties.
Both trials start from identical entropy, budget, seed, source and fold context;
the winning bytes, channels, fold output, mask and shared state are retained.

One statically dispatched driver now handles decoder and encoder all-band budget
balance, prior-band folding, conservative mask unions, aggressive-spread policy,
Hybrid-like starting-band tail duplication, dual/intensity transitions and
one-bit-per-sample fold-position updates. First-transient-band split-noise
protection is cleared for subsequent bands.

All 3,456 complete original resynthesized packets match full bytes, range and bit
counters, selected-channel reconstruction, masks and seed. They span four sizes,
mono/stereo, starts 0/17, short/long blocks, every spread and 8..1275-byte storage,
including 629 dual-stereo and 751 RDO-eligible frames. Native and Rust paired
decoding agree. Another 4,271 original weights/correlation cases distinguish
intermediate rounding and conservative weighting hidden by final winner selection.
All 21 targeted mutations are detected. Malformed input/amplitude/TF/lifecycle
and inactive-bin tests pass; test-only injected capacity/vector errors after the
last band's work verify whole-writer rollback. A bounded small-capacity search
found no natural late failure; this is not a proof that all valid contexts fit.

```sh
python3 tools/celt_partition_fixtures.py /path/to/opus-v1.6.1 --encode-spectra > /tmp/celt-spectrum-encoder-fixtures.txt
python3 tools/celt_partition_fixtures.py /path/to/opus-v1.6.1 --rdo-math > /tmp/celt-rdo-math-fixtures.txt
diff -u tests/celt-spectrum-encoder-fixtures.txt /tmp/celt-spectrum-encoder-fixtures.txt
diff -u tests/celt-rdo-math-fixtures.txt /tmp/celt-rdo-math-fixtures.txt
```

The optional native oracle enables RESYNTH and observes prefix symbols with a
nested-call guard to construct an identical canonical writer context. Its
unchanged original quant_all_bands then performs encoding and paired decoding.
Native decoder scratch outside selected bins is excluded from signal comparison;
Rust returns zeros there. These external probes remain outside Cargo.


## CELT TF and spreading control encoding

`opura::celt::control::encode_tf` takes immutable binary band decisions and a
select policy, then returns the effective `TfResolution` for spectral coding.
It writes native differential flags, reserves/selects the alternate mapping only
when relevant and retains the current state for decisions that cannot fit.
Intervals require `start<=end<=21`; empty intervals retain native reservation
semantics and return zero resolutions. LM0 cannot be transient. The API stages
the full writer, leaving it and the input unchanged on every error.

`control::encode_spread` writes the existing spread ICDF when four whole physical
bits remain, otherwise returning Normal without writing. Both APIs require
2..1275-byte capacity, an in-budget cursor and an unfinished writer, including
empty/no-symbol cases. These functions encode caller decisions; selecting
transients, TF analysis and spread policy remain separate encoder analysis work.
Energy coarse coding owns the intra flag, so a future prefix writer must stop
before that flag instead of duplicating it.

All 34,944 independent original encoded control cases match complete bytes,
range/whole/eighth-bit accounting and effective TF/spread values. Native and Rust
paired decoding agree across sizes, intervals, binary patterns, both select
policies, raw/range prefixes, scarce budgets and 2..1275-byte capacities, including
8,736 empty intervals and 15,345 spreading fallbacks. Sixteen targeted mutations
are detected. A statically injected symbol writer verifies complete rollback at
the first, middle and final select writes; production uses the native range-writer
method directly through static dispatch. Existing control decoder evidence stays
unchanged.

```sh
python3 tools/celt_control_fixtures.py /path/to/opus-v1.6.1 --encode > /tmp/celt-control-encoder-fixtures.txt
diff -u tests/celt-control-encoder-fixtures.txt /tmp/celt-control-encoder-fixtures.txt
```

Original function probes remain optional external evidence, outside Cargo.
Allocation encoding and full audio packet assembly are still under development.


## CELT dynamic and fine bit-allocation encoding

`opura::celt::allocation::encode_allocation` writes dynamic boosts, trim,
retained-band and stereo decisions, returning the existing immutable `Allocation`
for energy and spectrum stages. `AllocationEncoding` supplies positive-boost flag
counts (u8 per band, clipped by native physical/cap limits), trim 0..10,
intensity/dual requests, previous retained-band count 0..21 and inclusive desired
bandwidth index 0..20. Stereo intensity is in selected start..=end and is clipped
to retained coded bands; mono ignores it but requires at most 21. Inactive boost
counts are ignored. The selected layout is nonempty, capacity is 2..1275 bytes
and the unfinished cursor must fit. Every error preserves the complete writer.

A statically dispatched private policy adapter lets both directions use one
cap/interpolation/threshold/skip/fine/PVQ/priority/balance implementation. Encoding
adds native boost-count signalling, scarce trim fallback, anti-collapse reserve,
7/9-depth retained-band hysteresis and intensity/dual signals. Requested policy
selection remains separate from perceptual audio analysis. No allocation occurs.

All 26,480 original complete packets match bytes, accounting and every effective
allocation field across all 3,696 size/channel/interval/transient combinations.
Native and Rust paired decoding agree. The set includes 1,101 dual, 5,318
anti-reserved, 6,579 boosted and 17,829 partially skipped allocations. Dense
unboosted budgets and exact trim-reservation boundaries retain cases missing from
the initial set, exposed by four mutations. All twenty targeted mutations are
now detected, as are malformed/lifecycle inputs. Static symbol-failure injection
checks complete rollback at boosts, intermediate decisions and the last dual bit.
Earlier decoder allocation fixtures reproduce unchanged.

A further 160 test frames compose signed16 PCM, pre-emphasis/overlap, analysis,
prefix/energy, TF/spread, allocation, shapes and final energy into Opus packets.
The public decoder matches component reconstruction PCM and final ranges across
all five input rates, four durations and both channel layouts. Packet histories
start with seed zero and use final range for the next frame; the local folding
seed returned by spectrum encoding is distinct. This is test-level assembly,
not a public audio encoder or an independent full-encoder quality measurement.

```sh
python3 tools/celt_allocation_fixtures.py /path/to/opus-v1.6.1 --encode > /tmp/celt-allocation-encoder-fixtures.txt
diff -u tests/celt-allocation-encoder-fixtures.txt /tmp/celt-allocation-encoder-fixtures.txt
```

Original dynamic/trim/rate probes remain optional and external to Cargo. Audio
packet API, signal-driven policies, SILK/Hybrid encoding and quality comparisons
remain under development.


## Stateful CELT PCM packet encoder

`opura::celt::encoder::CeltEncoder` is a safe, zero-dependency, fixed-storage
PCM-to-packet API. Construct it with `InputRate` (8/12/16/24/48 kHz) and one/two
channels, then encode one interleaved signed16 frame at 2.5/5/10/20 ms. Input
rate and PCM layout stay fixed; frame duration, encoded layout and payload
budget may change. `set_encoded_channels` selects mono/stereo wire channels
bounded by the input layout while retaining both input histories.
`reset()` clears every history while retaining geometry and settings.

```rust
use opura::celt::{FrameSize, encoder::CeltEncoder, input::InputRate};

let mut encoder = CeltEncoder::new(InputRate::Khz48, 1)?;
let packet = encoder.encode(FrameSize::Ms20, &[0; 960], 128)?;
assert_eq!(packet.as_bytes().len(), 129);
# Ok::<(), Box<dyn std::error::Error>>(())
```

Input length is exactly `size.samples()/rate.factor()*channels`. Payload budget
is 2..1275 bytes, excluding the one-byte TOC; successful packets are budget+1
bytes. Small budgets can fail explicitly. `EncodedPacket` exposes complete bytes,
frame size, channels and final range. No container/transport framing is added.
All errors preserve the entire encoder state and immutable caller PCM, including
late entropy finalization. Normal operation allocates no heap memory.

The initial reproducible policy uses long MDCT windows, disabled pitch comb and
silence suppression, standard TF/Normal spread, zero boosts, trim five,
full selected intensity/no dual stereo, two-pass energy and stereo theta RDO.
It includes native stable-energy quarter-error feedback. Pre-emphasis and
120-sample overlap persist across durations, retained-band history moves at most
one band per frame, and the next folding seed is the previous packet final range.
8 kHz input selects NB, 12/16 kHz WB, 24 kHz SWB and 48 kHz FB. SILK/Hybrid,
pitch prefilter analysis, DTX/VBR/FEC encoding, containers and quality
optimization remain work. Optional TF, spreading and allocation policies are
described below. This baseline is a usable CELT encoder, not a claim of
reference-adaptive quality or SOTA performance.

All 720 independently generated original-component packets match full bytes,
energy/residual/pre-emphasis/overlap/coded-band histories and bit accounting.
They include signal extremes, tiny/impulse/noise/periodic/stable inputs,
changing durations/budgets and resets. Unchanged public libopus verifies PCM,
final ranges and decoder energy histories in 7,200 rate/channel decodes; no
native encoder/decoder energy-history differences occurred in this set. Original
two-pass residual scratch outside selected bands is uninitialized/unused, so
optional observations canonicalize only those entries. Another 2,560 public
budget/rate/channel/duration cases all encode/decode successfully. Invalid/reset,
pre/post-finalization rollback and 22 targeted mutations are verified.

```sh
python3 tools/celt_encoder_fixtures.py /path/to/opus-v1.6.1 > /tmp/celt-encoder-fixtures.txt
diff -u tests/celt-encoder-fixtures.txt /tmp/celt-encoder-fixtures.txt
```

The optional oracle uses unchanged original components under the same explicit
baseline decisions and the original public decoder. Its full external native
build is never a Cargo dependency. This comparison validates implementation and
packet interoperability; comparison against the reference's adaptive encoder
requires separate quality/performance measurements and representative audio.


## CELT tone and transient analysis

`celt::analysis::analyze_transients` accepts immutable planar pre-emphasized
context before pitch filtering: one/two channels, exactly
`channels*(size.samples()+120)` coefficients, each magnitude at most 2^28.
It uses native tone LPC/frequency estimation and forward/backward temporal
masking, returning strong/weak decisions, Q14 TF estimate, selected channel,
Q13 tone frequency, Q29 pole radius and mask metric. The optional weak policy
suppresses moderate attacks. LM0 diagnostics are supported independently of
packet coding, which always uses long windows at 2.5 ms. Invalid shape or
magnitude returns a typed error. Analysis uses fixed storage and preserves input.

`CeltEncoder::set_transient_detection(true)` enables this analysis when a short
window can be signalled. It selects short MDCT blocks, standard transient TF
fallback, transient allocation and the native anti-collapse bit based on the
previous short/disabled-window streak. `transient_detection()` reports the
setting; reset retains it and clears histories. The default remains the baseline
all-long policy, preserving existing packet bytes. Adaptive spread/dynamic/pitch
analysis and the reference's missed-transient spectral patch remain future work.

All 1,788 original analysis contexts match both weak policies (3,576 results),
including exact 200/600 mask thresholds, odd stereo rounding and a seeded paired
energy-rounding regression. Another 65,537 original acos inputs and 4,290 wide-Q29
ratios verify the scalar helpers. Wide division is private; existing public
fractional-division domains remain unchanged. Canonical constants use the original
C floating-literal macro results, which can differ from real-number rounding.

A separate original-component sequence produces 960 complete packets, including
611 short-window frames, changing durations/budgets, consecutive attacks and
resets. Full bytes, histories, streaks and accounting match. Unchanged public
libopus PCM/final-range/energy histories match all 9,600 output-rate/channel
cases, with no native energy-history differences in this corpus. The old 720
baseline packets remain unchanged. Invalid input, toggles, reset and injected
pre/post-finalization errors retain atomic stream behavior. All 31 targeted
mutations are detected. These checks establish compatibility, not encoder quality
or speed superiority.

```sh
python3 tools/celt_encoder_fixtures.py /path/to/opus-v1.6.1 --analysis > /tmp/celt-transient-analysis-fixtures.txt
diff -u tests/celt-transient-analysis-fixtures.txt /tmp/celt-transient-analysis-fixtures.txt
python3 tools/celt_encoder_fixtures.py /path/to/opus-v1.6.1 --analysis-math > /tmp/celt-transient-math-fixtures.txt
diff -u tests/celt-transient-math-fixtures.txt /tmp/celt-transient-math-fixtures.txt
python3 tools/celt_encoder_fixtures.py /path/to/opus-v1.6.1 --analysis-tables > /tmp/transient_tables.rs
rustfmt --edition 2024 /tmp/transient_tables.rs
diff -u src/celt/analysis/transient_tables.rs /tmp/transient_tables.rs
python3 tools/celt_encoder_fixtures.py /path/to/opus-v1.6.1 --transients > /tmp/celt-encoder-transient-fixtures.txt
diff -u tests/celt-encoder-transient-fixtures.txt /tmp/celt-encoder-transient-fixtures.txt
```

The optional diagnostic observer copies the original transient routine only to
read the internal mask metric, and cross-checks ordinary outputs against the
unchanged original routine on every context. Original native code remains an
external validation tool and is never a build/test dependency.


## CELT time/frequency resolution analysis

`celt::analysis::analyze_tf` accepts one selected normalized Q24 channel of exactly
`size.samples()` coefficients and an exclusive band end in 1..21. Each active
band must have squared norm at most 5/4 in Q48; zero bands are accepted, and
inactive bins are ignored. `TfAnalysisOptions` supplies long/short context,
Q14 temporal estimate in 0..16384, a `u16` switching penalty and 21 `u8` importance
weights. LM0 cannot be marked transient. Default controls use long input,
zero estimate, penalty 80 and neutral weights 13. Invalid geometry, controls and
active norms return typed errors, without input mutation or partial results.

The result exposes binary TF decisions, an alternate mapping flag, analyzed end
and Q1 best-level metrics. Existing native Haar stages search frequency/time
resolution using biased L1 costs, including the transient extra level and
narrow-band midpoint correction. Weighted Viterbi search preserves original
predecessor/final ties; alternate mapping is chosen only for transients with
strictly lower cost. Storage is fixed and analysis allocates no heap memory.

`CeltEncoder::set_tf_analysis(true)` enables packet integration independently of
transient detection; `tf_analysis()` reports the setting and reset retains it.
The native gate requires at least 15 payload bytes/channel and excludes strong
tones after temporal-estimate confidence adjustment. Penalty is
`max(80,20480/payload_bytes+2)`; the detector selects the analysis channel.
Importance is neutral 13 unless perceptual dynamic-allocation analysis is enabled. Both settings default false, preserving baseline packet bytes;
transient-only packets also retain their previous bytes.

All 672 original normalized contexts match 7,056 long/short/penalty/weight
analyses, including 221 alternate selections, zero-weight path ties and inactive
extreme tails. The scalar oracle covers all 16,385 temporal estimates and 2,400
L1 results across 96 vectors. Another 960 complete original-component TF-policy
packets match full bytes/history/accounting, native enable gates and effective
TF resolutions. Analysis is enabled in 770 frames, including 176 alternate
mappings. Public original libopus verifies all 9,600 PCM/range/energy-history
decodes; no native history differences occur in this corpus. Exact 15/30-byte
enable thresholds, 144 additional TF budget frames, setting independence/reset
and all four setting combinations under injected pre/post-finalization errors
are checked. All 31 non-equivalent targeted mutations are detected. These are
implementation/interoperability checks, not quality or performance comparisons against the adaptive reference encoder.

```sh
python3 tools/celt_encoder_fixtures.py /path/to/opus-v1.6.1 --tf-analysis > /tmp/celt-tf-analysis-fixtures.txt
diff -u tests/celt-tf-analysis-fixtures.txt /tmp/celt-tf-analysis-fixtures.txt
python3 tools/celt_encoder_fixtures.py /path/to/opus-v1.6.1 --tf-math > /tmp/celt-tf-math-fixtures.txt
diff -u tests/celt-tf-math-fixtures.txt /tmp/celt-tf-math-fixtures.txt
python3 tools/celt_encoder_fixtures.py /path/to/opus-v1.6.1 --tf-packets > /tmp/celt-encoder-tf-fixtures.txt
diff -u tests/celt-encoder-tf-fixtures.txt /tmp/celt-encoder-tf-fixtures.txt
```

A separate original TF observer reads only internal metrics and cross-checks
ordinary decisions/select against unchanged original analysis for every context.
All native code remains optional external validation, with no Cargo dependencies.


## CELT perceptual dynamic allocation

`celt::analysis::analyze_dynalloc` analyzes immutable current, comparison and
previous Q24 band logs. `DynallocInput` borrows three `[[i32;21];2]` arrays;
`BandLayout` selects one/two channels and a nonempty interval with end>=3 for
median windows. Every active channel's logs in bins0..end must be in [-32,32]
log units, including bins preceding the selected start. Inactive channels/tails
are ignored. Options bound payload to 2..1275 bytes, precision to 8..24 bits,
selected surround boosts to 0..4 Q24, tone confidence to 0..1 Q29 and Q13 tone
frequency to -1..25736. LM0 cannot be transient. Invalid controls/logs return
errors; inputs remain unchanged and all output/scratch uses fixed storage.

Native analysis combines the noise floor, forward/backward masking, median
followers, stereo cross-talk, surround requests and tone compensation. It returns
raw requests, writer-ready counts, TF importance, spreading weights, maximum
signal depth and intended eighth-bit boost. Importance precedes CBR halving and
frequency-dependent boost weighting. CBR and constrained non-transient VBR use
the original two-thirds budget cap; low budgets and LFE retain neutral importance.

Raw cap-hit requests can be negative because the native comparison first rounds
the cumulative boost down to whole bytes. Negative requests emit no positive
flags. `boosts()` clamps raw requests to 0..255 for the existing allocation API;
`requested_boosts()` preserves exact native diagnostics. Original caps match Rust
across all 168 standard band/channel/duration cells and admit at most 66 positive
symbols, proving larger counts are indistinguishable at the writer. Raw requests
and intended totals are distinct from effective entropy-limited allocation.

`CeltEncoder::set_dynamic_allocation(true)` independently enables perceptual
analysis; `dynamic_allocation()` reports it. Reset retains this setting and clears
histories. Packet policy uses signed16 precision, CBR, no LFE/surround adjustment,
and adjusted tone confidence. For transient frames, a second long MDCT supplies
comparison energies with native LM/2 scaling. Analysis uses previous quantized
energy before current coarse quantization and stable-energy feedback. Boost
counts feed allocation; TF, when enabled, consumes perceptual importance.
Spreading defaults to Normal with a separate adaptive setting; trim remains five.
All settings default disabled,
preserving the three previous baseline/transient/TF packet corpora.

The independent original corpus matches 21,889 analyses over 2,737 contexts:
masking, depth, requests, importance and spreading weights, including exact
budget/tonal/rising-band boundaries and a retained second-channel follower index.
Extreme inactive slots verify prefix-only inspection. Native scalar checks cover
24,579 importance-rounding inputs, 270 median cases and canonical constants.
All 40 targeted mutations are detected, including cap units/clipping, median/masking
boundaries, old-energy ordering, long-window scaling and perceptual TF weights.
Another 1,024 bounded random contexts and typed invalid/domain endpoints are
checked. The adapter retains native negative and large cap-hit requests as raw
observations while supplying wire-equivalent counts.

All 960 complete original-component dynamic-policy packets match bytes, complete
histories, TF diagnostics, perceptual analysis and accounting; 242 frames request
positive boosts. Unchanged public libopus agrees in 9,600 PCM/range/energy-history
decodes, with no native history differences in this corpus. Another 512 frames
exercise every dynamic threshold and independent TF/window choices. Reset,
toggles, immutable input and pre/post-finalization failure rollback are verified
for all eight setting combinations. These results establish implementation and
packet compatibility; quality and speed comparisons remain separate work.

```sh
python3 tools/celt_encoder_fixtures.py /path/to/opus-v1.6.1 --dynalloc-analysis > /tmp/celt-dynalloc-analysis-fixtures.txt
diff -u tests/celt-dynalloc-analysis-fixtures.txt /tmp/celt-dynalloc-analysis-fixtures.txt
python3 tools/celt_encoder_fixtures.py /path/to/opus-v1.6.1 --dynalloc-math > /tmp/celt-dynalloc-math-fixtures.txt
diff -u tests/celt-dynalloc-math-fixtures.txt /tmp/celt-dynalloc-math-fixtures.txt
python3 tools/celt_encoder_fixtures.py /path/to/opus-v1.6.1 --dynalloc-tables > /tmp/dynalloc_tables.rs
rustfmt --edition 2024 /tmp/dynalloc_tables.rs
diff -u src/celt/analysis/dynalloc_tables.rs /tmp/dynalloc_tables.rs
python3 tools/celt_encoder_fixtures.py /path/to/opus-v1.6.1 --dynalloc-packets > /tmp/celt-encoder-dynalloc-fixtures.txt
diff -u tests/celt-encoder-dynalloc-fixtures.txt /tmp/celt-encoder-dynalloc-fixtures.txt
```

External original analysis and full packet writing consume raw native requests;
the reference checkout stays unchanged. No native code enters Cargo builds/tests.
Pitch prefilter analysis, complete SILK/Hybrid encoding and representative
quality/performance evaluation remain in development.


## CELT pulse spreading analysis

`celt::analysis::SpreadingState` maintains native sparsity/HF averages, pitch
tapset suggestion and last effective spreading decision. Defaults match original
encoder initialization (tonal 256, HF 0, tapset 0, Normal). `from_history` accepts
checked tonal 0..768, HF 0..64 and tapset 0..2 histories; `reset` restores defaults.
`SpreadingOptions` supplies one/two channels, end 1..21, active weights 0..32 and
optional HF update. `process` requires exactly `channels*size.samples()` planar
Q24 coefficients, each active band's squared norm at most 5/4 Q48. Inactive bins
and weights are ignored. Invalid shapes, weights, histories or norms leave the
whole state/input unchanged. No heap allocation occurs.

The native CDF of truncated Q14 magnitudes drives per-band sparsity, masking
weights, recursive averaging and strict hysteresis choices. A final band with
at most 8 coefficients returns None while retaining numeric histories. Otherwise
at least one eligible band must have positive weight. Optional HF/tapset updates
preserve the original upper-band selection and divisor, including HF decay when
there are no high-frequency counts. The result and state decision always agree.
`BandAnalysis::normalized_values()` exposes the complete immutable planar output
for direct composition with this and subsequent frame analysis.

`CeltEncoder::set_spreading_analysis(true)` enables packet selection independently
of transient, TF and dynamic allocation; `spreading_analysis()` reports it and
reset retains it. The native gate requires a long window, at least 10 payload
bytes/channel and four remaining whole entropy bits. Fallback writes Normal and
retains numeric histories while recording the actual effective decision. Mask
weights are computed even with dynamic allocation disabled; that setting still
gates boost requests and perceptual TF importance. Pitch is currently disabled,
so PCM encoding retains HF/tapset histories without updating them.

The original corpus matches 41,486 analyses over 2,019 normalized contexts,
including checked native constructor observations, every tonal history value in 0..768
under four previous modes, exact CDF/half-count and hysteresis boundaries,
explicit HF 22/18 thresholds, early-None and inactive extreme tails. All four spread
modes and all three tapsets occur. Invalid histories/weights/geometry and late
second-channel/exact norm errors preserve state and input.

Another 960 original-component packets match bytes, numeric/effective spreading
histories, energy/range/accounting, used perceptual/TF diagnostics and 9,600 public
native PCM decodes. Four independent analysis policies and within-stream spread
off/on toggles verify that spreading alone does not enable boosts or change TF
importance. Exact 10/20-byte gates, 768 additional independent-setting frames,
reset and all 16 setting combinations under injected finalization failures are
checked. Previous four packet corpora remain unchanged. This evidence establishes
compatibility, not quality or performance superiority.

Another 72 isolated entropy-tail cases cover 0..8 remaining bits, using real raw
prefixes and valid normalized spectra. Full writer bytes/ranges/accounting and
stream history match the original. These cases verify that analysis does not
advance numeric history when only three bits remain and signalling falls back to
Normal. A bounded search of 66,640 physical PCM contexts did not reach this
boundary; the retained regression checks stage composition directly. All 40
targeted mutations are detected.


```sh
python3 tools/celt_encoder_fixtures.py /path/to/opus-v1.6.1 --spreading-analysis > /tmp/celt-spreading-analysis-fixtures.txt
diff -u tests/celt-spreading-analysis-fixtures.txt /tmp/celt-spreading-analysis-fixtures.txt
python3 tools/celt_encoder_fixtures.py /path/to/opus-v1.6.1 --spreading-packets > /tmp/celt-encoder-spreading-fixtures.txt
diff -u tests/celt-encoder-spreading-fixtures.txt /tmp/celt-encoder-spreading-fixtures.txt
python3 tools/celt_encoder_fixtures.py /path/to/opus-v1.6.1 --spreading-tails > /tmp/celt-spreading-tail-fixtures.txt
diff -u tests/celt-spreading-tail-fixtures.txt /tmp/celt-spreading-tail-fixtures.txt
```


## CELT trim and stereo allocation analysis

The allocation policy components support standalone analysis and optional packet
integration. `CeltEncoder` defaults to trim five, full-band intensity and no dual
stereo; enable allocation analysis to select these controls from the signal.

`analysis::equivalent_rate` reproduces the byte-derived equivalent bitrate and
short-frame overhead for one/two channels and 2..1275 payload bytes. It applies
no outer bitrate cap and can return negative low-budget rates. `analyze_stereo`
accepts exactly two planar normalized Q24 channels, inspects the first 13 bands
with per-band squared norm <=5/4 Q48, and returns the native L1 preference for
independent L/R over M/S coding. Later bins are ignored. LM0 diagnostics are
supported independently of the eventual packet policy, which forces M/S there.

`IntensityState` restores checked boundaries in 0..21, starts/resets at zero,
and selects from an `i16` equivalent kb/s value with native threshold/hysteresis
tables. The selected boundary is clamped to the validated `BandLayout` interval,
including empty intervals. This desired state precedes allocation's retained-band
clipping; it must not be replaced by that effective clipped boundary.

`TrimState` starts/resets saving at zero and restores checked Q8 saving in
0..1280. `process` takes immutable normalized spectra, Q24 logs and `TrimOptions`,
returning index 0..10, raw Q8 trim and Q19 averaged spectral tilt. Input length is
exactly channels*frame samples. Mono supports end 2..21 and ignores coefficient
values; stereo supports end 8..21 and inspects only max(8,intensity) normalized
bands, each norm <=5/4. Intensity is 0..end, TF estimate 0..16384 Q14, surround tilt
[-4,4] Q24, and equivalent bitrate may span `i32`. Logs inspect active channels
and `0..end-1` only in [-32,32] Q24; the last band and other tails are ignored.
The mathematical final weighted tilt sum after input>>5 must fit signed32;
otherwise analysis rejects without committing any saving or changing inputs.

Native correlation sums full Q24 products before the final shift. Negative LF
averaging truncates before taking absolute values; the half-log is evaluated
before Q20 compensation and signed Q8 rounding. Saving has the native quarter
unit slew, stereo minimum-correlation cap and mono retention. Rate interpolation,
spectral tilt, temporal/surround adjustments and final trim rounding preserve
original order. All storage is fixed and no heap allocation occurs. Call trim
analysis only when its symbol is available: packet integration uses the actual
post-dynamic budget position, preserving saving on skipped fields.

Original evidence matches 525,825 correlation-log pairs (target saving 0..1022),
all 10,192 byte-equivalent rates and 1,441,792 `i16` rate/history intensity pairs.
Another 2,016 stereo spectra match both native costs and decisions, including
313 L/R selections and 1,342 ties. Trim matches 16,233 outputs across 1,090 contexts,
all 11 indices, a 40-step saving sequence and explicit negative LF averaging.
Another 1,216 contexts intentionally exceed the checked tilt domain and reject
atomically; original wrapped outputs are not adopted outside that public domain.
Invalid shapes/history/control fields, exact norm boundaries, unused prefixes,
reset and 1,024 bounded random contexts are verified. All 44 targeted mutations
are detected. These are component analysis
checks; complete packet quality and speed comparisons remain later work.

```sh
python3 tools/celt_encoder_fixtures.py /path/to/opus-v1.6.1 --allocation-analysis-tables > /tmp/allocation_analysis_tables.rs
rustfmt --edition 2024 /tmp/allocation_analysis_tables.rs
diff -u src/celt/analysis/allocation_tables.rs /tmp/allocation_analysis_tables.rs
python3 tools/celt_encoder_fixtures.py /path/to/opus-v1.6.1 --allocation-analysis-math > /tmp/celt-allocation-analysis-math-fixtures.txt
diff -u tests/celt-allocation-analysis-math-fixtures.txt /tmp/celt-allocation-analysis-math-fixtures.txt
python3 tools/celt_encoder_fixtures.py /path/to/opus-v1.6.1 --stereo-analysis > /tmp/celt-stereo-analysis-fixtures.txt
diff -u tests/celt-stereo-analysis-fixtures.txt /tmp/celt-stereo-analysis-fixtures.txt
python3 tools/celt_encoder_fixtures.py /path/to/opus-v1.6.1 --trim-analysis > /tmp/celt-trim-analysis-fixtures.txt
diff -u tests/celt-trim-analysis-fixtures.txt /tmp/celt-trim-analysis-fixtures.txt
```

Optional read-only model/trim observers cross-check every ordinary result and
saving against unchanged original helpers before emitting internal diagnostics.
Native code remains external validation and is never a Cargo dependency.


## CELT adaptive allocation packets

`CeltEncoder::set_allocation_analysis(true)` independently enables native trim,
intensity hysteresis and L1 stereo selection. It applies byte-equivalent bitrate
with short-frame overhead, forces M/S for 2.5 ms frames and preserves desired
intensity before allocation clips it to retained coded bands. Trim consumes the
stable-energy-feedback-adjusted target logs and native temporal estimate. The
existing allocation engine invokes its private static trim callback only when
six bits fit after dynamic boost reservation. Skipped fields keep default trim
five and preserve saving. No extra allocation pass or heap storage is needed.

Reset retains all five analysis settings and clears intensity/saving histories.
Turning allocation analysis off keeps its histories for later re-enabling;
other settings remain independent. The existing full-stream transaction also
covers late allocation/entropy failures. The public fixed-policy allocation
writer retains its existing contract; lazy callback errors preserve its writer.

All 1,440 original component-assembled packets match complete bytes, persistent
state, effective trim/intensity/stereo and bit accounting. Six base policies
(only allocation; plus transient; plus TF; plus dynamic; plus spreading; all five)
and within-stream off/on switches span every rate, channel count and duration.
A one-active-channel regression explicitly checks the LM0 M/S restriction.
The corpus exercises eight trim values, 53 requested/48 effective dual-stereo
frames and desired/effective intensity differences. Unchanged public libopus
matches PCM/range/history in 14,400 output-geometry decodes, with no native
active-energy history differences.

Another 1,056 isolated entropy-tail cases match all allocation budgets, controls,
bytes, ranges and saving across dynamic requests, initial saving 0/704 and
0..32 remaining bits. These prefixes test resource boundaries separately from
complete PCM packets. Another 4,096 PCM frames cover all 32 setting combinations,
small budgets, NB/FB, reset and invalid-input rollback; injected pre/post-finalize
failures preserve the full stream in all 32 combinations. Earlier packet and
analysis corpora remain unchanged. All 36 targeted mutations are detected,
including eager/duplicate trim updates and the LM0 regression. All 21 optional
evidence modes reproduce and all seven offline gates pass with zero dependencies.
Pitch prefilter, SILK/Hybrid encoding,
DTX/VBR/FEC and representative quality/performance comparisons remain work.

```sh
python3 tools/celt_encoder_fixtures.py /path/to/opus-v1.6.1 --allocation-packets > /tmp/celt-encoder-allocation-fixtures.txt
diff -u tests/celt-encoder-allocation-fixtures.txt /tmp/celt-encoder-allocation-fixtures.txt
python3 tools/celt_encoder_fixtures.py /path/to/opus-v1.6.1 --allocation-tails > /tmp/celt-allocation-tail-fixtures.txt
diff -u tests/celt-allocation-tail-fixtures.txt /tmp/celt-allocation-tail-fixtures.txt
```


## CELT encoder pitch search

`pitch::encoder_pitch(size, left, optional_right, previous)` analyzes exactly
1024 raw pre-emphasized history samples followed by one 2.5/5/10/20 ms frame
per channel. Inputs are immutable Q12 values in +/-536870911. `PitchHistory`
checks previous periods 15..1022 and gain 0..24576 Q15; initial period zero
requires gain zero. It exposes read-only period/gain and defaults to zero.
The immutable fixed-storage `EncoderPitch` exposes coarse and refined periods,
raw Q15 gain and all active half-rate whitened samples. Refined periods are
15..1023 before the outer prefilter clamp to 1022.

PLC and encoder now share private factor-two lowpass/LPC4/FIR5 whitening and
coarse/fine search with bounded geometry/scratch storage. Encoder search retains
the original odd maximum lag 979, offset 512 and frame-dependent scaling.
Refinement follows native normalized reciprocal-square-root gain, exponent
parity, rolling energies, subharmonic checks/second correlations, continuity
bias, strict gain selection and pseudo-interpolation. The original later
high-pitch threshold branch is unreachable after its earlier broader condition;
the port preserves the effective order. Signed32 final inner products, rolling
energies and normalization domains are checked; unsupported accumulations return
an error with no input/history mutation. No heap allocation or unsafe is used.

All 11,972 original contexts match coarse/refined period, gain and fingerprints
of every active whitened sample. They cover all initial periods 15..1023,
mono/stereo, every duration, signal extremes and all eight signal recipes,
nearby previous periods/gains and eight 40-step continuity sequences. Refinement
changes 10,407 coarse periods. All 82,025 original gain scalars match across
zero, exponent/normalization boundaries, signed correlations and saturation.
Another 203,729 isolated original refinement cases cover bounded whitened
mixtures/double pulses, last-lag lookup and continuity boundaries. A retained
seed 8382 regression detects a one-unit change of the Q15 gain threshold:
original period 120 becomes 239 in the faulty variant. Original PLC evidence
remains unchanged. Invalid geometry/signal/history,
exact input endpoints, checked accumulator failures and 1,024 bounded random
contexts are verified. All 42 non-equivalent targeted mutations are detected;
all four optional pitch evidence modes reproduce and all seven offline gates
pass with zero dependencies. These are standalone component results. The prefilter stage below adds tone
shortcuts, thresholds/quantization/cancellation and separate-source comb filtering;
optional packet integration is described below.

```sh
python3 tools/celt_pitch_fixtures.py /path/to/opus-v1.6.1 --encoder > /tmp/celt-encoder-pitch-fixtures.txt
diff -u tests/celt-encoder-pitch-fixtures.txt /tmp/celt-encoder-pitch-fixtures.txt
python3 tools/celt_pitch_fixtures.py /path/to/opus-v1.6.1 --encoder-math > /tmp/celt-encoder-pitch-math-fixtures.txt
diff -u tests/celt-encoder-pitch-math-fixtures.txt /tmp/celt-encoder-pitch-math-fixtures.txt
python3 tools/celt_pitch_fixtures.py /path/to/opus-v1.6.1 --refinement > /tmp/celt-pitch-refinement-fixtures.txt
diff -u tests/celt-pitch-refinement-fixtures.txt /tmp/celt-pitch-refinement-fixtures.txt
```


## CELT encoder prefilter

`prefilter::PrefilterState::new(channels)` owns separate raw 1,024-sample pitch
histories and filtered 120-sample overlaps for one/two channels. `process` takes
one immutable planar Q12 frame (120/240/480/960 samples/channel in +/-536870911)
and checked `PrefilterOptions`, returning a fixed-storage `PrefilteredFrame`
with channels*(N+120) filtered MDCT context values. Read-only getters expose
geometry, active flag, gain index, internal pitch/gain/tap and both histories.
Reset clears histories/parameters and retains channel count. Every error,
including a constituent pitch failure, preserves all stream state and input.

The caller owns packet resource gating. Options accept enabled, complexity0..10,
available bytes0..1275, TF estimate0..16384 Q14, tone frequency-1..25736 Q13,
adjusted tone radius0..2^29 Q29, loss percentage0..100 and a standard tapset.
Fields are validated even on inactive branches. Native strong-tone period
alias/doubling bypasses ordinary search regardless of low complexity; ordinary
search requires complexity>=5 and applies native 0.7/loss attenuation. Continuity,
TF0.98, byte25/35 and previous-gain adjustments precede native snapping and
three-bit gain quantization. A zero transmitted gain retains native internal
period/tap history for the next frame.

Separate-source negative comb filtering reads raw lag samples and never feeds
filtered output back into its source. It preserves original coefficient/window
rounding, 120-sample crossfade, constant-filter shortcut, biases and saturation.
Before/after Q12 L1 checks preserve the original signed16 stereo threshold
narrowing. Cancellation restores current raw samples and recomputes only the
old-to-zero overlap. Only immutable tap/window data is shared with the decoder
feedback filter. No heap allocation, unsafe or Cargo dependency is used.

All 2,048 unchanged-original contexts match every filtered sample, parameter,
raw and filtered history; 515 frames retain a nonzero new gain. Eight signal/
control families exercise all durations/channels, mixtures, tone/complexity,
loss, exact TF/byte gates, extremes, within-stream disables and reset. Another
1,024 bounded contexts, invalid/endpoint/reset checks and injected pitch failure
pass. All 62 targeted mutations are detected. All 22 optional encoder evidence
modes reproduce and all seven offline gates pass with zero dependencies.
`CeltEncoder` defaults to disabled comb; the optional packet integration below
enables it. These component results establish no encoder quality or speed lead.

```sh
python3 tools/celt_encoder_fixtures.py /path/to/opus-v1.6.1 --prefilter > /tmp/celt-prefilter-fixtures.txt
diff -u tests/celt-prefilter-fixtures.txt /tmp/celt-prefilter-fixtures.txt
```


## CELT pitch-filtered packets

`CeltEncoder::set_pitch_prefilter(true)` independently enables native pitch
selection and comb signaling. Raw pre-emphasized overlap feeds tone/transient
analysis before filtering. The filtered MDCT context feeds both normal/short
transforms and transient long-window allocation comparison. Native payload
threshold >12*channels and cached-prefix resources gate new gain; transient
availability is checked after comb fields. The private fresh-prefix writer
signals native period+1/octave, quantized gain and tapset. It rejects positive
period1023/nonquantized gain even though the general DSP parameter type permits
these values. Default packet behavior remains unchanged.

Raw1024 and filtered120 histories advance on every frame. Disabling ramps the
old gain out; setters preserve history. Reset retains all six settings and clears
histories. Spreading HF/tap estimation runs only with actual active long filtering
and its existing independent setting/resource guards; the new tap affects the
next frame. Full-frame staging includes all new state and late errors.

All 96,828 original comb-field cases match complete bytes, ranges and counters,
covering every positive period15..1022, eight gains and three tapsets over four
budgets, plus disabled/cached-budget cases and paired decoding. Another 1,920
complete original packets match state, prefilter history fingerprints and
19,200 unchanged public native PCM/range/history decodes. Eight policies and
within-stream pitch toggles exercise 382 active frames, 53 active short frames,
21/37 tap1/tap2 requests and HF average up to41, with zero native active-energy
history differences. Periodic LF+HF pilots retain real filtering while testing
HF/tap propagation.

Another 8,192 packets cover all 64 setting combinations, NB/FB, small budgets,
reset and invalid-input contracts. Pre/post-finalize injection preserves full
state for all 64 combinations, including positive gain histories. All 16 focused
integration mutations are detected; the existing prefilter/pitch component
campaigns remain applicable. All 24 optional encoder modes reproduce and all
seven offline gates pass with zero dependencies. SILK/Hybrid encoding, audio
tooling and representative quality/performance comparisons remain work.

```sh
python3 tools/celt_encoder_fixtures.py /path/to/opus-v1.6.1 --comb-fields > /tmp/celt-comb-encoder-fixtures.txt
diff -u tests/celt-comb-encoder-fixtures.txt /tmp/celt-comb-encoder-fixtures.txt
python3 tools/celt_encoder_fixtures.py /path/to/opus-v1.6.1 --pitch-packets > /tmp/celt-encoder-pitch-packets-fixtures.txt
diff -u tests/celt-encoder-pitch-packets-fixtures.txt /tmp/celt-encoder-pitch-packets-fixtures.txt
```
