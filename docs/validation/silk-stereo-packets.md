# Fixed-bandwidth stereo SILK encoding

Opura 0.1.68 adds `silk::encoder::stereo::StereoEncoder`: interleaved PCM16
at 8/12/16/24/48 kHz to fixed NB/MB/WB SILK packets of 10/20/40/60 ms.
`StereoOptions` shares the mono complexity, capped-variable-output and loss
scaling controls. Packet budgets include TOC/padding, exclude container overhead;
CBR fills the exact budget. Loss scaling does not emit FEC. External/internal
rates and channel count remain fixed. Errors preserve all input, stereo,
channel, entropy and bitrate state; reset clears stream history.

The path connects per-channel DC rejection/resampling, delayed M/S analysis,
adaptive predictor and width quantization, activity-based rate allocation,
predictor signaling and mid-only suppression with the existing channel encoder.
Side resumption resets prediction/shape/quantizer state while preserving input
delay, frame counter, entropy and VAD history. Multi-frame packet scheduling
distinguishes independent, conditional and independent-without-scaling coding.

## Independent evidence

The external reference is unchanged fixed-point scalar libopus v1.6.1, revision
`22244de5a79bd1d6d623c32e72bf1954b56235be`. It is never a Cargo dependency.

- 144 public native encoding calls match transformed mid/side PCM, all 13
  stereo history fields, allocated rates, predictor indices, mid-only decisions,
  entropy range/tell/fractional tell and complete prefix bytes. The corpus
  changes 64/10/64 kb/s budgets over silence, correlated and independent loud
  channels, for NB/MB/WB and 10/20 ms frames. Regeneration is byte-identical.
- 120 Rust-generated streams cover all 15 rate pairs, four packet durations
  and CBR/capped-variable modes. Their 2880 packets include silence, correlated
  channels, decorrelated noise and loss-scaling changes at complexities 0/6/10.
  The original public decoder matches sample counts, full PCM fingerprints and
  final ranges for every packet in each continuous stream.
- Ordinary Rust tests exercise mid-only omission and side restart, full-scale
  stereo analysis, invalid controls, finalization errors, tight-budget failure,
  reset and complete state rollback. CLI tests cover 120 mono/stereo geometries,
  including empty, one-sample and packet-boundary files and fractional budgets.
- FFmpeg/libopus independently decodes 60 stereo CLI files across all input
  rates, bandwidths and durations with exact source duration. Another 60 mono
  cases pass after integration. Both layouts use 312-sample pre-skip at 48 kHz
  (6.5 ms), lookahead flushing and final-granule trimming. JSON reports retain
  binary hash/version, external tool version, PCM hashes and individual results.

The FFmpeg builtin `opus` decoder's known tail-trimming failure also occurs
in stereo: 48 kHz input, 6874 samples/channel, NB/10 ms yields 6860 samples/channel.
Opura and FFmpeg/libopus yield 6874. The tool retains this failure and requires
the same duration error on a separately FFmpeg/libopus-encoded file; the native
decoder must preserve that reference file's exact duration. This is the same
issue retained in the [mono record](silk-mono-packets.md), not a passing builtin
decoder duration check.

## Reproduction

Use new output paths. The external source checkout must be clean and pinned.

```sh
python3 tools/silk_encoder_stereo_fixtures.py target/reference/opus target/stereo-analysis.txt
cmp tests/silk-encoder-stereo-fixtures.txt target/stereo-analysis.txt
python3 tools/silk_stereo_fixtures.py target/reference/opus --tables > target/stereo-tables.rs
rustfmt --edition 2024 target/stereo-tables.rs
cmp src/silk/stereo_tables.rs target/stereo-tables.rs
cargo run --offline --locked --release --example silk_stereo_vectors -- target/stereo-packets.bin
python3 tools/silk_mono_validation.py target/reference/opus target/stereo-packets.bin
cargo build --offline --locked --release --bin opura
python3 tools/silk_cli_validation.py target/release/opura target/stereo-cli --channels 2
python3 tools/silk_cli_validation.py target/release/opura target/mono-cli
```

All seven required offline gates apply. These results establish usable fixed-
bandwidth stereo encoding and synthetic interoperability. DTX/FEC, adaptive
bandwidth and Hybrid encoding remain unfinished; the full SILK task stays open.
Representative listening/quality, CPU, latency and memory comparisons remain
required. No SOTA or perceptual-quality advantage follows from these checks.
