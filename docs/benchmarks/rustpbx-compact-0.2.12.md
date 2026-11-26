# Compact Opura 0.2.12 on RustPBX media egress

The compact decoder is a candidate for a RustPBX pilot with many simultaneous Opus-to-PCMU calls. On 500 one-way speech channels, it reduced the worker's resident memory by about **88 MiB** compared with opus-rs 0.1.42. Paired CPU use fell by **13.9% on Golos** and **19.3% on AMI**. The longer AMI runs still missed the 20 ms deadline on some batches. [Measurements for all 48 processes](results/rustpbx-compact-0.2.12.json)

| Corpus and channels | Decoder | CPU cores | Worker RSS, MiB | Batch p99, ms | Batches over 20 ms |
| --- | --- | ---: | ---: | ---: | ---: |
| Golos, 250 | opus-rs | 0.290 | 63.9 | 8.75 | 0/704 |
| Golos, 250 | Opura compact | 0.250 | 19.7 | 7.48 | 0/704 |
| Golos, 500 | opus-rs | 0.557 | 121.3 | 17.72 | 0/704 |
| Golos, 500 | Opura compact | 0.491 | 33.6 | 14.28 | 0/704 |
| AMI, 500 | opus-rs | 0.599 | 122.9 | 31.03 | 44/4,000 |
| AMI, 500 | Opura compact | 0.475 | 35.2 | 16.84 | 33/4,000 |

CPU and RSS in the table are medians across processes. Percentage CPU changes use the geometric mean of within-pair ratios. Compact used less CPU in all eight Golos pairs at each load and all four AMI pairs. The p99 uses nearest rank across batches. On AMI, the counts of late batches in the four processes were 22, 0, 0 and 11 for compact, and 0, 40, 0 and 4 for stock. The worst batch in each arm took 116.65 and 121.70 ms respectively on this shared host. A deployment with a strict 20 ms deadline needs its own longer load test.

**Choice of Opura API matters.** A separate four-pair control on 500 Golos channels used the same Opura 0.2.12 release in both arms. Selecting `CompactOpusDecoder` lowered worker RSS from 46.4 to 33.5 MiB and used 12.8% less CPU than `OpusDecoder`. Both met all 352 batch deadlines. This comparison covers mono SILK speech. CELT, Hybrid, packet loss and two-way SIP calls have different resource requirements.

The three decoders produced identical PCMU bytes in separate 15-leg checks on both corpora (211,200 bytes for Golos and 2,400,000 for AMI). The 48 timed processes produced 5,408,000 frames in total. Each process matched the expected frame count, silence count and output checksum for its load.

Before timing, the pilot criteria required matching PCMU output, at least 20% lower median RSS, a paired CPU ratio at or below one and at most as many late batches as stock in each comparison. Compact met all four criteria. The two longer AMI runs with late batches in each arm show why a separate deployment deadline budget matters.

## What the worker measures

Each active, one-way call leg receives mono Opus packets. RustPBX consumes its prebuffer, decodes to 48 kHz PCM, resamples to 8 kHz and emits PCMU. All legs become due together every 20 ms. Batch latency runs from the scheduled start until the worker has processed the final leg and checked its output. A late batch can therefore include time waiting for the preceding batch or for the operating system to schedule the worker. This measures a transcoding worker on one logical CPU. For a whole-server example including signaling and transport, see the [eight-call SIP experiment](rustpbx-call-0.2.8.md).

The control uses the decoder from audio-codec with opus-rs 0.1.42. The compact arm calls Opura 0.2.12 at the same sample rate. RustPBX takes ownership of the PCM vector on every packet. The adapter sizes the vector for that packet and decodes directly into it. A third arm uses Opura's fixed decoder with the same output ownership. [Upstream egress and buffer contract](https://github.com/restsend/rustpbx/blob/339f75b99341bcfc440545d70294f5de55652525/crates/rustpbx-media/src/egress.rs#L742)

## Reproduction

Use Linux x86-64, Rust 1.100.0 and the [reproduction archive](../../tools/benchmarks/rustpbx-transcode/compact-0.2.12-reproduce.tar.gz). Extract it into an empty directory. The build script reads the pinned commits from local Opura and RustPBX clones and builds in a new directory. The worker selects its decoder at runtime. All three arms share one executable and the same egress, resampler and PCMU encoder. The lockfile retains the 384 other packages from the [earlier RustPBX test](rustpbx-transcode-0.2.8.md#reproduce).

Prepare the 15 Golos packet files using the [active-speech fixture procedure](active-speech-capacity-0.2.8.md#reproduce). For the second corpus, obtain the AMI Consortium's mixed-headset WAV files as described in the [AMI reproduction instructions](compact-speech-0.2.11.md#reproduce). Place them in a directory under the names `ES2002a.wav`, `IS1000a.wav` and `TS3003a.wav`. The preparation script extracts five separate 21-second segments from each meeting and encodes each starting from fresh encoder state. The receipt records source, PCM and packet fingerprints and the FFmpeg build. [AMI Meeting Corpus licence](https://groups.inf.ed.ac.uk/ami/corpus/license.shtml)

```sh
sh build.sh /path/to/opura /path/to/rustpbx work
python3 prepare_ami.py /path/to/ami-wavs ami-packets
mkdir results
python3 run.py work/runner/target/release/opura-pbx-path /path/to/golos-packets ami-packets protocol.json results
cp protocol.json summarize.py results/
python3 results/summarize.py
```

The runner verifies the input fingerprints before timing. When another FFmpeg build generates identical packets, its version or binary digest can still change the generated manifest. Compare the packet hashes with `reference-ami.json` before using that reference manifest for a matched replay. The frozen schedule uses CPU 4 on a Ryzen AI 9 HX 370. Each run starts a new worker process, warms each leg with 12 packets, then times either 88 or 1,000 consecutive batches. CPU is worker process time divided by the offered real-time duration. RSS is the resident set of the whole worker at the end of the timed window. The raw results also include peak RSS and nanosecond latency for every batch. [Complete measurements](results/rustpbx-compact-0.2.12.json)
