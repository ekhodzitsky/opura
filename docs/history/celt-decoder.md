# CELT decoder implementation

Historical notes from the milestones leading to v0.2.5. Commands assume the
repository root. For the integrated codec, see the [current support and evidence](../validation/codec-readiness.md).

## CELT fixed-point math

`opura::celt::math` supplies the scalar math needed for forthcoming CELT energy,
PVQ and loss reconstruction. `integer_sqrt(u32)` returns the exact floor of the
root (including zero) in at most 16 steps. Other functions reproduce the pinned
reference's integer approximations, rather than correctly rounded real math:
`sqrt`, `rsqrt_normalized`, `rsqrt_normalized_q31`, `cos_norm`, `log2`, `exp2`, `reciprocal`,
`fractional_divide_q29` and saturated-Q31 `fractional_divide`.

Rustdoc specifies Q formats, saturation, zero sentinels and valid input domains.
Invalid inputs return `MathError` before arithmetic. Fractional division accepts
positive denominators with numerator magnitude no greater than the denominator;
negative Q31 saturation uses -2147483647. All operations use bounded fixed-width
integer arithmetic without allocation or dependencies. Arithmetic follows the
reference scalar fast-64-bit multiplication path on every Rust target.

[Reference evidence](../../tests/celt-math-fixtures.txt) includes 624 FNV-1a blocks
covering 638975 individual results and 29164 boundary/wide/division cases.
Exhaustive blocks include signed Q10 exponents, normalized Q16 reciprocal roots,
two Q15 cosine periods and small integer sqrt/log/reciprocal domains. Each hash
covers four little-endian bytes per signed result. Separate tests independently
check integer-sqrt inequalities around every square up to 65535² and 65536 wide
values. Five mutations detect rounding, normalization and Newton corrections.

Optional regeneration compiles original external reference routines into a
temporary library, requiring the same clean pinned source, Python and C compiler:

```sh
python3 tools/celt_math_fixtures.py target/reference/opus > /tmp/opura-celt-math.txt
diff -u tests/celt-math-fixtures.txt /tmp/opura-celt-math.txt
```

These primitives do not yet decode CELT/Hybrid packets or establish full-codec
conformance/performance. CELT transform and band decoding are the next stages.


## CELT band energies

`opura::celt::energy::EnergyState` retains 21 Q24 log-energy values per channel.
`BandLayout::new(channels, start, end)` validates one/two encoded channels and a
half-open interval within 0..21 (including empty intervals). Unselected bands
and channels retain their histories. `FrameSize` selects 2.5/5/10/20 ms at the
internal 48 kHz clock; its `samples()` reports 120/240/480/960 respectively.

Call `coarse(entropy, layout, size, intra)` after reading the intra flag. It uses
Laplace, three-symbol, one-bit or no-bit delta fallbacks according to remaining
physical bits, then applies temporal/inter-band prediction and reference clamps.
`fine(entropy, layout, bits)` reads zero through eight raw refinement bits per
band, skipping a whole band when insufficient bits remain. After shape decoding,
`finalise(entropy, layout, bits, priorities, bits_left)` refines eligible bands in
priority order and returns unused bits. Its budget cannot exceed the remaining
physical input. These stages must be positioned around allocation/PVQ by the
future packet decoder; this API does not consume a complete CELT frame itself.

All operations commit energies and entropy together. Invalid layouts/quantizers,
invalid final budgets and arithmetic overflow reject without changing history.
`reset()` zeros both channel histories; `channel(index)` inspects all 21 energies.
The classic Opus path is supported; QEXT/RES24 refinement is excluded.

`RangeDecoder::decode_laplace(zero_frequency, decay)` exposes the bounded classic
CELT entropy distribution with validated Q15 zero frequency 1..32736 and Q14
decay 0..11456. Like other range primitives it zero-extends compressed input;
the energy stages handle physical budgets. Invalid models leave entropy unchanged.

[Reference sequences](../../tests/celt-energy-fixtures.txt) use original pinned
`unquant_coarse_energy`, `unquant_fine_energy`, `unquant_energy_finalise` and
`ec_laplace_decode`, recording all energies and entropy diagnostics after each
stage. Tests cover 768 chained frames across all LM/channel/intra contexts,
selected/empty intervals and 0–200 byte payloads, plus 2688 standalone symbols.
Every two-byte prefix is checked with three boundary Laplace models. Injected
second-channel refinement overflow verifies rollback after earlier samples and
raw bits have advanced. Five mutations detect accumulator truncation, temporal
clamp, inter-band prediction, fine midpoint and final priority errors.

```sh
python3 tools/celt_energy_fixtures.py target/reference/opus > /tmp/opura-celt-energy.txt
diff -u tests/celt-energy-fixtures.txt /tmp/opura-celt-energy.txt
python3 tools/celt_energy_fixtures.py target/reference/opus --tables > /tmp/opura-celt-energy-tables.rs
rustfmt --edition 2024 /tmp/opura-celt-energy-tables.rs
diff -u src/celt/energy_tables.rs /tmp/opura-celt-energy-tables.rs
```

CELT shape decoding, transform synthesis and complete Opus conformance remain
incomplete. No end-to-end CELT audio or performance claim follows from these tests.


## CELT integer pulse vectors

`opura::celt::pulses::PulseCodebook::new(dimensions, pulses)` accepts 2..176
coordinates and 0..128 pulses when the signed codebook fits a `u32` index.
`cardinality()`, `dimensions()` and `pulses()` expose the validated geometry.
`decode_index(index)` returns a fixed-storage `PulseVector` in canonical CWRS
coordinate order; `values()` exposes its signed coordinates and `squared_norm()`
the exact sum of squares. Absolute coordinate values sum to the pulse count.
Zero pulses yields a single all-zero vector.

`decode(entropy)` reads the uniform index and commits entropy only on success.
Large indices use raw suffix bits, which must be present physically; range input
retains the usual zero-extension semantics and the future band decoder enforces
codec bit budgets. Invalid geometry, overflowing cardinalities and out-of-range
indices return typed `PulseError` values. The one-dimensional sign case belongs
to band orchestration and is excluded from this primitive.

