# Memory and CPU with the compact speech decoder

At 768 concurrent speech streams on one x86-64 worker, switching from the fixed-storage `OpusDecoder` in 0.2.10 to the opt-in `CompactOpusDecoder` in 0.2.11 reduced warm process RSS from **29.89 to 10.24 MiB**. Once every stream had decoded a CELT packet, RSS was 30.02 and 30.05 MiB respectively. A compact decoder saves memory while a stream can use SILK alone. [Measurements and per-run values](results/compact-speech-0.2.11.json)

| Streams | Warm RSS, MiB fixed / compact | Warm private dirty, MiB fixed / compact | CPU seconds per minute of speech, fixed / compact | Paired compact/fixed CPU ratio | First CELT p99, µs fixed / compact |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 4.16 / 4.18 | 1.74 / 2.30 | 0.04896 / 0.05429 | 1.1002 | 111 / 121 |
| 128 | 8.44 / 5.18 | 6.00 / 3.30 | 0.05823 / 0.05724 | 0.9877 | 65.3 / 81.8 |
| 768 | 29.89 / 10.24 | 27.47 / 8.35 | 0.05839 / 0.05804 | 0.9752 | 62.4 / 80.7 |

Each arm has eight fresh processes per load. The table uses the median across processes for each arm and the median of paired within-round ratios for the CPU comparison. The ratio can differ from the quotient of the two medians. At 128 streams the descriptive 95% paired bootstrap interval for the geometric mean CPU ratio was 0.968–1.021. At 768 it was 0.935–0.998. One-stream results give no evidence of a CPU or RSS saving. The measured inline object occupied 34,520 bytes for fixed storage and 7,648 bytes for compact storage. RSS includes the corpus, output buffers, executable, allocator and all decoder states.

## Workload

One unpaced worker decoded 20 ms SILK packets to 320 mono i16 samples at 16 kHz with `decode_into`. Each stream retained its decoder and output buffer. Streams rotated across three 120-second AMI meeting excerpts at deterministic, staggered offsets and warmed on 16 packets. The one-stream case used only ES2002a. At 1, 128 and 768 streams the worker then decoded 4,096, 128 and 64 timed packets per stream respectively. Each process advanced the streams in round-robin order. The thread CPU clock covered the packet calls and iteration. The memory snapshot came after warmup, while every stream was resident. The receipt also gives steady-state values and the process high-water mark.

Each warm stream then decoded its first wideband CELT packet. Time was measured around each public call using a monotonic clock. The table gives the median of each process's nearest-rank 99th percentile. With one stream this is a median of eight single observations. The same fixed 20 ms CELT packet, with a stereo TOC decoded to mono, came from the baseline's checked-in mode-transition fixture. A separate full replay fingerprinted every sample, PCM length and final range for warmup, timed speech and the first switch. Both arms matched all three expected SHA-256 values. [Switch fixture](../../tests/opus-packet-fixtures.txt) · [Measured worker](../../tools/benchmarks/compact-speech/worker.rs)

The compact decoder still stages a full decoder in temporary storage on a call. The memory saving comes from the smaller state kept between packets. A separate allocator probe observed one allocation on the compact decoder’s first CELT or Hybrid use. The warmed CELT, Hybrid and subsequent SILK packet calls in that probe produced zero additional allocation or free events.

The test compared the 0.2.10 release to the compact decoder implementation prepared for 0.2.11. All builds used Rust 1.87.0 with the generic Cargo release profile. The host ran Linux x86-64 on an AMD Ryzen AI 9 HX 370 with the performance governor. Workers were pinned to CPU 10, whose SMT sibling was CPU 22. Load order rotated between rounds, and the first implementation alternated within each load. Other workstation activity and clock frequency were uncontrolled. These results cover unpaced speech streams and one type of switch on this host. Real-time packet scheduling, speech recognition and mixed-mode traffic require separate measurements.

## Reproduce

Use Linux x86-64, Python 3.11 or later, taskset and the Rust 1.87.0 toolchain. Obtain the mixed-headset WAV for each meeting from the AMI Consortium's AMI Meeting Corpus, distributed under CC BY 4.0. Place the original `ES2002a.Mix-Headset.wav`, `IS1000a.Mix-Headset.wav` and `TS3003a.Mix-Headset.wav` in one directory. The receipt includes their individual download URLs and SHA-256 hashes. [AMI Meeting Corpus and licence](https://groups.inf.ed.ac.uk/ami/corpus/license.shtml) · [Source and packet hashes](results/compact-speech-0.2.11.json)

The driver selects samples 960,000 through 2,879,999 from each original 16 kHz recording. The measured FFmpeg 7.0.2 static build with libopus encoded mono speech at 24 kbit/s CBR, 20 ms, VoIP application, compression level 10. The driver verifies the complete source, excerpt and length-prefixed packet hashes. Each recording produces 6,001 sixty-byte packets. From a checkout containing both tags, replace the input directory and CPU as needed.

```sh
git worktree add --detach /tmp/opura-fixed v0.2.10
git worktree add --detach /tmp/opura-compact v0.2.11
python3 /tmp/opura-compact/tools/benchmarks/compact-speech/run.py \
  --baseline /tmp/opura-fixed --candidate /tmp/opura-compact \
  --wav-dir /path/to/ami --out /tmp/opura-compact-run --cpu 10
```

The output directory must be new. It retains the generated inputs, both release workers, the output qualification and 48 rows in `observations.jsonl`, including each first-switch duration. For an offline repeat with those prepared packets, replace `--wav-dir /path/to/ami` with `--packets /tmp/opura-compact-run/inputs` and select a new output directory. Add `--qualify-only` to stop after checking the PCM and final ranges. [Driver](../../tools/benchmarks/compact-speech/run.py)

## Decoder correctness

Both public decoders matched all 120 rows in the 0.2.10 RFC 8251 report, including the PCM hashes and 200,750 packet final-range checks. The standard vector procedure exercises the fixed decoder. To reproduce the compact run, use a separate checkout of the same release, change the import in `examples/decode_vectors.rs` from `decoder::OpusDecoder` to `decoder::CompactOpusDecoder as OpusDecoder` and repeat the procedure. The compact report records the source fingerprint of that consumer. The regression suite also covers first use of CELT and Hybrid, reset, packet loss and retry after a failed call. [Vector procedure](../validation/decoder-0.1.40.md#reproduce) · [Compact vector report](../validation/results/compact-speech-0.2.11-conformance.json) · [Transition tests](../../tests/compact_decoder.rs)
