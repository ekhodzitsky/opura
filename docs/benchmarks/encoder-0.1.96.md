# Unified encoder scalar baseline: Opura0.1.96

This benchmark compares public in-process encoding APIs. It adds tooling and
measurements; codec algorithms remain those of0.1.95. It is separate from
whole-process WAV/Ogg diagnostics and does not establish perceptual quality,
SIMD/best-available performance or whole-market superiority.

## Matched workload

The full profile has60 declared cases, all at complexity10:

- Fifteen2s mixed-signal cases: five applications times CBR/VBR/CVBR,
  48kHz mono,20ms and32kbps.
- Ten2s AUDIO/VBR cases: five input rates times mono/stereo,20ms and64kbps.
- Eight2s AUDIO/CVBR stereo cases at48kHz/64kbps with2.5/5/10/40/60/80/100/120ms
  packets.20ms is already covered above.
- Three8s VOIP/VBR cases with DTX,20% loss/FEC, or both, at48kHz mono/32kbps.
- Twenty-four complete recording cases: checksum-pinned10.8s mono speech and
  90.7962s stereo music, AUDIO/VOIP/low-delay/restricted SILK times all three
  rate modes,20ms,32kbps speech and64kbps music.

Synthetic input repeats periodic200Hz waveform plus small noise, noise,
impulses, silence and channel-dependent noise segments. It is a deterministic
mixed signal, not a model of representative speech/music. Real recordings are
the official Opus examples already used by the project; their hashes are in
`tools/audio_baseline.py`. Raw recordings remain local and are not redistributed.

All other controls match: automatic signal/bandwidth/wire selection, fullband
maximum, explicit application and loss/DTX/FEC settings. Restricted SILK cannot
use sub10ms frames; its declared cases use20ms. PCM is padded once before any
measurement, including the application's lookahead and the final partial frame.
Both implementations process exactly the same prepared input blocks. Actual
packet modes, bandwidths, wire layouts, LBRR, empty and one-byte frame counts,
physical bitrate and record identity remain visible. A one-byte compressed
frame is a size observation and is not assumed to prove DTX.
The three loss/DTX cases use two seconds of active input, three seconds of
silence, two seconds active and one second silent. The quiet spans outlast
native VAD hangover. Before timing, requested DTX must reach native
`OPUS_GET_IN_DTX` state and suppressed empty/one-byte frames in both outputs;
requested FEC must produce observable LBRR backups in both outputs. Size markers
alone are not used to claim DTX. Native DTX state queries are untimed.

## Reference and compilation

The reference is pristine Opus1.6.1 revision
`22244de5a79bd1d6d623c32e72bf1954b56235be`, scalar-dispatch fixed fast64,
with floating analysis disabled and without optional neural/QEXT paths.
Opura retains its disclosed Q24 missed-transient correction; this benchmark
does not silently apply that correction to the native encoder. Therefore equal
settings do not promise identical compressed packets, quality or computational
work. Differences in mode mix and physical bitrate are reported explicitly.

The Linux adapters use generic target instructions. On x86-64, C uses
`-O3 -fwrapv -march=x86-64 -mtune=generic` plus the recorded fixed-point build
defines; Rust uses pinned1.87.0, `-C target-cpu=x86-64`, opt-level3,16 codegen
units, no LTO, unwind, no debug/overflow checks/assertions and no incremental
build. Compiler auto-vectorization is allowed; explicit runtime SIMD dispatch
is not enabled. These are scalar-dispatch comparisons, not optimized native
CPU feature comparisons. Compiler, source and binary hashes are recorded.

## Timing, correctness and memory

Input loading and native control setup are untimed. Public encoder creation has
its own timing. Stream resets occur outside measured encoding passes. The
throughput pass includes public encode calls and consuming/releasing their
outputs, including Rust's owning-packet allocation/drop contract. It contains
no decoding, fingerprint hashing or file I/O. A separate pass records individual
public call p50/p95/p99/max latency; timer overhead is not subtracted.