Compile-time symmetric U recurrence rows use capped counts and checked V sums.
No runtime allocation or table generation is needed; descending-dimension
ordering takes at most dimensions+pulses search/coordinate iterations. Capped
values reject overflowing codebooks and cannot change any valid index ordering.

[Original generic CWRS evidence](../../tests/celt-pulse-fixtures.txt) covers all 22575
configuration outcomes (1834 fit), 70423 indices exhaustively across 57 small
codebooks, 13401 wide/boundary vectors, and 566 entropy steps across 73 streams.
Exhaustive fingerprints hash little-endian signed-32 squared norm followed by
every coordinate for each index. Separate tests independently count codewords
using signed occupancy/binomial sums and check pulse/norm invariants. Invalid
requests and raw suffix exhaustion preserve entropy, including after a preceding
range symbol. Four mutations detect sign boundary, coordinate order, U recurrence
and index-interval errors.

```sh
python3 tools/celt_pulse_fixtures.py target/reference/opus > /tmp/opura-celt-pulses.txt
diff -u tests/celt-pulse-fixtures.txt /tmp/opura-celt-pulses.txt
```

The optional oracle compiles original `cwrs.c` with `SMALL_FOOTPRINT` so arbitrary
valid geometries can be checked without the optimized standard-mode table's
shape restrictions. CELT normalized bands, rotations, allocation and inverse
transforms remain future work; these integer vectors do not provide CELT audio.


## Normalized CELT PVQ bands

`opura::celt::pvq::decode_band(entropy, codebook, spread, blocks, gain)` composes
positive-pulse CWRS decoding, Q24 normalization and inverse spreading. Gain is a
nonnegative Q31 value, and `Spread` selects None/Light/Normal/Aggressive. Blocks
must be 1/2/4/8 and divide the coordinate count. `NormalizedBand::values()` exposes
Q24 spectral coordinates; `collapse_mask()` marks contiguous raw-pulse blocks
with nonzero entries before spreading, independently of gain.

Refined normalization uses `math::rsqrt_normalized_q31`, which maps Q31 input
in [2^29,2^31) to a Q29 reciprocal root with the reference Newton step. Spreading
preserves the original two-pass rotation order, secondary stride and deliberate
Q14 rounding between rotations. Output and intermediate storage are bounded;
input validation and pulse/raw-bit failures leave entropy unchanged. The caller
still enforces the surrounding CELT budget and handles zero-pulse folding,
one-dimensional signs, recursive splits and stereo orchestration.

[Original alg_unquant evidence](../../tests/celt-pvq-fixtures.txt) matches all spectral
coordinates, masks and range/tell/tell_frac in 28464 bands across 4116 streams.
It covers sparse/dense and boundary codebook indices, dimensions 2–176, zero,
half and full gains, every spread/block combination, including one-coordinate
short blocks. Another 196608 normalized reciprocal-root inputs are hashed from
original mathops.c, including fractional boundaries between Q16 seed values.
Five mutations detect missing root refinement, Q14 rounding, reverse rotation,
secondary-stride coefficients and incorrect collapse masks.

```sh
python3 tools/celt_pvq_fixtures.py target/reference/opus > /tmp/opura-celt-pvq.txt
diff -u tests/celt-pvq-fixtures.txt /tmp/opura-celt-pvq.txt
```

This is a normalized-band primitive, not CELT packet or audio output. Allocation,
recursive bands, transform synthesis, Hybrid and full conformance remain future work.


## Standard CELT complex FFT

`opura::celt::fft::FftPlan::new(FrameSize)` selects the standard 60/120/240/480
complex-point FFT used by a 120/240/480/960-sample CELT frame. `Complex` has signed
32-bit `real` and `imag` coordinates; callers choose signal scale/headroom.
`points()` reports the exact input length. Output needs at least that many
entries; longer output tails stay unchanged. Invalid geometry or downshift
returns `FftError` without modifying any output.

`forward(input, output, downshift)` transforms natural input to natural output
order with a negative exponent and no initial scale, distributing a 0..31-bit
shift across the reference radix stages. `forward_scaled(input, output)` matches
`opus_fft_c`, approximately dividing by N. `inverse(input, output)` matches the
unscaled positive-exponent `opus_ifft_c`. Arithmetic intentionally wraps at
signed-32 boundaries, matching the canonical scalar reference; avoid clipping by
providing suitable headroom/downshift. Stage rounding means these integer
transforms are not correctly rounded real DFTs or exact floating inverses.

Plans, shared Q15 twiddles, factors and bit-reversal permutations are static.
Radix 2/3/4/5 butterflies and downshift scheduling need no allocation or runtime
planning. The [1632 original FFT cases](../../tests/celt-fft-fixtures.txt) compare every
complex output for all sizes, all 32 shifts, canonical wrappers, impulses,
DC/Nyquist inputs, full-range signed extremes and deterministic arrays. Separate
floating DFT checks verify sign and natural frequency ordering at low amplitude;
exact impulse tests independently verify unscaled and 1/N forward amplitudes.
Five mutations detect bit reversal, plan twiddle stride, radix-3 sign, radix-5
coefficient and one-bit rounding errors.

```sh
python3 tools/celt_fft_fixtures.py target/reference/opus > /tmp/opura-celt-fft.txt
diff -u tests/celt-fft-fixtures.txt /tmp/opura-celt-fft.txt
python3 tools/celt_fft_fixtures.py target/reference/opus --tables > /tmp/opura-celt-fft-tables.rs
rustfmt --edition 2024 /tmp/opura-celt-fft-tables.rs
diff -u src/celt/fft_tables.rs /tmp/opura-celt-fft-tables.rs
```

The optional oracle observes original standard plans and scalar routines. FFT
alone does not synthesize CELT audio; inverse MDCT/window overlap and output
stages are described below. Recursive bands/allocation and full packet
orchestration remain incomplete.


## CELT inverse MDCT and overlap

