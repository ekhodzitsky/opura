# Opura 0.2.8 on the RustPBX Opus-to-PCMU media path

With 500 active one-way transcoding legs, changing the decoder from opus-rs 0.1.42 to Opura 0.2.8 reduced median worker RSS from **121.4 to 46.5 MiB**. Median CPU rose from 0.386 to 0.415 of one core. Four of 440 Opura batches finished after their 20 ms deadline, all in the fifth run. The stock worker finished every batch on time. At 250 legs the worker saved 37.6 MiB and both decoders met every batch deadline. A deployment with a memory cap and spare CPU is a candidate for a pilot at its expected call load. The upstream benchmark establishes that RustPBX exercises this workload. Operator demand for a decoder change remains unverified. [Individual runs and fingerprints](results/rustpbx-transcode-0.2.8.json)

A separate end-to-end trial ran eight bidirectional calls through the complete RustPBX 0.5.0 SIP server. That release uses opus-rs 0.1.32. Both builds completed all eight calls. The server with Opura used about 1.3 MiB less resident memory and 0.019 more CPU core in the single paired speech run. [Full server trial](rustpbx-call-0.2.8.md)

## Results

Each row covers five fresh processes. RSS is the median final resident set of the entire benchmark worker. CPU is the median sum of user and kernel CPU seconds per 1.76 seconds of offered audio. A batch contains one frame for every leg. Its latency runs from its scheduled start to the last encoded PCMU output. The p99 uses the nearest rank over all 440 batches per row.

| Active legs | Decoder | RSS, MiB | CPU cores | Batch p99, ms | Batches over 20 ms |
| ---: | --- | ---: | ---: | ---: | ---: |
| 100 | opus-rs | 29.5 | 0.080 | 2.93 | 0/440 |
| 100 | Opura | 14.1 | 0.085 | 3.21 | 0/440 |
| 250 | opus-rs | 63.9 | 0.193 | 6.31 | 0/440 |
| 250 | Opura | 26.3 | 0.205 | 5.93 | 0/440 |
| 500 | opus-rs | 121.4 | 0.386 | 11.55 | 0/440 |
| 500 | Opura | 46.5 | 0.415 | 19.96 | 4/440 |

Each pair emitted the same number of frames, silent frames and identical 64-bit checksums of the outgoing PCMU payloads. A separate 15-leg check compared the complete output byte for byte. Both workers produced 211,200 bytes with SHA-256 `95f082b8958fe75327d10f20323baa275d0848e67fea85b557377fe17dbad6c1`.

## What the worker runs

RustPBX selects a transcoder when the two media legs negotiate different codecs. The benchmark feeds actual upstream sample tracks and calls the upstream egress task's `next_frame` method. Both arms use the same media prebuffer, 48-to-8 kHz resampler and PCMU encoder. The reference is the Opus decoder created by audio-codec. The alternate factory supplies Opura and adapts its signed 16-bit mono PCM to the existing decoder interface. Matched-codec calls may bypass decoding entirely. [RustPBX bridge setup](https://github.com/restsend/rustpbx/blob/339f75b99341bcfc440545d70294f5de55652525/crates/rustpbx-media/src/media_bridge.rs#L651-L679) · [Egress implementation](https://github.com/restsend/rustpbx/blob/339f75b99341bcfc440545d70294f5de55652525/crates/rustpbx-media/src/egress.rs)

The release build used rustc 1.100.0, rustpbx-media 0.5.2-rc, rustrtc 0.6.9 and audio-codec 0.4.14. Both arms ran the same executable on logical CPU 0 of a shared AMD Ryzen AI 9 HX 370 under x86-64 Linux. The worker used 15 distinct recordings, assigned cyclically across independent call legs. Each leg consumed 12 warmup packets and the next 88 consecutive 20 ms packets. All legs became due together. Within each paired repeat, the runner varied load order and alternated decoder order across repeats. The host also served other workloads. The four misses in one run and the short measurement window limit conclusions about sustained capacity.

These timings cover the egress worker through its encoded output. SIP setup, the opposite-direction Opus encoder, operating-system packet transport, packet loss, stereo and playout are outside the measurement. The adapter implements the valid mono decode path exercised here. The upstream 500-call SIP test measured both directions and reported roughly 504–508 MB peak memory for the server with its own decoder. That result supplies deployment context on different hardware. [Upstream RustPBX SIP load test](https://github.com/restsend/rustpbx/blob/339f75b99341bcfc440545d70294f5de55652525/tests/bench/bench.md#L294-L345)

## Reproduce

On Linux, prepare the pinned Golos packets with the active-speech driver and its `--qualify-only` option. The example below uses that driver's example output directory. Install rustc 1.100.0 with rustup, Git, taskset and the Linux build prerequisites for RustPBX. Choose an available logical CPU and a fresh work directory. The command fetches the pinned upstream revision, builds a standalone locked project, verifies the input and output fingerprints and runs all 30 worker processes. It writes the observations to `paired.jsonl`. [Fixture preparation](active-speech-capacity-0.2.8.md#reproduce) · [Driver and measured code](../../tools/benchmarks/rustpbx-transcode/)

```sh
python3 tools/benchmarks/rustpbx-transcode/run.py \
  --packets /tmp/opura-active-speech/packets \
  --cpu 0 --work /tmp/opura-pbx-transcode
```
