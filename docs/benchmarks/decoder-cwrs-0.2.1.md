# CWRS decoder optimization:0.2.1 versus0.2.0

The change specializes signed pulse-vector unranking in CELT. It retains the
same compile-time capped recurrence and validated public codebook. Many-pulse
cases traverse a row; many-dimension cases recognize a zero coordinate or
traverse a column. The last two coordinates use the exact closed form for
`U(2,k)=2k-1` when `k>0`. Exhausted pulse suffixes remain zero. No entropy,
normalization, rate policy, state layout or transaction contract changes.

## Attribution and correctness

Timer-based gprofng profiles of0.2.0 public API runs identified `decode_index`
among decoder hot paths (9.14% exclusive sampled CPU at48kHz stereo over the
complete official corpus). Memory copies also account for substantial samples.
These are sparse attribution samples, not timing or competitive results.
Hardware perf was unavailable under the host's existing restrictions. Optional
profiling used release optimization with debug symbols; retained speed
measurements below use ordinary release binaries without debug symbols.

Existing native fixtures cover codebook counts, exhaustive small indices, wide
vectors and entropy. An additional test compares specialized output with the
previous generic traversal throughout every valid2..176dimension/0..128pulse
geometry, including zero/full indices, sign boundaries and deterministic random
indices. More than100000 comparisons also check independent ranking, L1 pulse
count and squared energy. Two deliberate source mutations of sign selection and
the last coordinate fail running tests; the restored suite passes.

The complete120-case RFC8251 matrix passes and retains every0.2.0 PCM SHA256
and encoded nonzero final range. This includes all five output rates and both channel counts.
The normal suite additionally covers explicit loss/FEC, mode transitions and
rollback, which are not timed in the official packet replay.

The direct table accesses retain the constructor's bounds. A representable
codebook has `min(n,k+1)<15`: otherwise the capped positive U terms overflow
its checked cardinality sum. A suffix codebook embeds in the original codebook,
so this property remains true as coordinates/pulses decrease. Thus `k>=n`
selects an existing row `n<15`; `k<n` selects existing rows `k` and `k+1`.
Column indices never exceed176 and pulse indices never exceed129. Decrementing
searches terminate at `U(0,n)=0`; their unsigned index cannot require a negative
pulse count. Coordinate magnitudes are at most128 and squared energy at most
16384. Native fixtures and the generic comparison exercise these boundaries;
this argument does not claim whole-codec correctness from the helper alone.

## Reproduction and scope

Use clean source checkouts of `v0.2.0` and `v0.2.1`, pinned Rust1.87.0 and the
checksum-verified official archive. No external dependency enters Cargo. Clear
Rust/Cargo profile overrides and build the same public adapter in each checkout:

```sh
cargo build --offline --locked --release --example decoder_bench
```

First generate full source-bound conformance reports in both checkouts with the
existing `tools/check_conformance.py`. Run the candidate comparison tool with
explicit binaries, source roots and those reports:

```sh
python3 tools/decoder_change.py \
  --before /path/to/before/target/release/examples/decoder_bench \
  --after /path/to/after/target/release/examples/decoder_bench \
  --before-source /path/to/before --after-source /path/to/after \
  --before-conformance /path/to/before-conformance.json \
  --after-conformance /path/to/after-conformance.json \
  --archive /path/to/opus_testvectors-rfc8251.tar.gz \
  --output /new/path/comparison --rounds 15 --cpu 0
python3 tools/decoder_change.py --check /new/path/comparison/report.json
```

The tool verifies the archive and all120 source-bound conformance PCM hashes
before measuring. It compares the complete20075-packet corpus at all ten output
geometries in15 paired rounds. Case and implementation order are randomized
with seed6716. Each process validates output, warms up once, times one full
replay, separately measures call latency and revalidates output. Only the codec
replay's adapter timing is compared; file I/O, fingerprinting and profiler
instrumentation are outside it. Both implementations return the same owning PCM
API object. Every retained timed sample must exceed200ms. Each run checks full
PCM/range fingerprints, duration and mode geometry, plus binary/source identity.

The ratio is candidate median codec time divided by baseline median codec time.
Equal-tail95% intervals use2000 paired bootstrap resamples. The aggregate gives
each geometry equal weight and resamples the same round indices across cases.
All samples, including slow ones, are retained. CPU affinity is fixed; frequency,
thermals and other activity are not controlled. This is a before/after change
measurement, not an updated comparison against native libopus or other codecs.
It does not measure encoder speed or representative perceptual quality.

## Correctness checkpoint

All seven offline gates pass (172 library and41 Python tests). Retained
[correctness evidence](../validation/results/decoder-cwrs-0.2.1-correctness.json)
and [full conformance](../validation/results/decoder-cwrs-0.2.1-conformance.json)
record exact source identity and the pre-commit provenance. The codec and public
adapter sources used for the final measurement match this verified source.
The measurement driver is the exact version retained in commit `5345d83`;
a later checker-only change additionally rejects absent/changed hardware and
compiler metadata. The original generated report passes that stricter checker.
No measured codec code, binary or sample was changed by this follow-up.

## Retained results

The complete15-round run retains300 samples on AMD Ryzen AI 9 HX 370 w/ Radeon 890M,
7.0.0-38-generic (x86_64), pinned CPU0. The equal-case
geometric mean of candidate/baseline median-time ratios is **0.9639**, with
paired95% interval **[0.9455,0.9791]**: **3.6% less codec time** in this aggregate.
The preliminary debug-symbol pilot's approximately10% reduction is superseded.

All ten per-case point estimates are lower, but every per-case interval includes
1.0. Individual configuration wins are therefore unproven. The upper bounds for
12kHz mono and16kHz stereo exceed1.05, so a5% per-case regression ceiling is also
not established. These uncertainties remain visible and must not be replaced by
the aggregate result. No native/best-competitor or SOTA target is met by a
before/after change measurement.

Median codec microseconds per packet across15 samples; each sample replays all
20075 official packets. All PCM/range fingerprints and durations match, the
120-case conformance SHA256 matrix is unchanged, and decoder state remains34520
bytes in both binaries. This is not a peak-memory or allocation measurement.

| Output Hz | Channels |0.2.0 µs|0.2.1 µs|Ratio|95% interval|
|---|---:|---:|---:|---:|---|
|8000|1|35.10|33.01|0.940|[0.908,1.012]|
|8000|2|39.26|38.62|0.984|[0.928,1.030]|
|12000|1|35.04|33.31|0.950|[0.911,1.068]|
|12000|2|40.67|39.56|0.973|[0.897,1.004]|
|16000|1|33.71|33.27|0.987|[0.954,1.030]|
|16000|2|39.59|39.01|0.985|[0.907,1.052]|
|24000|1|34.38|33.37|0.971|[0.898,1.011]|
|24000|2|41.21|38.59|0.936|[0.882,1.034]|
|48000|1|34.23|32.56|0.951|[0.891,1.032]|
|48000|2|39.93|38.48|0.964|[0.914,1.009]|

The [complete generated report](results/decoder-cwrs-0.2.1.json) retains raw
samples, separate call-latency quantiles, hardware/compiler identity, the paired
schedule and source/binary/driver/corpus hashes. Baseline codec source is
`b05cb7451a256b61b9f3964137553027561ef087` (`v0.2.0`); candidate codec source is
`5345d83` with the same Rust/Cargo/adapter contents as the final `v0.2.1` release.
The optional comparison tool and report-checking tests are the only code updated
after measurement. No other build, test or fixture job ran during the retained
timing phase. Frequency and CPU exclusivity remain uncontrolled as disclosed.
