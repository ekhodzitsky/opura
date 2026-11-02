# Initial real-recording CELT file baseline: Opura 0.1.66

This is a usable-file and signal-diagnostic baseline on two recordings. It
establishes neither perceptual quality leadership nor competitive codec speed.
It is deliberately a CELT comparison: the reference's SILK/Hybrid speech modes,
adaptive bandwidth, VBR, DTX and FEC are outside this experiment.

## Workload and controls

The [official Opus examples](https://opus-codec.org/examples/) provide the
uncompressed `speech_orig.wav` (10.8 seconds, mono) and `music_orig.wav`
(90.796 seconds, stereo). Both are 48 kHz signed16 PCM. Original downloads and
listening outputs stay under ignored `target/`; no recordings are redistributed.
Exact source URLs, SHA256 hashes, sample counts, input peaks and clipping
counts are retained in the [raw report](results/audio-0.1.66.json).

Each recording is encoded at two bitrates (speech 24/48 kb/s; music 64/128
kb/s), with 5 and 20 ms frames. Opura enables all six adaptive CELT settings.
The unchanged fixed-point scalar fast64 libopus v1.6.1 reference is pinned to
`22244de5a79bd1d6d623c32e72bf1954b56235be`. It uses the public restricted-low-delay
encoder, fullband, CBR, forced source channels and complexity 10. This gives
CELT with the same 120-sample lookahead. Every packet's byte size and packet
count must agree; bitrates include TOC bytes and exclude Ogg overhead. The
reference compile flags are recorded; its processing time is not measured.

Both packet streams are decoded with the same unchanged native decoder before
signal comparison. Opura's actual Ogg-to-WAV decode must also match that native
PCM byte for byte, with exact original sample counts. All eight cases pass.
One full file warmup precedes five measured Opura encode/decode process pairs;
encoded and decoded file hashes must remain identical across repetitions.
Raw wall times include process startup and WAV/Ogg file I/O, with warmed cache.
The paths run sequentially and CPU affinity/frequency are not controlled. The
report records hardware, allowed affinity, OS, compiler versions, binary, tool
and codec source hashes. These wall times are an Opura baseline; they cannot
be divided by native core-codec timings to claim a competitive speed ratio.

## Results

Measured on AMD Ryzen AI 9 HX 370, Linux x86-64, Rust 1.87.0 release build.
The retained timing run starts after repository checks finish. RTF is process
seconds divided by audio seconds; smaller is faster, and 1.0 means real time.
No test/build process from this task runs alongside that retained measurement.

| Recording | kb/s | Frame ms | SNR Opura dB | SNR libopus dB | Encode RTF | Decode RTF |
|---|---:|---:|---:|---:|---:|---:|
| speech | 24 | 5 | 12.489 | 12.636 | 0.00497 | 0.00277 |
| speech | 24 | 20 | 15.252 | 15.178 | 0.00341 | 0.00166 |
| speech | 48 | 5 | 20.188 | 20.010 | 0.00538 | 0.00407 |
| speech | 48 | 20 | 19.868 | 19.714 | 0.00435 | 0.00208 |
| music | 64 | 5 | 13.240 | 13.141 | 0.00824 | 0.00366 |
| music | 64 | 20 | 15.224 | 15.055 | 0.00584 | 0.00234 |
| music | 128 | 5 | 19.317 | 18.940 | 0.01113 | 0.00430 |
| music | 128 | 20 | 20.862 | 20.314 | 0.00818 | 0.00306 |

The music original already contains 34 samples at the signed16 endpoints;
encoded results retain 16–40 such samples in Opura and 13–33 in libopus.
Speech has no endpoint samples before or after either encoder. No normalization
or clipping removal is applied to improve the metrics. All PCM diagnostics and
output PCM hashes reproduce across preliminary and final measurement runs.

## Interpretation and next work

The aligned SNR below is `10 log10(sum(input²)/sum(error²))` over the entire
interleaved recording. It is sensitive to waveform phase and is not a listening
score. Correlation, RMSE, mean/max error, RMS, peak and clipped-sample counts
remain in the report, including losing cases. No subjective listening trial,
perceptual metric, confidence interval, expanded corpus, loss test or native
speed comparison is claimed. The two examples are not a representative corpus.

The recordings show a working continuous audio path and no gross timing or
cross-decoder mismatch. They do not justify another correctness-only CELT
micro-milestone. The next major codec work is end-to-end SILK encoding, followed
by Hybrid and the remaining encoder controls. It enables a meaningful standard
Opus speech comparison at low bitrate; decoder optimization remains tied to the
existing separate paired native benchmark rather than this file timing.

## Reproduction and listening artifacts

```sh
env -u RUSTFLAGS -u CARGO_ENCODED_RUSTFLAGS -u CARGO_BUILD_TARGET \
  cargo build --offline --locked --release --bin opura
python3 tools/audio_baseline.py target/release/opura target/reference/opus \
  target/audio-recordings target/audio-baseline-check --rounds 5 --download
```

The output directory must be new. Without `--download`, the hash-verified
recordings must already exist. The clean external reference checkout must be
at the pinned revision. Python's standard library and a C compiler suffice;
no optional dependency enters Cargo or ordinary tests. Each case retains
`opura.opus`, `opura.wav` and `reference.wav` for local playback, plus a complete
`report.json`. Focused ordinary Python tests verify that the diagnostics expose
gain errors, phase reversal, silence, clipping and unequal sample lengths.

All seven mandatory offline gates pass for 0.1.66; the Python tool suite now
contains 19 passing tests. No Rust codec behavior changes in this milestone.