`opura::celt::mdct::MdctState` stores the pending 60-sample overlap for one channel.
`synthesize(size, input, stride)` applies original inverse rotations/headroom
control, the verified FFT and the standard 120-sample TDAC window. It returns a
`TimeFrame` containing 120/240/480/960 signed-32 internal signal values through
`samples()`. These precede CELT postfiltering, de-emphasis and signed-16 PCM.
The caller supplies the reference spectral signal scale after denormalization.

Input bin i is read at `input[i*stride]`. Stride is 1/2/4/8, the selected
size*stride must not exceed 960, and input must contain all selected bins in at
most 960 entries. Other entries are ignored. Interleaved short blocks use
successive slices offset by block index, retaining overlap on each call.
State and output commit together; invalid geometry leaves overlap unchanged.
`overlap()` inspects the pending 60 samples; `reset()` clears them. Frame-size
changes preserve overlap, matching upstream synthesis buffers.

[398 original inverse-MDCT cases](../../tests/celt-mdct-fixtures.txt) match every time
sample and overlap entry (198360 values). Chained cases cover all size/stride
geometries, resets, silence, tiny signals, impulses, DC, full-scale signed limits,
alternating extremes and deterministic spectra. Additional sequences process
all 2/4/8 short blocks from a shared interleaved spectrum. Original headroom
observations cover pre/FFT shift extrema 0 and 29 and post-shift extrema 0 and
13, including pathological overflow behavior. Invalid requests preserve
state; five mutations detect lost overlap, wrong window complement, omitted FFT
shift, real/imaginary swap and incorrect saved tail.

```sh
python3 tools/celt_mdct_fixtures.py target/reference/opus > /tmp/opura-celt-mdct.txt
diff -u tests/celt-mdct-fixtures.txt /tmp/opura-celt-mdct.txt
python3 tools/celt_mdct_fixtures.py target/reference/opus --tables > /tmp/opura-celt-mdct-tables.rs
rustfmt --edition 2024 /tmp/opura-celt-mdct-tables.rs
diff -u src/celt/mdct_tables.rs /tmp/opura-celt-mdct-tables.rs
```

These are internal transform outputs; the complete synthesis backend is below.
CELT allocation/recursive band assembly, full packet decoding and complete
conformance remain incomplete; no end-to-end CELT audio or competitive performance claim is made.


## CELT spectral scaling and output de-emphasis

`opura::celt::output::denormalize(size, input, energies, bands, rate, silence)`
converts one normalized Q24 spectrum using 21 Q24 log energies plus the standard
band means. `Spectrum::values()` supplies reference-scale inverse-MDCT input.
The validated band range lies within 0..21, and the input exactly matches frame
size. Silence zeros the whole spectrum. Bins below selected bands and above the
output-rate cutoff are zeroed; reference gain underflow, caps, fractional energy
rounding and signed wrapping are preserved. The shared fractional exponent
polynomial includes the rounded Q10 endpoint needed by Q24 energies.

`Decimation::{By1, By2, By3, By4, By6}` selects 48/24/16/12/8 kHz respectively;
`factor()` and `sample_rate()` report its geometry. `DeemphasisState::process`
accepts a 120/240/480/960-sample 48 kHz left signal, an optional equal-length right
signal and decimation. It filters all internal samples before selecting output,
then rounds/saturates to signed-16 mono or interleaved stereo `PcmFrame` samples.
The frame reports channels, rate and samples per channel with at most 1920 values.

Feedback uses the canonical coefficient and clipping. Inactive right-channel
memory and both memories across rate changes are retained; `memory()` inspects
them and `reset()` clears them. Invalid signals leave both memories unchanged.
Comb postfiltering, when enabled, must precede de-emphasis. This API does not
perform Hybrid accumulation or supply a complete CELT packet decoder.

[Reference evidence](../../tests/celt-output-fixtures.txt) contains 640 original
spectral-scaling cases and 384 chained original de-emphasis calls, comparing
all spectra/PCM/memory values across sizes, rates, channel changes, bands,
silence, clipping, signed extremes and fractional energy boundaries. The
optional oracle extracts the original function bodies verbatim into a temporary
probe with original headers/constants; it does not substitute implementations.
Five mutations detect wrong mean-energy format, mantissa rounding, rate cutoff,
feedback gain and PCM rounding. Invalid geometry and reset preserve contracts.

```sh
python3 tools/celt_output_fixtures.py target/reference/opus > /tmp/opura-celt-output.txt
diff -u tests/celt-output-fixtures.txt /tmp/opura-celt-output.txt
python3 tools/celt_output_fixtures.py target/reference/opus --tables > /tmp/opura-celt-output-tables.rs
rustfmt --edition 2024 /tmp/opura-celt-output-tables.rs
diff -u src/celt/output_tables.rs /tmp/opura-celt-output-tables.rs
```

The comb postfilter and synthesis backend are described below. Recursive CELT
bands/allocation, Hybrid and complete Opus packet output remain future work. Stage equality does not establish complete
conformance or SOTA performance.


## CELT comb postfilter

`opura::celt::postfilter::PostfilterParams::new(period, gain, tapset)` validates
classic decoder periods 15..1023 and nonnegative Q15 gains 0..24576. Period zero
is accepted for zero gain; default parameters disable the filter. `Tapset`
selects standard Set0/Set1/Set2; parameter getters expose the validated values.

`PostfilterState::process(input, new_params)` accepts one internal-rate channel
of 120/240/480/960 samples and returns a `FilteredFrame` through `samples()`.
The first 120 values transition old to current parameters. Longer frames then
transition current to newly decoded parameters before constant filtering. A
120-sample frame stores new parameters for the next call. The two parameter
generations follow upstream period clamping even when gain is zero.

Filtering retains 1026 processed samples, uses exact windowed coefficients,
different transition/constant biases and signed wrapping/clipping. Disabled
segments pass input unchanged. `parameters()` and `history()` inspect state;
`reset()` clears it. Validation precedes any mutation. Use independent instances
per output channel and apply de-emphasis after filtering. This API follows normal
decoder frame scheduling; loss and packet control belong to later orchestration.

[576 original cases](../../tests/celt-postfilter-fixtures.txt) compare all outputs,
every history entry and all old/current parameter fields through period/gain/tap
changes, constant/disabled filters, short/long frames, clipping, extremes and
resets. Separate checks verify short-frame delay, disabled long-frame tails and
invalid-request rollback. Five mutations detect transition/constant bias,
processed history, premature new parameters and omitted constant-filter shortcut.

