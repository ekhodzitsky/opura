# PVQ partition output: 0.2.5 candidate versus 0.2.4

Positive-pulse CELT PVQ leaves now reconstruct directly into their unpublished
partition output. This avoids returning an owning normalized-band array and
copying its active coordinates. Public owning output, entropy/budget/owner
transactions, exact Q24 arithmetic, spreading and full 16-bit collapse masks
remain unchanged. The [ownership and fault contract](../validation/pvq-output-transactions.md)
describes the private caller boundary and independent native coverage.

The lead is source inspection plus sparse copy attribution in a historical
0.2.3 clock profile. This does not quantify current copy cost or expected gain.
Pulse vectors and other temporary buffers remain; no peak-memory claim follows.

## Correctness and provenance

All seven offline gates pass at candidate codec commit `0f23d33`, including
181 library tests in debug/release and 44 Python tests. All 120 official
conformance cases pass with every 0.2.4 PCM SHA256 unchanged and encoded nonzero
final ranges verified. Native corpora check 28464 decoded PVQ bands, 196608
refined roots, partition/band/spectrum/encoder output and 1680 complete-decoder
gain/long-packet/PLC/FEC/transition/reset operations. Five post-write fault tests
and four detected/restored source mutations protect the changed transaction
boundary. See the [correctness record](../validation/results/decoder-pvq-0.2.5-correctness.json)
and [conformance report](../validation/results/decoder-pvq-0.2.5-conformance.json).

Baseline codec source is `330df5d8f5c29664a79895304df883fa5b4bafd5` (`v0.2.4`).
Candidate codec source is `0f23d33`. Both public decoder adapters use pinned
Rust 1.87.0 with the same ordinary release profile, no Rust flags or Cargo
profile overrides. Both sides use the public fixed-storage owning PCM API;
no relaxed transaction or benchmark-only decoder path is used.

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
The equal-case aggregate candidate/baseline median-time ratio is **0.9554**,
with paired 95% interval **[0.9266, 0.9702]**: **4.5% less codec time** in this
aggregate. The exploratory seven-pair pilot is superseded by the full result.

All ten case point estimates are lower. Six individual intervals exclude 1:
8kHz stereo, 12kHz mono, 16kHz stereo, 24kHz mono/stereo and 48kHz stereo.
The remaining four intervals include 1, so their individual wins are unproven.
Every individual upper bound is below 1.05 under the retained per-case protocol;
these percentile intervals do not give a simultaneous family-wise confidence
bound or guarantee performance on other hardware/workloads.

Measured on AMD Ryzen AI 9 HX 370 w/ Radeon 890M, x86_64,
7.0.0-38-generic, CPU0. Every sample uses one full pass; the shortest retained
sample is 511.092508ms, above the required 200ms. Both public decoder owners
remain 34520 bytes. Peak stack/heap/RSS are unmeasured.

Median codec microseconds per packet across 15 samples per implementation;
each sample replays all 20075 official packets. Lower ratio is faster.

| Output Hz | Channels |0.2.4 µs|0.2.5 µs|Ratio|95% interval|
|---|---:|---:|---:|---:|---|
|8000|1|27.21|26.49|0.973|[0.927,1.011]|
|8000|2|33.04|30.92|0.936|[0.826,0.963]|
|12000|1|28.20|26.31|0.933|[0.911,0.972]|
|12000|2|32.46|31.14|0.959|[0.921,1.045]|
|16000|1|26.82|26.57|0.991|[0.961,1.003]|
|16000|2|31.68|31.10|0.982|[0.935,0.993]|
|24000|1|28.11|26.56|0.945|[0.824,0.979]|
|24000|2|33.10|31.63|0.955|[0.932,0.989]|
|48000|1|28.10|27.26|0.970|[0.902,1.017]|
|48000|2|34.21|31.21|0.912|[0.831,0.952]|

The [complete generated report](results/decoder-pvq-0.2.5.json) retains every raw
sample, separate initialization/call-latency observations, randomized schedule
and hardware/compiler/source/binary/driver/corpus hashes. Final source/binary/
driver bindings, validated source-file hashes and all 120 conformance hashes
were independently rechecked before retention. Measured codec, Cargo and
adapter contents are identical to commit `0f23d33`; subsequent reporting changes
only Markdown/JSON. No build/test/fixture job ran during retained timing.

This does not refresh the losing 0.2.3 scalar native comparison or combine
separate version-to-version ratios into a new native or cumulative claim.
Best-available/other competitor, encoder quality/speed, resource and portability
acceptance remain open. No SOTA or full-goal completion follows.
