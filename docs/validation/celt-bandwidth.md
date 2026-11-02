# CELT bandwidth integration

Version0.1.84 connects selected bandwidth to the actual CELT PCM/packet path.
It extends the stateful channel work in0.1.83 toward the unified encoder;
shared SILK/Hybrid/input ownership and real cross-mode prefills/tails remain
unfinished. Fixed encoder and audio CLI defaults retain their previous meaning.

## Contract

`CeltEncoder::set_bandwidth` selects NB/WB/SWB/FB and `bandwidth()` returns the
prospective selection. Maximum/default bands remain NB at8kHz, WB at12/16kHz,
SWB at24kHz and FB at48kHz. CELT has no MB packet configuration, so MB and
above-input bands return `InvalidBandwidth` without changing state. The12kHz
WB default retains native CELT promotion and spectral zero filling above input
Nyquist. Every valid narrower band is supported at the same fixed input rate.

TOC configuration and spectral ends13/17/19/21 follow the selected band.
Input preemphasis, overlap, pitch histories, active-band quantizer history,
coded-band smoothing, spectral average and rate feedback continue across changes.
After quantization/silence handling and mono-energy mirroring, native packet
completion zeros reconstructed energies outside the active interval in both
API channels, including the dormant encoded channel. Primitive EnergyEncoder
quantization retains its documented inactive-history semantics; cleanup belongs
to the enclosing CELT packet engine. The shared high-band engine also applies
the native below-start cleanup.

Changing a control alone does not clear histories. Narrowing and restoring a
band without encoding preserves the complete owner. Cleanup runs in the staged
packet transaction; invalid requests and injected late CBR/VBR/CVBR termination
errors preserve all state. Reset clears histories while retaining band, wire
layout and analysis settings.

## Independent evidence

Pinned reference: original scalar fixed-point Opus1.6.1 revision
`22244de5a79bd1d6d623c32e72bf1954b56235be`, fast64 Q24. The optional
`tools/celt_encoder_fixtures.py --bandwidth-packets` extends the existing
component baseline with selected end/TOC and native post-frame energy cleanup.
Original transform/filter/analysis/quantization/entropy and public decoder
functions are unchanged. This compares exact component-policy packets/state,
not the entire native encoder's byte stream. No C code is linked or compiled
by ordinary Cargo builds or tests.

640 packets cover all five input rates, all four CELT durations, repeated band
and wire-layout changes, resets, four analysis-switch profiles, small budgets,
identical/inverted/one-sided signals, noise, silence and impulses. Packet bytes,
range, both input overlaps/preemphasis, reconstructed energies/residuals,
spreading/trim/intensity, coded-band history and both pitch histories match.
Ten unchanged public native decoder contexts per frame additionally match
sample counts, final range and FNV64 fingerprints of little-endian PCM at all
five output rates and both output layouts:6400 decoder records.

The actual Rust VBR/CVBR/CBR-toggle/silence/loss generator supplies80 stereo-input
streams/10240 decoder records. Unchanged native decoding matches every PCM
fingerprint/final range, including440 losses,2292 silence packets,2240 received
wire changes and5576 received band changes. Received bands are5896 NB,
2240 WB,1104 SWB and560 FB. See the [native replay report](results/celt-bandwidth-0.1.84-vectors.json).
These records establish interoperability, not full native encoder target/byte
identity or perceptual quality.

A separate mono-input replay adds80 streams/10240 records with the same five
input rates and both fixed decoder output layouts. Native PCM fingerprints and
ranges match throughout, including440 losses,2298 silence packets and5576
received band changes. Every received packet has mono wire layout, including
streams decoded to stereo. See the [mono-input report](results/celt-bandwidth-0.1.84-mono-vectors.json).
Together the actual replays cover20480 records and880 losses across both PCM
input owners, without inferring mono-input behavior solely from stereo downmix.

Five isolated compiled source mutations are detected: omit cleanup, clear only
the coded channel, retain input-default TOC, retain input-default spectral end,
and clear energies prematurely in the setter. Each was tested separately with
the source restored before final checks. See the [mutation record](results/celt-bandwidth-0.1.84-mutations.json).
This establishes sensitivity to these mistakes, not exhaustive correctness.

Fixture SHA256:
`0d2a0fa8aaa24d4c65bb064a2f083e61764e80582c4143fb35c60d0241c09224`.
Actual replay SHA256:
`32aee587699fda0a4d0adb2018162cc2ce670c0d94b6b3c0815f29435f8d49b1`.
Mono-input replay SHA256:
`2d74357a31bfc7a997a2fdf554318e2e1a05dce6de4525270c6a8fe01ff7d657`.
The preceding channel-only component corpus regenerates byte-identically.
The final Rust code also reproduces the prior [fixed-layout VBR corpus](results/celt-vbr-0.1.79-vectors.json)
and [channel-only VBR corpus](results/celt-channel-0.1.83-vectors.json)
byte-identically, including their decoder PCM/range fingerprints.

All seven final offline gates pass: formatting, debug/release locked tests,
all-targets Clippy with warnings denied, warning-free rustdoc, zero dependencies
of every Cargo kind and23 Python tests. Previous fixed encoder/decoder/audio
CLI regressions stay in the normal suites; no decoder code was changed.

## Reproduction

Use a clean local checkout of the pinned reference and new output paths:

```sh
python3 tools/celt_encoder_fixtures.py target/reference/opus --bandwidth-packets > target/bandwidth-fixtures.txt
cmp tests/celt-bandwidth-packet-fixtures.txt target/bandwidth-fixtures.txt
cargo test --offline --locked --lib channel_encoder_tests
cargo run --offline --locked --release --example celt_vbr_vectors -- target/bandwidth-vectors.bin --bandwidth-transitions
python3 tools/celt_vbr_validation.py target/reference/opus target/bandwidth-vectors.bin target/bandwidth-report.json --bandwidth-transitions
cargo run --offline --locked --release --example celt_vbr_vectors -- target/mono-bandwidth-vectors.bin --bandwidth-transitions --mono-input
python3 tools/celt_vbr_validation.py target/reference/opus target/mono-bandwidth-vectors.bin target/mono-bandwidth-report.json --bandwidth-transitions --mono-input
```

Automatic frontend controls still require a common PCM/layer owner before the
unified adaptive library/file path can be advertised. Competitive end-to-end
quality, latency and speed evaluation remains later work.