```sh
python3 tools/celt_postfilter_fixtures.py target/reference/opus > /tmp/opura-celt-postfilter.txt
diff -u tests/celt-postfilter-fixtures.txt /tmp/opura-celt-postfilter.txt
```

The optional oracle extracts original scalar comb routines unchanged and applies
the original normal-decoder scheduling with the pinned standard window. The complete synthesis backend follows below; recursive bands, allocation,
packet control and conformance remain incomplete; this stage does not itself decode Opus packets.


## Complete CELT synthesis backend

`opura::celt::synthesis::CeltSynthesis::new(decimation, output_channels)` selects
fixed output rate and one/two API channels. `synthesize(spectra, energies,
controls)` takes one/two decoded normalized Q24 left/right spectra, one 21-band
Q24 energy array per encoded channel, and `SynthesisControls` for frame size,
selected bands, transient/silence flags and new postfilter parameters. Spectra
must exactly match the 120/240/480/960-bin frame size. This API starts after
entropy, allocation and recursive spectral decoding; it accepts no Opus packets.

The backend combines spectral scaling, reference mono/stereo projection, long
or interleaved-short inverse MDCTs, synthesis clipping, delayed comb filtering
and final de-emphasis/decimation. Stereo to mono halves each spectrum before
addition; mono to stereo preserves each output channel's existing histories.
Clipping occurs after all MDCT blocks without clipping pending overlap. Silence
zeros new spectra while allowing overlap/filter continuation. Returned signed-16
`PcmFrame` values are final for this classic synthesis path.

All channel, overlap, postfilter and de-emphasis changes commit atomically.
`reset()` retains output geometry. Read-only `overlap(channel)`,
`postfilter(channel)` and `deemphasis_memory()` expose diagnostic state;
`channels()` and `sample_rate()` report configuration. Invalid channels, energy
counts, spectral lengths and band ranges preserve every history.

[480 original complete-backend cases](../../tests/celt-synthesis-fixtures.txt) compare
final PCM, both MDCT overlaps, every postfilter history entry/parameter generation
and both de-emphasis memories. They cover every rate/frame/encoded/API channel
and long/transient combination, channel changes, selected/Hybrid high bands,
silence, gain/period/tap switches, clipping and resets. The oracle composes
original unchanged celt_synthesis/comb/de-emphasis with original standard mode,
MDCT and FFT. Five mutations detect downmix rounding, transient stride/layout,
lost overlap, omitted synthesis clipping and skipped postfilter integration.

```sh
python3 tools/celt_synthesis_fixtures.py target/reference/opus > /tmp/opura-celt-synthesis.txt
diff -u tests/celt-synthesis-fixtures.txt /tmp/opura-celt-synthesis.txt
```

Encoded CELT frame parsing/allocation/recursive bands, loss recovery, Hybrid
mixing and official full Opus conformance remain incomplete. This stage evidence
is not a complete codec or a competitive-performance result.


## CELT frame controls and TF resolution

`opura::celt::control::decode_header(packet, frame_index)` accepts a CELT-only
packet frame and returns immutable `FramePrefix` plus its positioned borrowed
entropy decoder. The prefix exposes size, silence, typed postfilter parameters,
transient/short-block count and intra-energy flag. This wrapper does not decode
coarse energies or the rest of the compressed audio.

`decode_prefix(entropy, size, start)` supports a correctly positioned cursor for
CELT start0 or Hybrid start17 after SILK. Payloads must contain 2..1275 bytes;
empty/one-byte loss handling is separate. A cursor already beyond its physical
budget rejects. Original cached-tell guards decide field presence. Silence
advances bit accounting to the physical end without changing normalized range
or reading more bytes; accounting errors leave entropy unchanged.

After coarse energies, call `decode_tf(entropy, size, bands, transient)` for a
half-open interval within 0..21. Its `TfResolution::values()` exposes 21 signed
resolution values, zero outside selected bands. Differential flags, first/later
probabilities, reserved select bit and conditional select consumption follow
upstream. `decode_spread(entropy)` then reads the spreading decision or returns
Normal when fewer than four whole bits remain. Allocation follows later.
Every control operation is transactional, bounded and allocation-free.

[Reference control sequences](../../tests/celt-control-fixtures.txt) compare all values
and range/tell/tell_frac on 3360 prefix→original coarse-energy→TF→spread cases.
They cover all sizes/start contexts/channels, positioned range/raw cursors,
2..1275-byte budgets, silence, near-exhaustion and all postfilter octave/pitch/
gain/tap boundaries. Another 1824 standalone TF cases target exact remaining-bit
boundaries, every LM/transient model and selected/empty intervals. Tests exhaust
all 65536 two-byte values at all four sizes and both starts, verify invalid
rollback/packet errors and silence-accounting overflow, and detect six mutations
in silence accounting, pitch, TF reservation/aggregation/select and spread fallback.

```sh
python3 tools/celt_control_fixtures.py target/reference/opus > /tmp/opura-celt-control.txt
diff -u tests/celt-control-fixtures.txt /tmp/opura-celt-control.txt
python3 tools/celt_control_fixtures.py target/reference/opus --tables > /tmp/opura-celt-control-tables.rs
rustfmt --edition 2024 /tmp/opura-celt-control-tables.rs
diff -u src/celt/control_tables.rs /tmp/opura-celt-control-tables.rs
```

The oracle extracts original prefix/spread decision blocks and `tf_decode`
unchanged, with original entropy/coarse-energy routines and tables. These stages
are not full compressed CELT or Hybrid decoding; allocation, recursive bands,
loss recovery and official complete Opus conformance remain incomplete.


## CELT bit allocation

`opura::celt::allocation::decode_allocation(entropy, size, layout, transient)`
runs after spreading and before fine-energy/shape decoding. The validated
`BandLayout` must select at least one band; payloads contain 2..1275 bytes and
an already over-budget cursor rejects. Standard caps, dynamic boosts, trim,
anti-collapse reservation, six-step curve interpolation, skipped bands,
intensity/dual stereo and fine/PVQ/excess redistribution follow original rate.c.
Every operation uses fixed storage and commits entropy only after success.

