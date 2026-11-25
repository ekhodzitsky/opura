# Direct CELT angle decoding against Opura 0.2.7

The aggregate candidate/baseline decode time ratio was **0.9345** (paired 95% bootstrap interval **0.9263–0.9536**). The candidate met both performance criteria. This comparison covers the full decoder on the 12 official RFC 8251 streams.

## Correctness

The candidate computes symbol intervals directly for two distributions used to decode the CELT split angle. The regression test checks the intervals against the cumulative tables for every possible frequency at every even resolution from 2 to 256. It compares the complete entropy decoder state after each decoded symbol. [Motivating CPU profiles](decoder-competition-0.2.7.md#where-to-optimize)

The candidate produced identical PCM hashes to v0.2.7 in all 120 official stream, output rate and channel combinations. All 200,750 nonzero final ranges matched. [Baseline conformance](../validation/results/decoder-pulse-storage-0.2.7-conformance.json) · [Candidate conformance](../validation/results/decoder-theta-0.2.8-conformance.json)

## Measurements

Median microseconds per packet on an AMD Ryzen AI 9 HX 370. Each timed sample replayed all 20,075 packets after a full warmup. There were 15 paired rounds per configuration, 300 timed samples in total. The aggregate equally weights the ten output configurations. The acceptance criteria set before timing require the upper 95% bound to be below 1.00 in aggregate and at most 1.05 in every configuration. [Raw timings and build identities](results/decoder-theta-0.2.8.json)

| Output Hz | Channels | Baseline µs | Candidate µs | Ratio | Paired 95% interval |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 8000 | 1 | 24.88 | 23.62 | 0.9496 | 0.9335–1.0018 |
| 8000 | 2 | 30.63 | 28.84 | 0.9416 | 0.9001–1.0011 |
| 12000 | 1 | 24.72 | 23.98 | 0.9701 | 0.8982–0.9966 |
| 12000 | 2 | 30.69 | 28.69 | 0.9345 | 0.9131–1.0024 |
| 16000 | 1 | 26.16 | 23.39 | 0.8940 | 0.8717–0.9577 |
| 16000 | 2 | 30.89 | 27.43 | 0.8882 | 0.8595–0.9462 |
| 24000 | 1 | 24.58 | 23.39 | 0.9515 | 0.9194–0.9667 |
| 24000 | 2 | 31.14 | 29.42 | 0.9445 | 0.9176–1.0336 |
| 48000 | 1 | 25.75 | 24.48 | 0.9509 | 0.8932–1.0076 |
| 48000 | 2 | 30.87 | 28.52 | 0.9237 | 0.9028–0.9914 |

Both binaries used the pinned Rust 1.87.0 default release profile and logical CPU 2 under Linux. The machine was shared. The earlier opus-rs comparison used a different compiler and native CPU tuning.

## Direct comparison with opus-rs and system libopus

The release candidate ran a separate comparison with the published opus-rs 0.1.42 and system libopus 1.6.1 on the same machine. Both Rust implementations used rustc 1.100.0 with native CPU tuning. All 72 qualification outcomes, PCM hashes and metric results matched the previous competitive trial. Opura and libopus passed all 24 stream/output combinations. Opus-rs passed 23, with vector 07 failing the stereo PCM check. Each row uses the streams that qualified for both implementations. [Raw observations and candidate source identity](results/decoder-competition-0.2.8.json) · [Comparison protocol](decoder-competition-0.2.7.md)

| Output | Comparator | Streams | Packets | Opura µs/packet | Comparator µs/packet | Paired time ratio (95% interval) |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| 16000 Hz mono | opus-rs | 12 | 20,075 | 21.37 | 16.44 | 1.3063 (1.2925–1.3113) |
| 16000 Hz mono | libopus | 12 | 20,075 | 21.37 | 16.06 | 1.3257 (1.3194–1.3486) |
| 48000 Hz stereo | opus-rs | 11 | 15,889 | 31.04 | 26.38 | 1.1809 (1.1765–1.1870) |
| 48000 Hz stereo | libopus | 12 | 20,075 | 26.92 | 22.30 | 1.2034 (1.1971–1.2180) |

The measured Opura build used a local snapshot of the 0.2.8 release sources. To repeat with a cached copy of the published crate, use the command below and point LIBOPUS_SO at the installed shared library. The paired release comparison above measures the gain over 0.2.7 using a separate compiler and build profile.

## Reproduce

Use the v0.2.7 and v0.2.8 tags for the two checkouts. Build each adapter using `cargo build --offline --locked --release --example decoder_bench`. Set BEFORE and AFTER to the two checkout directories, REPORTS to the checked-in conformance report directory, ARCHIVE to the verified RFC 8251 archive, and OUT to a new directory.

```sh
python3 tools/decoder_change.py \
  --before "$BEFORE/target/release/examples/decoder_bench" --before-source "$BEFORE" \
  --after "$AFTER/target/release/examples/decoder_bench" --after-source "$AFTER" \
  --before-conformance "$REPORTS/decoder-pulse-storage-0.2.7-conformance.json" \
  --after-conformance "$REPORTS/decoder-theta-0.2.8-conformance.json" \
  --archive "$ARCHIVE" --output "$OUT" --rounds 15 --cpu 2
python3 tools/decoder_change.py --check "$OUT/report.json"
```

```sh
python3 tools/benchmarks/competitor/run.py --opura-version 0.2.8 --vectors "$VECTORS" \
  --source "$OPUS_SOURCE" --libopus "$LIBOPUS_SO" --cpu 2 --out target/competition-rebuild.json
python3 tools/benchmarks/competitor/summarize.py target/competition-rebuild.json
```
