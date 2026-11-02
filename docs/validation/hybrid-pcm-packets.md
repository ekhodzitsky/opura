# Connected Hybrid PCM encoding

Version0.1.75 adds the normal fixed-band Hybrid PCM/frame/file path. It reuses
the existing SILK channel analysis/NSQ and CELT frame engine with one live range
writer. The SILK payload is neither terminated nor concatenated with a separate
CELT payload. Hybrid DTX/FEC, VBR, cross-mode switching and general mode/channel
selection remain unfinished. This milestone does not finish the full codec.

## Contracts

`hybrid::encoder::HybridEncoder` accepts signed16 interleaved PCM, one or two
channels,24/48kHz and10/20ms frames.24kHz accepts SWB;48kHz accepts SWB/FB.
Its byte budget includes the TOC and is3..1276. Successful packets fill that
budget. Very small budgets can fail explicitly after bounded SILK quantizer
search; a valid byte-count argument does not promise that the frame fits.

Normal encoding includes AUDIO DC rejection, native SILK input resampling and
buffering, WB16kHz channel analysis/quantization, stereo predictor/side restart,
native Hybrid rate splitting and SILK caps, the conditional false transition
redundancy flag, CELT bands17..19(SWB)/21(FB), coarse/fine/final energy coding,
high-band transient/TF/spreading/allocation and spectral coding. Hybrid has no
CELT-only silence/comb syntax or variable TF analysis. Its low-byte/unvoiced and
weak-transient rules, spreading policy and fixed trim are selected separately
from the existing CELT-only path. Complexity and expected loss are per-call
settings; loss currently affects LTP/intra policy and does not enable FEC.

CELT input is delayed4ms to align it with SILK. Delay history is retained before
applying native high-band gain and stereo-width fades so that fades do not enter
SILK's input history. Pre-skip is312 samples on the48kHz Opus clock. The CLI
flushes enough frames, retains fractional packet-byte credit and writes the
original duration through Ogg granules. Empty and partial input frames work.
Errors stage the complete input/filter/resampler/stereo/SILK/CELT state; even an
injected failure after entropy termination leaves the live stream unchanged.
Reset restores a fresh stream at the same rate/channel/band configuration.

The CLI accepts `--mode hybrid --bandwidth swb|fb --frame-ms 10|20`. Defaults are
32kbps mono/64kbps stereo,20ms and the input-rate maximum bandwidth. Unsupported
DTX/FEC/bands/rates/durations are rejected before creating an output. Later
encoding/I/O failures can leave a newly created partial output, consistent with
the existing CLI contract; existing files are never overwritten.

## Original-reference evidence

The unchanged external reference is Opus v1.6.1 revision
`22244de5a79bd1d6d623c32e72bf1954b56235be`, scalar fixed-point fast64, built with
`-O2 -fwrapv -DVAR_ARRAYS -DOPUS_BUILD -DFIXED_POINT -DDISABLE_FLOAT_API`.
Native code is optional validation only, outside Cargo dependencies/tests.

- The unchanged original rate/cap oracle retains8688 exact control snapshots.
- The original Q15 exponential/gain expression and in-place gain/stereo-fade
  functions retain15 gain results and656 fade fingerprints across24/48kHz,
  mono/stereo,10/20ms, zero/unity/intermediate/extreme signed gains and in-place
  PCM. Canonical original windows and caller unity-skip gates are used. Forced
  extreme-rate fixed-point narrowing is retained rather than silently changed.
- 240 encoded streams produce5760 decoder records, including480 explicit lost
  frames. They cover both channel layouts, all three eligible rate/band pairs,
  complexity0/1/2/6/10, four budgets, silence, tonal/noisy/full-scale input,
  mid/side quiet intervals and changing10/20ms geometry. Normal, isolated-loss,
  burst-loss and terminal-loss PCM and public final range match the unchanged
  native decoder exactly. These are Opura-encoded packets, not native-encoder
  byte/PCM equality claims. See the [raw-vector result](results/hybrid-pcm-0.1.75-vectors.json).
- 16 actual CLI files use the previously hash-pinned10.8s mono speech and90.796s
  stereo music recordings, SWB/FB,10/20ms and two bitrates per recording.
  Speech uses32001/48001bps and music48001/64001bps, deliberately exercising
  fractional packet budgets. All60976 actual packets preserve expected
  mode/band/duration/channel geometry, budgets and zero LBRR. CLI WAV output
  equals independent original-native PCM exactly after312 pre-skip with the
  original sample count. See the [file result](results/hybrid-pcm-0.1.75-files.json).
- Six additional impulse diagnostics cover all three rate/band pairs and both
  channel layouts with default CLI rates. A20000-amplitude impulse at10ms in a
  100ms zero recording peaks at output sample480 in each channel after trimming.
  This checks the nominal alignment for those inputs, not arbitrary perceptual
  latency or a frequency-dependent group-delay bound.

Ordinary offline tests cover stream geometry, final range, rate/band/duration
combinations, invalid/extreme controls, tiny/max budget admission, immutable
input, reset and forced-finalization rollback of both layers.60 actual CLI cases
cover empty, one-sample, partial, exact and over-one-frame recordings,312 pre-skip,
exact duration, fractional budgets and rejected unsupported settings. Existing
SILK/CELT packet/state/PCM fixtures remain unchanged. All seven required gates
pass, including23 Python tests.

## Reproduction

```sh
cargo run --offline --locked --release --example hybrid_encoder_vectors -- /tmp/hybrid-vectors.bin
python3 tools/hybrid_encoder_validation.py /path/to/opus-v1.6.1 /tmp/hybrid-vectors.bin /tmp/hybrid-result.json
python3 tools/hybrid_encoder_fade_fixtures.py /path/to/opus-v1.6.1 /tmp/hybrid-fades.txt
diff -u tests/hybrid-encoder-fade-fixtures.txt /tmp/hybrid-fades.txt
cargo build --offline --locked --release
python3 tools/hybrid_file_validation.py /path/to/opus-v1.6.1 target/release/opura /path/to/hash-pinned-recordings /tmp/new-hybrid-file-results
```

The vector path and file-results directory must be new. Raw recordings, listening
outputs, vectors, binaries and logs remain local; only sanitized numerical
results and fixed numerical fixtures are tracked. File reports retain binary,
tool, codec-source and recording hashes. Recording checks compare two decoders of
actual Opura output; they do not compare native encoder quality or timing.
No subjective, competitive performance, universal quality or SOTA claim follows
from these interoperability results. Broader evaluation remains part of the
full-codec objective.
