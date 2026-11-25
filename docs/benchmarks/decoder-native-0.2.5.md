# Opura v0.2.5 decoder against scalar libopus 1.6.1

On an AMD Ryzen AI 9 HX 370, Opura used **1.4619×** the decode time of libopus across ten equally weighted output configurations (paired bootstrap 95% interval **1.4563–1.4757**). The interval favored libopus in all ten configurations. The run missed the declared speed target. [Raw samples, probes and build identities](results/decoder-native-0.2.5.json)

## Measurements

Each sample decoded all 12 official RFC 8251 streams (20,075 packets). The table gives median codec time in microseconds per packet over 15 paired rounds per configuration. A smaller ratio favors Opura.

| Output Hz | Channels | Opura µs/packet | libopus µs/packet | Opura/libopus | Paired 95% interval |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 8000 | 1 | 25.65 | 16.63 | 1.543 | 1.504–1.568 |
| 8000 | 2 | 30.43 | 21.04 | 1.446 | 1.431–1.511 |
| 12000 | 1 | 25.81 | 16.98 | 1.519 | 1.491–1.551 |
| 12000 | 2 | 30.96 | 21.44 | 1.444 | 1.418–1.467 |
| 16000 | 1 | 25.15 | 16.56 | 1.518 | 1.508–1.533 |
| 16000 | 2 | 29.84 | 21.00 | 1.421 | 1.406–1.432 |
| 24000 | 1 | 25.64 | 17.26 | 1.485 | 1.470–1.506 |
| 24000 | 2 | 30.56 | 22.25 | 1.373 | 1.346–1.404 |
| 48000 | 1 | 26.08 | 18.04 | 1.445 | 1.423–1.508 |
| 48000 | 2 | 31.40 | 21.93 | 1.432 | 1.409–1.459 |

At 48 kHz stereo, the following quantiles summarize individual decode calls. Each cell is the median of 15 per-process observations. The last column measures the space occupied by the decoder state as reported by each API.

| Decoder | p95 µs | p99 µs | Decoder state bytes |
| --- | ---: | ---: | ---: |
| Opura | 109.52 | 177.08 | 34,520 |
| libopus | 70.75 | 128.27 | 27,124 |

## Protocol and scope

The benchmark used the annotated Opura tag `v0.2.5` (`c9d3ef1d5ca7`) and a clean libopus 1.6.1 checkout (`22244de5a79b`). Both adapters targeted generic x86-64 with optimization and scalar dispatch. The Rust verifier reproduced [all 120 accepted PCM hashes](../validation/results/decoder-pvq-0.2.5-conformance.json). The adapters agreed on all ten output fingerprints and passed ten live validation probes before timing began.

The driver pinned execution to CPU 0 on 7.0.0-42-generic. CPU frequency and background system activity were uncontrolled. Each of the 300 retained samples exceeded the 200 ms minimum (shortest 327.96 ms). The predefined speed gates require an upper 95% bound of at most 0.900 overall and 1.050 for each configuration. The report contains every sample, randomized order, uncertainty estimates and both binary hashes. This workload covers normal packet decoding, including the costs of the respective public APIs. Process memory, explicit loss recovery, encoder quality and optimized SIMD decode require separate measurements. [Benchmark protocol](decoder-0.2.3.md)

## Reproduction

Run from an unmodified checkout of `v0.2.5`, with a clean external `v1.6.1` checkout and the RFC 8251 archive identified in `tools/conformance_manifest.json`. Use a new output path.

```sh
python3 tools/decoder_bench.py /path/to/opus /path/to/opus_testvectors-rfc8251.tar.gz \
  --conformance docs/validation/results/decoder-pvq-0.2.5-conformance.json \
  --out /new/path/report.json --rounds 15 --minimum-ms 200 --cpu 0
python3 tools/decoder_bench.py --check /new/path/report.json
```

The checker recomputes the paired summaries against the frozen source. A speed-gate failure produces a nonzero exit status. The driver validated the complete evidence before writing the final report.
