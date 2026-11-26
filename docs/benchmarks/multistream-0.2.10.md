# Multistream decoder CPU time in Opura 0.2.10

The public `MultistreamDecoder::decode` used **6.08% less CPU time** than 0.2.9 on three mapped clips at 48 kHz. The paired 95% interval for the reduction was 4.76–7.93%. The library stages one copy of the multistream state before decoding an ordinary packet. Each constituent stream now writes its history into that copy. A later-stream error discards the whole staged state, so the application can retry. [Decoder source](../../src/multistream.rs) · [Rollback and retry test](../../tests/multistream.rs) · [Raw timings and fingerprints](results/multistream-0.2.10.json)

| Output channels | Elementary streams | 0.2.9 median µs/packet | 0.2.10 median µs/packet | Paired CPU ratio (95% interval) |
| ---: | ---: | ---: | ---: | --- |
| 3 | 2 | 103.44 | 97.64 | 0.944 (0.934–0.955) |
| 6 | 4 | 222.65 | 211.59 | 0.949 (0.939–0.959) |
| 8 | 8 | 350.26 | 328.78 | 0.924 (0.876–0.957) |

All 97,920 interleaved PCM samples and 18 packet final ranges matched 0.2.9. Each timed process created a decoder per six-packet clip and decoded it from start to finish, with 8 warmup passes followed by 128 timed passes. The measurement includes decoder construction, packet processing and output disposal. File I/O occurs before timing. Results describe these three short clips on one x86-64 host.

The standard single-stream decoder also reproduced every 0.2.9 output hash in the complete RFC 8251 matrix, covering 200,750 packet decodes across 120 configurations. [Official vector report](../validation/results/multistream-0.2.10-conformance.json)

## Single-stream speech and memory

A separate replay of 3,101 mono speech packets checked both public single-stream APIs against 0.2.9. At 16 kHz the paired time ratios were 1.0064 for owned output (95% interval 0.9954–1.0218) and 1.0101 for caller output (1.0002–1.0206). At 48 kHz the ratios were 1.0038 (0.9945–1.0147) and 0.9950 (0.9784–1.0108). All four upper bounds met the 1.05 tolerance. The four configurations also reproduced the libopus reference PCM and final ranges. [Speech replay procedure](decoder-speech-0.2.9.md#reproduction) · [Timing and stack observations](results/multistream-0.2.10-mono.json)

The first implementation increased the resident stack of a fresh mono decoder thread from 216 to 240 KiB with owned output. The shipped implementation allocates the output in its caller and returns the number of samples across the transaction boundary. In all 24 observations, both releases used 216 KiB with owned output and 148 KiB with caller output. These values measure the worker's resident stack on two mono speech workflows. Multistream process RSS and production server capacity remain unmeasured in this comparison.

## Reproduce the mapped benchmark

Use the tagged sources for 0.2.9 and 0.2.10, the same checkout path for each build, Rust 1.87.0 on Linux, and the committed fixture files. The measurements used an AMD Ryzen AI 9 HX 370 with default release code generation and an idle logical CPU 6. Save the [replay source](../../tools/benchmarks/mapped-decoder/replay.rs) outside the checkout before switching versions. For each version, set `REPLAY` to that source, `BINARY` to a distinct output path, and run:

```sh
cargo +1.87.0 build --lib --release --locked --offline
rustc +1.87.0 --edition=2024 -C opt-level=3 --crate-name mapped_replay \
    "$REPLAY" --extern opura=target/release/libopura.rlib \
    -L dependency=target/release/deps -o "$BINARY"
taskset -c 6 "$BINARY" tests/audio/mapped/family1-6ch-gain0.opus
```

Repeat for `family1-3ch-gain0.opus` and `family255-8ch-gain0.opus`. The executable emits thread CPU nanoseconds. Passing a second argument writes the complete PCM, sample counts and ranges for bytewise comparison. Run 20 paired rounds per clip, rotating clip order and alternating binary order as recorded in the receipt. Average log ratios with equal weight across clips and rounds. Resample paired rounds 20,000 times with seed 111 for the percentile intervals. Acceptance required identical output, an aggregate upper ratio below 1 and each clip's upper ratio at most 1.05.
