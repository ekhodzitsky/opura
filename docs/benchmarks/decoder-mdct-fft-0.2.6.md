# Fused inverse-MDCT FFT preparation: unaccepted candidate versus 0.2.5

This experiment writes inverse-MDCT prerotation directly into the FFT plan's
bit-reversed buffer, avoiding a separate natural-order array and permutation
pass. It follows the pinned native inverse-MDCT preparation order. Public FFT/
MDCT APIs, signed wrapping/headroom/downscale, overlap, owned TimeFrame and
complete public owner transactions remain unchanged. The 1020-element inverse
workspace and remaining output copies stay. No unsafe, heap allocation,
dependency, public layout or encoder policy change is introduced. Forward MDCT
and public scaled/inverse FFT preparation are unchanged.

Baseline codec source is `c9d3ef1` (`v0.2.5`); candidate source is `99312a0`.
Both public adapters use pinned Rust 1.87.0 and the same ordinary release
profile, with no Rust flags or Cargo profile overrides. Neither rejected
rollback nor owned-MDCT-output code is included. The source lead is pinned
pristine Opus 1.6.1 inverse prerotation plus the validated 0.2.5 diagnostic
study, which does not predict CPU fractions or gains.

## Correctness and provenance

All seven offline gates pass (183 library tests in debug/release, 44 Python).
Two new private FFT tests check 1536 independent native direct-forward cases
across all four plans and shifts 0..31, with adjacent canaries and exact-length/
shift validation before mutation. Existing 1632 public FFT, 398 MDCT and 480
synthesis cases pass, along with 1680 native complete-decoder operations,
multistream fixtures and all nine late synthesis/PVQ transaction tests.
Four restored runtime mutations catch omitted length/shift validation, ignored
requested shift and wrong inverse-MDCT permutation.

All 120 official cases and encoded final ranges pass, with every 0.2.5 PCM hash
unchanged. See the experimental [contract](../validation/mdct-fft-preparation.md),
[correctness record](../validation/results/decoder-mdct-fft-0.2.6-correctness.json)
and [conformance report](../validation/results/decoder-mdct-fft-0.2.6-conformance.json).
Source/binary/driver/archive and correctness-file bindings were independently
verified after collection. No builds, tests, fixture jobs or measured source/
binary edits ran during timing. Later Markdown/JSON reporting does not alter
the measured codec content.

The exploratory seven-pair 48kHz stereo pilot ratio 1.0111 had no demonstrated
benefit. Its samples remain separate and are not part of the complete run.
No ratios from different experiments or historical versions are combined.

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
candidate/baseline median-time ratio is **1.0116**, paired 95% interval
**[0.9844,1.0354]**. The 1.2% slower point estimate does not establish a benefit
or a slowdown. Every individual interval includes 1. The implementation is
rejected for release; no production adoption or 0.2.6 tag follows. Do not repeat
the unchanged experiment in search of significance.

The 24kHz stereo point estimate is 6.5% slower. Nine per-case upper bounds
exceed 1.05; only 16kHz stereo has an upper bound below that ceiling. All slower
and uncertain results remain visible. This experiment does not establish
individual wins or a per-case regression ceiling.

Measured on AMD Ryzen AI 9 HX 370 w/ Radeon 890M, x86_64 Linux
7.0.0-38-generic, CPU 0. The minimum retained sample is 508.450316ms. Both
public decoder owners are 34520 bytes; no peak-stack/heap/RSS claim follows.
Explicit caller-requested PLC/FEC and malformed-error throughput are not
measured by this normal-packet workload.

Median codec microseconds per packet over 15 samples per implementation;
each sample replays all 20075 official packets. Lower ratio is faster.

| Output Hz | Channels |0.2.5 µs|Candidate µs|Ratio|95% interval|
|---|---:|---:|---:|---:|---|
|8000|1|26.98|26.64|0.9873|[0.93081,1.09677]|
|8000|2|31.06|31.67|1.0195|[0.84250,1.06736]|
|12000|1|26.76|26.48|0.9896|[0.96359,1.07798]|
|12000|2|31.17|31.61|1.0141|[0.92823,1.13212]|
|16000|1|26.48|27.29|1.0305|[0.87713,1.15904]|
|16000|2|31.03|31.01|0.9991|[0.97570,1.03210]|
|24000|1|26.13|26.67|1.0209|[0.97899,1.06808]|
|24000|2|31.10|33.12|1.0649|[0.96551,1.20894]|
|48000|1|27.42|27.94|1.0189|[0.85420,1.09946]|
|48000|2|33.02|32.16|0.9737|[0.90494,1.05541]|

The [complete report](results/decoder-mdct-fft-0.2.6.json) retains every sample
and provenance. Native/best-available/other-competitor, cumulative, encoder
quality/speed, resource, portability and SOTA superiority remain unestablished.
