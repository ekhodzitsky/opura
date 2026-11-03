# Decoder diagnostic snapshots after 0.2.5

The [0.2.5 paired decoder comparison](decoder-pvq-0.2.5.md) demonstrates a 4.5%
aggregate time reduction versus 0.2.4. This separate investigation selects the
next optimization lead; it does not refresh the native speed comparison.
Codec, Cargo and adapter contents remain those of `c9d3ef1` / `v0.2.5`.

## Rejected clock data

The new gprofng Rust/native runs completed with correct output, but collected
no PC samples and report clock-initialization errors. Their successful process
exit does not establish valid profiling. Controls using the `hi` period collect
sparse data but warn that the interval timer changed from 997 microseconds to
zero. A timer syscall trace observes 100ms timer signals rather than useful
coverage at the declared period. A live perf task-clock probe is unavailable/
denied with `perf_event_paranoid=4`.

The historical 0.2.3 gprofng experiment has the same timer warning. Its sample
percentages must not be used as reliable cost weights or gain predictions.
The accepted uninstrumented version comparisons are independent and remain
valid. All failed experiments and control traces are retained locally.

## Working diagnostic method

An external GDB process launches each unchanged ordinary benchmark executable
on CPU0. Async MI interrupts the child at seeded uniform 12..28ms wall-clock
intervals, records the current PC and up to 25 physical frames, then resumes.
Auto-load and remote debuginfod are disabled. These are diagnostic stack
snapshots, not CPU time fractions, cycle counts or unbiased estimates of cost.
Debugger stop/query overhead, scheduling and unwind/inlining limits apply.

Both executables replay 50 passes of all 20075 official packets at 48kHz stereo.
Each pass represents 320.635 seconds of audio; normal validation/warmup and the
separate latency pass also execute. Each target exits normally, reports the
expected pass/packet/sample/mode counts and matches the complete PCM/range/
duration fingerprint `1d15af7e1c0f5b9c`. Binary hashes and current Rust source
remain unchanged. Rust uses the measured 0.2.5 adapter; native uses the unchanged
pinned scalar Opus 1.6.1 adapter from the 0.2.3 report. The
[sanitized record](results/decoder-profile-0.2.5.json) retains identities/counts.

A one-shot private-PVQ breakpoint verifies unwinding and resumed output. The
one-pass async controls collect 159 Rust / 112 native snapshots and preserve
output. Full runs collect 1666 Rust / 1196 native snapshots; 1638 / 1191 retain
public decoder ancestry. Core-filtering excludes setup and most fingerprint/
replay work, but does not isolate modes, durations or explicit PLC/FEC workloads.

## Observed call sites

Counts below are snapshots with the PC in the named function, restricted to
public-decoder ancestry. They are not function invocation counts or percentages
of decoder time; counts across implementations are not speed ratios.

| Rust function | Snapshots | Native function | Snapshots |
|---|---:|---|---:|
| libc memcpy |275| FFT |126|
| MDCT synthesis |141| CELT decode |109|
| FFT transform |135| inverse MDCT |94|
| partition theta |94| pulse decoding |86|
| CWRS unranking |85| SILK resampler IIR/FIR |74|
| SILK synthesis |69| SILK decode core |61|
| deemphasis |67| partition reconstruction |56|
| libc memset |66| comb filter |52|

GDB resolves the copy routine as `__memcpy_avx512_unaligned_erms`, rather than
an anonymous address. Both scalar codec programs may use optimized system libc;
this is not a change to codec dispatch or compilation profiles.

Across all Rust snapshots, 298 memcpy PCs have immediate callers including
whole `OpusDecoder::clone` (58), private PVQ output (44), staged synthesis (36),
benchmark replay (23), `decode_packet` (21), CELT layer decode (19), CWRS (14),
packet-frame orchestration (12), and MDCT (10). The native run has 12 memcpy
snapshots, principally CELT decode (8). These observations identify leads;
they do not quantify saved time. PVQ still owns an intermediate pulse vector:
0.2.5 removed the normalized-band return/copy, not every PVQ copy.

## Reproduction and next work

Use the [exact historical driver snapshot](decoder-profile-0.2.5-driver.md)
with the source-bound normal binaries and verified vector directory. Its
input paths are relative to the checkout: Rust `target/pvq-025-binaries/after`,
native `target/current-decoder-023-binaries/native`, and official vectors under
`target/decoder-bench/vectors`. The binary hashes are checked against the
retained 0.2.5/0.2.3 reports. New builds need fresh bindings as described in the
snapshot; raw MI logs, mappings and captured PCs remain local-only.

```sh
python3 target/profile-025-gdb.py --passes 1 --output /new/path/control
python3 target/profile-025-gdb.py --passes 50 --output /new/path/profile
```

The next bounded candidate is the complete Opus owner's successful transaction
commit copy. Audit a private rollback guard that takes one snapshot, permits
mutation only under exclusive mutable access, restores every field on errors/
unwind, and disarms on success. This must retain normal/PLC/FEC, gain, transition
and complete-owner rollback/resume contracts, without heap allocation, unsafe
or API changes. Existing public CELT/Hybrid/synthesis transactions remain
independent. No implementation or benefit is established by this report.

Owned MDCT and pulse-vector returns remain alternative leads. Any accepted
change requires independent native/120-case conformance, all offline gates and
full paired measurement against a frozen baseline. Best-available/other
competitor, representative encoder quality, resource and portability acceptance
remain open. Profiling elapsed times and historical ratios are not a new speed
or SOTA result.
