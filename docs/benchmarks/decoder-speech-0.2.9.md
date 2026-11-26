# Opura 0.2.9 decoder CPU time and output storage

At 48 kHz mono, Opura 0.2.9 decoded a 20 ms speech packet in a median of **12.96 µs** with the existing owned-output API and **12.35 µs** with the new caller-owned buffer API. Opura 0.2.8 took **14.25 µs**, and opus-rs 0.1.42 took **12.07 µs**. All arms produced the same float PCM samples. [Raw timings, output fingerprints and build identities](results/decoder-speech-0.2.9.json)

## API and memory

Use `OpusDecoder::decode_into(packet, &mut output)` to decode into an existing slice of signed 16-bit samples. The return value counts interleaved samples across all channels. A buffer of **320 samples (640 bytes)** is sufficient for a 20 ms mono packet at 16 kHz. A 20 ms stereo packet at 48 kHz needs 1,920 samples. A short buffer reports the required count and leaves both state and output intact. Later decode errors preserve the decoder for retry and may write part of the requested output. The existing `decode` method returns an owned result whose sample array accommodates up to 120 ms at 48 kHz stereo (23,040 bytes).

SILK now writes its decoded layer directly into the packet's destination. The mono resampler also writes directly into that space. The same implementation serves both Opura APIs. SILK's fixed probability tables for excitation are known when the codec is built. Using them directly avoids checking their shape for each decoded symbol. The public entropy API continues to validate caller-supplied distributions. [Decoder](../../src/decoder.rs) · [SILK output](../../src/silk/decoder.rs) · [Fixed pulse models](../../src/silk/excitation.rs)

A separate Linux measurement decoded all 15 clips on a fresh thread with a 4 MiB stack. All stacks initially had 8 KiB resident. After processing the corpus the stack had **296 KiB resident for 0.2.8**, **216 KiB for 0.2.9 with owned output**, and **148 KiB for 0.2.9 with caller output** at both 16 and 48 kHz. Five fresh processes per configuration produced these same values. A second thread read the mapping containing the worker's stack pointer from `/proc/self/smaps` while the worker waited at a barrier. These figures measure resident stack pages on this compiler and workload. Heap state, process RSS and multi-stream capacity require separate measurements. [Stack probe and observations](results/decoder-speech-0.2.9.json)

## Speech workload

Median thread CPU time per packet, in microseconds, across ten runs. Each run offered eight complete passes over the 3,101 packets after one warmup pass, without pausing between packets. Every implementation decoded to the requested rate and produced the same mono float output. Both Opura calls include signed-to-float conversion in the timed region. A new decoder was initialized per recording in each pass, preserving inter-packet history. The caller reuses the same output storage across packets.

| Output rate | 0.2.8 owned | 0.2.9 owned | 0.2.9 caller buffer | opus-rs 0.1.41 | opus-rs 0.1.42 |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 16 kHz | 11.92 | 10.60 | 10.03 | 7.03 | 6.93 |
| 48 kHz | 14.25 | 12.96 | 12.35 | 12.03 | 12.07 |

Using the geometric mean of within-round CPU ratios, the existing API used 10.8% less CPU at 16 kHz and 7.2% less at 48 kHz than 0.2.8. With caller output the reductions were **16.7%** (paired 95% bootstrap interval 14.6–18.6%) and **13.1%** (11.1–14.9%). Compared with opus-rs 0.1.42, Opura with caller output used 43.6% more CPU at 16 kHz and 4.1% more at 48 kHz. Results cover the decoder on short mono recordings. Neither speech recognition inference nor application scheduling was timed.

## Full decoder workloads

The existing owned-output API took 2.14% less time than 0.2.8 across all 12 RFC 8251 streams, five output rates and two channel counts (aggregate paired time ratio 0.9786, 95% interval 0.9637–0.9877). The benchmark used Rust 1.87.0 and 15 paired rounds per configuration. Every run decoded 20,075 packets after warmup. The upper confidence limit for the aggregate was below 1.00 and each configuration's upper limit was below the preregistered 1.05 cutoff. The largest upper limit was 1.0389 at 16 kHz mono. At that rate the measured time ratios were 1.0061 for mono and 1.0001 for stereo. [All timings and source identities](results/decoder-speech-0.2.9-official.json)

Follow the [official-stream benchmark procedure](decoder-theta-0.2.8.md#reproduce) with tags `v0.2.8` and `v0.2.9`, their respective conformance reports linked below, and `--rounds 15 --cpu 4`. Verify the archived result with `python3 tools/decoder_change.py --check docs/benchmarks/results/decoder-speech-0.2.9-official.json`.

## Correctness

Both output APIs matched on all 200,750 official packet decodes across 120 vector/rate/channel combinations, yielding 103,885,740 identical interleaved i16 samples. An output sentinel survived every call, and 15,500 deliberate short-buffer calls reported the expected count and then succeeded on retry. The owned decoder reproduced all 120 published 0.2.8 PCM hashes and matched all expected nonzero final ranges. The regression suite additionally exercises framing failure, a failure in the second frame of a packet, gain, mode changes, DTX and all supported output rates and channel counts. [0.2.9 conformance](../validation/results/decoder-speech-0.2.9-conformance.json) · [0.2.8 reference](../validation/results/decoder-theta-0.2.8-conformance.json) · [Public API tests](../../tests/opus_decoder.rs)

## Reproduction

Use x86-64 Linux and prepare the pinned Golos packet files with the active-speech runner and its `--qualify-only` option. The measured CPU was an AMD Ryzen AI 9 HX 370 and the build used rustc 1.98.1, standard Cargo release settings and generic x86-64 code generation. [Prepare inputs](active-speech-capacity-0.2.8.md#reproduce) · [Benchmark executable source](../../tools/benchmarks/speech-api/replay.rs)

Place the replay source in `runner/src/main.rs` and check out Opura in a sibling `codec` directory. Use this `runner/Cargo.toml` for the 0.2.9 and opus-rs 0.1.42 runs.

```toml
[package]
name = "speech-api"
version = "0.0.0"
edition = "2024"

[dependencies]
opura = { path = "../codec", optional = true }
opus-rs = { version = "=0.1.42", optional = true }

[features]
owned = ["dep:opura"]
caller = ["dep:opura"]
reference = ["dep:opus-rs"]
```

Build each executable with `cargo +1.98.1 build --release --features FEATURE`, using one of `owned`, `caller`, or `reference`. Save the binary after each build. For the Opura baseline, check out tag `v0.2.8` in the same codec directory and build with `owned`. For opus-rs 0.1.41, pin `=0.1.41` and replace `let range = decoder.final_range();` with `let range = 0u32;` in its copy of the replay source because this release does not expose the final range. Check the PCM independently in this case.

Run each executable as `taskset -c 2 BINARY PACKETS HZ 8` at 16000 and 48000 Hz. Its output is the calling thread's CPU time in nanoseconds for the eight corpus passes. A fourth argument writes the warmup output, with the per-packet range followed by the f32 samples in little-endian order. For each rate, compare with libopus 1.6.1 decoding the same packets to i16 and scaling by 1/32768. The receipt preserves all hash comparisons. Rotate the five implementations through each execution position over five rounds and reverse the order for five more. Resample the ten paired ratios with replacement 10,000 times (seed 106) for the 95% percentile intervals.

To reproduce the stack probe, use the same executable source, mark `pass` as `#[inline(never)]` to keep its stack work after the initial barrier, and replace `main` with the stack measurement entry point in the receipt. Its arguments are the packet directory and output rate. Run it on a fresh process for each observation.
