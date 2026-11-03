# Private MDCT destinations: unaccepted 0.2.6 candidate versus 0.2.5

This experiment renders inverse-MDCT output directly into the CELT synthesis
owner's unpublished time blocks. It avoids an intermediate owned 960-element i32
TimeFrame return and the subsequent active sample copy. Public owned MDCT/API,
1020-element inverse workspace, FFT preparation, exact wrapping/headroom/window
arithmetic, overlap and all public owner transactions remain unchanged.
No unsafe, heap allocation, dependency or encoder policy change is introduced.

Baseline codec source is `c9d3ef1` (`v0.2.5`); candidate source is `cd02054`.
Both public adapters use pinned Rust 1.87.0 with the same ordinary release
profile, no Rust flags or Cargo profile overrides. This experiment starts from
stable 0.2.5 code and does not include the rejected rollback implementations.
The lead comes from source audit and validated0.2.5 diagnostic stack snapshots;
those snapshots are not CPU cost fractions or speed evidence.

## Correctness and provenance

All seven offline gates pass (183 library tests in debug/release,44 Python).
Two new private-output tests check all 398 independent native chained MDCT cases
and adjacent destination canaries, invalid geometry/unchanged output/overlap,
frame-size changes, resets and resume. Existing 480 native synthesis cases,
1680 native full-decoder operations and multistream fixtures pass. All nine
late synthesis/PVQ transaction tests pass. Three restored runtime mutations
catch missing destination-length validation, a missing final output sample
and repeated writes to the first transient block.

All 120 official cases and encoded final ranges pass, with every 0.2.5 PCM SHA256
unchanged. See the experimental [contract](../validation/mdct-output-transactions.md),
[correctness record](../validation/results/decoder-mdct-0.2.6-correctness.json)
and [conformance report](../validation/results/decoder-mdct-0.2.6-conformance.json).
Source/binary/driver/archive and correctness-file hashes were independently
verified after collection. No build, test, fixture or measured-source/binary
edit ran during retained timing. Later Markdown/JSON reporting does not change
the measured codec content.

The exploratory seven-pair 48kHz stereo pilot ratio 0.9676 is not final evidence.
None of its samples is included in this complete comparison. Its favorable
point estimate does not override the final result or justify another unchanged
run. No ratios from separate experiments are combined.

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

## Inconclusive result; no release

All 300 samples complete and pass the structural checker. The aggregate
candidate/baseline median-time ratio is **1.0027**, paired 95% interval
**[0.9745,1.0345]**. The 0.3% slower point estimate does not establish a benefit
or a slowdown. Every individual interval includes 1. This candidate is rejected
for release; no production adoption or 0.2.6 tag follows. Do not repeat the
unchanged experiment in search of significance.

The 16kHz mono point estimate is 5.4% slower. Upper bounds exceed 1.05 at 8kHz mono,
12kHz mono,16kHz mono,24kHz mono and24kHz stereo. These uncertain and possibly
slower cases are all retained, rather than hidden by the aggregate.

Measured on AMD Ryzen AI 9 HX 370 w/ Radeon 890M, x86_64 Linux 7.0.0-38-generic,
CPU 0. The minimum retained sample is 505.693738ms. Both decoder owners remain
34520 bytes; this is not peak-stack/heap/RSS or allocation evidence. The explicit
caller-requested PLC/FEC and malformed-error throughput are not measured here.

Median codec microseconds per packet over 15 samples per implementation;
each sample replays all 20075 official packets. Lower ratio is faster.

| Output Hz | Channels |0.2.5 µs|Candidate µs|Ratio|95% interval|
|---|---:|---:|---:|---:|---|
|8000|1|27.26|27.69|1.0157|[0.93862,1.21110]|
|8000|2|31.15|31.89|1.0237|[0.95131,1.04540]|
|12000|1|27.43|26.61|0.9702|[0.92967,1.05847]|
|12000|2|32.39|31.64|0.9768|[0.93173,1.01228]|
|16000|1|27.72|29.23|1.0542|[0.93188,1.17790]|
|16000|2|32.47|31.28|0.9633|[0.94428,1.01974]|
|24000|1|26.31|26.71|1.0151|[0.90982,1.19936]|
|24000|2|31.14|30.95|0.9941|[0.95861,1.05306]|
|48000|1|26.70|27.24|1.0204|[0.96746,1.03190]|
|48000|2|31.73|31.62|0.9966|[0.96902,1.02786]|

The [complete report](results/decoder-mdct-0.2.6.json) preserves all samples and
provenance. Native/best-available/other-competitor, cumulative, encoder quality/
speed, resource, portability and SOTA superiority remain unestablished.
