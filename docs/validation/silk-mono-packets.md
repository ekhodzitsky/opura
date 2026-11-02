# Fixed-bandwidth mono SILK packet interoperability

Opura 0.1.67 `silk::encoder::mono::MonoEncoder` accepts mono PCM16 at
8/12/16/24/48 kHz and a fixed internal NB/MB/WB bandwidth. It connects DC
rejection, encoder resampling, the native one-sample channel carry, VAD, native
rate/SNR tables and bit-reservoir adjustment with the buffered channel encoder.
Packets cover 10/20/40/60 ms and contain one/two/three internal frames. The byte
budget includes TOC and packet padding, excludes transport overhead. CBR fills
that exact length; variable output is capped by it. Compressed payloads are
trimmed before padding is added through RFC6716 packet framing. Errors preserve
all input/codec/rate state.

An optional corpus starts 120 independent mono streams: five input rates,
three internal bandwidths, four durations and CBR/variable output. Each stream
contains 12 packets, with initial silence, periodic speech-like signals, loud
noise and loss-scaling changes. The 1440 packets target 32 kb/s packet budgets
and complexities 0/6/10 for NB/MB/WB. The Rust example retains complete packets,
expected decoded sample counts, final ranges and PCM FNV64 fingerprints.
The pinned unchanged fixed-point scalar libopus v1.6.1 decoder at revision
`22244de5a79bd1d6d623c32e72bf1954b56235be` independently decodes each continuous
stream. All 1440 PCM fingerprints, sample counts and final ranges match.

Forty additional native DC-filter cases cover all five input rates, initial
silence, signed16 endpoints, alternating/periodic signals, random PCM and
persistent filter history. Output PCM fingerprints and final states agree;
regeneration reproduces the stored fixture exactly. The SNR table module also
regenerates exactly from the pinned source. Ordinary Rust tests exercise every
rate/bandwidth/duration, padding length boundaries, reset and complete rollback
on invalid input/options and late packet-budget errors.

This establishes synthetic packet interoperability and connected PCM encoding.
It does not establish perceptual quality, real-recording quality, competitive
speed or complete SILK product readiness. Bandwidth remains fixed; adaptive
rate/bandwidth transitions, stereo and DTX/FEC remain work. Mono CLI conversion
is available with `--mode silk`, optional `--bandwidth nb|mb|wb` and
`--frame-ms 10|20|40|60`. It writes312-sample pre-skip and flushes lookahead.
The existing full-SILK task stays open. No native codec is a Cargo dependency.

Reproduce using a new output path and the unchanged pinned external source:

```sh
cargo run --offline --locked --release --example silk_mono_vectors -- target/mono-check.bin
python3 tools/silk_mono_validation.py target/reference/opus target/mono-check.bin \
  --dc-fixtures target/mono-dc-check.txt
cmp tests/silk-encoder-dc-fixtures.txt target/mono-dc-check.txt
python3 tools/silk_encoder_rate_tables.py target/reference/opus > target/mono-rates.rs
rustfmt --edition 2024 target/mono-rates.rs
cmp src/silk/encoder/rate_tables.rs target/mono-rates.rs
```

The oracle uses only Python's standard library, a C compiler and the external
reference. Generated libraries and packet traces remain local under `target/`.


The CLI additionally passes 60 independent FFmpeg/libopus file decodes across
all input rates/bandwidths/durations, with exact source sample counts. Ordinary
CLI tests cover empty/one-sample/exact-boundary files, fractional packet-byte
budgets, pre-skip, impulse alignment, unsupported options and output refusal.

One external default-decoder failure is retained: for a48kHz mono input of6874
samples, NB/10ms packets, the FFmpeg builtin `opus` decoder outputs6860 samples,
with a non-monotonic-DTS warning. FFmpeg `libopus` and Opura both output6874.
The same failure reproduces after FFmpeg remux and on a file produced by
FFmpeg's own libopus/VoIP/CBR encoder (16kb/s,4kHz cutoff,10ms). Thus it is not
specific to Opura's encoder or Ogg writer. The optional tool checks both decoder
paths, retains the failure and requires the independent encoder reproduction;
it does not silently count builtin failures as successful duration checks.

```sh
cargo build --offline --locked --release --bin opura
python3 tools/silk_cli_validation.py target/release/opura target/silk-cli-check
```

The new output directory retains input/output files and a JSON report with
binary/version provenance, both decoder results and the independent failure
reproduction. This file-level evidence does not establish quality or speed
leadership. All seven required offline gates must pass for the release.
