# Compact Opura in bidirectional RustPBX calls

All 128 calls in the primary test completed, 64 per decoder. At 16 simultaneous calls, compact Opura 0.2.12 reduced the median resident set of the full RustPBX server by **4.0 MiB** compared with stock opus-rs 0.1.32. The median growth above idle was **2.8 MiB smaller**. Opura used **4.3% more server CPU** by the geometric mean of the four paired ratios. The individual ratios ranged from 0.998 to 1.099. [Observations and source identities](results/rustpbx-call-0.2.12.json)

| Full-server metric, 16 simultaneous calls | opus-rs 0.1.32 | Opura 0.2.12 compact |
| --- | ---: | ---: |
| Median steady CPU, equivalents of one core | 0.253 | 0.264 |
| Median idle RSS, MiB | 39.6 | 38.3 |
| Median live RSS, MiB | 62.2 | 58.2 |
| Median live RSS minus the same process's idle RSS, MiB | 22.7 | 19.9 |

All 256 receive streams in the primary runs spanned the measurement window. The observers counted 409,558 RTP packets with zero missing sequence numbers between the first and last observed packets of each stream, zero duplicates and zero reordering. The longest interval between endpoint reads was 36.1 ms for stock and 37.6 ms for compact.

The **64-call feasibility test** completed with both builds. Every call had an active bridge and media in both directions throughout the 35-second measurement window. Stock used 104.8 MiB live RSS and 0.944 cores. Compact used 95.0 MiB and 0.993 cores. Of 767,635 observed RTP arrivals in this pair, neither endpoint had an internal sequence gap, duplicate or reorder. Stock had 50 read intervals above 40 ms across both endpoints, with a maximum of 65.5 ms. Compact's longest was 39.6 ms. One pair on a shared host establishes feasibility at this load. It cannot establish a tail-latency advantage.

For a memory-constrained RustPBX deployment, the compact decoder merits an opt-in pilot. The stock decoder used less CPU in three of the four primary pairs. A production decision still needs a sustained run on the target server with its expected call mix, network conditions and acceptable audio gaps.

In separate two-call checks, both endpoints recorded 12 seconds of incoming audio. All eight recordings tracked the public speech fixture, with 20 ms RMS envelope correlations from 0.984 to 0.993. The time-reversed reference scored about 0.60. Separate single-call tone checks detected the 440 Hz source in all 24 quarter-second windows in both directions on each server.

## Call path

Two local SIP endpoints register with RustPBX 0.5.0. The caller sends Opus. RustPBX decodes and forwards PCMU to the echo endpoint, then encodes the returning audio with its stock Opus encoder. Each run uses a private loopback network. Server CPU and resident memory exclude both endpoints. The control is the stock version's locked opus-rs 0.1.32. The comparison uses Opura 0.2.12's compact decoder. The earlier 500-channel worker test used a different RustPBX revision and opus-rs 0.1.42. [Media worker comparison](rustpbx-compact-0.2.12.md) · [Earlier eight-call SIP test](rustpbx-call-0.2.8.md)

The compact adapter supplies a reusable signed PCM buffer. Opura returns the number of interleaved samples written. The adapter converts that count to frames per output channel before handing the decoded audio back to RustPBX. A stereo regression check caught an initial adapter that counted the channels twice.

The speech workload repeats RustPBX's public 5.44-second mono recording. Of its 272 non-overlapping 20 ms windows, 67 have signed 16-bit RMS above 500. This mix of speech and quiet intervals affects how much work the codecs do. [Original speech fixture](https://github.com/restsend/rustpbx/blob/339f75b99341bcfc440545d70294f5de55652525/fixtures/hello_book_course_zh_16k.wav)

## Measurements

The primary schedule specified four paired runs with fresh server processes. Each run offered 16 calls at two calls per second, with 32 seconds of audio per call. CPU and memory observations use seconds 10–29 after the caller starts. Each direction must have 16 distinct RTP streams spanning this window. The runner also checks SIP responses, active bridges, received packet counts and endpoint audio detection. The secondary feasibility test offers 64 calls at four calls per second for 60 seconds per call and uses seconds 20–55 for process measurements.

CPU is the change in user plus kernel CPU ticks for the PBX process divided by elapsed time, in equivalents of one core. Live RSS is the median of the server's samples during the full-concurrency window. The PBX ran on CPUs 4–7, the clients on 8–11 and the controller on CPU 2 of a shared Ryzen AI 9 HX 370. Results apply to this workload and host.

An observer inside each endpoint timestamps loopback RTP reads and records header fields. Sequence gaps count missing packets between the first and last sequence numbers observed for a stream. End effects can produce different counts at the socket and at the higher-level endpoint statistics. Read intervals include application scheduling. The experiment uses 20 ms packets. An interval above 20 ms by itself does not measure an audio dropout or an internal codec deadline.

## Reproduction

Extract the [reproduction bundle](../../tools/benchmarks/rustpbx-call/compact-0.2.12-reproduce.tar.gz) in a new directory. Use Linux x86-64, Rust 1.98.1, a C toolchain, OpenSSL development files, a protobuf compiler, Python 3.12 or later, and bubblewrap with unprivileged network namespaces. Obtain the official RustPBX 0.5.0, audio-codec 0.4.6 and sipbot 0.2.82 crate archives with their standard `NAME-VERSION.crate` filenames. The build script checks all three hashes. Supply the original speech fixture linked above as `calls/speech.wav`. The runner creates a fresh loopback namespace and SQLite database for each case.

```sh
RUSTUP_TOOLCHAIN=1.98.1 sh build/build.sh /path/to/archives work
cp work/stock work/opura work/sipbot calls/
cp /path/to/opura/tools/benchmarks/rustpbx-call/call.py calls/base_call.py
cp /path/to/opura/tools/benchmarks/rustpbx-call/site.toml calls/
gcc -O2 -Wall -Wextra -Werror -shared -fPIC -o qa/rtp_meter.so qa/rtp_meter.c -ldl
python3 calls/campaign.py
python3 calls/summarize_cases.py
```

The primary schedule and conditional 64-call check are in `calls/protocol.json`. Each case retains its endpoint counters, server samples, bridge lifetimes and header observations. The [measurement file](results/rustpbx-call-0.2.12.json) contains the public fixture and binary identities and the per-process measurements.
