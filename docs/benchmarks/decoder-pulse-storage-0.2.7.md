# CELT pulse-vector storage experiment against v0.2.6

The candidate used **4.00% less codec time** across ten equally weighted
output configurations. The candidate/baseline ratio was **0.9600**
with a paired bootstrap 95% interval of **0.9535–0.9688**. The trial met both acceptance criteria.
All ten median times decreased. Nine of the ten individual 95% intervals fell
below 1.00. The 8 kHz stereo interval included 1.00.
[All 300 samples, build hashes and paired order](results/decoder-pulse-storage-0.2.7.json)

## Change and correctness

The stable decoder returns an owning pulse vector to the CELT partition leaf.
The candidate decodes the CWRS index into a caller-owned scratch slice and
returns the squared norm. The public codebook API still returns an owning
vector. The partition retains its enclosing entropy transaction and uses
the same normalization, spreading and collapse-mask logic as the baseline.

In these optimized x86-64 binaries, the baseline leaf contains four calls to
`memcpy` with a constant 360-byte length. The candidate leaf contains none.
This observation concerns compiler output for this function. The timings below
include full normal packet decoding through the public API.

Both builds passed the 12 official RFC 8251 streams at all five output rates
in mono and stereo. Every PCM SHA-256 and all 200,750 encoded final-range
checks agreed. A separate candidate build prefilled the pulse buffer with
a nonzero sentinel. The native PVQ and recursive partition fixtures still
passed. Deliberately omitting the zero-suffix write caused the PVQ fixture
to fail. The candidate passed debug and release tests, formatting, Clippy,
rustdoc, dependency checks, the Python tests and the package consumer check.
[Baseline conformance](../validation/results/decoder-baseline-0.2.6-conformance.json)
· [Candidate conformance](../validation/results/decoder-pulse-storage-0.2.7-conformance.json)

## Timings

Median microseconds per packet, measured on an AMD Ryzen AI 9 HX 370.
Lower ratios mean less codec time.

| Output Hz | Channels | Baseline µs | Candidate µs | Ratio | Paired 95% interval |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 8000 | 1 | 25.58 | 24.44 | 0.9556 | 0.9380–0.9965 |
| 8000 | 2 | 30.14 | 29.22 | 0.9694 | 0.9544–1.0041 |
| 12000 | 1 | 26.35 | 24.60 | 0.9335 | 0.9146–0.9765 |
| 12000 | 2 | 30.70 | 29.64 | 0.9654 | 0.9495–0.9987 |
| 16000 | 1 | 26.20 | 24.99 | 0.9538 | 0.9210–0.9652 |
| 16000 | 2 | 30.70 | 29.97 | 0.9761 | 0.9442–0.9855 |
| 24000 | 1 | 25.64 | 24.84 | 0.9687 | 0.9380–0.9845 |
| 24000 | 2 | 31.10 | 29.73 | 0.9561 | 0.9281–0.9805 |
| 48000 | 1 | 26.24 | 25.05 | 0.9546 | 0.9332–0.9869 |
| 48000 | 2 | 30.90 | 29.91 | 0.9679 | 0.9512–0.9880 |

Each arm replayed all 20,075 packets once for warmup and once for measurement
in each of 15 paired rounds. The shortest retained sample was 480.31 ms.
The acceptance criteria set before timing were an aggregate upper 95% bound
below 1.00 and a per-configuration upper bound of at most 1.05 for every case.
The existing driver uses 2,000 paired bootstrap resamples. CPU affinity was
pinned to logical CPU 0. Frequency and other system activity were uncontrolled.

## Reproduction

Build the two versions from tags `v0.2.6` and `v0.2.7`, respectively.
Build each adapter with `cargo build --offline --locked --release --example decoder_bench`
using Rust 1.87.0 and the default release profile. Set `BEFORE` and `AFTER` to the
absolute checkout directories and `REPORTS` to this repository's
`docs/validation/results` directory. Use the verified RFC 8251 archive and
a new output directory.

```sh
python3 tools/decoder_change.py \
  --before "$BEFORE/target/release/examples/decoder_bench" \
  --after "$AFTER/target/release/examples/decoder_bench" \
  --before-source "$BEFORE" --after-source "$AFTER" \
  --before-conformance "$REPORTS/decoder-baseline-0.2.6-conformance.json" \
  --after-conformance "$REPORTS/decoder-pulse-storage-0.2.7-conformance.json" \
  --archive "$ARCHIVE" --output "$OUT" --rounds 15 --cpu 0
python3 tools/decoder_change.py --check "$OUT/report.json"
```
