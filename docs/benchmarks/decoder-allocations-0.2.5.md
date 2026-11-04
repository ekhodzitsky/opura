# Decoder allocation-entry observations at 0.2.5

The unchanged Opura 0.2.5 decoder and pinned scalar Opus 1.6.1 show **zero
observed allocator-entry events** inside all measured normal replay windows.
This covers ten output geometries per implementation, five complete windows
per geometry and **1,003,750 normal packet decode calls per implementation**.
Every target exits normally and matches the complete PCM/range/duration/mode
fingerprint. This does not establish zero allocation for every input/API,
zero process heap use, peak memory or a memory/performance winner.

## Scope and identities

Rust codec/Cargo/adapter content remains `c9d3ef1` / `v0.2.5`; subsequent main
commits change diagnostic reporting only. The frozen Rust executable is the
ordinary release binary from the accepted PVQ comparison. The native executable
is the unchanged scalar-dispatch fixed-point Opus 1.6.1 binary from the 0.2.3
comparison, reference revision `22244de5a79bd1d6d623c32e72bf1954b56235be`.
Both retain the existing public APIs and complete output validation.

The SHA256-verified official archive supplies 20075 packets per pass. A fresh
verified extraction is used. Five output rates and mono/stereo form ten cases.
Each process executes validation, warmup, one throughput pass, a latency pass
and final validation through five replay calls. Every replay entry and normal
return is observed. Resets between the twelve streams are inside the windows.
Corpus loading, decoder construction, latency-buffer creation, formatting and
teardown are outside them. The adapter's additional mode-latency work is also
outside these replay windows. Explicit PLC/FEC, malformed-error paths, mapped
streams and encoding are not this workload.

Source, binary, corpus, libc, control-program and driver SHA256 identities and
tool versions are retained in the [complete result](results/decoder-allocations-0.2.5.json).
All twenty observations and their raw producer counters were independently
rechecked after collection. Current codec source still equals the measured
0.2.5 source. No crate, Cargo or benchmark-adapter change is introduced; the
seven offline checks already recorded for that source remain applicable.
This diagnostic report does not substitute new checks for changed codec code.

## Observation method and controls

External GDB starts the unchanged executable on CPU 0. Replay entry enables
allocator breakpoints; a finish breakpoint disables them on return. Both
window boundaries, phase count and normal target exit are required. Auto-load
and remote debuginfod are disabled. A missing window is rejected even when
GDB and the program exit successfully with zero counters.

The observed libc entries are malloc, calloc, realloc, free, posix_memalign,
aligned_alloc and memalign. Rust also observes the standard alloc, alloc_zeroed,
realloc and dealloc shims. Resolved aliases at one address share a breakpoint.
Counts are **entry events**, not unique allocations: nested wrappers/backend
calls can count separately, and Rust/system tiers must not be added together.
Direct mmap, custom allocators and unobserved APIs are not measured. Stack
allocation, requested/live heap bytes and peak RSS are not measured.

A safe standalone Rust allocating control exercises alloc, zeroed alloc,
realloc, dealloc and the over-aligned system path. A separate C control exercises
all seven observed libc APIs. An empty Rust window stays at zero despite
allocations outside that window. Controls are external executables and never
Cargo dependencies or linked codec implementations.

Six live negative controls are rejected: disabled hooks on a known allocating
program; omitted return observation; a missing window; program exit code 7;
and incorrect PCM/range fingerprints in each actual benchmark executable.
The last two targets exit with code 1 after their validation pass. GDB exit
code 0 alone never makes an observation valid. Early exploratory symbol-parser
failures remain local and are not treated as zero-allocation evidence.

The collector bounds each owned process and cleans up an interrupted inferior.
Instrumented elapsed/latency figures are intentionally excluded from the
sanitized result; they are not accepted speed measurements or CPU profiles.

## Results

Each row contains 100375 normal packet calls and five balanced replay windows
per implementation. All twenty observations have zero events at every watched
entry, successful exits and matching output fingerprints.

| Output Hz | Channels | Opura events | Native events |
|---|---:|---:|---:|
|8000|1|0|0|
|8000|2|0|0|
|12000|1|0|0|
|12000|2|0|0|
|16000|1|0|0|
|16000|2|0|0|
|24000|1|0|0|
|24000|2|0|0|
|48000|1|0|0|
|48000|2|0|0|

These observations give no evidence that allocator calls explain the measured
normal-decoder speed gap. They do not identify the CPU bottleneck or refresh
the native timing comparison. State-size differences, transient stack, total
heap/RSS, malformed-input resources, portability and encoder quality remain
open acceptance work. No SOTA or memory-superiority claim follows.

## Reproduction

Use the [exact historical driver snapshot](decoder-allocations-0.2.5-driver.md)
and source-bound binaries from the linked 0.2.5/0.2.3 reports. Copy its four
files under ignored `target/alloc-025/`. It requires local GDB, pinned Rust,
a C compiler, the unchanged native binary and the checksum-verified archive;
ordinary Cargo builds/tests require none of these external tools.

```sh
python3 target/alloc-025/collect.py --output target/decoder-allocations-new
```

Output must be a new directory under ignored `target/`. The driver verifies
all positive/negative controls, output fingerprints and unchanged identities
before accepting the complete observation matrix. Nonzero case counters are
retained rather than discarded. Rebuilt binaries need fresh declared source/
binary bindings and a new run; historical counts cannot silently be transferred.
Raw GDB logs/configuration remain local-only. Linux x86-64 and this libc/toolchain
are the observed platform, not a portability guarantee.
