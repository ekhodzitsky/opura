# Packet framing measurements: 0.1.5

Local experiment, 2026-10-29. This continues the
[0.1.4 comparison](local-results.md), with the same 317 packets, seven groups,
21 paired rounds, native harnesses, compiler flags, libopus binary and thresholds.
No comparison requirement was removed or relaxed. This is still a synthetic
framing microbenchmark, not PCM decoding or a comparison with every competitor.

Median nanoseconds per parse plus span fingerprint, lower is better. Baseline
0.1.4 was measured again in this session, not copied from the earlier report.
The libopus column is the paired reference in the final run.

| Workload | Opura 0.1.4 | Opura 0.1.5 | Paired libopus | 0.1.5/libopus |
|---|---:|---:|---:|---:|
| single | 2.62 | 2.64 | 4.97 | 0.531 |
| two_cbr | 3.70 | 4.03 | 6.47 | 0.622 |
| two_vbr | 3.91 | 4.15 | 6.42 | 0.647 |
| multi_cbr | 34.67 | 30.06 | 25.36 | 1.185 |
| multi_vbr | 33.26 | 16.00 | 10.45 | 1.530 |
| padding | 12.48 | 6.81 | 7.44 | 0.916 |
| invalid | 2.48 | 2.34 | 4.08 | 0.573 |

## Gate result

The aggregate target now passes (one-sided 95% bootstrap upper bound <=0.90).
Five per-group targets pass, including padding. **The overall packet gate still
FAILS**: multi-frame CBR has an upper ratio bound of 1.211 and VBR 1.586, both
above 1.050. Their median times remain 19% and 53% above libopus respectively.
Full-codec superiority also fails because PCM decoding and required competitor
adapters are missing. We do not claim overall leadership.

Compared with the fresh 0.1.4 baseline, multi-frame CBR time falls about 13%, VBR
52%, and padding 45%. There are tradeoffs: two-frame CBR/VBR times rise about
9%/6% in these runs, although both remain faster than the paired libopus artifact.
The benchmark consumer's inlined `packet::fingerprint` machine code grows from
2448 to 5629 bytes (Rust 1.87.0, measured with `nm -S --demangle`). That is not
whole-library size, but signals a code-size/instruction-cache cost. Fixed
`Packet` size remains 792 bytes on x86-64. No allocation or dependency is added.

## Retained changes and validation

- Share validated one/two-frame handling across packet codes, including padding.
- Avoid a length array for CBR. VBR uses a 96-byte u16 length buffer instead of
  384 bytes of usize values on x86-64. Check the final frame size before narrowing;
  keep aggregate arithmetic in usize.
- Use eight-slot inline storage for counts 3–8, with fixed 48-slot fallback.
  Small-array initialization and copies replace full-capacity work in that range.
- Construct views only after count, size, total length and duration checks.
  Internal view builders are inlined to avoid array-return copies; safe Rust and
  the public API remain unchanged.

All 45 Rust tests pass in debug and release. Exact-span tests cover every count
1–48 with CBR/VBR, padded/unpadded packets and lengths 0/1/251/252/1275. Additional
regressions reject 1276/65536-byte frames before narrowing. Three isolated mutations
were caught: missing CBR size check, missing VBR final-size check, and changing
the small-buffer boundary from 8 to 9. Formatting, Clippy with denied warnings,
rustdoc, zero-dependency checks and four competitive-gate unit tests pass.

## Evidence and reproduction

[Fresh 0.1.4 baseline](results/packet-0.1.5-baseline.json) and
[0.1.5 final samples](results/packet-0.1.5-final.json) retain raw timings, order,
correctness status, source/corpus/harness/binary hashes and machine configuration.
The baseline source hash matches commit `d8d808e`; the final source hash identifies
the measured working tree before the release commit. Both native harnesses and
all corpus hashes match between these runs. Runtime frequency/thermals/system
load remain uncontrolled; distributions do not remove systematic benchmark bias.
Samples are batch means, not individual-frame tail latency. C uses its shared
library ABI while Rust permits inlining: these numbers include those API costs.

```sh
python3 tools/packet_bench.py --libopus /absolute/path/to/libopus.so.0 --out target/benchmarks/new.json
python3 tools/competitive_gate.py target/benchmarks/new.json --scope packet
python3 tools/competitive_gate.py target/benchmarks/new.json --scope decoder
```

The last two commands are expected to exit 1 on the current state. Archived
reports are historical evidence; the CLI rejects them after source/harness
changes. See the original [protocol and limitations](local-results.md) and
[pinned competitor registry](competitors.json). The remaining priority is a real
PCM decoder and full-codec comparisons; a framing win cannot substitute for them.