Both adapters first encode and decode the complete input outside timing,
exporting packet bytes, final ranges and full decoded PCM fingerprints. Both
outputs are then decoded by both implementations. Exported record bytes/counts
must agree with the adapter's metadata and whole-byte CBR budgets where used.
Every timed pass must retain packet/byte counts; another untimed complete check
after timing must reproduce the original output. Subsequent process samples
must match the independently verified preflight fingerprint and geometry.
Live wrong-range, wrong-PCM, oversized-packet and wrong-geometry records must
be rejected by both verification adapters before retained measurement begins.

One warmup precedes each process's measured passes. Calibration estimates an
initial pass count with25% headroom from the preflight. Each measured process
then retains every pass and extends its loop until both that count and at least
200ms of accumulated encode time are reached. It fails at1000 passes rather
than publish a short measurement. Resets stay outside accumulated time. Actual
pass counts are reported and used to normalize each sample; no fast passes are
discarded or rerun in a replacement process. The driver checks the full minimum
immediately after every invocation, and the report checker checks it again.
This replaces single-preflight fixed-count calibration, which proved too short
under changing runtime conditions and caused a complete development run to be
rejected. Its data remain local and are not accepted baseline evidence. Fifteen
paired rounds shuffle case order and implementation order with seed6716. CPU
affinity is pinned; governor, frequency, available thermal sensors and load
are recorded before/after timing. Frequency and CPU exclusivity are not controlled.
All task build/test/fixture jobs finish before the retained run.

The report preserves every raw sample. Ratios divide Opura median codec time
by native median codec time; lower is faster. Per-case equal-tail95% paired bootstrap
intervals use2000 resamples (2.5th/97.5th percentile endpoints). The existing
conservative97.5th-percentile upper gate is preserved. The aggregate gives each declared
case equal weight, uses the geometric mean of per-case median ratios and
resamples the same round indices across all cases. This weighting is explicit,
not an estimate of real-world traffic. The speed-only target is aggregate upper
bound≤0.90 with every per-case upper bound≤1.05; passing it would still not prove
quality, memory or market-wide leadership.

`encoder_object_bytes` is Rust `sizeof(OpusEncoder)` excluding reachable heap
versus native's advertised contiguous encoder size, which overestimates its
restricted applications. These are distinct API layout figures. GNU time's
maximum RSS covers the entire child, including PCM loading, validation decoder,
output/corpus buffers and initialization. It is not peak encoder heap or a
steady-state allocation count; no memory winner is inferred from it.

## Reproduction

Use a clean local checkout with the pinned Rust toolchain, a C compiler, GNU time,
a clean pinned external reference, and the existing checksum-verified recordings.
Neither native code nor third-party packages enter Cargo or ordinary tests.

```sh
python3 tools/encoder_bench.py target/reference/opus target/audio-recordings \
  target/encoder-0.1.96 --profile full --rounds 15 --minimum-ms 200
python3 tools/encoder_bench.py --check target/encoder-0.1.96/report.json
```

`--profile smoke --rounds 3 --minimum-ms 20` is an explicitly incomplete development
run. `--preflight-only` checks the full corpus/decoding without retained timings.
The final report checker rejects either as a complete baseline. It also rejects
missing cases/rounds, changed controls/output hashes, invalid timing/memory,
unpaired or altered schedules, stale sources and confidence intervals that do
not recompute from the raw samples. Output directories must be new so prior
failed or successful evidence cannot be overwritten.

Prepared input uses a32-byte little-endian `OPURAB01` header (rate, API channels,
input-rate frame samples, bitrate, cap, application, complexity, rate mode,
loss, FEC, DTX, frame count), then exact interleaved PCM16. It is bounded to64MiB
and100,000 frames. Exported `OPURAE01` records have a24-byte geometry/lookahead
header, then14-byte length/range/PCM-FNV64 headers followed by packet bytes.
These are optional benchmark formats, not transport or production codec APIs.

## Retained results

The complete 60-case run used 15 paired rounds on AMD Ryzen AI 9 HX 370 w/ Radeon 890M,
Linux 7.0.0-38-generic (x86_64), pinned CPU 0.
The equal-case geometric-mean Opura/native median-time ratio is **1.244**,
with paired 95% percentile interval **[1.230, 1.259]**.
The speed-only target is **not met**.
56 cases have intervals wholly above 1.0; 0 wholly below 1.0.
All cases, including losing ones, remain in the table and
[raw report](results/encoder-0.1.96.json). No overall codec/quality win follows.

