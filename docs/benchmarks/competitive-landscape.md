# Opus competitive landscape and comparison contract

Research snapshot: 2026-10-28. Scope: discoverable GitHub Opus implementations,
Rust adoption alternatives, and relevant file tools. This is a prioritized
landscape, not a claim that every repository or private implementation was found.
Searches covered Rust Opus codecs/decoders, pure-Go codecs, portable ports,
crate repository metadata and known Xiph projects. Stars were not used as a
technical quality ranking.

## Evidence and current position

Rows below summarize **maintainer claims and repository contents**, not benchmarks
independently reproduced by Opura. Source links pin reviewed README revisions.
Selected Rust manifests were also inspected at those revisions. No competitor
was built or timed for this survey. Every comparative performance result is
currently **unmeasured**.

Opura 0.1.2 implements framing, range decoding and SILK side-information
primitives, not a complete PCM decoder. Several competitors already advertise
complete encoders and decoders. Pure Rust and zero dependencies alone are not
unique advantages. Opura's strict policy includes normal, build, dev, optional
and target dependencies; a competitor's “zero runtime dependencies” is a
different claim. Current Opura also forbids unsafe code; SIMD optimization must
respect that policy unless explicitly reconsidered.

## Comparison set

| Repository and pinned evidence | Role | Why it matters / evidence limits |
|---|---|---|
| [xiph/opus](https://github.com/xiph/opus/blob/a8b2545eec71e0f53ecead2d583e7b6baa42684d/README) | Core reference | C codec and compatibility/performance baseline; benchmark both the pinned 1.6.1 oracle and current stable release. |
| [Remade-With-Rust/rusty-opus](https://github.com/Remade-With-Rust/rusty-opus/blob/bccc3a5e7829ad89526c45affd8b74cccda255b2/README.md) | Direct / highest priority | Claims complete Rust codec, conformance and SIMD speedups; runtime zero-dependency claim does not include its dev dependencies. |
| [stephenberry/opus-pure](https://github.com/stephenberry/opus-pure/blob/cf954a76381ae8968b2bc35bb5d578b3a2311d53/README.md) | Direct / highest priority | Claims complete codec plus Ogg and zero dependencies; inspected manifest has empty normal/dev dependency sections. Strong installation and file-workflow baseline. |
| [dignifiedquire/opurs](https://github.com/dignifiedquire/opurs/blob/f0f8941dcc1836a4564cb12f0d3fbe4c0b0c5a6a/README.md) | Direct / highest priority | Claims bit-exact full codec, SIMD, multistream and projection; manifest includes several normal dependencies and optional C comparison tooling. |
| [restsend/opus-rs](https://github.com/restsend/opus-rs/blob/7ad96ec77df7ab7db6d5156b888988dea1d03833/README.md) | Direct / highest priority | Claims full codec and heap-free no_std option; default runtime footprint differs from optional libm/portable-atomic and test dependencies. |
| [jmg049/ruopus](https://github.com/jmg049/ruopus/blob/23166c3c961519349c6c0fc93664c2ed1fc8cc30/README.md) | Direct / highest priority | Claims conformant decoder, compatible encoder, Ogg and Python integration; no_std decoder uses alloc. Good API and packaging comparison. |
| [0x4D44/ropus](https://github.com/0x4D44/ropus/blob/23a8b2885a01f1ce3e656249f3db0416fa904f30/README.md) | Direct / highest priority | Fixed-point Rust port with comparison harness, codec modules, four CLIs, C ABI and player integration. Important fixed-point and tooling baseline. |
| [RealEffect/opus1](https://github.com/RealEffect/opus1/blob/c5e607b7f117568d7c5d0a41f0962ca8968b77d0/README.md) | Direct | Fork of opus-rs with codec controls, multichannel and performance claims. Keep lineage visible rather than count shared code as independent evidence. |
| [TadeuszWolfGang/Rusopus](https://github.com/TadeuszWolfGang/Rusopus/blob/ecb22cf694314eb09384de32a9b9a68059b6ff54/README.md) | Direct decoder | Published as opus-decoder; claims all modes, all output rates, 12 conformance vectors and no unsafe/FFI. Closest safety-oriented decoder baseline. |
| [safewords/rivet-opus](https://github.com/safewords/rivet-opus/blob/67f5278e78c2a385e278f6e5b2d5b3014817457c/README.md) | Direct | Claims RFC-based full decoder/encoder and exact final ranges on all official vectors; one thiserror dependency. Final-range parity alone is not PCM parity. |
| [thesyncim/gopus](https://github.com/thesyncim/gopus/blob/88e87250952ee5cc363fc57f9368258804e2fda5/README.md) | Portable / high priority | Pure-Go codec with caller-buffer APIs, Ogg, multistream, ambisonics and RTP RED; explicit matched-workload benchmarks and scalar/SIMD distinctions. |
| [tphakala/go-opus](https://github.com/tphakala/go-opus/blob/94499993126288e971b59dcec02b0a6808b7eafb/README.md) | Portable | Pure-Go port with separate SIMD dependency; claims complete decoder and reference parity. Compare exact advertised encoder modes separately. |
| [pion/opus](https://github.com/pion/opus/blob/e7b8aa55508cfd30a968dbc695d4272a8e04f2f2/README.md) | Portable / ecosystem | Pure-Go codec in the Pion ecosystem; API readability and WebRTC integration baseline. README alone does not establish complete encoder support. |
| [lostromb/concentus](https://github.com/lostromb/concentus/blob/3885c4e46513ef0fc81fca100189e54f1714c6ca/README.md) | Portable | C#/Java/Go ports based on older fixed-point libopus; useful portability/API baseline, not the sole modern speed or quality reference. |
| [darui3018823/opus](https://github.com/darui3018823/opus/blob/7a9eebe76f646d5cc8d04fb7e7efd48b396a3bc3/README.md) | Portable | Claims full decoder conformance and standards-compatible encoder, with documented exclusions. Do not infer byte-exact encoder parity. |
| [skrashevich/go-opus](https://github.com/skrashevich/go-opus/blob/8c776dbf081651aa45f2c41180fc5e6065f8b3ea/README.md) | Portable | Transpiled Go implementation; compare compiler/runtime overhead separately from hand-written portable implementations. |
| [SpaceManiac/opus-rs](https://github.com/SpaceManiac/opus-rs/blob/31e8ba1ae8abfa31bbe37817dbf0a8ebdeffc31c/README.md) | Rust FFI / API | Safe bindings to libopus (crate opus), requiring C build tooling by default. Not an independent codec; useful Rust ergonomics and FFI-overhead baseline. |
| [Lakelezz/audiopus](https://github.com/Lakelezz/audiopus/blob/701dbf9c07afba9f39e8944c9acdad46b5f921e1/README.md) | Rust FFI / API | High-level libopus binding; useful integration baseline. Record the linked libopus version rather than attribute its performance to a separate codec. |
| [xiph/opus-tools](https://github.com/xiph/opus-tools/blob/5f894e95a50b2004d8c8171082a5fd7327f93653/README.md) | CLI | Encode, inspect and decode file workflows; baseline for eventual Opura CLI behavior and output correctness. |
| [xiph/opusfile](https://github.com/xiph/opusfile/blob/6dfd29e7adb87f2e193575fc3fa88cbf1a0b27df/README.md) | File API | Ogg Opus decoding and seeking API; baseline for pre-skip, gain, seeking, duration and chained-file behavior. |
| [FFmpeg/FFmpeg](https://github.com/FFmpeg/FFmpeg/blob/28fda48e0bf6d9b47d450250f769a58998f0491f/README.md) | Tool ecosystem | Multimedia CLI and library workflows; isolate codec work from demux, resampling, process startup and I/O in performance comparisons. |
| [pathscale/ps-rusty-opus](https://github.com/pathscale/ps-rusty-opus/blob/61be54065d88f464f6b9927a8c861ab46d2b8131/README.md) | Related fork | Synchronous rusty-opus fork; track API/packaging differences without treating inherited benchmarks as independent measurements. |
| [OxideAV/oxideav-opus](https://github.com/OxideAV/oxideav-opus/blob/aaa19fe2bf96ee2f7daf878a01201c5e0cd1bc68/README.md) | Watchlist | Current README describes a clean-room rebuild with SILK PCM work. Search snippets describe an older state; assess each current mode independently. |
| [hasenbanck/opus-native](https://github.com/hasenbanck/opus-native/blob/4ba35348da10381d73b76ac8e64b498d1c83db3c/README.md) | Historical/watchlist | README explicitly says most functionality is not working; not a full-codec performance baseline. |
| [lu-zero/opus](https://github.com/lu-zero/opus/blob/152f1312432da63bfcd836b2bbb641d2a81aa693/README.md) | Historical/watchlist | Pure-Rust decoder project; brief README provides too little evidence to rank current conformance or speed. |

The crates.io metadata for `mousiki` pointed to `cijiugechu/mousiki`, but the
GitHub API returned 404 during this review. Track it as an unresolved candidate;
do not infer deletion, privacy, current capabilities or performance from that
response. Likewise, published package pages and search snippets can lag HEAD
(as observed for oxideav-opus and several version numbers).

## What counts as winning

These are proposed engineering targets, not achieved results. We should win
specific supported workloads and publish tradeoffs. Universal superiority over
all hardware, languages and use cases is not a meaningful testable guarantee.

| Axis | Required evidence and initial target |
|---|---|
| Compatibility | All official decoder conformance vectors; exact final ranges; fixed-point PCM parity where promised; documented float tolerances. Then cross-decoder encoder tests, all modes, durations, rates, stereo, mode changes, FEC/PLC, DTX, malformed packets and multistream. No faster result counts if its output fails correctness. |
| Decoder speed | Identical packets and PCM output format. Initial target: at least 10% lower median codec time than the fastest verified baseline on the primary workload aggregate, with confidence intervals and no unexplained >5% regression in a critical case. Publish per-case results, not only the aggregate. |
| Tail latency | p50/p95/p99/max frame time, initialization separately, cold/warm runs, no steady-state allocations. Do not average away glitches or worst-case malformed-input cost. |
| Memory and size | Bytes/state, peak stack, peak resident/heap memory, allocation count, linked binary size and build time. Initial target: no per-frame allocation and no regression versus the smallest equivalent supported decoder; quantify initialization costs separately. |
| Audio quality | For decoding valid packets, first reproduce correct audio rather than claim perceptual enhancement. Evaluate PLC/FEC separately with fixed loss traces. For the later encoder compare measured bitrate, delay and compute budgets using speech/music corpora, objective distortion and blinded listening with uncertainty estimates; never trade quality away to advertise speed. |
| Usability | Measured time to first successful raw-packet and Ogg roundtrip, compilable examples, caller-owned buffers, actionable typed errors, explicit loss/reset handling, stable API, useful CLI help/exit codes, deterministic offline builds and cross-compilation. |
| Reliability | Bounded work/memory for untrusted input, regression corpus, fuzzing, panic-free public decoding, cross-platform parity, clear transaction semantics and reproducible CI. Report coverage and known limitations instead of promising bug-free behavior. |
| Portability | Linux/macOS/Windows, x86-64/ARM64 and wasm; embedded/no_std only when implemented and tested. Keep unavailable modes visible rather than score them as fast. |

## Benchmark protocol

1. Pin each implementation commit, dependency lockfile, compiler, target flags,
   enabled features and reference version. Record CPU, OS, governor, affinity,
   thermal state and memory. Preserve the current 1.6.1 oracle for regression;
   separately test the current stable libopus before making market claims.
2. Compare scalar-to-scalar and best-available-to-best-available separately.
   Match CPU instruction availability, float/fixed-point mode, output sample
   representation, channels, rate, frame size, bitrate mode, encoder complexity,
   FEC/DTX and analysis/enhancement features. Show unsupported combinations.
3. Use versioned, redistributable speech (several speakers/languages), music,
   transients, noise and silence. Record hashes and licenses. Separate ordinary
   audio, mode/rate transitions, loss traces and adversarial packets. Include
   official conformance vectors independently of subjective-quality material.
4. Measure the in-process codec separately from file/container I/O and process
   startup; also publish an end-to-end CLI track. Use identical workloads,
   warmup, repeated paired runs, randomized run order and raw samples. Report
   distributions, confidence intervals, aggregate weighting and outliers.
5. Use separate external executables for C/Go/C# or dependency-heavy competitors;
   none becomes a Cargo dependency of Opura. Save commands and machine-readable
   results with revision identifiers. Never run a competitor's setup scripts
   simply to read its documentation.
6. A release comparison must reproduce correctness and benchmark results from
   a clean checkout. Self-reported README speed ratios are leads to investigate,
   not numbers to copy into our scorecard.

## Execution order

Finish SILK excitation and synthesis, then stereo/resampling and frame-state
orchestration; complete CELT/Hybrid and loss handling before full-codec claims.
The first external benchmark milestone is a conformant decoder compared with
libopus, rusty-opus, opus-pure, opurs and opus-decoder. Add the remaining direct
Rust implementations and portable implementations as their adapters are verified.
Measure before choosing optimizations. API/Ogg/CLI usability follows a working
decoder; encoder rate-distortion comparisons follow the decoder-first plan.

No speed, quality or overall leadership claim is supported by this research.