The immutable `Allocation` retains frame/layout/transient context. Getters expose
all 21 caps/boost offsets, PVQ budgets in **eighth bits** (not pulse counts),
fine bits/priorities, trim, intensity boundary, dual flag, carry balance and the
exclusive coded-band endpoint. `fine_bits()` and `fine_priority()` feed existing
energy refinement APIs. `anti_collapse_reserved()` marks the one raw bit to read
after shapes. Selected-band outputs are zero outside the interval; standard caps
are defined for all bands. Fine energy is zero through eight bits per channel.

[16296 original allocation cases](../../tests/celt-allocation-fixtures.txt) match every
array/scalar plus range/tell/tell_frac, and the following original fine-energy
values/counters. They cover all rates' shared internal frame models, encoded
channels, transient/long contexts, 2..1275-byte budgets, fresh/positioned cursors,
boosted/skipped/capped bands, every trim and stereo mode. Full-prefix cases first
traverse original prefix/coarse/TF/spread. Additional cases exhaust all 231
nonempty band intervals at each LM/channel/transient context. Tests independently
check budget conservation and fine limits, malformed rollback and 512 bounded
adversarial intervals. Six mutations detect boost quantum, protected skip band,
dual-bit refund, two-coefficient fine offset, balance and anti-collapse reserve.

```sh
python3 tools/celt_allocation_fixtures.py target/reference/opus > /tmp/opura-celt-allocation.txt
diff -u tests/celt-allocation-fixtures.txt /tmp/opura-celt-allocation.txt
python3 tools/celt_allocation_fixtures.py target/reference/opus --tables > /tmp/opura-celt-allocation-tables.rs
rustfmt --edition 2024 /tmp/opura-celt-allocation-tables.rs
diff -u src/celt/allocation_tables.rs /tmp/opura-celt-allocation-tables.rs
```

The optional oracle uses the unchanged decoder allocation block, init_caps,
rate.c, entropy and fine-energy routines with the pinned original standard mode.
Recursive spectral partitions/folding/stereo assembly, anti-collapse and complete
compressed CELT/Hybrid decoding remain future work.


## CELT band arithmetic and transforms

`opura::celt::bands` provides `split_gains` (Q15 cosine/sine), Q11 `log2_tan`,
`haar`, `deinterleave`/`interleave`, legacy `renormalize` and `stereo_merge`.
These are fixed-storage primitives for recursive spectral decoding. Coded theta
accepts endpoints 0/16384 and interior 64..16320: classic qn<=256 never produces
smaller interior values, where upstream cosine arithmetic is undefined.
Log-ratio inputs must be positive i16 values. Vector lengths are 1..176;
permutation strides 1/2/4/8/16 must divide the length, with Hadamard stride>=2.
Haar uses a power-of-two stride and even per-stride length. Products truncate
before sums, using the original single-precision-derived coefficient.

Renormalization accepts nonnegative Q31 gain, rounded Q14 coordinates fitting
i16 and one plus squared energy fitting positive i32. Stereo merge accepts
matching Q24 vectors, each squared norm <=2 Q48 units, and nonnegative Q31 mid
gain. It preserves the original near-cancellation fallback. Validation/scalar
errors leave all input vectors unchanged; successful operations follow original
fixed-point rounding, truncation and wrapping exactly.

[Original band evidence](../../tests/celt-band-fixtures.txt) covers all 16259 supported
coded angles, all 1..256 sine/cosine pairs plus wide Q15 boundaries (68644 pairs),
every supported permutation/Haar geometry (1531 cases), 420 folded/noise/edge
normalizations and 400 stereo merges including cancellation. Tests also check
permutation inversion, invalid domains/rollback and seven targeted mutations.

```sh
python3 tools/celt_band_fixtures.py target/reference/opus > /tmp/opura-celt-bands.txt
diff -u tests/celt-band-fixtures.txt /tmp/opura-celt-bands.txt
python3 tools/celt_band_fixtures.py target/reference/opus --tables > /tmp/opura-celt-band-tables.rs
rustfmt --edition 2024 /tmp/opura-celt-band-tables.rs
diff -u src/celt/band_tables.rs /tmp/opura-celt-band-tables.rs
```

The optional oracle extracts the original band/vq functions unchanged and uses
pinned original scalar math. Recursive partition entropy, folding/stereo band
assembly, anti-collapse, complete CELT/Hybrid decoding and conformance follow.


## CELT recursive mono partitions

`opura::celt::partition::decode_partition(entropy, state, controls, fold)` decodes
a standard mono spectral partition with fixed storage and transactional entropy
and state. `PartitionControls` supplies standard band index, LM -1..3, block
count 1/2/4/8/16, eighth-bit budget 0..81600, nonnegative Q31 gain, spreading and
fill mask. Dimension is derived from band/LM and must be 2..176, divisible by
blocks. One-coordinate bands belong to later band orchestration. Payloads must
contain 2..1275 bytes; already over-budget cursors reject.

`PartitionState::new(remaining_bits, seed)` supplies a separate bounded shape
budget and folding seed; returned state exposes both. Theta can overrun this
budget on malformed input, preserving reference behavior for this primitive;
a subsequent call rejects a negative budget. The surrounding frame decoder must
check final entropy/budget. Fold input must match dimension and have rounded Q14
coordinates/energy fitting signed scalar arithmetic, including a conservative
four-unit coordinate dither margin. Failure preserves entropy and shared state.
`Partition::values()` returns Q24 time-block coefficients and `collapse_mask()`
returns u16 flags. Existing public PVQ still accepts at most eight blocks/u8 masks.

Standard pulse-cache selection, theta resolution/model, split gains, transient
allocation, larger-half-first decoding, three-bit redistribution reserve and
remaining-budget pulse reduction follow original `quant_partition`. Empty shapes
use original LCG noise or dithered folding plus legacy normalization. LM decreases
to -1, bounding recursive decoding to four split levels (31 nodes).

