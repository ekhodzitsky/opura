# CELT backend staging: 0.2.4 candidate versus 0.2.3

This change removes repeated CELT backend cloning inside an already staged
decoder owner. Normal synthesis and noise/pitch concealment use private
in-place rendering. Public synthesis and complete CELT/Hybrid/Opus decoder
transactions retain rollback of every history after errors. The
[ownership and fault contract](../validation/celt-synthesis-transactions.md)
describes the audited callers and failure boundary.

The optimization lead is a sparse clock profile of the measured 0.2.3 Rust
binary. Its copy samples identify candidates for investigation; they do not
quantify saved bytes, expected gain or peak memory. Other buffers and complete
decoder staging remain present.

## Correctness and provenance

All seven offline gates pass at candidate codec commit `bbdc532`, including
176 library tests in debug/release and 44 Python tests. Every one of the 120
official conformance PCM SHA256 hashes is unchanged from 0.2.3, and encoded
nonzero final ranges pass. Native fixtures check 480 complete synthesis/history
cases and 1680 unified decoder operations across rate, channels, signed gain,
long packets, PLC, prefixed FEC, transitions and reset. Four late-failure unit
tests and three detected/restored source mutations protect the changed rollback
boundary. See the [correctness record](../validation/results/decoder-staging-0.2.4-correctness.json)
and [conformance report](../validation/results/decoder-staging-0.2.4-conformance.json).

Baseline codec source is `51671564ad3ea5c668b819af2be991848d6ae858` (`v0.2.3`).
Candidate codec source is `bbdc532`. Both public decoder adapters use pinned
Rust 1.87.0 with the same ordinary release profile, no Rust flags or Cargo
profile overrides. The API returns owning fixed-storage PCM on both sides.
No relaxed transaction or alternative benchmark-only decoder path is used.

## Reproduction and interpretation

Build the public adapter and create a complete source-bound conformance report
in clean checkouts of the two milestones:

```sh
cargo build --offline --locked --release --example decoder_bench
python3 tools/check_conformance.py /path/to/opus \
  /path/to/opus_testvectors-rfc8251.tar.gz --report /new/path/conformance.json
```

Run the strict paired comparison with new output paths:

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

The checksum-verified official corpus supplies 20075 packets and 320.635 seconds
of audio per pass. Five output rates and mono/stereo form ten equally weighted
cases. Fifteen paired rounds retain all 300 samples. Case/implementation order
uses seed 6716; no slow samples are discarded. Each process validates output,
warms up, times one full pass, records separate call latency and revalidates.
Every retained timing sample must exceed 200ms. Complete PCM/range/duration/mode
fingerprints and source/binary identities must match throughout the run.

Ratios divide candidate median codec time by baseline median codec time.
Equal-tail 95% paired bootstrap intervals use 2000 resamples. The aggregate
geometric mean gives equal weight to cases, with the same resampled round
indices across cases. CPU affinity is pinned; thermals, frequency and background
activity are not controlled. No other build, test or fixture job runs during
retained timing. Intervals describe observed run variation, not universal
hardware guarantees.

This version-to-version comparison does not refresh the native competitor
baseline, establish cumulative gains from earlier separate runs, measure
explicit PLC/FEC throughput, or prove encoder quality/speed or SOTA leadership.
Owner bytes exclude returned PCM, transient stack, heap and RSS. The exploratory
seven-pair 48kHz stereo pilot is not final evidence.

## Retained results

The complete 300-sample, 15-paired-round run passes the structural checker.
The equal-case aggregate candidate/baseline median-time ratio is **0.9798**,
with paired 95% interval **[0.9548, 0.9981]**: **2.0% less codec time** in this
aggregate. The upper bound is close to 1; the modest result should be interpreted
with the uncontrolled background/frequency limitations above. The exploratory
pilot is superseded by this complete result.

Only the 48kHz stereo interval is entirely below 1 (point reduction 12.7%).
All other nine intervals include 1, so their individual wins are unproven.
The 8kHz stereo and 24kHz stereo point estimates are slower by 0.4% and 2.3%.
The upper bounds exceed 1.05 at 16kHz mono, 24kHz mono/stereo and 48kHz mono;
the 5% per-case regression ceiling is not established. Retaining this modest
aggregate improvement does not turn these uncertain cases into wins.

Measured on AMD Ryzen AI 9 HX 370 w/ Radeon 890M, x86_64,
7.0.0-38-generic, CPU0. Every sample uses one full pass; the shortest retained
sample is 540.705058ms, above the required 200ms. Both decoder owners remain
34520 bytes. Peak stack/heap/RSS are unmeasured.

Median codec microseconds per packet across 15 samples per implementation;
each sample replays all 20075 official packets. Lower ratio is faster.

| Output Hz | Channels |0.2.3 µs|0.2.4 µs|Ratio|95% interval|
|---|---:|---:|---:|---:|---|
|8000|1|29.15|29.06|0.997|[0.960,1.017]|
|8000|2|34.76|34.90|1.004|[0.901,1.041]|
|12000|1|29.70|29.22|0.984|[0.922,1.016]|
|12000|2|35.58|34.76|0.977|[0.910,1.025]|
|16000|1|30.05|29.94|0.996|[0.951,1.077]|
|16000|2|34.86|34.05|0.977|[0.871,1.049]|
|24000|1|29.94|29.64|0.990|[0.968,1.083]|
|24000|2|35.23|36.04|1.023|[0.899,1.074]|
|48000|1|30.02|29.60|0.986|[0.937,1.091]|
|48000|2|40.76|35.58|0.873|[0.819,0.980]|

The [complete generated report](results/decoder-staging-0.2.4.json) retains
all raw samples, separate initialization/call-latency observations, randomized
schedule and hardware/compiler/source/binary/driver/corpus hashes. Final
source/binary/driver bindings and all 120 conformance hashes were independently
rechecked before retention. Measured codec, Cargo and adapter contents are
identical to commit `bbdc532`; subsequent reporting changes only Markdown/JSON.
No build/test/fixture job ran during retained timing.

This result does not refresh the losing 0.2.3 scalar native comparison or combine
it with this ratio into a new native claim. Best-available/other competitor,
quality, resource and portability acceptance remain open. No SOTA or full-goal
completion follows.
