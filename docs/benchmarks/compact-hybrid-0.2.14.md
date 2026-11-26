# Retained memory on active mono Hybrid SIP streams

Opura 0.2.14 reduced warm process RSS by **34.1%** and private dirty memory by
**37.3%** at 768 simultaneous mono Hybrid streams compared with the published
0.2.13 compact decoder. Both builds used Rust 1.87.0 and default Cargo release
settings on an AMD Ryzen AI 9 HX 370, Linux x86-64.
[Measurements and protocol](results/compact-hybrid-0.2.14.json)

| Active streams | Warm RSS, KiB (0.2.13 / 0.2.14) | Warm private dirty, KiB (0.2.13 / 0.2.14) |
| ---: | ---: | ---: |
| 1 | 2,872 / 2,928 | 388 / 376 |
| 128 | 7,240 / 5,548 | 4,676 / 3,040 |
| 768 | 28,780 / 18,968 | 26,276 / 16,476 |

Each entry is a median over five fresh processes. Every decoder processed
the first 24 consecutive 20 ms packets of the captured Mandarin Hybrid
fullband SIP stream from the [0.2.13 study](sip-decoder-0.2.13.md). The worker
shared caller-owned PCM buffers and compared each stream's samples and final
range with the first stream. All retained decoder states were live at the
`/proc/self/smaps_rollup` reading. The measurements exclude SIP signaling,
network buffers and application state.

## CPU and packet latency

Two independent 32-second continuous calls were replayed at 48 kHz mono.
Each phase used 20 mirrored pairs of fresh processes pinned to CPU 4.
CPU measures a complete call with preloaded inputs and fresh decoder states.
The p99 phase measures individual packet wall-clock decode duration with
`Instant`, independently of total thread CPU. Ratios are geometric paired
ratios with descriptive 95% intervals bootstrapped over mirrored blocks.

| Recording | Paired CPU change (95% interval) | Median per-run p99, µs | Paired p99 change (95% interval) |
| --- | ---: | ---: | ---: |
| Mandarin | +1.74% (−0.31% to +3.82%) | 55.54 / 57.88 | +2.85% (−1.67% to +8.14%) |
| AMI | −0.75% (−1.09% to −0.41%) | 53.74 / 52.00 | −3.03% (−12.17% to +6.90%) |

The preregistered gates require at least 6,144 KiB lower private dirty
memory at 768 streams, CPU ratio at most 1.02 with upper interval at most
1.05, and p99 ratio and median ratio at most 1.05 with upper interval at most
1.10. All passed. These data establish a memory saving with bounded measured
CPU and latency changes on the tested workloads.

## Retained state and diagnostic reads

Mono output has a single synthesis and concealment channel even when a packet
encodes stereo. Both encoded-channel energy histories still have to be kept.
A compact mono stream now keeps 7,672 bytes inline and 13,808 bytes of CELT
history on the heap on this target, versus 7,648 and 26,896 before. The
structural saving is 13,064 bytes per active stream, excluding allocator
overhead. Stereo output continues to retain both synthesis channels. The
fixed-storage decoder remains allocation-free and keeps its existing layout.
Transactions still stage the complete fixed decoder.

The public `celt()` method returns a reference to a complete diagnostic
`CeltDecoder`. On an active packed mono stream the first call creates a
full diagnostic snapshot. The next successful CELT-state commit reuses that
snapshot's allocation and drops the packed storage. The stream then retains
full state until reset. Failed operations preserve the diagnostic snapshot.
Logical equality, cloning and debug output remain independent of whether
diagnostics have been read.

In a supplemental 768-stream experiment reading `celt()` on every packet
increased median warm RSS from 28,796 KiB for 0.2.13 to 39,172 KiB for 0.2.14.
Private dirty memory rose from 26,272 to 37,244 KiB. This includes allocator
retention after promotion. Applications using this diagnostic method on
every stream should account for the additional peak and retained process
memory. Ordinary `decode_into` and `final_range()` do not materialize the
snapshot.

## Correctness and reproducibility

The independent test compared both public APIs and both output channel
counts at 48 kHz against the published 0.2.13 crate on all 12 RFC 8251 streams
and both SIP recordings. All 104 trace hashes matched. The extended traces
include loss, FEC, failed small buffers, reset, gain, clone equality and
diagnostic history inspection. Plain traces exercise uninterrupted packed
state. The extra diagnostics regression covers equal logical state before
and after promotion and failed operations.

Both public decoders passed all 120 official RFC 8251 vector, rate and channel
combinations with 200,750 final-range checks per API. Every PCM digest
matched 0.2.13. Debug and release tests each passed 454 tests, and the
installed CLI reproduced all 15 Golos WAV digests.

Recreate the length-prefixed Mandarin and AMI inputs using the
[0.2.13 replay bundle](../../tools/benchmarks/rustpbx-call/decoder-0.2.13-reproduce.tar.gz).
The [0.2.14 measurement bundle](../../tools/benchmarks/rustpbx-call/compact-hybrid-0.2.14-reproduce.tar.gz)
contains the worker sources, frozen protocol, and instructions to reproduce
the measurements and correctness checks with the published 0.2.13 package
and the v0.2.14 source checkout.
