# Inverse MDCT FFT preparation

Status: correctness-validated experiment, rejected for release. Production
0.2.5 retains separate inverse-MDCT FFT preparation. This describes experimental
codec commit `99312a0`, not the released implementation.

The inverse MDCT prerotation writes directly into the plan-specific BITREV
indices of its private Complex buffer. The FFT then transforms that buffer
without a separate natural-order input array or permutation pass. This follows
the pinned pristine Opus1.6.1 inverse prerotation order. The1020-element inverse
workspace, postrotation/de-shuffle/window, public owned TimeFrame, and all
remaining output copies stay unchanged. This is not a fully in-place MDCT or
a measured peak-stack/heap/RSS improvement.

The private FFT entry requires exactly the selected plan length and a0..31
shift, checking both before mutation. Its input order is guaranteed by the
private MDCT caller; it does not infer or validate semantic permutation from
arbitrary complex values. Public natural-order/scaled/inverse FFT APIs and
forward encoder MDCT retain their original preparation, validation/error order
and exact wrapping/downscale arithmetic. Input geometry and overlap publication
remain unchanged. Existing staged synthesis and CELT/Hybrid/Opus transactions
still protect complete owner state when later rendering fails. No public API/
layout, unsafe, heap allocation, dependency or encoder policy changes.

Two private tests check1536independent native direct-forward FFT cases covering
all four plans and all32downshifts, with adjacent canaries. Invalid exact-length
and downshift combinations preserve every coordinate, including validation
precedence when both are invalid. Existing1632public FFT/398MDCT/480synthesis
cases, native full-decoder/multistream fixtures and all nine late synthesis/PVQ
transaction tests pass. Four restored runtime mutations catch omitted length,
omitted shift limit, ignored requested shift and wrong inverse-MDCT input order.

The exploratory seven-pair48kHz stereo pilot ratio1.0111 shows no benefit;
it is not a complete acceptance result. All seven final gates pass (183 library tests in debug/release,44 Python).
All120official cases pass and every0.2.5 PCM hash remains unchanged. The
[correctness record](results/decoder-mdct-fft-0.2.6-correctness.json) and
[conformance report](results/decoder-mdct-fft-0.2.6-conformance.json) bind these
checks to final source. The completed [paired comparison](../benchmarks/decoder-mdct-fft-0.2.6.md)
retains 300 samples over all ten geometries: ratio 1.0116, 95% interval
[0.9844,1.0354]. Every individual interval includes 1. No aggregate benefit
is demonstrated; the implementation is not adopted, released or tagged.
Do not repeat this unchanged experiment in search of significance. The rejected rollback/owned-MDCT-output
experiments are not included; separate historical ratios are not combined.
No native/best-available/other-competitor, encoder speed/quality, resource,
portability or SOTA claim follows.
