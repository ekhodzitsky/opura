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

All seven offline gates pass (172 library and40 Python tests). Retained
[correctness evidence](../validation/results/decoder-cwrs-0.2.1-correctness.json)
and [full conformance](../validation/results/decoder-cwrs-0.2.1-conformance.json)
record exact source identity and the pre-commit provenance. The final paired
ordinary-release measurement is pending; no final speed result is claimed by
this checkpoint.
