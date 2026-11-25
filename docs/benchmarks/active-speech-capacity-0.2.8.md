# Active speech decoder capacity on one Linux core

At 768 active speech streams, Opura 0.2.8 used 30.1 MiB of peak process RSS and 58.7% of one CPU. opus-rs 0.1.42 used 136.4 MiB and 43.1%. These are medians of three paired trials. Both decoders completed every packet within 20 ms at this load. Choosing Opura saved 106.3 MiB and cost 15.6 percentage points of CPU for the same float PCM output in this worker.

## Capacity under a memory budget

A load passed when **all three trials** stayed within the listed peak RSS budget, used at most 85% of one CPU, had p99 completion latency of at most 20 ms and missed at most 0.1% of packet deadlines. Each row shows the highest passing tested load and the highest load for which every tested step starting at 128 also passed.

| Worker RSS budget | Opura highest | Opura contiguous | opus-rs highest | opus-rs contiguous |
|---:|---:|---:|---:|---:|
| 64 MiB | 768 | 768 | 256 | 256 |
| 128 MiB | 768 | 768 | 512 | 512 |
| 256 MiB | 768 | 768 | 1,280 | 512 |

At 640 streams, one opus-rs trial missed 15.66% of deadlines, although every trial at 768 streams finished on time. One Opura trial at 1,024 streams had p99 latency above 465 ms. The shared workstation, short trial duration and nonmonotonic results limit the conclusions about sustained capacity. The highest passing count describes an observation at a tested load.

## Workload and measurements

A single worker decoded packets from 15 Golos speech recordings. Each stream had its own persistent decoder, a contiguous excerpt with at least 12 warm-up packets and a retained 320-sample float PCM buffer. A stream received a 20 ms mono packet every 20 ms. Arrivals across the streams were uniformly staggered. On overload the worker drained the queue and counted every late completion. Opura's i16-to-f32 conversion ran inside the timed loop.

CPU is the worker's user and kernel time divided by elapsed wall time. Latency starts at a packet's scheduled arrival and ends after decoding and PCM conversion, including time behind earlier packets. A completion more than 20 ms after its arrival is a miss. Peak RSS is the Linux process high-water mark and includes initialization, decoder states, output buffers, shared input, allocator and measurement storage. Memory budgets were applied to the observed high-water marks. The timing excludes file I/O and decoder initialization.

Each trial offered 80 consecutive packets per stream over 1.6 seconds. Three paired repetitions randomized arm and load order and varied audio offsets using seeds 7, 71 and 113. The initial grid covered 128–1,024 streams. A follow-up grid covering 1,152–1,792 streams was specified after the initial observations and before testing the higher loads. Both grids used the same acceptance criteria. The 84 fresh worker processes completed 6,451,200 timed packets. A separate full-corpus check compared all 3,101 packets with libopus 1.6.1. Both Rust arms produced bit-identical float PCM and final ranges for all 15 recordings.

The host was an AMD Ryzen AI 9 HX 370 running x86-64 Linux 7.0.0-42-generic with the performance governor. Each worker was pinned to logical CPU 2. Its SMT sibling was CPU 14. Other workstation activity and clock frequency were uncontrolled. This replay models an always-active speech worker and omits transport, jitter buffers, mixing, silence gating, packet loss and ASR. The GigaSTT pilot measures a separate application workload. [GigaSTT pilot](gigastt-decoder-0.2.7.md)

## Observations

CPU, peak RSS and p99 show the median and maximum of three independent processes per cell. The miss column gives the largest fraction of packets late in one trial. Decisions use the unrounded values. The receipt contains all trial values, pass counts for every budget, build settings and corpus, decoder output and binary hashes. [Machine-readable receipt](results/active-speech-capacity-0.2.8.json)