Every output passes both decoders before timings count. Exported validation
records are identical between encoders in 42/60 cases, and primary
mode counts agree in 60/60. Other records retain differences in
encoded bytes, decoded PCM fingerprints, physical bitrate, bandwidth/wire mix
and recovery/suppression diagnostics. The intentional Q24 correction and
public API ownership/transaction differences remain relevant to interpretation.

For the 8s VOIP control inputs, native DTX state is observed and both outputs
contain suppressed frames; FEC profiles contain real LBRR backups. The report
also retains frame latency quantiles, per-process creation timings and scoped
object/RSS figures. These do not measure steady-state heap allocations or
representative perceptual quality.

Median encode microseconds per packet; each ratio compares equal geometry
within that row. Packet durations differ across rows. Confidence bounds are
the paired 2.5th/97.5th percentile endpoints.

| Case | Opura µs | Native µs | Ratio | 95% interval |
|---|---:|---:|---:|---:|
| frame-120 | 23.49 | 16.81 | 1.398 | [1.338, 1.716] |
| frame-1920 | 245.22 | 193.08 | 1.270 | [1.120, 1.366] |
| frame-240 | 47.40 | 33.55 | 1.413 | [1.290, 1.481] |
| frame-2880 | 366.65 | 288.97 | 1.269 | [1.159, 1.366] |
| frame-3840 | 496.85 | 379.64 | 1.309 | [1.223, 1.441] |
| frame-480 | 76.21 | 52.14 | 1.462 | [1.391, 1.620] |
| frame-4800 | 609.34 | 490.17 | 1.243 | [1.179, 1.303] |
| frame-5760 | 722.90 | 574.38 | 1.259 | [1.201, 1.318] |
| mixed-12000-1 | 61.01 | 45.25 | 1.348 | [1.143, 1.514] |
| mixed-12000-2 | 99.81 | 76.20 | 1.310 | [1.271, 1.584] |
| mixed-16000-1 | 60.95 | 47.83 | 1.274 | [1.085, 1.590] |
| mixed-16000-2 | 100.94 | 79.54 | 1.269 | [1.180, 1.381] |
| mixed-24000-1 | 66.93 | 54.29 | 1.233 | [1.117, 1.321] |
| mixed-24000-2 | 115.58 | 87.41 | 1.322 | [1.233, 1.413] |
| mixed-48000-1 | 83.00 | 60.12 | 1.381 | [1.263, 1.506] |
| mixed-48000-2 | 133.60 | 97.13 | 1.375 | [1.156, 1.515] |
| mixed-8000-1 | 66.80 | 42.55 | 1.570 | [1.434, 1.752] |
| mixed-8000-2 | 96.19 | 68.44 | 1.406 | [1.280, 1.488] |
| mixed-audio-cbr | 70.32 | 52.63 | 1.336 | [1.186, 1.497] |
| mixed-audio-cvbr | 68.07 | 51.70 | 1.316 | [1.226, 1.407] |
| mixed-audio-vbr | 70.78 | 55.43 | 1.277 | [1.050, 1.604] |
| mixed-celt-cbr | 67.19 | 50.96 | 1.319 | [1.261, 1.454] |
| mixed-celt-cvbr | 69.42 | 51.26 | 1.354 | [1.248, 1.478] |
| mixed-celt-vbr | 71.60 | 52.24 | 1.371 | [1.241, 1.442] |
| mixed-lowdelay-cbr | 63.68 | 50.68 | 1.256 | [1.227, 1.363] |
| mixed-lowdelay-cvbr | 65.29 | 51.37 | 1.271 | [1.187, 1.418] |
| mixed-lowdelay-vbr | 70.47 | 51.59 | 1.366 | [1.254, 1.444] |
| mixed-silk-cbr | 475.62 | 417.54 | 1.139 | [1.032, 1.259] |
| mixed-silk-cvbr | 185.41 | 165.07 | 1.123 | [1.015, 1.170] |
| mixed-silk-vbr | 172.48 | 172.22 | 1.002 | [0.956, 1.076] |
| mixed-voip-cbr | 222.70 | 203.34 | 1.095 | [1.008, 1.156] |
| mixed-voip-cvbr | 207.33 | 194.92 | 1.064 | [1.013, 1.129] |
| mixed-voip-vbr | 209.88 | 198.87 | 1.055 | [0.966, 1.141] |
| music_orig-audio-cbr | 125.97 | 100.18 | 1.257 | [1.227, 1.409] |
| music_orig-audio-cvbr | 143.21 | 102.96 | 1.391 | [1.212, 1.447] |
| music_orig-audio-vbr | 126.46 | 100.84 | 1.254 | [1.192, 1.336] |
| music_orig-lowdelay-cbr | 134.12 | 100.23 | 1.338 | [1.221, 1.536] |
| music_orig-lowdelay-cvbr | 130.88 | 100.83 | 1.298 | [1.176, 1.415] |
| music_orig-lowdelay-vbr | 134.93 | 102.54 | 1.316 | [1.214, 1.403] |
| music_orig-silk-cbr | 841.06 | 741.66 | 1.134 | [1.089, 1.195] |
| music_orig-silk-cvbr | 375.89 | 346.46 | 1.085 | [1.021, 1.121] |
| music_orig-silk-vbr | 379.44 | 338.17 | 1.122 | [1.010, 1.151] |
| music_orig-voip-cbr | 148.18 | 120.43 | 1.231 | [1.130, 1.313] |
| music_orig-voip-cvbr | 127.50 | 102.07 | 1.249 | [1.202, 1.370] |
| music_orig-voip-vbr | 151.00 | 109.16 | 1.383 | [1.138, 1.509] |
| speech_orig-audio-cbr | 62.77 | 52.19 | 1.203 | [1.161, 1.265] |
| speech_orig-audio-cvbr | 66.33 | 52.83 | 1.256 | [1.169, 1.306] |
| speech_orig-audio-vbr | 68.13 | 57.50 | 1.185 | [1.105, 1.295] |
| speech_orig-lowdelay-cbr | 67.89 | 51.21 | 1.326 | [1.255, 1.395] |
| speech_orig-lowdelay-cvbr | 67.35 | 53.81 | 1.252 | [1.154, 1.367] |
| speech_orig-lowdelay-vbr | 75.73 | 53.58 | 1.413 | [1.238, 1.496] |
| speech_orig-silk-cbr | 490.75 | 439.37 | 1.117 | [1.024, 1.146] |
| speech_orig-silk-cvbr | 193.94 | 174.49 | 1.111 | [1.028, 1.191] |
| speech_orig-silk-vbr | 190.76 | 170.55 | 1.119 | [1.047, 1.149] |
| speech_orig-voip-cbr | 226.82 | 211.51 | 1.072 | [1.003, 1.214] |
| speech_orig-voip-cvbr | 221.57 | 190.46 | 1.163 | [1.064, 1.315] |
| speech_orig-voip-vbr | 210.89 | 196.05 | 1.076 | [1.014, 1.190] |
| voip-dtx0-loss20 | 225.21 | 217.00 | 1.038 | [0.965, 1.136] |
| voip-dtx1-loss0 | 192.52 | 178.43 | 1.079 | [1.035, 1.161] |
| voip-dtx1-loss20 | 238.08 | 223.84 | 1.064 | [0.979, 1.151] |

Measurements were taken from the clean code revision recorded in the report,
after task build/test jobs finished. Hardware and code/binary provenance are
retained. The post-measurement source/report checker recomputes intervals and
checks the exact schedule and output identity. Reproduction from the final
milestone uses identical measured source files; the subsequent result/document
commit does not alter those sources.

The next performance work should profile the measured gaps and retain the
correctness, quality and transaction contracts. This baseline does not justify
weakening validation, changing codec output to make it cheaper, or claiming
leadership over unmeasured implementations. Multistream and representative
quality/broader competitor work remain open.
