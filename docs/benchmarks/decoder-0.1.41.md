# Decoder baseline: Opura 0.1.41

The complete packet-to-PCM decoder is slower than the pinned scalar fixed-point
libopus reference in every measured output configuration. The equal-weight
aggregate ratio has a one-sided 95% bootstrap upper bound of **1.654**, above
the 0.900 target. Every case also exceeds the 1.050 regression ceiling.
This is a baseline for improvement; decoder or whole-codec leadership is not established.

## Results

Median nanoseconds per packet across 15 paired rounds; lower is better.
The ratio divides the two medians. All ten configurations are retained.

| Output Hz | Channels | Opura ns | Reference ns | Ratio |
|---:|---:|---:|---:|---:|
| 8000 | 1 | 28576.7 | 16459.9 | 1.736 |
| 8000 | 2 | 33185.2 | 20864.5 | 1.591 |
| 12000 | 1 | 28918.7 | 16707.4 | 1.731 |
| 12000 | 2 | 33485.7 | 21213.5 | 1.579 |
| 16000 | 1 | 29303.1 | 16307.9 | 1.797 |
| 16000 | 2 | 33744.8 | 20695.8 | 1.631 |
| 24000 | 1 | 28803.3 | 17091.8 | 1.685 |
| 24000 | 2 | 33565.2 | 21830.0 | 1.538 |
| 48000 | 1 | 29183.5 | 17476.9 | 1.670 |
| 48000 | 2 | 34058.0 | 21493.6 | 1.585 |

Decoder state occupies 34,520 bytes in Opura and 18,404/27,124 bytes in the
mono/stereo reference. This excludes stack temporaries, returned PCM, executable
code, benchmark storage and process memory; it is not a peak-memory measurement.

The latency pass separately times individual public decode calls. Below are
medians of per-pass quantiles at 48 kHz stereo, in microseconds. All quantiles,
maxima and mode totals for all configurations remain in the raw report.

| Implementation | p50 | p95 | p99 |
|---|---:|---:|---:|
| opura | 20.63 | 122.87 | 202.19 |
| libopus | 14.51 | 69.41 | 126.53 |

## Correctness and workload

Each pass replays all 12 verified official RFC8251 streams: 20,075 packets and
320.635 seconds of audio per output configuration. Both adapters preserve state
within a stream and reset between streams. Every encoded nonzero final range
and a fingerprint over all returned PCM, sample counts and final ranges agree.
Before sampling, the current decoder independently reproduces all 120 accepted
[conformance PCM SHA256 hashes](../validation/decoder-0.1.40.md). The lightweight
per-process fingerprint is a regression check, not a cryptographic proof.

The corpus contains 4,526 SILK, 4,491 Hybrid and 11,058 CELT TOCs. Latency mode
buckets describe the signaled TOC, including transitions and implicit loss/DTX;
they do not isolate component CPU time. Dedicated explicit PLC/FEC throughput
workloads have not been measured.

## Protocol and provenance

Measured on AMD Ryzen AI 9 HX 370, Linux x86-64, CPU 0, performance governor
with amd-pstate-epp. CPU frequency, thermals and other system activity were not
controlled, and the CPU was not reserved exclusively. Intervals reflect paired
run variation, not systematic bias. No observations or losing cases were removed.

Rust 1.87.0 / LLVM 20.1.1 uses the normal release profile and generic host target,
with external Rust flag overrides cleared. GCC 15.2.0 compiles unchanged external
libopus 1.6.1 revision `22244de5a79bd1d6d623c32e72bf1954b56235be` using:

```text
-O3 -fwrapv -DVAR_ARRAYS -DOPUS_BUILD -DFIXED_POINT -DDISABLE_FLOAT_API
```

The reference uses scalar dispatch and fast64 fixed-point arithmetic. Compiler
auto-vectorization is allowed for both implementations. Best-available native
SIMD, floating-point builds, other competitors, architectures and compilers
remain unmeasured. GCC reports two upstream array-bound diagnostics in
`silk/decode_indices.c`; the source and diagnostics are not suppressed.

Both adapters preload inputs and initialize decoders outside timing. Each child
performs correctness checks and one full warmup pass before a timed replay.
The 200 ms calibration selects one full pass for every case. Timing includes
stream resets and public decode/result observation, but excludes file I/O,
allocation, process startup, fingerprint calculation and the separate latency
pass. Order is shuffled with seed 6716 for 15 paired rounds on one pinned CPU.
Bounds use 2,000 paired bootstrap resamples, seed 6716, equal-weight geometric
mean across configurations and the same ratio thresholds as the framing gate.

[Raw results](results/decoder-0.1.41.json) retain every sample, calibration, latency
quantile, mode count/total, decoder size, randomized order, hardware snapshots,
compiler settings and source/binary/corpus hashes. The base revision is 0.1.40;
the source hash identifies the measured 0.1.41 implementation and harness:

`967d3d6148ae6cc16bcd136bb407e5005bbf33596301dedd6be3968d06fce095`

## Reproduce

With the pinned external source checkout, official archive, Linux, Python, GCC
and the pinned Rust toolchain already available, no network is needed:

```sh
python3 tools/decoder_bench.py /path/to/opus-v1.6.1 /path/to/opus_testvectors-rfc8251.tar.gz --out target/decoder-new.json
python3 tools/decoder_bench.py --check target/decoder-new.json
python3 -m unittest discover -s tools -p 'test_*.py'
```

The runner rejects altered source/archive inputs, verifies hashes and refuses
to overwrite reports. A completed measurement returns success even when speed
targets fail; `--check` returns failure for this baseline, as intended. Foreign
reference code is compiled only into an optional external executable; no build
or test dependency is added to Cargo. Ordinary correctness gates do not depend
on machine timing or the optional archive.
