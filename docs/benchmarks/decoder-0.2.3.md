# Current scalar decoder comparison:0.2.3

This milestone refreshes the native comparison after the CWRS and packet-local
PCM changes. Codec algorithms are unchanged from0.2.2. The optional measurement
adapters and evidence validation change; no native code or third-party dependency
enters Cargo, normal builds or ordinary tests.

## Workload and matching

All12official RFC8251 streams supply20075 packets/320.635seconds of audio per
pass. Five output rates and mono/stereo form ten equally weighted cases.
The Rust public owning PCM API is compared with the pristine public libopus
signed16 caller-buffer API at Opus1.6.1 revision
`22244de5a79bd1d6d623c32e72bf1954b56235be`. API ownership and complete-owner
transaction contracts differ; their costs remain inside their public calls.
No alternate fast path or relaxed error transaction is used for Rust timing.

Both builds target generic x86-64 instructions with compiler auto-vectorization
allowed and scalar dispatch. C uses `-O3 -fwrapv -march=x86-64 -mtune=generic`,
`VAR_ARRAYS`, `OPUS_BUILD`, `FIXED_POINT`, `DISABLE_FLOAT_API`. Rust1.87.0 uses
`-C target-cpu=x86-64`, opt-level3,16codegen units, no LTO/debug/assertions/overflow
checks/incremental build, and unwind. External Rust/Cargo profile overrides are
cleared before these explicit settings are applied. Compiler/source/binary
hashes and the matched profile are retained. QEXT, neural and float paths and
best-available native SIMD are outside this track.

## Minimum time and correctness

A calibrated pass count with25% headroom is only a minimum hint. Both adapters
retain every full pass until that count and the full200ms accumulated decode
time are reached. They fail at1000passes rather than publish a short sample.
Each sample reports its actual count and is normalized by that count; extensions
are not discarded or rerun in another process. The collector and schema2
checker enforce the full minimum immediately and independently. Schema1 retains
its historical schedule/statistics and now also rejects samples below the full
declared minimum. The retained0.1.41 report already exceeds its full200ms in
every sample; its minimum is326.234762ms, so this repair does not invalidate it.

Before timing, current compiled output independently reproduces all120 accepted
PCM SHA256 conformance hashes and encoded nonzero ranges. Both adapters compare
full PCM/range/duration fingerprints. Each process validates before timing,
warms up, times complete passes, separately records call latency and validates
again. Sampled packets/samples/mode counts must remain unchanged. Every subsequent
invocation is checked against calibration; source and binary hashes stay fixed.

Live probes reject altered encoded ranges and incorrect expected PCM/range
fingerprints in both adapters. Separate probes exercise actual extension beyond
one pass, reject1000-pass exhaustion on bounded tiny vectors, and check the
original CLI form without optional arguments. Probe samples are retained.
The optional adapter CLI adds `[MINIMUM_MS] [EXPECTED_FINGERPRINT]` to its prior
arguments; ordinary old-form calls still work.

Fifteen rounds randomize case and implementation order with seed6716. All300
samples, including losing cases, remain visible. Paired equal-tail95% percentile
intervals use2000 bootstrap resamples; aggregate point is the geometric mean of
per-case median-time ratios with equal case weight. The same resampled round
indices are used across cases. The speed-only target remains aggregate upper
bound<=0.90 and every per-case upper bound<=1.05. Passing it alone would not prove
quality, memory, portability or whole-market leadership.

Initialization is timed separately at public construction: Rust creates its
owning state; native creation allocates its state. Per-call p50/p95/p99/max
latency includes timer overhead and is separate from throughput. Owner bytes
mean Rust `sizeof(OpusDecoder)` versus native `opus_decoder_get_size`, excluding
stack temporaries, returned PCM, corpus buffers, process RSS and peak heap.
No peak-memory or steady-state allocation winner follows from these figures.

CPU affinity is fixed and hardware/compiler state is recorded. Frequency,
thermals, background activity and CPU exclusivity are not controlled. No other
build/test/fixture job runs during retained timing. Confidence intervals reflect
run variation, not systematic bias or universal hardware guarantees.

## Reproduction

Use a clean checkout of the annotated milestone, the pinned clean external
reference and checksum-verified archive. All output paths must be new.

```sh
python3 tools/check_conformance.py /path/to/opus \
  /path/to/opus_testvectors-rfc8251.tar.gz \
  --report /new/path/conformance.json
python3 tools/decoder_bench.py /path/to/opus \
  /path/to/opus_testvectors-rfc8251.tar.gz \
  --conformance /new/path/conformance.json \
  --out /new/path/decoder.json --rounds 15 --minimum-ms 200 --cpu 0
python3 tools/decoder_bench.py --check /new/path/decoder.json
```

`--preflight-only` independently checks conformance, matching public output and
live probes, but publishes no retained timings or speed result. Partial snapshots
remain visibly incomplete and cannot pass the checker. Schema2 checking rejects
stale current source and recomputes summaries; historical reproduction requires
the recorded milestone checkout. Structural evidence failures are distinguished
from a valid measurement that fails the speed target. A losing valid report is
retained; `--check` reports the target failure without claiming superiority.

## Result status

All seven offline gates pass at the final source (172 library/44 Python tests).
The full120-case conformance report retains every0.2.2 PCM SHA256. The independent
[preflight report](results/decoder-0.2.3-preflight.json) also reproduces all120
hashes through the newly compiled benchmark profile, verifies all ten native/Rust
fingerprints, and records successful live probes including actual extensions.
A changed-source checker probe is rejected. These results and source identities
are retained in the [correctness evidence](../validation/results/decoder-current-0.2.3-correctness.json)
and [conformance report](../validation/results/decoder-current-0.2.3-conformance.json).

The complete retained comparison is still pending; preflight does not establish
a speed result, current native target success or SOTA claim.
