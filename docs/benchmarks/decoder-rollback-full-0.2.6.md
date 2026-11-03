# Full-snapshot rollback: unaccepted 0.2.6 candidate versus 0.2.5

Unified normal/PLC/FEC calls now retain one local snapshot and render under a
small private rollback guard. Full success leaves state in place; errors and
unwinding restore every owner field. This avoids copying a successful working
owner back. Public owning PCM output, layout, validation, integer arithmetic,
gain and transition ordering remain unchanged. The [transaction contract](../validation/decoder-rollback.md)
describes the exclusive-borrow boundary and failure-path tradeoff.

The lead is the [0.2.5 diagnostic stack study](decoder-profile-0.2.5.md).
Its observed copy call sites justify an experiment, not a predicted gain or
CPU cost. Errors now restore a snapshot rather than discard a working clone;
this normal-packet workload does not measure malformed-error or explicit
caller-requested PLC/FEC throughput. No peak-memory claim follows.

## Correctness and provenance

All seven offline gates pass at codec commit `ed8e6f0` (184 library tests in
debug/release, 44 Python tests). All 120 official conformance cases pass with
every 0.2.5 PCM SHA256 unchanged and encoded nonzero final ranges verified.
Native normal/gain/long/PLC/FEC/transition and multistream corpora pass. Twelve
transaction tests include three new post-complete-render error/unwind/resume
cases. Three detected/restored mutations cover omitted restoration, premature
disarm before a real rendering error, and failure to commit success. See the
[correctness record](../validation/results/decoder-rollback-full-0.2.6-correctness.json)
and [conformance report](../validation/results/decoder-rollback-full-0.2.6-conformance.json).

Baseline codec source is `c9d3ef1` (`v0.2.5`); subsequent `c8a006e` reporting
changes no codec, Cargo or adapter content. Candidate codec source is `ed8e6f0`.
Both public decoder adapters use Rust 1.87.0 with the same ordinary release
profile and no Rust flags or Cargo profile overrides. Both sides use the public
fixed-storage owning PCM API, including complete-owner error transactions.

The first owning-Option guard prototype had no demonstrated pilot benefit
(point ratio 1.0119). Its source/binary identities and all seven pilot pairs
remain local. The revised guard borrows one ordinary local snapshot and holds
only references plus a commit flag. Its exploratory pilot point ratio 0.9749
is also not release evidence. The final complete run must decide acceptance;
no samples from either pilot are mixed into it.

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
candidate/baseline median-time ratio is **0.9880**, with paired 95% interval
**[0.9772, 1.0041]**. The 1.2% lower point estimate does not demonstrate an
aggregate improvement. This full-snapshot candidate is not accepted for release
and has no version tag. A subsequent change needs new source-bound evidence;
this result must not be rerun unchanged until it happens to pass.

Only 24kHz mono has an individual interval below 1; the other nine include 1.
8kHz stereo has a slower point estimate (2.4%). Upper bounds exceed 1.05 at
8kHz stereo, 12kHz mono (exact upper 1.0500272509) and 24kHz stereo. The modest
point estimates must not hide these uncertainties. All losing/uncertain data
are retained.

Measured on AMD Ryzen AI 9 HX 370 w/ Radeon 890M, x86_64,
7.0.0-38-generic, CPU0. All samples use one full pass and the minimum retained
sample is 504.899254ms. Both public decoder owners are 34520 bytes; no peak
stack/heap/RSS or malformed-error throughput claim follows.

Median codec microseconds per packet across 15 samples per implementation;
each sample replays all 20075 official packets. Lower ratio is faster.

| Output Hz | Channels |0.2.5 µs|Candidate µs|Ratio|95% interval|
|---|---:|---:|---:|---:|---|
|8000|1|25.77|25.71|0.9979|[0.98600,1.02567]|
|8000|2|31.01|31.74|1.0237|[0.96734,1.05370]|
|12000|1|25.97|25.65|0.9876|[0.96055,1.05003]|
|12000|2|30.86|30.69|0.9945|[0.98327,1.04669]|
|16000|1|26.85|26.44|0.9846|[0.94816,1.03120]|
|16000|2|30.87|30.82|0.9983|[0.98382,1.04406]|
|24000|1|27.44|25.79|0.9396|[0.88700,0.99009]|
|24000|2|31.16|30.90|0.9915|[0.93838,1.09582]|
|48000|1|27.39|26.62|0.9719|[0.94563,1.01965]|
|48000|2|31.49|31.26|0.9927|[0.84956,1.04036]|

The [full generated report](results/decoder-rollback-full-0.2.6.json) retains
all samples and provenance. Source/binary/driver bindings were independently
rechecked before archival. Codec/Cargo/adapter contents are exactly `ed8e6f0`;
later changes to this experiment only add Markdown/JSON reporting. No build,
test or fixture work ran during retained timing. The native comparison remains
unrefreshed; no cumulative, native, quality, memory, portability or SOTA claim
follows from this inconclusive experiment.
