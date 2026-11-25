# Decoder choice for GigaSTT 2.22.0

GigaSTT produced the same transcripts with Opura 0.2.7, its deployed
`opus-rs` 0.1.41, and `opus-rs` 0.1.42 on all 15 Russian speech clips.
Each decoder made one normalized word error in 71 reference words (WER 1.41%).
All 135 warm HTTP responses matched their corresponding CLI transcripts.

This workload gives GigaSTT little reason to switch from 0.1.41 for CPU or
memory savings. In the warm service, Opura used 1.7% more CPU per request
than 0.1.42 in the median paired comparison. The trial covered 61.77 seconds
of source speech and one x86-64 machine. [Raw observations, source patches
and input hashes](results/gigastt-decoder-0.2.7.json)

## Application cost

Each decoder processed each file three times per track (45 observations per
decoder per track). CPU time includes all threads. Wall time is elapsed time.
Medians and p95 in this table are over individual observations.

| GigaSTT entry point | Decoder | CPU p50, ms | Wall p50, ms | Wall p95, ms | RSS p50, MiB |
| --- | --- | ---: | ---: | ---: | ---: |
| Fresh CLI | opus-rs 0.1.41 | 1240.1 | 857.0 | 935.2 | 363.43 |
| Fresh CLI | opus-rs 0.1.42 | 1233.8 | 848.8 | 924.9 | 362.87 |
| Fresh CLI | Opura 0.2.7 | 1216.7 | 839.2 | 978.6 | 364.39 |
| Warm HTTP | opus-rs 0.1.41 | 378.0 | 94.9 | 143.6 | 369.70 |
| Warm HTTP | opus-rs 0.1.42 | 375.1 | 93.7 | 137.3 | 368.94 |
| Warm HTTP | Opura 0.2.7 | 382.9 | 95.5 | 131.6 | 368.46 |

For a fresh CLI process, Linux `wait4` supplied user CPU, kernel CPU and peak
RSS. Wall time ran from fork through child exit, including cached model loading,
Ogg decoding, resampling, inference and JSON output. For a warm HTTP request,
the server process CPU clock covered all server threads from just before the
loopback POST until the client received its response. HTTP RSS is the process's
resident size immediately after each response. The raw data also includes
lifetime peak RSS for each service run.

## Paired comparison

Ratios divide Opura's cost by the reference's cost for the same recording and
round. Values below one favor Opura. Each recording contributes its median
of three paired ratios. The final estimate is the median across 15 recordings.

| Reference | CLI CPU (95% interval) | CLI wall | CLI peak RSS | HTTP CPU (95% interval) | HTTP wall |
| --- | ---: | ---: | ---: | ---: | ---: |
| opus-rs 0.1.41 | 0.9938 (0.9770–1.0071) | 0.9951 | 1.0012 | 1.0058 (0.9804–1.0331) | 1.0168 |
| opus-rs 0.1.42 | 0.9963 (0.9663–1.0149) | 1.0045 | 1.0004 | 1.0171 (1.0093–1.0363) | 1.0165 |

Intervals are descriptive percentile bootstraps of the 15 recording ratios
(10,000 draws, Python random seed 19782). They reflect variation within this
small corpus. Host activity and frequency were uncontrolled. The test used one
sequential request at a time and covers neither concurrency nor long streams.

## Reproduce the workload

1. Use three clean checkouts of GigaSTT commit
   `a2c09d4ef842158b5a33d0dc5ab01f40c35be059`. The upstream lockfile selects
   opus-rs 0.1.41. For the other two builds, extract the corresponding
   `source_patches` entry from the raw JSON and feed the decoded string to
   `git apply -` in its own checkout. The patches reproduce the recorded
   lockfile hashes. All 580 other locked packages share identical versions and
   sources. The Opura adapter preserves GigaSTT's frame splitting, 48 kHz
   decoding and resampling. It converts each signed 16-bit output sample to
   `f32` by division by 32768. [Pinned
   source](https://github.com/ekhodzitsky/gigastt/tree/a2c09d4ef842158b5a33d0dc5ab01f40c35be059)
2. Build each checkout with Rust 1.98.1 for `x86_64-unknown-linux-gnu`
   using `cargo build --release --locked -p gigastt --bin gigastt --no-default-features`.
   This omits the optional diarization and quantization tools. Use the same
   GigaAM v3 RNNT INT8 model and optimized graph in every arm. The GigaSTT
   model registry identifies the source artifacts. The raw JSON records
   SHA-256 for the three model graphs, vocabulary, manifest and optimized
   encoder graph. [Pinned model
   registry](https://github.com/ekhodzitsky/gigastt/blob/a2c09d4ef842158b5a33d0dc5ab01f40c35be059/crates/gigastt-core/src/model/variant.rs)
3. Encode each of the 15 upstream Golos WAV fixtures by running FFmpeg 7.0.2 with
   `ffmpeg -i input.wav -c:a libopus -application voip -b:a 24k -vbr off -frame_duration 20 output.opus`.
   Regeneration matched the compressed packet payloads for all 15 files. For
   each file, hash every audio packet with SHA-256, concatenate the 32-byte
   digests in packet order, then hash the concatenation to compare against
   `packet_digests_sha256`. This handles varying Ogg stream serial numbers.
   Score transcripts against the fixture manifest with GigaSTT's
   `benchmark/common.py::compute_wer`. [Pinned
   fixtures](https://github.com/ekhodzitsky/gigastt/tree/a2c09d4ef842158b5a33d0dc5ab01f40c35be059/crates/gigastt/tests/fixtures)
4. Pin all three binaries to the same four logical CPUs (4–7 on the measured
   AMD Ryzen AI 9 HX 370). Use `transcribe --log-level error --model-dir MODEL --model-variant rnnt --encoder-intra-threads 4 --punctuation off --format json FILE`.
   Warm each binary once on the first fixture. Shuffle the file order each
   round and the three decoder orders within each file with Python random seed
   6893. Run a fresh CLI process for every observation.
5. For the HTTP track use `serve --offline --host 127.0.0.1 --model-dir MODEL --model-variant rnnt --encoder-intra-threads 4 --pool-size 1 --pool-min-size 1 --punctuation off`.
   Start one server at a time, poll `/ready`, send one unmeasured warmup using
   the first fixture, then POST each Ogg file to `/v1/transcribe` as
   `application/octet-stream`. Start a fresh server for every decoder and round.
   Shuffle the initial decoder order and each round's file order with Python
   random seed 8957. Rotate decoder order across the three rounds. The raw
   arrays preserve execution order for both tracks.
