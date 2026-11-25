# Caller-owned storage for CELT band preparation

The candidate used **1.21% less decode time** than v0.2.8 in aggregate (paired 95% interval for the reduction, **0.19–2.48%**). The 8 and 12 kHz mono upper bounds allowed increases of 7.09% and 6.49%, exceeding the 5% release limit. Both intervals also included equal performance. The released decoder therefore retains the v0.2.8 implementation.

## Change and correctness

Both CELT entropy directions call `BandTransform::prepare`. The candidate places the 176-element folding buffer in the caller and returns a borrowed slice alongside the transform settings. Inspection of native x86-64 release builds confirmed removal of the fixed 704-byte copy on return. The input fold is still copied when present. [Machine code and encoder checks](../validation/results/decoder-band-return-trial-verification.json) · [Baseline implementation](https://github.com/ekhodzitsky/opura/blob/v0.2.8/src/celt/partition/band.rs)

All 120 official stream/rate/channel results matched v0.2.8, including complete PCM hashes, accepted reference metric results and all 200,750 nonzero final-range checks. A separate encoder check compared the CLI on mono speech at 24 kbit/s and stereo music at 64 kbit/s. With CELT frames of 2.5, 5, 10 and 20 ms in both CBR and VBR modes, all 31,216 audio packets matched byte for byte across 16 files. The Ogg vendor version string and its page checksum were normalized for comparison. [Reference conformance](../validation/results/decoder-theta-0.2.8-conformance.json) · [Candidate conformance](../validation/results/decoder-band-return-trial-conformance.json)

## Full decoder performance

Each sample replayed all 20,075 packets from the 12 official RFC 8251 streams. The two versions used the same pinned Rust 1.87.0 default release profile. The benchmark ran 15 paired rounds on an AMD Ryzen AI 9 HX 370, on logical CPU 2. Rates and channel counts had equal weight in the aggregate. The acceptance rule set before measurement required an aggregate upper 95% bound below 1.00 and a per-configuration upper bound of at most 1.05. The table reports median time in microseconds per packet. The host was shared. [All 300 observations and build identities](results/decoder-band-return-trial.json)

| Output Hz | Channels | v0.2.8 µs | Candidate µs | Ratio | Paired 95% interval |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 8000 | 1 | 23.72 | 24.30 | 1.0243 | 0.9724–1.0709 |
| 8000 | 2 | 29.76 | 28.66 | 0.9631 | 0.9461–1.0219 |
| 12000 | 1 | 24.09 | 24.68 | 1.0243 | 0.9427–1.0649 |
| 12000 | 2 | 29.57 | 28.62 | 0.9678 | 0.9491–1.0040 |
| 16000 | 1 | 23.13 | 23.27 | 1.0057 | 0.9386–1.0246 |
| 16000 | 2 | 27.62 | 27.73 | 1.0042 | 0.9813–1.0362 |
| 24000 | 1 | 24.57 | 23.74 | 0.9665 | 0.9346–0.9989 |
| 24000 | 2 | 29.12 | 28.53 | 0.9800 | 0.9602–1.0254 |
| 48000 | 1 | 24.47 | 23.91 | 0.9772 | 0.9641–0.9977 |
| 48000 | 2 | 30.50 | 29.55 | 0.9688 | 0.9154–0.9946 |

## Two profiled CELT streams

A separate 16 kHz mono comparison used rustc 1.100.0 with native CPU tuning, matching the build profile where the return copy was identified. After one warmup pass, each process timed eight passes per stream. Each stream had 15 paired rounds. The same host and logical CPU were used after the full decoder benchmark had finished. Both builds reproduced the reference PCM and all 6,333 encoded final ranges.

| RFC stream | v0.2.8 µs/packet | Candidate µs/packet | Paired median time ratio | 95% interval |
| --- | ---: | ---: | ---: | ---: |
| 01 | 33.045 | 30.992 | 0.9458 | 0.9183–0.9716 |
| 07 | 9.818 | 9.401 | 0.9793 | 0.9592–1.0133 |

Stream 01 improved by 5.42%. The interval for stream 07 includes equal performance. The release decision uses the ten configurations above. [Individual stream measurements, build identities and checksums](results/decoder-band-return-targeted.json)

## Reproduce

Check out v0.2.8 in two directories. Apply the [candidate patch](decoder-band-return-trial.patch) in AFTER using `git apply`. In both directories build the adapter using `cargo build --offline --locked --release --example decoder_bench`. Set BASE and AFTER to the checkout paths, REPORTS to the conformance reports in this repository, ARCHIVE to the verified RFC 8251 archive and OUT to a new directory.

```sh
python3 tools/decoder_change.py \
  --before "$BASE/target/release/examples/decoder_bench" --before-source "$BASE" \
  --after "$AFTER/target/release/examples/decoder_bench" --after-source "$AFTER" \
  --before-conformance "$REPORTS/decoder-theta-0.2.8-conformance.json" \
  --after-conformance "$REPORTS/decoder-band-return-trial-conformance.json" \
  --archive "$ARCHIVE" --output "$OUT" --rounds 15 --cpu 2
python3 tools/decoder_change.py --check "$OUT/report.json"
```