| Streams | Decoder | CPU % median / max | Peak RSS MiB median / max | p99 ms median / max | Worst misses % | Trials passing 128 MiB |
|---:|---|---:|---:|---:|---:|---:|
| 128 | Opura | 10.6 / 11.9 | 7.9 / 7.9 | 0.54 / 0.63 | 0.000 | 3/3 |
| 128 | opus-rs | 8.1 / 8.1 | 25.9 / 26.0 | 0.71 / 1.16 | 0.000 | 3/3 |
| 256 | Opura | 24.4 / 28.1 | 12.3 / 12.3 | 1.97 / 2.51 | 0.000 | 3/3 |
| 256 | opus-rs | 17.5 / 17.5 | 48.1 / 48.1 | 0.74 / 1.32 | 0.000 | 3/3 |
| 384 | Opura | 31.2 / 32.5 | 16.8 / 16.8 | 0.82 / 1.22 | 0.000 | 3/3 |
| 384 | opus-rs | 23.8 / 26.2 | 70.2 / 70.2 | 0.73 / 0.87 | 0.000 | 3/3 |
| 512 | Opura | 42.5 / 44.4 | 21.2 / 21.2 | 0.44 / 1.54 | 0.000 | 3/3 |
| 512 | opus-rs | 29.4 / 34.4 | 92.2 / 92.2 | 0.14 / 2.51 | 0.000 | 3/3 |
| 640 | Opura | 49.4 / 52.5 | 25.7 / 25.7 | 1.11 / 2.08 | 0.000 | 3/3 |
| 640 | opus-rs | 38.7 / 46.9 | 114.4 / 114.4 | 1.20 / 95.98 | 15.662 | 2/3 |
| 768 | Opura | 58.7 / 61.2 | 30.1 / 30.1 | 1.16 / 1.39 | 0.000 | 3/3 |
| 768 | opus-rs | 43.1 / 43.7 | 136.4 / 136.4 | 0.87 / 3.47 | 0.000 | 0/3 |
| 896 | Opura | 66.2 / 69.0 | 34.6 / 34.6 | 1.70 / 566.33 | 95.824 | 2/3 |
| 896 | opus-rs | 53.8 / 65.6 | 158.5 / 158.5 | 6.19 / 26.72 | 5.907 | 0/3 |
| 1,024 | Opura | 71.9 / 79.3 | 39.0 / 39.0 | 1.62 / 465.30 | 85.084 | 2/3 |
| 1,024 | opus-rs | 55.6 / 58.7 | 180.6 / 180.6 | 1.42 / 14.88 | 0.000 | 0/3 |
| 1,152 | Opura | 84.4 / 90.3 | 43.5 / 43.5 | 2.29 / 28.00 | 21.963 | 2/3 |
| 1,152 | opus-rs | 68.7 / 79.4 | 202.7 / 202.7 | 1.04 / 16.33 | 0.000 | 0/3 |
| 1,280 | Opura | 97.5 / 98.2 | 47.9 / 47.9 | 38.82 / 355.32 | 97.099 | 0/3 |
| 1,280 | opus-rs | 73.7 / 81.9 | 224.7 / 224.7 | 0.73 / 2.10 | 0.000 | 0/3 |
| 1,408 | Opura | 98.9 / 99.5 | 52.4 / 52.4 | 319.73 / 407.67 | 95.975 | 0/3 |
| 1,408 | opus-rs | 84.4 / 88.1 | 246.8 / 246.9 | 2.59 / 3.87 | 0.000 | 0/3 |
| 1,536 | Opura | 98.5 / 99.5 | 56.8 / 56.8 | 386.09 / 448.33 | 97.892 | 0/3 |
| 1,536 | opus-rs | 88.1 / 91.2 | 268.9 / 268.9 | 4.33 / 8.84 | 0.000 | 0/3 |
| 1,664 | Opura | 99.4 / 99.7 | 61.3 / 61.3 | 469.77 / 700.19 | 96.067 | 0/3 |
| 1,664 | opus-rs | 94.4 / 96.8 | 291.0 / 291.0 | 3.98 / 54.14 | 58.012 | 0/3 |
| 1,792 | Opura | 98.1 / 98.5 | 65.7 / 65.7 | 769.30 / 865.48 | 98.548 | 0/3 |
| 1,792 | opus-rs | 98.7 / 99.9 | 313.1 / 313.1 | 52.08 / 178.77 | 92.501 | 0/3 |

