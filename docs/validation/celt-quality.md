# CELT complexity and spectral transient audit

Version0.1.94 corrects the shared encoder's CELT complexity-zero spreading
policy and removes unused temporal masking work at that complexity, retaining
tone detection. At complexity5 and above, a pure CELT long-window frame can
switch to short windows when spectral energy reveals an attack missed by the
time-domain detector. The original long-window comparison is preserved for
allocation, with LM/2 log-energy compensation and a0.2 Q14 TF estimate.
Temporal VBR history advances from the initial MDCT, before that switch.
Existing fixed public encoder defaults are unchanged.

## Deliberate reference correction

The pinned reference is Opus1.6.1 revision
`22244de5a79bd1d6d623c32e72bf1954b56235be`, scalar fixed fast64 Q24 with
floating-point analysis disabled. In its `celt/celt_encoder.c`,
`patch_transient_decision` assigns Q24 energies to `opus_val16 x1, x2`.
Those16-bit local values cannot produce a mean exceeding the1.0 Q24 threshold.
The pristine detector therefore never selects a spectral patch in this build.
Rust retains32-bit Q24 local differences. This intentionally changes encoder
policy; it is not byte parity with the pristine encoder or a perceptual win.

`tools/celt_quality_fixtures.py --correct-q24` makes a temporary private copy
of that single source file, changing only `opus_val16 x1, x2;` to
`celt_glog x1, x2;`. It also includes the unmodified helper under a separate
name, recording pristine and corrected outcomes side by side. The pinned
checkout must be clean and is never changed. All decoding uses its original,
unmodified decoder. Omitting the flag tests the pristine encoder.

## Deterministic regression evidence

The two committed corrected-Q24 fixture files contain:

- 768 paired helper cases:1/2 channels, four end bands, three start bands,
  negative/positive histories and exact threshold ± one Q24 LSB. The pristine
  helper always returns false; Rust matches the corrected helper.
- 10,560 actual CELT calls: five input rates,1/2 channels,2.5/5/10/20ms,
  complexity0..10,24/160-byte capacities and alternating2/160-byte pressure.
  Stateful inputs include amplitude ramps, square-wave attacks and full-scale
  stationary binary noise. There are966 corrected spectral switches.

Ordinary Rust tests compare every transient flag, spreading decision, tonal/HF
average, tapset, temporal spectral-average history, final range and decoded
PCM hash. All10,560 PCM/range results match. Payload comparison finds10,557
identical packets; three different codewords decode identically and have the
same final range. Full encoder byte identity is not claimed.

Five isolated mutations are detected: restoring complexity-zero normal spread,
disabling spectral patches, clearing patched TF estimate, omitting long-window
compensation and accepting the exact threshold. Every mutation is restored.
Fixtures regenerate identically. Normal builds/tests need no C toolchain,
external codec or third-party Cargo dependency.

Regenerate optionally with a clean pinned reference:

```sh
python3 tools/celt_quality_fixtures.py target/reference/opus \
  target/celt-quality.txt --patch-output target/celt-patch.txt --correct-q24
cmp target/celt-quality.txt tests/celt-quality-corrected-q24-fixtures.txt
cmp target/celt-patch.txt tests/celt-patch-corrected-q24-fixtures.txt
cargo test --offline --locked --lib quality_tests
```

## Full recording diagnostics

`tools/encoder_quality_change.py` compares an isolated0.1.93 baseline
(commit348be2f) with0.1.94. It uses the existing checksum-pinned speech and
music recordings from `tools/audio_baseline.py`: complete10.8s mono speech
and90.7962s stereo music,48kHz. Both use automatic AUDIO20ms CBR at32/64kbps
and complexity0/5/10:12 paired cases,24 decoded files. Each output has exact
input duration/layout,80/160-byte packets and PCM identical to the unchanged
native decoder. Raw audio and generated listening files remain local.

The versioned JSON records corpus/binary hashes, packet modes/layouts,
waveform metrics and whole-process encode wall time. One warmup precedes
three measured runs with alternating execution order. These timings include
process/file overhead and are diagnostics on this machine, without CPU
isolation; fixed-corpus validation shared the machine during part of the run. They are not a core benchmark, latency/memory study or competitor
comparison. Small waveform SNR changes have both signs and do not measure
perceptual improvement. Two recordings cannot establish a representative
quality result or SOTA.

```sh
cargo build --offline --locked --release --bin opura
python3 tools/encoder_quality_change.py BASELINE_BINARY target/release/opura \
  target/reference/opus target/audio-recordings target/quality-change --rounds 3
```

The shared-owner short/long/pressure corpora are decoded independently after
these changes. Complete fixed SILK/Hybrid corpora are compared against0.1.93
hashes and record statistics; this identity check does not repeat their native
replay. See the [versioned audit report](results/celt-quality-0.1.94.json) and
[full recording diagnostics](results/celt-quality-0.1.94-recordings.json). The remaining native
control audit and matched-rate/latency representative perceptual and speed
comparisons remain open. This milestone does not declare the full codec ready.
