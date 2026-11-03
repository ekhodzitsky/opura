# Packet-local PCM rendering:0.2.2 versus0.2.1

This change renders unified decoded PCM into private frame-local views of the
staged packet output. It removes intermediate whole120ms PCM containers and
active-sample copies from normal/multi-frame decoding and PLC/FEC. Public
owning-output geometry, owner layout, per-frame gain, transition fades and
whole-owner error rollback remain unchanged. Transition PLC uses a bounded5ms
scratch. [Contracts and native validation](../validation/decoder-pcm.md) describe
the lifetime and frame-offset requirements.

The lead was a historical sparse timer profile of0.2.0 showing substantial
copying. Its sample percentages are not a current copy-cost measurement. Rust
source no longer materializes the intermediate containers, but this alone does
not measure peak stack, heap, RSS or a memory win.

## Correctness and builds

All seven offline gates pass at the final codec source (172 library/41 Python
tests). The complete120-case RFC8251 matrix preserves every0.2.1 PCM SHA256 and
encoded nonzero final range. An additional1680-operation pristine native corpus
checks rate/channel/gain geometries, long packets, PLC, prefixed FEC, both mode
transition directions and reset. It passes both0.2.1 and the candidate. Four
source mutations are detected and restored, including a frame-offset overwrite,
wrong PLC gain scope, undersized transition scratch and early owner commit.

Baseline codec source is `9c370ed76007349b2c0e60bf50b89abb68b4087b` (`v0.2.1`);
candidate codec source is `392abd3`. Both public decoder adapters use pinned
Rust1.87.0 and the same ordinary release profile, with no Rust flags or Cargo
profile overrides. Timing excludes file I/O, fingerprinting and instrumentation;
both sides use the same public fixed-storage owning PCM API.

## Reproduction and interpretation

Build `examples/decoder_bench.rs` in clean checkouts of the two versioned
milestones and generate complete source-bound conformance reports in each:

```sh
cargo build --offline --locked --release --example decoder_bench
python3 tools/check_conformance.py /path/to/opus \
  /path/to/opus_testvectors-rfc8251.tar.gz --report /new/path/conformance.json
```

Use the candidate's existing strict comparison driver:

```sh
python3 tools/decoder_change.py \
  --before /path/to/before/target/release/examples/decoder_bench \
  --after /path/to/after/target/release/examples/decoder_bench \
  --before-source /path/to/before --after-source /path/to/after \
  --before-conformance /path/to/before-conformance.json \
  --after-conformance /path/to/after-conformance.json \
  --archive /path/to/opus_testvectors-rfc8251.tar.gz \
  --output /new/path/comparison --rounds 15 --cpu 0
python3 tools/decoder_change.py --check /new/path/comparison/report.json
```

The archive is independently checksum-verified and all120 source-bound PCM
conformance hashes must agree before timing. Each sample replays all20075
packets from all12official streams,320.635seconds of48kHz-clock audio. Five
output rates times mono/stereo form ten equally weighted cases. Fifteen paired
rounds retain300 samples. Case and implementation order are randomized with
seed6716; no slow samples are removed. Each process validates output, warms up,
measures one full replay, records separate call latency and revalidates output.
All retained timing samples must exceed200ms. Source/binary identity and full
PCM/range/duration/mode fingerprints are checked.

The reported ratio divides candidate median codec time by baseline median codec
time. Equal-tail95% paired bootstrap intervals use2000 resamples. Aggregate
weight is equal per case, with the same resampled round indices across cases.
CPU affinity is pinned; frequency, thermals and background activity are not
controlled. Per-case intervals and any uncertain/lost case remain visible.
No other build/test/fixture job runs during the retained timing phase.

This is a version-to-version public decoder measurement. It does not update the
native/best-competitor comparison, measure explicit FEC/PLC throughput, establish
encoder speed/perceptual quality or prove SOTA leadership. The preliminary
7-round48kHz stereo pilot is not a final performance result.

## Retained results

The complete run retains300 samples on AMD Ryzen AI 9 HX 370 w/ Radeon 890M,
7.0.0-38-generic (x86_64), pinned CPU0. The equal-case
geometric mean of candidate/baseline median-time ratios is **0.9571**, with
paired95% interval **[0.9290,0.9883]**: **4.3% less codec time** in this aggregate.
The approximate8% pilot reduction is superseded by the full result.

All per-case point estimates are lower, but every per-case interval includes1.
Individual configuration wins are unproven. Upper bounds exceed1.05 for12kHz
stereo,16kHz stereo,24kHz stereo,48kHz mono and48kHz stereo; the5% per-case
regression ceiling is therefore not established. The aggregate must not hide
these uncertainties. Neither native/best-competitor nor SOTA targets are proved
by this before/after run.

Median codec microseconds per packet across15 samples; each sample replays all
20075official packets. Full PCM/range/duration/mode fingerprints match. Both
public decoder owners remain34520bytes; peak stack/heap/RSS are unmeasured.

| Output Hz | Channels |0.2.1 µs|0.2.2 µs|Ratio|95% interval|
|---|---:|---:|---:|---:|---|
|8000|1|33.95|30.89|0.910|[0.872,1.006]|
|8000|2|40.72|38.65|0.949|[0.877,1.046]|
|12000|1|34.36|34.22|0.996|[0.906,1.048]|
|12000|2|41.00|40.17|0.980|[0.886,1.077]|
|16000|1|34.43|32.95|0.957|[0.845,1.014]|
|16000|2|40.36|39.23|0.972|[0.901,1.069]|
|24000|1|33.25|32.56|0.979|[0.945,1.040]|
|24000|2|39.91|37.97|0.951|[0.910,1.096]|
|48000|1|35.07|33.32|0.950|[0.826,1.064]|
|48000|2|43.02|40.01|0.930|[0.842,1.072]|

The [complete generated report](results/decoder-pcm-0.2.2.json) retains every raw
sample, separate call-latency quantiles, hardware/compiler identity, randomized
schedule and source/binary/driver/corpus hashes. Codec, Cargo and public adapter
contents are identical to measured commit `392abd3`; later release reporting
changes only Markdown/JSON. The measured source/binary/driver bindings are
independently rechecked before retaining the result. This does not combine the
separate0.2.1 measurement into a new0.2.0-versus0.2.2 claim.