[9438 reference cases](../../tests/celt-partition-fixtures.txt) cover every supported
band/LM/block geometry, all spreading policies, low/max gains, folded/noise/zero
fills, exhausted budgets and fresh/range/raw-positioned cursors. 9409 cases match
all coefficients, masks, remaining budget, seed and range/tell/tell_frac; 29 corrupt
uniform cases reject atomically. Invalid domains and negative-budget continuation
also roll back. Eight mutations detect split threshold, cache tie, theta cost,
transient delta, rebalancing, LCG, dither and sixteen-block mask errors.

```sh
python3 tools/celt_partition_fixtures.py target/reference/opus > /tmp/opura-celt-partition.txt
diff -u tests/celt-partition-fixtures.txt /tmp/opura-celt-partition.txt
python3 tools/celt_partition_fixtures.py target/reference/opus --tables > /tmp/opura-celt-partition-tables.rs
rustfmt --edition 2024 /tmp/opura-celt-partition-tables.rs
diff -u src/celt/partition_tables.rs /tmp/opura-celt-partition-tables.rs
```

The optional oracle uses unchanged original theta/partition functions with pinned
standard mode, generic CWRS, vq and entropy. TF/stereo band orchestration,
anti-collapse, complete CELT/Hybrid packets and official conformance follow.


## CELT mono/stereo band reconstruction

`opura::celt::partition::decode_band(entropy, state, BandControls, fold)` adds TF
and stereo orchestration. Controls contain a `PartitionControls` request, a
standard `tf_change` and optional `StereoControls { intensity, disable_inversion }`.
LM must be 0..3 and initial blocks 1 (long) or 2^LM (transient); TF must belong to
the corresponding standard mapping. Only low block-count fill bits are accepted.
Stereo requires unit Q31 gain and intensity 0..=21. Fold, entropy and budget
bounds match partition decoding; every error preserves both shared states.

One-coordinate bands reconstruct unit signs with remaining-budget guards,
independently of gain. Other bands transform fold data, decode partitions and
undo time/frequency reorganization with exact Haar/Hadamard rounding/masks.
Stereo adds step/uniform theta, intensity inversion (consumed even when ignored),
N2 orthogonal-side signs, larger-half-first budgets and normalized channel merge.
`Band::left()`/`right()` return frequency-interleaved Q24 coordinates and
`collapse_mask()` the combined u8 mask. `folding()` captures scaled mono/mid data
before stereo merge; N2 stereo captures its stronger primary basis.

[15708 original band cases](../../tests/celt-band-decode-fixtures.txt) exhaust every
frame size, band, long/transient TF mapping and mono/stereo combination, with
varied budgets/gains, intensity/inversion, spreading, folding and entropy cursor
positions. 15680 match both channels, folding vectors, masks, remaining budget,
seed and range/tell/tell_frac; 28 corrupt uniform cases reject atomically.
Malformed-control/fold rollback and nine mutations cover stereo probability,
intensity boundary, inversion override, N1 budget, TF mask, fold scaling, N2
primary/sign and final inversion. Mutation testing exposed a correlated oracle
parameter combination; the regenerated corpus independently exercises override.

```sh
python3 tools/celt_partition_fixtures.py target/reference/opus --bands > /tmp/opura-celt-band-decode.txt
diff -u tests/celt-band-decode-fixtures.txt /tmp/opura-celt-band-decode.txt
```

The optional oracle extends the unchanged original partition/theta functions
with original quant_band/quant_band_stereo and Haar/Hadamard/stereo helpers.
All-band folding/budget orchestration, anti-collapse and complete CELT/Hybrid
packet decoding are still needed; this evidence does not establish a full codec.


## CELT all-band normalized spectra

`opura::celt::partition::decode_spectrum(entropy, allocation, tf, spread, seed,
disable_inversion)` assembles all selected bands after fine energy. Immutable
allocation supplies frame size, encoded channels, bands, PVQ budgets, balance,
anti-collapse reservation and stereo context. TF values must match the standard
long/transient mapping. Physical payload is 2..1275 bytes with a cursor within its
budget. All-band starts are restricted to real packet modes 0 (CELT) and 17
(Hybrid). Generic allocation-only intervals remain available separately.

All-band decoding follows original balance distribution, fold position freeze,
source collapse-mask aggregation, aggressive-spread folding rules, special
Hybrid second-band tail and dual-to-intensity folding conversion. Each band uses
the verified sign/TF/stereo/partition core. `Spectrum` exposes frame/layout,
channel Q24 values, 21 per-channel collapse masks and final folding seed.
Unselected bins/inactive channels stay zero; every error preserves entropy.
The returned spectrum still needs reserved anti-collapse control, final energy
refinement and optional anti-collapse before the synthesis backend.

[4032 complete-stage cases](../../tests/celt-spectrum-fixtures.txt) traverse original
coarse energy, TF, spreading, allocation and fine energy before quant_all_bands;
1008 also traverse the original prefix. They cover every frame/channel, nine
CELT/Hybrid intervals, intra/transient models, fresh/positioned cursors and
2..1275-byte zero/FF/random payloads. 3779 match all selected coordinates, masks,
seed and range/tell/tell_frac, while 253 corrupt streams reject atomically.
Original quant_all_bands borrows the unselected highest band as internal scratch;
comparisons exclude that private scratch and independently require our unused
output bins to be zero. Invalid starts/TF/payloads/over-budget cursors roll back.
Eight mutations detect reserved budget, balance divisor, Hybrid offset/tail,
dual averaging/budget, collapse-mask combination and fold-position freeze.

```sh
python3 tools/celt_partition_fixtures.py target/reference/opus --spectra > /tmp/opura-celt-spectrum.txt
diff -u tests/celt-spectrum-fixtures.txt /tmp/opura-celt-spectrum.txt
```

The optional oracle uses unchanged original quant_all_bands and preceding stage
blocks/functions with pinned scalar code. It does not yet validate anti-collapse,
stateful CELT PCM packets, Hybrid synthesis or official complete Opus conformance.


## CELT transient anti-collapse

