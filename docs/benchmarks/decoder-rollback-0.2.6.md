# Selective rollback: unaccepted 0.2.6 candidate versus 0.2.5

This experiment leaves successful unified decoder state in place under a
private borrowing rollback guard. Normal packets snapshot CELT plus complete
top-level metadata only when packet mode and previous actual mode are both
CELT; source-path audit and mutation tests cover the absence of SILK writes.
Other normal contexts, explicit PLC and FEC retain a full-owner snapshot.
Errors and unwinding restore all observable state. No public API/layout,
integer arithmetic, validation, dependency or unsafe policy changes.

The earlier [full-snapshot experiment](decoder-rollback-full-0.2.6.md) was
inconclusive. Its source and all measurements remain separate. This revised
comparison measures directly against stable 0.2.5; no ratios or pilot samples
are combined. The selective seven-pair stereo pilot ratio 0.9643 was only an
exploratory signal and is not release evidence.

## Correctness and provenance

Baseline codec source is `c9d3ef1` (`v0.2.5`); candidate source is `461081e`.
Both adapters use pinned Rust 1.87.0 and the same ordinary release profile,
without Rust flags or Cargo profile overrides. Source, binary, driver and
archive SHA256 bindings were independently rechecked after the complete run.
No builds, tests, fixture generation or measured source/binary edits ran during
timing; later Markdown/JSON reporting does not alter measured codec content.

All seven offline gates pass (185 library tests in debug/release, 44 Python).
All 120 official cases pass with every 0.2.5 PCM hash unchanged. Native
normal/gain/long/PLC/FEC/transition and multistream corpora pass. Thirteen
transaction tests include four new post-render error/unwind/resume tests;
five restored mutations cover restoration, premature/absent disarm, unsafe
snapshot selection and missing metadata restoration. See the experimental
[contract](../validation/decoder-rollback.md), [correctness record](../validation/results/decoder-rollback-0.2.6-correctness.json)
and [conformance report](../validation/results/decoder-rollback-0.2.6-conformance.json).

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
candidate/baseline median-time ratio is **0.9916**, with paired 95% interval
**[0.9739,1.0075]**. The 0.8% lower point estimate does not establish aggregate
benefit. This implementation is not accepted into production, released or
tagged. Do not rerun this unchanged experiment to seek significance.

Only 8kHz mono/stereo and 48kHz mono have intervals below 1. The other seven
include 1. Point estimates are slower at 12kHz stereo (1.8%) and 16kHz stereo
(7.1%); upper bounds exceed 1.05 at 12kHz mono, 16kHz stereo and 24kHz stereo.
These uncertainties and possible regressions reinforce the rejection.

Measured on AMD Ryzen AI 9 HX 370 w/ Radeon 890M, x86_64 Linux
7.0.0-38-generic, CPU0. The minimum retained sample is 506.402438ms. Both
public decoder owners remain 34520 bytes; this is not peak-stack/heap/RSS
evidence. Error restoration adds a copy on failure; malformed-error and
explicit PLC/FEC throughput are not measured by this normal-packet workload.

Median codec microseconds per packet over 15 samples per implementation;
each sample replays all 20075 official packets. Lower ratio is faster.

| Output Hz | Channels |0.2.5 µs|Candidate µs|Ratio|95% interval|
|---|---:|---:|---:|---:|---|
|8000|1|25.94|25.39|0.9788|[0.95749,0.99769]|
|8000|2|30.99|30.24|0.9760|[0.88022,0.98816]|
|12000|1|25.95|25.78|0.9936|[0.96359,1.06723]|
|12000|2|30.71|31.25|1.0178|[0.97970,1.03647]|
|16000|1|26.53|25.85|0.9743|[0.96256,1.04740]|
|16000|2|31.18|33.38|1.0705|[0.98187,1.12754]|
|24000|1|26.08|25.90|0.9929|[0.96742,1.01236]|
|24000|2|30.86|30.64|0.9926|[0.96981,1.10853]|
|48000|1|27.25|25.86|0.9490|[0.87475,0.98593]|
|48000|2|32.20|31.40|0.9751|[0.92553,1.00319]|

The [complete report](results/decoder-rollback-0.2.6.json) preserves all samples,
including uncertain and slower cases. The latest full native comparison
remains 0.2.3; this experiment provides no native/other-competitor, cumulative,
encoder quality/speed, resource, portability or SOTA superiority claim.
