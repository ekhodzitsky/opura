# CELT input and encoded channel layouts

Version0.1.83 connects separate input/wire layouts in the actual CELT PCM
encoder. A two-channel input owner can emit mono or stereo CBR/VBR/CVBR packets
without resetting its histories. This is a shared-owner integration step;
automatic SILK/CELT/Hybrid selection, common outer filtering/delay and native
cross-mode prefills/redundancy are still unfinished.

## API and signal history

`CeltEncoder::channels()` retains its fixed PCM input meaning. The additive
`set_encoded_channels(1|2)` selects the next packet's encoded layout;
`encoded_channels()` returns it and `EncodedPacket::channels()` reports the
wire layout. A mono input owner accepts only mono coding. Reset retains the
selection and analysis settings. Changing selection does not consume PCM or
reset input/quantizer/rate histories. Existing same-layout APIs, defaults and
audio CLI paths retain their previous behavior.

Preemphasis, raw overlap, transient analysis and pitch filtering continue over
both input channels. Mono coding downmixes transformed spectra with native
fixed-point half/add ordering. Allocation, quantization, equivalent rate,
perceptual analysis and VBR targets use encoded channels; the nominal bitrate
clamp still uses input channels, matching native CELT control semantics. TF
uses channel0 after mono spectral downmix even if the attack was on input1.
Completed mono packets mirror reconstructed energies into the dormant coded
channel; residuals there remain zero. These operations are inside the packet
transaction, including VBR/CVBR feedback. Invalid geometry/controls/caps and
injected late termination errors preserve the complete encoder state.

## Reference evidence

Reference is clean Opus1.6.1 revision
`22244de5a79bd1d6d623c32e72bf1954b56235be`, scalar fast64 fixed-point Q24.
`tools/celt_encoder_fixtures.py --channel-packets` extends the existing
component-based oracle through `tools/celt_channel_fixtures.py`. Orchestration
keeps API channels separate from coded channels and includes native mono
energy mirroring. Transform, filtering, analysis, quantization, entropy and
public decoder bodies come from unchanged original sources. This is an exact
component-policy baseline, not byte parity with the complete native encoder.
Ordinary Cargo builds/tests never compile or link foreign code.

The checked-in corpus has640 packets over all five input rates, all four CELT
durations, repeated wire-layout changes, resets and four analysis-switch
profiles. It includes identical/inverted stereo, one-sided signals, noise,
silence, endpoint impulses and small byte-budget boundaries. Tests compare
packet bytes, range, coded-band history, both preemphasis/overlap/energy/residual
histories, spreading/trim/intensity and both pitch histories. Ten unchanged
public native decoder contexts per packet verify sample counts, final ranges
and FNV64 fingerprints of little-endian PCM at all five rates, mono/stereo.
Thus6400 native decoder records cover the changed packet path.

The actual VBR/CVBR example additionally emits80 streams/10240 records through
the modified Rust encoder and decoder. Unchanged public native decoding matches
all PCM fingerprints and final ranges, including440 losses,2292 silence
packets,5360 received mono packets,4440 received stereo packets and2240
received wire-layout changes. Nominal rates, hard caps, constrained feedback,
CBR toggles, silence/resume and frame durations vary while input stays stereo
and decoder output stays fixed. See the [replay report](results/celt-channel-0.1.83-vectors.json).
This establishes interoperability and the stated component arithmetic, not
perceptual quality or native full-encoder packet equality in VBR.

Fixture SHA256:
`3f75d60401dcf915e66889f7ab4c03edc9f80901507bdb64734c1e4327740dca`.
Adaptive-layout replay SHA256:
`34ba0c14910f5480004a1231c71965c67a066dcaab058461130d75f410408357`.
The unchanged default VBR replay retains its previously recorded SHA256:
`ab48e6cb096a9a6535bf03291c9870923c1b8ba9b246ac64596c6889c21a6eae`.
The existing optional pitch corpus also regenerates byte-identically.

Four isolated mutations are detected by compiled test failures: omit mono
energy mirroring, gate pitch filtering by input instead of coded channels,
stop updating the second input history, and select the original right-channel
TF index after mono downmix. Each was tested separately with the source
restored before final checks. The [mutation record](results/celt-channel-0.1.83-mutations.json)
identifies the test and exit codes. This tests sensitivity to those errors;
it is not an exhaustive proof.

All seven final repository gates pass: formatting, debug/release offline locked
tests, all-targets Clippy with warnings denied, warning-free rustdoc, the
zero-dependency checker and23 Python tests. The final optional corpus generator
preserves both hashes above, including with non-UTF8 output paths.

## Reproduction

Use a clean local reference checkout at the pinned revision and new output
paths (the Rust example refuses to overwrite existing files):

```sh
python3 tools/celt_encoder_fixtures.py target/reference/opus --channel-packets > target/channel-fixtures.txt
cmp tests/celt-channel-packet-fixtures.txt target/channel-fixtures.txt
cargo test --offline --locked --lib channel_encoder_tests
cargo run --offline --locked --release --example celt_vbr_vectors -- target/channel-vectors.bin --channel-transitions
python3 tools/celt_vbr_validation.py target/reference/opus target/channel-vectors.bin target/channel-report.json --channel-transitions
```

Packet component byte/state equality and native decoding do not establish a
complete adaptive encoder or competitive speed/quality. Those remain part of
the broader codec goal after shared SILK/CELT/Hybrid ownership is connected.