After spectrum decoding, read the reserved raw bit when allocation requests it,
then call the existing energy final refinement with the remaining whole bits.
If the raw bit is on, call `Spectrum::anti_collapse(allocation, current, previous,
previous2)`. The three Q24 energy arrays each contain both 21-band channels,
including the inactive channel history for mono max merging. Every energy must
be within -32..=32 log2 units. Allocation must match spectrum size/layout and
reserve anti-collapse for a transient 10/20ms frame; invalid context/energy or
normalization errors preserve the complete spectrum.

Original bit-depth and energy thresholds, LM3 scaling, interleaved empty-block
noise and legacy renormalization determine the restored unit spectra. Masks and
stored seed remain unchanged: upstream anti-collapse takes its seed by value.
Frame-level energy history updates and final RNG policy belong to the forthcoming
stateful decoder, rather than this primitive.

[2016 final-stage cases](../../tests/celt-anti-collapse-fixtures.txt) extend the original
coarse/TF/spread/allocation/fine/all-band pipeline with varied two-channel energy
histories, reserved control decoding, final energy and unchanged anti_collapse.
1850 cases match final energies, selected channel coefficients, masks, seed and
entropy, including 504 prefix cases and 395 enabled anti-collapse calls, of which
135 change coefficients. 166 corrupt streams reject atomically. Invalid context
and energy rollback plus seven mutations verify thresholds, mono history, energy
attenuation, LM3 gain, noise ordering, normalization and persistent seed policy.

```sh
python3 tools/celt_partition_fixtures.py target/reference/opus --anti > /tmp/opura-celt-anti.txt
diff -u tests/celt-anti-collapse-fixtures.txt /tmp/opura-celt-anti.txt
```

The spectral pipeline now includes optional anti-collapse. Stateful compressed
CELT PCM packets, CELT loss recovery, Hybrid and full conformance remain pending.


## Normal CELT packets to final PCM

`opura::celt::decoder::CeltDecoder::new(Decimation, output_channels)` selects
fixed 8/12/16/24/48kHz output and one/two interleaved channels. `decode(packet)`
accepts a complete CELT-only normal packet; `decode_frame(payload, FrameConfig)`
accepts one explicit 2.5/5/10/20ms payload and NB/WB/SWB/FB encoded context.
Encoded channel count may change independently of API channels. Empty/one-byte
frames require future loss/DTX handling; SILK/Hybrid and mediumband CELT contexts
reject. Packet framing supports up to 48 frames/120ms and padding.

The decoder connects prefix, mono energy-history merging, coarse/TF/spreading,
allocation/fine, recursive all-band reconstruction, reserved control/final energy,
anti-collapse, silence and final PCM synthesis. It retains both channels' normal/
transient/background energy histories, final entropy RNG, MDCT overlap,
postfilter and de-emphasis across valid frame/channel/bandwidth changes. Default
inversion policy follows current scalar reference (disabled for mono API).
Every failure rolls back the complete frame or multi-frame packet. `reset()`
clears all history while retaining output geometry. Output exposes PCM geometry
and the last frame's public final-range diagnostic; history getters support audits.

```rust
use opura::celt::{decoder::CeltDecoder, output::Decimation};
let mut decoder = CeltDecoder::new(Decimation::By1, 2)?;
let pcm = decoder.decode(&[0xf8, 0xff, 0xff])?; // full-band 20ms silence
assert_eq!(pcm.samples_per_channel(), 960);
assert_eq!(pcm.channels(), 2);
# Ok::<(), opura::celt::decoder::DecoderError>(())
```

[960 real encoder packets](../../tests/celt-packet-fixtures.txt) match public pinned
scalar fixed-point `opus_decode` PCM and `OPUS_GET_FINAL_RANGE` exactly across
all rates, API/encoded channels, bandwidths, frame sizes, silence/impulses/noise,
configuration changes, resets, VBR/CBR, padding and 2..48-frame packets up to120ms.
Read-only observers appended to original source also compare every energy/log/
background value, de-emphasis memory, filter parameter, overlap and 1026-sample
filter history. Original decoding statements stay unchanged. Tests independently
compare packet versus individual-frame decoding, check late-frame failure/reset
and 512 bounded adversarial payloads, and catch eight history/RNG/inversion
integration mutations. All builds/tests remain pure Rust and dependency-free.

```sh
python3 tools/celt_packet_fixtures.py target/reference/opus > /tmp/opura-celt-packets.txt
diff -u tests/celt-packet-fixtures.txt /tmp/opura-celt-packets.txt
```

Optional regeneration compiles external pinned libopus into a temporary library,
uses its real encoder/repacketizer/public decoder, and records independent state
snapshots. Normal CELT support is now end-to-end; CELT PLC/DTX, Hybrid, mode
transitions, official full conformance, encoder and competitive evaluation remain.


## CELT LPC and concealment filters

`opura::celt::lpc::autocorrelation(input, lag, windowed)` accepts 3..2048 i16
samples, lag0..24 and at least three samples beyond the lag. Optional standard
120-sample end windows require length>=240. Returned correlations include lag0
in [2^28,2^29) and an energy scaling exponent. The estimator includes original
rounding headroom and exact power-of-two normalization decisions.

`analyse(correlation, coefficients)` supports order1..24, with one extra ACF lag,
lag0 in0..2^30 and other lag magnitudes <=lag0. Unstable intermediate reflection
coefficients reject; this is not a general positive-definiteness validator.
Fast64 accumulation, Q31 reflection, gain stopping and Q25→Q12 bandwidth fitting
match upstream. All-zero ACF clears output. The ten-fit fallback retains the
original behavior: coefficient0 becomes4096 and other incoming outputs remain.

`fir(input, coefficients, output)` requires order3..24, output1..1024 and explicit
leading history (input length=output length+order). Signed32 sums wrap, then
round/saturate to +/-32767. `iir(values, coefficients, memory)` works in place
on Q12 i32 data at order4/8/12/16/20/24, length order..1080 divisible by4, as used
by classic PLC. It takes newest-first i16 memory; the canonical original kernel
returns low16 bits of raw Q12 outputs, which PLC discards afterwards. This differs
from a conventional rounded persistent IIR memory and is reproduced deliberately.
All routines use fixed storage, bounded work and atomic failures.

