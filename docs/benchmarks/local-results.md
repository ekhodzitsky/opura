# Local competitive measurements (0.1.4)

Latest follow-up: [0.1.5 measurements](packet-0.1.5.md). This report retains the
historical 0.1.4 results and comparison protocol.

Measured 2026-10-29, Opura 0.1.4 working tree. This is a packet-framing
microbenchmark, **not an audio-decoder benchmark or overall leadership claim**.
The [registry](competitors.json) preserves 25 discovered GitHub repositories
with pinned research evidence, not an exhaustive census. Competitors other
than the explicitly measured system libopus artifact remain unmeasured.

## Results

Median nanoseconds per parse plus frame-span fingerprint; lower is better.
Before is source at `1b6031985d94c411ea19d0772dc028a627b44177`;
after is identified by the full source SHA-256 in the report. Both use the same
317 synthetic framing fixtures and compiler options. Libopus below is from the
after run; before-run reference values are also saved.

| Workload | Packets | Opura before | Opura after | libopus 1.6.1 | After/reference |
|---|---:|---:|---:|---:|---:|
| single | 69 | 32.79 | 2.72 | 5.06 | 0.537 |
| two_cbr | 68 | 35.24 | 3.72 | 6.24 | 0.596 |
| two_vbr | 74 | 35.49 | 3.95 | 6.32 | 0.624 |
| multi_cbr | 13 | 55.29 | 35.82 | 24.77 | 1.446 |
| multi_vbr | 61 | 38.74 | 32.17 | 10.28 | 3.130 |
| padding | 4 | 34.96 | 12.16 | 7.30 | 1.665 |
| invalid | 28 | 9.75 | 2.40 | 4.06 | 0.590 |

**The packet superiority gate FAILS.** One/two-frame packets and invalid inputs
are faster than this libopus artifact, but multi-frame CBR/VBR and padding remain
slower. No losses are excluded. The aggregate ratio's 95% upper bound is 0.997,
above the 0.900 target. The full-codec gate also FAILS: Opura has no PCM decoder.
Encoder quality, loss recovery, memory, portability, Ogg/CLI and usability
superiority remain unestablished. We have not beaten all competitors.

The optimization avoids initializing 48 frame views for one/two-frame packets
and enables cross-crate parsing/accessor inlining. Validation and the public API
are preserved. On x86-64 fixed `Packet` size increases from 784 to 792 bytes:
a speed/size tradeoff, not a memory win. A narrowly scoped Clippy allowance
explains why boxing the large enum variant would violate allocation-free
parsing. No unsafe Rust, heap allocation or dependency was introduced.

## Reproduce offline

On Linux with the pinned Rust toolchain, C compiler, Python standard library
and an existing libopus shared library:

```sh
python3 tools/packet_bench.py --libopus /absolute/path/to/libopus.so.0 --out target/benchmarks/new.json
python3 tools/competitive_gate.py target/benchmarks/new.json --scope packet
python3 tools/competitive_gate.py target/benchmarks/new.json --scope decoder
python3 -m unittest discover -s tools -p 'test_*.py'
```

The runner refuses to overwrite evidence. Separate native executables are built
under ignored `target/benchmarks`; foreign code is never linked into Opura.
There are no downloads or Cargo dependencies. Native timing excludes startup,
file I/O and allocation. Each process warms up for 1000 corpus passes before
20000 timed passes. The 21 paired rounds randomize workload/implementation order
with seed 6716 and pin processes to one allowed CPU.

Every run verifies acceptance/count/offset/length fingerprints against the
fixtures before timing; normal Rust tests independently compare exact spans
and metadata. Timing covers acceptance and spans, not metadata getters, payload
processing or PCM. Fingerprints are regression checks, not cryptographic
correctness proofs. Synthetic boundary fixtures are not a representative
network/audio traffic mix.

C calls the installed shared library through its public ABI; Rust uses its
normal inlinable API. Results include API/ABI costs and do not establish a win
against statically linked, inlined or specially tuned C. The libopus binary hash
identifies the actual artifact; distribution build flags are unknown. Do not
equate it with the README revision in the research registry. The cached opus-rs
0.1.40 private `frame_payloads` helper has a different contract and is not scored
as a public packet parser. It still needs a full-decoder adapter.

Machine: AMD Ryzen AI 9 HX 370, Linux x86-64, CPU 0, performance governor,
Rust 1.87.0 / LLVM 20.1.1, GCC 15.2.0. Frequency, thermals and system load are not
controlled. Rerun on an idle machine before release claims, then add ARM64 and
other toolchains. Raw samples are retained rather than discarding outliers.

## Evidence and gates

[Before samples](results/packet-before.json) and
[after samples](results/packet-after.json) include source/corpus/binary hashes,
compiler flags, environment, correctness status, order and paired distributions.
The baseline source hash was checked against its Git revision. After evidence
records a dirty tree because it preceded the commit; its source hash identifies
the measured code. Harness formatting/provenance metadata improved after the
baseline; the native measured algorithm is unchanged.

The packet gate requires all seven unchanged groups, both implementations,
correct output, provenance, at least 15 finite positive paired samples per group,
a one-sided 95% bootstrap upper bound at most 0.90 for the equal-weight geometric
mean ratio, and at most 1.05 for every group. It uses 2000 paired resamples with
seed 6716. CLI checks reject stale source/harness hashes. Intervals describe
run-to-run noise, not systematic benchmark bias. Samples are **batch means**,
not individual-packet p95/p99 latency.

The full-codec gate currently always rejects: adapters and required conformance,
quality and resource measurements are missing. This is an explicit unimplemented
gate, not a full-codec test suite. Missing competitors cannot count as victories.
Negative unit tests cover absent/corrupt evidence, wrong output, missing baselines,
invalid samples, ties, insufficient gains and hidden per-case regressions.
Ordinary CI runs these deterministic tests; machine-dependent targets run
separately and remain red without making correctness tests flaky.

Next: reduce remaining general-parser overhead; implement PCM synthesis and
frame orchestration; add full-decoder adapters for Rust/portable competitors.
Conformance must precede performance ranking.
