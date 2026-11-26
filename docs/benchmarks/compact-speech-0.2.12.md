# Compact decoder CPU on SILK speech

Opura 0.2.12 used **16.5% less thread CPU** with one compact decoder than the same API in 0.2.11 on this 16 kHz speech replay. The paired 95% interval for the saving was 14.2–17.9%. At 768 active speech streams the worker used **17.9% less CPU** while retaining its memory saving. [Measurements, per-run observations and input/output fingerprints](results/compact-speech-0.2.12.json)

| Streams | Fixed 0.2.11 CPU-s/min | Compact 0.2.11 CPU-s/min | Compact 0.2.12 CPU-s/min | 0.2.12 / 0.2.11 compact paired CPU ratio (95% interval) |
| ---: | ---: | ---: | ---: | ---: |
| 1 | 0.04972 | 0.05350 | 0.04406 | 0.835 (0.821–0.858) |
| 128 | 0.06048 | 0.05748 | 0.04873 | 0.848 (0.825–0.863) |
| 768 | 0.06151 | 0.05968 | 0.04899 | 0.821 (0.800–0.859) |

Each CPU-s/min value is the median across fresh processes. The ratio column uses the median of within-cycle ratios. The one-stream trial included all three AMI clips; the compact ratios per clip were 0.847, 0.842 and 0.824. At this load 0.2.12 compact also used 11.8% less CPU than the fixed 0.2.11 decoder in the paired comparison (95% interval 9.4–12.0%). The worker timed packet decoding and retained PCM buffers. Service request processing, audio I/O and speech recognition were outside its timed section.

| Streams | Warm process RSS, MiB fixed 0.2.11 / compact 0.2.11 / compact 0.2.12 | First CELT p99, µs fixed 0.2.11 / compact 0.2.11 / compact 0.2.12 |
| ---: | ---: | ---: |
| 1 | 4.18 / 4.16 / 4.14 | 109.9 / 92.0 / 113.5 |
| 128 | 8.44 / 5.18 / 5.14 | 67.2 / 78.2 / 82.6 |
| 768 | 29.92 / 10.25 / 10.21 | 61.0 / 78.0 / 76.9 |

Each process supplied one nearest-rank p99 across its streams. The table shows the median of those values across 18 processes per arm and load. At one stream this is the median of 18 single transitions. Small differences in process RSS include runtime, allocator and executable pages. Each compact decoder occupied 7,648 bytes before retaining CELT history, as in 0.2.11. Once all 768 streams had used CELT, median worker RSS was about 30.1 MiB in each arm.

## Serial decoder comparison with opus-rs

A separate serial replay compared the compact decoder in 0.2.12 with opus-rs 0.1.42 on the same six minutes of 16 kHz mono AMI speech. Median thread CPU cost was **48.9 ms per audio minute for Opura and 37.1 ms for opus-rs**. The paired geometric ratio was 1.327 (descriptive 95% interval 1.255–1.396). Both produced identical normalized float PCM and final ranges across 18,003 packets. This comparison used a different compiler, output format and schedule from the release comparison above.

To reproduce, use the [speech API replay and build procedure](decoder-speech-0.2.9.md#reproduction), select `CompactOpusDecoder` in the caller arm, pin opus-rs to 0.1.42 and build both arms with rustc 1.98.1 in the generic release profile. For the runner's 15 packet inputs, cycle ES2002a, IS1000a and TS3003a five times in that order. Use one warmup and one timed pass at 16000 Hz, pinning the worker to one core. Each pass covers 30 audio minutes and starts a fresh decoder per recording. Run 16 paired rounds in eight blocks, reversing arm order within each block. The interval resamples whole blocks 20,000 times (seed 117). [Per-process CPU times and output hash](results/compact-speech-0.2.12.json)

## Switches with a redundant CELT tail

A SILK packet can also carry a redundant CELT tail. The compact decoder learns this while reading the speech layer. It retries the packet from the saved stream state so it can decode both layers. In a separate check, a 10 ms packet with such a tail took 34.2 µs of thread CPU in 0.2.11 and 46.2 µs in 0.2.12. Each of 12 paired runs timed 3,200 first transitions from the same speech state at 16 kHz mono. The measured packet is included in the transition regression test. Deployment frequency and other tail payloads were outside this check. [Packet and state checks](../../tests/compact_decoder.rs) · [Individual measurements](results/compact-speech-0.2.12.json)

Both the fixed and compact decoders reproduced the 0.2.11 PCM hashes and expected packet final ranges across all 120 RFC 8251 vector configurations. [Compact vector report](../validation/results/compact-speech-0.2.12-conformance.json) · [Reproduce either decoder](compact-speech-0.2.11.md#decoder-correctness)

## Reproduce

Use Linux x86-64, Rust 1.87.0, Python 3.11+, taskset and the three AMI packet files or source WAVs from the [0.2.11 experiment](compact-speech-0.2.11.md#reproduce). The source WAVs require FFmpeg with libopus. The worker uses generic Cargo release settings and checks packet and PCM/range fingerprints before collecting timings. From a 0.2.12 checkout, obtain a separate 0.2.11 source tree and select a free logical CPU.

```sh
git worktree add --detach /tmp/opura-fixed-0211 v0.2.11
python3 tools/benchmarks/compact-speech/compare_compact.py \
  --baseline /tmp/opura-fixed-0211 --candidate . --cpu 10 \
  --packets /path/to/prepared/inputs --out /tmp/opura-compact-cpu
```

Use `--wav-dir /path/to/ami` in place of `--packets` for original recordings or add `--qualify-only` to verify outputs. We used a Ryzen AI 9 HX 370 on x86-64 Linux, pinned the worker to logical CPU 10 under the performance governor. CPU frequency and other workstation activity, including activity on its SMT sibling 22, were uncontrolled. Other builds may have overlapped part of the timing run.

The 162 timing processes ran in 18 paired cycles at three loads. Each group of six cycles permuted the order of all three arms, and loads rotated between cycles. Every single-stream clip saw every arm ordering once. The intervals use 20,000 paired bootstrap resamples of the median, stratified by clip at one stream, with seed 117 plus stream count. Packet count, staggered multi-stream offsets, warmup and first CELT input follow the original 0.2.11 worker. Every arm reproduced the full PCM and final-range fingerprints in all five qualification cases. The measurements cover SILK speech and one subsequent switch to CELT on this host. [Comparison driver](../../tools/benchmarks/compact-speech/compare_compact.py) · [Measured worker](../../tools/benchmarks/compact-speech/worker.rs)