[Original scalar LPC evidence](../../tests/celt-lpc-fixtures.txt) compares every value
and memory on 1601 correlations, 2680 coefficient analyses, 832 FIR and 336 IIR
cases: silence/DC/tones/noise/extremes, windows, scaling, high-order positive
Toeplitz covariance, every supported order and short/long filter geometry.
Exact estimator boundaries and clustered spectra were added after mutation
survivors exposed missing scale/fitting cases. Independent zero-filter and
correlation bounds, malformed/unstable rollback and seven targeted mutations pass.

```sh
python3 tools/celt_lpc_fixtures.py target/reference/opus > /tmp/opura-celt-lpc.txt
diff -u tests/celt-lpc-fixtures.txt /tmp/opura-celt-lpc.txt
```

The optional oracle compiles unchanged original celt_lpc/pitch scalar kernels
and math; clustered covariance models define test inputs, not expected outputs.
These helpers prepare CELT pitch search/PLC; loss synthesis is not yet integrated.


## CELT concealment pitch analysis

`opura::celt::pitch::plc_pitch(left, optional_right)` analyzes exactly2048 latest
processed internal Q12 samples per channel, each within +/-536870911 (SIG_SAT).
The fixed-storage `PlcPitch` exposes a48kHz period within100..=720 and the full
1024-sample half-rate whitened signal. Inputs remain borrowed and unchanged.

The implementation follows original factor2 triangular downsampling with stereo
headroom, stabilized LPC4, FIR5 rounding, quarter-rate coarse correlation, two
fine candidate neighborhoods, normalized two-best ranking/sliding energy and
pseudo interpolation. Encoder pitch geometry and remove-doubling refinement
are available separately, as described below.

[2202 original pitch cases](../../tests/celt-pitch-fixtures.txt) compare every whitened
sample and period across every target period100..720 in mono/stereo, low/full
amplitudes, triangle/square/impulse/saw/noise/silence/DC/alternating signals and
matching/opposite/shifted/independent channel contexts. Integer signal recipes
are fixture inputs; expected outputs come solely from pinned original scalar
pitch_downsample/pitch_search and LPC/math. Domain and deterministic silence
checks plus seven mutations verify stereo headroom, lag window, FIR rounding,
coarse/fine scaling, candidate radius, interpolation and sliding energy.

```sh
python3 tools/celt_pitch_fixtures.py target/reference/opus > /tmp/opura-celt-pitch.txt
diff -u tests/celt-pitch-fixtures.txt /tmp/opura-celt-pitch.txt
```

These helpers prepare waveform PLC. CELT loss synthesis/recovery and Hybrid
remain incomplete; normal CELT packet-to-PCM decoding is already supported.


## CELT loss, DTX and recovery

`CeltDecoder::conceal_samples(duration)` conceals an explicit missing interval on
a 48kHz clock: 120..5760 samples in120-sample steps (2.5..120ms). For example,
960 requests20ms at every output rate. Scheduling follows the last packet's
frame size and valid2.5/5/10/20ms subframes. Before audio history exists it returns
silence. `decode`/`decode_frame` also accept empty/one-byte compressed frames as
PLC/DTX; the current TOC supplies duration while the prior decoded band limit is
retained. An absent transport packet should use conceal_samples, not an empty
byte slice passed as packet framing.

Periodic PLC estimates pitch from2048 processed samples, analyzes stabilized LPC24,
extracts/decays excitation, extrapolates, applies IIR and limits synthesis energy.
After100ms of concealment, noise PLC uses background-limited log-energy decay and
normalized random spectra. Two consecutive normal frames re-enable periodic PLC
after noise. Pending negative comb prefilter and TDAC join synthesized loss with
the next MDCT, and recovery adjusts predicted energies/background growth.
Loss and PLC durations saturate at10000 units of2.5ms; per-call work/output remain
bounded. Every error preserves the entire decoder across the whole operation.

`DecodedPcm::final_range()` / decoder final_range are zero for loss/DTX, while
`rng_state()` retains the internal noise RNG. `concealment()` exposes read-only
mode/duration/pitch, LPC, full signal/future history and scheduling diagnostics.
`reset()` clears histories/diagnostics and restores the initial frame clock;
it retains the internal band limit as native OPUS_RESET_STATE does. Fresh
construction starts fullband. Initial loss remains silent in either case.

[872 public-reference scenarios](../../tests/celt-loss-fixtures.txt) match PCM, final
range, all energy/log/background values, deemphasis, postfilter parameters/full
history, MDCT overlap, full2048-sample/future signal, LPC, RNG and PLC controls.
They cover all rates/API/encoded channels/bandwidths, frame changes, initial and
implicit loss, mixed/multi-frame packets, short/long bursts, recovery/reset and
210 consecutive120ms requests reaching the duration caps. The oracle distinguishes
fresh construction (reset2) from native reset (reset1), and appends read-only
observers without changing original decoding statements.

[208 original bandwidth and232 energy-guard cases](../../tests/celt-plc-filter-fixtures.txt)
exercise coefficient/extreme/sum boundaries, zero/explosion cutoffs, attenuation
ratio/rounding and overlap crossfade independently of realistic stream coverage.
Invalid guard/duration checks, packet/frame equivalence, a retained corrupt-uniform
late-frame regression and256 adversarial normal/loss/recovery sequences verify
bounds and rollback. Fifteen mutations detect scheduling, mode/decay/warmup,
prefilter/TDAC, recovery/background, counters/RNG/reset and guard arithmetic.

```sh
python3 tools/celt_packet_fixtures.py target/reference/opus --loss > /tmp/opura-celt-loss.txt
diff -u tests/celt-loss-fixtures.txt /tmp/opura-celt-loss.txt
python3 tools/celt_plc_fixtures.py target/reference/opus > /tmp/opura-celt-plc-guards.txt
diff -u tests/celt-plc-filter-fixtures.txt /tmp/opura-celt-plc-guards.txt
```

CELT-only normal and loss paths are now end-to-end. Hybrid, cross-mode transitions,
complete official conformance, encoder and competitive evaluation remain pending.