## CPU profile at 768 streams

In one additional Opura run on CPU 2 with seed 7, 28 of 60 sampled instruction pointers in the worker executable fell inside `SynthesisState::apply`. The next most frequent function, `silk::excitation::shell`, had 10. The worker completed 61,440 packets with zero misses. GNU libc `profil(3)` sampled only the timed interval between the worker’s two `ticks` calls. The histogram excludes shared-library and kernel addresses. With one short sample this identifies SILK synthesis as a candidate for investigation. It gives no estimate of the speedup from changing that function. [SILK synthesis implementation](../../src/silk/synthesis.rs)

<details>
<summary>Reproduce the instruction-pointer sample on x86-64 Linux with GNU libc and GDB</summary>

From the standalone output directory produced below, find the file addresses of `main` and the mangled `active_speech_capacity5ticks` symbol with `nm opura`. The measured binary used `0x3ba30` and `_RNvCsfEWn78RJYXZ_22active_speech_capacity5ticks`. Run `taskset -c 2 gdb -nx -q ./opura` and enter these commands, substituting the symbols for your build when needed.

```gdb
handle SIGPROF nostop noprint pass
break _RNvCsfEWn78RJYXZ_22active_speech_capacity5ticks
run 768 80 7
set language c
set $buf = (char *)calloc(1048576,1)
set $base = (long)&main - 0x3ba30
print (int)profil($buf,1048576,$base,65536)
continue
print (int)profil(0,0,0,0)
dump binary memory profile-glibc.bin $buf $buf+1048576
continue
```

Each little-endian unsigned 16-bit entry at index `i` counts samples for executable addresses `2*i` and `2*i+1`. Sum entries between a function's file address and its end using the start and size reported by `nm -n -S -C opura`. The sum of all entries is the executable sample count.

</details>

## Reproduce

Use little-endian Linux with Python 3.10+, taskset, rustup and FFmpeg/ffprobe with libopus support. The measured inputs were encoded by FFmpeg 7.0.2 at 24 kbit/s constant bitrate in VoIP mode. The runner checks every packet digest before measuring. From the Opura repository root, choose a free CPU and a new output directory. The following commands use CPU 2 and the original fixture revision. [Pinned Golos recordings](https://github.com/ekhodzitsky/gigastt/tree/a2c09d4ef842158b5a33d0dc5ab01f40c35be059/crates/gigastt/tests/fixtures)

```sh
git clone https://github.com/ekhodzitsky/gigastt.git /tmp/opura-capacity-gigastt
git -C /tmp/opura-capacity-gigastt checkout --detach a2c09d4ef842158b5a33d0dc5ab01f40c35be059
rustup toolchain install 1.100.0 --profile minimal
cargo +1.100.0 info opura@0.2.8
cargo +1.100.0 info opus-rs@0.1.42
python3 tools/benchmarks/active-speech/run.py \
  --wav-dir /tmp/opura-capacity-gigastt/crates/gigastt/tests/fixtures \
  --cpu 2 --out /tmp/opura-active-speech
```

The driver creates a standalone Cargo project in the output directory and builds each published crate separately with the recorded lockfile, release profile and native CPU tuning. It checks the decoded float samples and final ranges against the reference hashes, then writes per-trial results to `primary.jsonl` and `supplemental.jsonl`. Add `--qualify-only` to stop after verifying the input packets and both decoders. [Driver](../../tools/benchmarks/active-speech/run.py) · [Measured Rust worker](../../tools/benchmarks/active-speech/worker.rs)
