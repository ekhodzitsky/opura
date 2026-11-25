# Opura 0.2.8 in the VoiceMCU conference server

With 64 connected participants in eight rooms, changing the server decoder from native libopus 1.6.1 to Opura 0.2.8 reduced median process RSS from **26.88 to 17.84 MiB (34%)**. It increased median server CPU from **0.505 to 0.575 of one core (14%)** and median run-level p95 audio-marker delay from **43 to 52 ms**. Under a separately enforced **16 MiB memory cgroup with swap disabled**, Opura completed all three 64-participant trials and delivered all 288 expected audio changes. The kernel killed the stock server in all three trials. Both servers completed at 20 MiB. These were short local experiments with the upstream application. [Results from all 30 server processes](results/voicemcu-0.2.8.json)

## Audio delivery and CPU

Each participant kept a real QUIC connection, decoder and encoder. One participant in each room sent alternating tones and seven transmitted encoded silence. Every 20 ms the upstream server processed the incoming packets, applied its jitter buffer and voice activity detector, mixed each listener's audio, re-encoded it and sent it over QUIC. All seven listeners recorded arrivals. One listener per room also decoded the returned audio with native libopus and checked the received frequency. The server's native encoder and the clients were the same in both variants. [VoiceMCU room mixer](https://github.com/Cod-e-Codes/voicemcu/blob/d7e2748732bdb2b57368c6ef8a47596310ff5162/server/src/main.rs#L682-L884) · [Upstream codec configuration](https://github.com/Cod-e-Codes/voicemcu/blob/d7e2748732bdb2b57368c6ef8a47596310ff5162/common/src/codec.rs)

| Participants | Server decoder | Process RSS, MiB | CPU cores | Worst listener, packets/s | Arrival gap p99, ms | Audio-marker p95, ms | Received / expected markers |
| ---: | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 8 | libopus | 10.20 | 0.075 | 50.00 | 21.9 | 37.5 | 36 / 36 |
| 8 | Opura | 9.51 | 0.078 | 50.00 | 22.0 | 38.9 | 36 / 36 |
| 64 | libopus | 26.88 | 0.505 | 50.00 | 22.8 | 43.0 | 288 / 288 |
| 64 | Opura | 17.84 | 0.575 | 49.75 | 24.4 | 51.5 | 288 / 288 |
| 128 | libopus | 45.84 | 0.998 | 23.07 | 71.6 | 195.4 | 476 / 576 |
| 128 | Opura | 27.24 | 0.990 | 23.11 | 79.7 | 194.0 | 465 / 576 |

Values are medians of three independent runs per configuration. Marker counts are totals. At 64 participants the Opura p95 ranged from 45.1 to 116.4 ms, versus 42.9 to 43.2 ms for libopus. All classified-observer frames were audible with either decoder at 8 and 64 participants. With 128 participants both servers saturated the available core, missed the target rate of 50 packets/s and lost detectable tone changes. The p95 at that load describes only successfully matched markers.

## Memory limits at 64 participants

The two budgets were chosen during exploration after the unrestricted measurements. Only the test server entered the transient cgroup. Its memory controller enforced the limit and disabled swap. The table uses observed cgroup charge, which accounts for memory differently from process RSS. These local budgets establish no upstream service requirement.

| Memory limit | Server decoder | Completed | Observed cgroup peak, MiB | Matched markers | Worst listener across completed runs, packets/s |
| ---: | --- | ---: | ---: | ---: | ---: |
| 20 MiB | libopus | 3 / 3 | 20.12–20.21 | 288 / 288 | 49.75 |
| 20 MiB | Opura | 3 / 3 | 11.63–11.92 | 288 / 288 | 49.74 |
| 16 MiB | libopus | 0 / 3 | 16.00 | — | — |
| 16 MiB | Opura | 3 / 3 | 11.59–11.75 | 288 / 288 | 49.997 |

At 20 MiB the stock server accumulated 1,109–1,219 `memory.events:max` events per run. Opura accumulated zero at either budget. At 16 MiB all 64 clients joined each stock run. All three stock servers exited during the one-second audio warmup with SIGKILL, with systemd reporting `oom-kill` for each scope. Opura used 0.545–0.578 core and its p95 marker delay ranged from 42.7 to 46.2 ms. The collector polled the kernel's `memory.peak` and `memory.events` every 15 ms. Destruction of a scope can hide the final event counters. The experiment did not collect a `memory.current` time series.

## Method and scope

The host used a Ryzen AI 9 HX 370 running x86-64 Linux. Each fresh server had one Tokio worker pinned to one logical CPU. The client ran on four other physical cores. Each variant used rustc 1.100.0 in release mode, the same native libopus 1.6.1 shared library and upstream audiopus 0.2.0. The Opura adapter decoded 48 kHz mono packets and converted its signed 16-bit samples to the mixer's floating-point format. It also provided the server's 960-sample packet-loss concealment call. Both variants passed the four upstream codec tests, including signal preservation and concealment after a real packet. [Opura 0.2.8 decoder](https://github.com/ekhodzitsky/opura/blob/v0.2.8/src/decoder.rs) · [Build and dependency identities](results/voicemcu-0.2.8.json)

The observer established eight connections per room via upstream signaling and sent the default 24 kbit/s narrowband CBR format at 48 kHz mono. The talker alternated 400 Hz and 800 Hz every 300 ms. The run used 250 frames, including a 50-frame warmup, and measured approximately four seconds. The uncapped runs randomized workload and variant order with a fixed seed. A timestamp on the outgoing encoded tone transition started the delay measurement. Reception of the downlink packet containing the new tone ended it. A marker required the previous tone within 70 ms and a match within 200 ms. Process counters and packet rates use the same measurement window. The sender and receiver shared a monotonic clock on loopback.

The test covered one host, one active speaker per room, a synthetic signal and short, established calls. Device capture and playout, human listening quality, Internet jitter, long-running memory behavior and concurrent real speakers require separate evidence. VoiceMCU adoption and operator demand were outside the experiment.

## Reproduce

The source bundle includes the decoder adapter patch, native-codec observer, workload and cgroup drivers, both dependency lockfiles and build instructions. It pins VoiceMCU at `d7e2748732bdb2b57368c6ef8a47596310ff5162` and Opura at `0.2.8`. Run from a fresh directory outside the Opura checkout on Linux with rustc 1.100.0, libopus 1.6.1, `pkg-config`, Python 3, `taskset` and a C linker. The limited-memory trials additionally use cgroup v2 and a systemd user manager. [Download the reproduction bundle](../../tools/benchmarks/voicemcu/voicemcu-0.2.8-reproduce.tar.gz)

Copy the archive to the working directory and verify its SHA-256 before extraction.

```sh
echo '48618c59ae0cd6ce77d80db1c85faf2e52967d5ae0707214672d5b07173371cd  voicemcu-0.2.8-reproduce.tar.gz' | sha256sum -c -
tar -xzf voicemcu-0.2.8-reproduce.tar.gz
cd voicemcu-0.2.8-reproduce
cat README.md
```

The README builds the stock and patched servers from the same upstream revision and gives commands for the 18 unrestricted and 12 capped runs. The runner writes its packet records, per-run measurements and generated local certificate into the experiment directory. If the machine has only the shared libopus runtime, set `LIBOPUS_LIB_DIR` to an absolute directory containing a `libopus.so` symlink to the installed `libopus.so.0` before building.
