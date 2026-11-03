# Private inverse-MDCT destinations

Status: correctness-validated experiment, rejected for release. Production
0.2.5 retains the owned intermediate. The following describes experimental
codec commit `cd02054`, not the current released implementation.

The public `MdctState::synthesize` still returns an owned `TimeFrame`. Private
CELT synthesis instead supplies exactly one unpublished time block, avoiding
that intermediate owned result and the subsequent block copy. Long frames use
one block; transient frames use successive 120-sample destinations and the
original interleaved-spectrum strides. Adjacent blocks remain untouched.

The 1020-element inverse workspace and FFT preparation remain unchanged. This
is not a fully in-place FFT/MDCT and does not remove every copy. Windowing,
wrapping/headroom/rounding, overlap commit and clipping/postfilter/deemphasis
order remain unchanged. Exact destination length and all spectral/stride
geometry are validated before writes. FFT errors precede any destination or
overlap mutation; after FFT the computation and final publication are infallible.
The public owned result stays unpublished until success. Existing staged
synthesis and complete CELT/Hybrid/Opus transactions preserve owner state when
later rendering fails. No public API/layout or encoder policy changes.

Two private-output tests check all 398 independent native chained MDCT fixtures,
including pathological full-scale inputs, all ten valid size/stride geometries,
frame-size changes and resets. Offset destinations have both prefix and suffix
canaries. Invalid destination lengths/spectra/strides preserve destination and
overlap; successful resume matches the public control. Existing independent
native synthesis and full decoder/multistream fixtures and all nine late
synthesis/PVQ transaction tests pass. Three deliberately wrong changes are
caught at runtime and restored: omitted destination-length validation, missing
final output sample and overwriting the first transient block repeatedly.

The exploratory seven-pair 48kHz stereo point ratio 0.9676 is not release proof.
All seven final gates pass (183 library tests in debug/release,44 Python).
All120 official cases pass and every 0.2.5 PCM hash remains unchanged. The
[correctness record](results/decoder-mdct-0.2.6-correctness.json) and
[conformance report](results/decoder-mdct-0.2.6-conformance.json) bind these
checks to final candidate source. The completed [paired comparison](../benchmarks/decoder-mdct-0.2.6.md)
retains 300 samples/all ten geometries: ratio1.0027,95% interval[0.9745,1.0345].
Every individual interval includes 1. No aggregate benefit is established,
so the implementation is not adopted, released or tagged. Do not rerun this
unchanged candidate until a favorable interval appears.
No native/competitor, encoder speed/quality, peak-stack/heap/RSS or SOTA claim.
