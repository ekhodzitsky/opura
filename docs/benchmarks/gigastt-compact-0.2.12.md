# Opura 0.2.12 in GigaSTT file transcription

The compact decoder accounted for **0.86–0.90% of the server's CPU time** on short Golos uploads. Halving its packet cost would save about **0.45% of total CPU** if the rest of the service stayed unchanged. Switching the decoder produced the same transcripts and gave no reproducible reduction in service CPU or peak resident memory on this shared host. [Per-request observations and input-stage timings](results/gigastt-compact-0.2.12.json)

| Campaign | Decoder | CPU-s / speech min | HTTP median / p95, ms | Median peak RSS, MiB |
| --- | --- | ---: | ---: | ---: |
| Primary | opus-rs 0.1.41 | 6.149 | 106 / 144 | 371.5 |
| Primary | Opura 0.2.12 | 6.045 | 101 / 137 | 372.7 |
| Repeat | opus-rs 0.1.41 | 5.913 | 101 / 223 | 371.1 |
| Repeat | Opura 0.2.12 | 5.807 | 100 / 132 | 372.5 |

The primary paired Opura/stock CPU ratio was **0.967** (95% interval 0.904–1.033). A build was active on other cores during that campaign. Repeating the same schedule after the study builds and probes finished gave **0.939** (0.880–0.998). An unrelated compiler remained active throughout the repeat. The isolated packet cost differed by only 0.0028 CPU-seconds per speech minute between decoders. The larger differences in server CPU and tail latency cannot be attributed to packet decoding from these runs.

All 360 timed responses and 24 warmups matched the expected transcript for their file. Both decoders made one normalized word error across 71 reference words on the 15 distinct recordings. In both campaigns the observed memory peak occurred during the warmed request window. The repeat also recorded Linux `wait4` peak RSS at exit after resetting the high-water mark at warmup. Its medians were 370.9 MiB for stock and 372.2 MiB for Opura.

## Workload

The GigaSTT 2.22.0 server accepted one Ogg/Opus upload at a time at `/v1/transcribe`. Both builds ran the GigaAM v3 RNNT INT8 model on a Ryzen AI 9 HX 370 with four encoder threads, one loaded model instance and punctuation disabled. The control used the opus-rs 0.1.41 from GigaSTT's lockfile. The other build used Opura 0.2.12's `CompactOpusDecoder::decode_into`, with a reusable signed 16-bit sample buffer and conversion to the application's float format. Both retained GigaSTT's packet handling, 48 kHz decoder output and downstream resampling. [Pinned GigaSTT source](https://github.com/ekhodzitsky/gigastt/tree/e396a253a1c5d1db18aa71b3abf14e946bdbe3d2)

The corpus comprised 15 short mono Russian speech recordings from Golos. Encoding them at 24 kbit/s produced 3,101 Opus packets covering 61.765 seconds of source audio. Each of six paired rounds used two fresh server processes. The client sent one warmup file and then uploaded the 15 files in the same shuffled order to each server. The server ran on logical CPUs 4–7 and the loopback client on CPU 2. [Fixture, encoding and model preparation](gigastt-decoder-0.2.7.md#reproduce-the-workload)

The stock HTTP executable was retained from the [earlier 2.22.0 study](gigastt-silk-pilot.md). Its checksum was verified before use. The stock stage probe and both Opura binaries were built for this comparison. The measurement file records each executable's checksum.

## Where the CPU time goes

| Stage | Stock CPU-s / speech min | Opura CPU-s / speech min |
| --- | ---: | ---: |
| Decode preloaded packets to normalized float samples | 0.0550 | 0.0522 |
| Read preloaded Ogg files through the model's audio window iterator | 0.1504 | 0.1568 |

The second row includes the first. It also covers demuxing, mono processing, resampling and assembling the model's sample windows. Both rows use medians from six fresh processes per decoder and stage. The packet probe starts a decoder for each clip. The file probe consumes all windows returned by the same `FileWindows` path as an ordinary RNNT upload. It supplies a shared byte buffer as the HTTP handler does. The probes run separately from the server. Dividing their costs by the corresponding whole-server measurements estimates each stage's share.

For Opura, the entire file input accounts for **2.6–2.7% of server CPU**. In elapsed time, packet decoding accounts for about **3.4–3.6%** of the cumulative serial HTTP request time and the entire input for **10.3–10.7%**. Client and server can use different amounts of CPU during the same interval. These figures concern one active file upload. A separate decoder-only replay measured a larger CPU cost for Opura on 16 kHz AMI speech, with a different packet corpus, sample rate and opus-rs version. [16 kHz comparison](compact-speech-0.2.12.md#serial-decoder-comparison-with-opus-rs)

## Reproduction

1. Check out GigaSTT at the revision above in two directories. For each checkout, extract the corresponding `stock_build_patch` or `opura_build_patch` string from the [measurement file](results/gigastt-compact-0.2.12.json) and apply it with `git apply`. On Linux x86-64, build with Rust 1.98.1 using `cargo build --release --locked -p gigastt --no-default-features --bin gigastt --bin input_probe`. The probe wrappers expose the input stages for timing. Both lockfiles retain the same 579 other package identities.
2. Prepare the model, optimized graph and 15 encoded fixtures as in the earlier GigaSTT study. Use the server options in that study with four encoder threads and pool size one. Issue serial loopback POSTs in the recorded process and file order. Read the server process CPU clock across each request and measure elapsed time from the client. Compute CPU seconds per speech minute from each corpus total. For the paired CPU ratio, take the geometric mean of the six within-round Opura/stock ratios. Resample whole rounds for the descriptive 95% interval. Compute p95 by nearest rank across the 90 request latencies for each decoder and campaign.
3. Read Linux `VmHWM` at readiness and after warmup. Reset it by writing `5` to `/proc/PID/clear_refs`. Sample again after the reset, after each response and at the end. Take the largest observed high-water mark in the relevant window for each process, then the median across processes. Memory figures include the model, ONNX Runtime and server.
4. For the stage probes, provide either the 15 Ogg files named `golos_00.opus` through `golos_14.opus` or files named `golos_00` through `golos_14` containing only audio packets. Prefix each packet with its four-byte big-endian length. Run `taskset -c 4 input_probe DIR input` or `taskset -c 4 input_probe DIR packets` in the recorded order. Each process preloads the corpus, warms it once and times 24 passes. Convert process CPU ticks using the recorded ticks per second and elapsed nanoseconds to seconds. Divide by the 24 passes' source duration in minutes. [Raw stage and HTTP observations, source patches and fingerprints](results/gigastt-compact-0.2.12.json)
