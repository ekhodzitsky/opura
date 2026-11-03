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

Retained results will be added after the verified clean-source run. The full
codec goal, multistream and representative perceptual/broader competitor
comparisons remain open.
