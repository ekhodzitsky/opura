# GigaSTT cost per minute of speech

On GigaSTT 2.22.0, the deployed opus-rs decoder used **5.11 CPU-seconds per minute of speech** through the warm HTTP endpoint. Opura 0.2.8 used **5.19** and the transposed SILK pilot used **5.12**. The pilot-to-stock CPU ratio ranged from 0.951 to 1.076 across five paired rounds. Switching the file decoder showed no consistent service-level CPU or resident-memory saving. All 450 responses reproduced the same transcript for each recording. Every configuration made one normalized word error in 71 reference words. [Per-request results, binary identities and input probe](results/gigastt-silk-pilot.json)

## CPU, latency and memory

Each process handled one request at a time with the GigaAM v3 RNNT INT8 model and one loaded model instance. The request contained an Ogg/Opus file. CPU-seconds include all server threads. Latency covers the loopback POST through the complete JSON response. RSS is the median server resident set after a request. Use the latency column when choosing the number of inference encoder threads.

| File decoder | Encoder threads | CPU-seconds / speech minute | Median response, ms | p95 response, ms | RSS, MiB |
| --- | ---: | ---: | ---: | ---: | ---: |
| opus-rs 0.1.41 | 4 | 5.11 | 84 | 123 | 371.4 |
| Opura 0.2.8 | 4 | 5.19 | 87 | 119 | 371.5 |
| Opura transposed pilot | 4 | 5.12 | 86 | 119 | 369.5 |
| opus-rs 0.1.41 | 2 | 3.94 | 130 | 188 | 367.0 |
| opus-rs 0.1.41 | 1 | 3.47 | 231 | 337 | 369.0 |

With the stock decoder, two encoder threads saved 20.7% CPU and one saved 32.2%, using median within-round ratios against four threads. Absolute CPU costs in the table are the median corpus totals for each configuration. Two threads offer a concrete option for a CPU-budgeted batch service if its latency requirement permits. Four threads gave the shortest median response here. The thread setting controls ONNX inference in the speech recognition model. GigaSTT can select it automatically based on available CPUs and pool size. [GigaSTT thread selection](https://github.com/ekhodzitsky/gigastt/blob/e396a253a1c5d1db18aa71b3abf14e946bdbe3d2/crates/gigastt/src/boot/modes.rs#L116) · [Upstream tuning guidance](https://github.com/ekhodzitsky/gigastt/blob/e396a253a1c5d1db18aa71b3abf14e946bdbe3d2/docs/troubleshooting.md)

## Cost of the audio input

| Decoder | Packet decode and float output, CPU-s/min | Complete in-memory file input, CPU-s/min |
| --- | ---: | ---: |
| opus-rs 0.1.41 | 0.054 | 0.147 |
| Opura 0.2.8 | 0.065 | 0.158 |
| Opura transposed pilot | 0.059 | 0.151 |

The stock packet path cost about **1.1%** of the stock server's four-thread CPU budget. The complete file input API, including demuxing, decoding, mono conversion and resampling, cost about **2.9%**. These are estimates from separate runs. The pilot saved about 0.006 CPU-seconds per speech minute over stable Opura in the packet probe. GigaSTT's PCM16 WebSocket input bypasses Opus decoding altogether. [File decoder](https://github.com/ekhodzitsky/gigastt/blob/e396a253a1c5d1db18aa71b3abf14e946bdbe3d2/crates/gigastt-core/src/inference/audio/opus.rs) · [WebSocket input](https://github.com/ekhodzitsky/gigastt/blob/e396a253a1c5d1db18aa71b3abf14e946bdbe3d2/docs/stream-ctc-confirmation.md)

## Conditions and reproduction

The public GigaSTT revision was `e396a253a1c5d1db18aa71b3abf14e946bdbe3d2`. Its lockfile selected opus-rs 0.1.41. The two comparison builds used local Opura sources at `v0.2.8` and `ff19c2d090ab8d0f5bb0154e49bc7f6a111bb4bd`, using the same GigaSTT packet handling and 48 kHz output. All 579 other package identities matched. The adapter, diagnostic wrapper and Rust probe are in the results file. Build with Rust 1.98.1 in release mode using `-p gigastt --bin gigastt --no-default-features`. The server process ran on logical CPUs 4–7 on a shared x86-64 AMD Ryzen AI 9 HX 370. [GigaSTT source](https://github.com/ekhodzitsky/gigastt/tree/e396a253a1c5d1db18aa71b3abf14e946bdbe3d2) · [Pilot source](https://github.com/ekhodzitsky/opura/commit/ff19c2d090ab8d0f5bb0154e49bc7f6a111bb4bd)

Use the 15 Golos recordings, Ogg encoding, model files, CPU clock and HTTP procedure from the preceding GigaSTT study. Their input hashes matched. The corpus contained 61.765 seconds of distinct speech. Serve offline with punctuation off, pool size and minimum size 1, and explicit `--encoder-intra-threads` of 4, 2 or 1. Start a fresh process per configuration and round, warm it with one unmeasured request, then send each recording once to `/v1/transcribe`. Shuffle the starting configuration order with Python seed 26011 and rotate it each round. Shuffle files once per round, preserving the same order across configurations. [Model, fixture and HTTP setup](gigastt-decoder-0.2.7.md#reproduce-the-workload)

The first round populated previously absent optimized model caches for one and two threads. Those two processes had median RSS around 565 MiB. A sixth round was added so the five primary rounds all started with cached graphs and each configuration occupied each order position once. The receipt retains the complete first round. CPU cost sums server process CPU across all 15 requests, divides by source duration in minutes and takes the median across the five cached rounds. The p95 is the nearest rank of 75 request latencies per configuration.

For the input probe, preload either the 15 compressed files or their 3,101 demuxed packets. Use a persistent decoder within each clip and a fresh decoder for each new clip. Invoke GigaSTT's packet adapter or its public `decode_audio_bytes` API for one warmup corpus pass, then time 24 passes per process with the Linux user-plus-system process CPU counter. Use CPU 4 and three rounds per decoder and mode. The receipt includes the measured probe, input hashes, and raw ticks. Packet decoding is a component of full file input, so their costs overlap.

The trial covers short mono uploads, serial requests and a CPU-only server. Concurrent clients, long recordings and production billing depend on deployment conditions.
