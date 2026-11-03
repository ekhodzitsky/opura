# Experimental complete-owner rollback without a successful commit copy

Status: correctness-validated experiments, rejected for release. Production
0.2.5 retains its original staged-owner transaction. The following describes
experimental codec commit `461081e`, not the current released implementation.

Unified decoding snapshots every field the selected operation can change.
A small stack guard immutably borrows the saved state and holds exclusive
access to the working owner. Intermediate state is unobservable through the public API. Full
success disarms rollback and leaves the resulting state in place. Errors and
unwinding restore the complete snapshot in `Drop`.

This replaces cloning a working owner and copying that entire owner back on
every successful call. Public `OpusDecoder`, owning PCM output, signatures and
error variants remain unchanged. No heap allocation or unsafe code is added.
Direct public CELT, Hybrid and synthesis transactions remain independent.

## Commit boundary and ordering

Framing and requested-duration validation retain their original order before
the transaction. Inner codec validation and integer arithmetic retain their
original rendering order. Normal decoding processes every packet frame before
disarming; PLC finishes the complete requested interval; FEC finishes any PLC
prefix and the recovery/fallback frame. Per-frame gain, transition fades,
diagnostic range and last-duration updates all occur before commit.

All transitions, non-CELT packets, explicit PLC and FEC retain a full-owner
snapshot. Normal packets use CELT plus complete top-level metadata only when
both the packet and the preceding actual mode are CELT. Their normal and
implicit-loss paths cannot mutate SILK; output geometry getters only read it.
Every frame retains the same packet mode. All other contexts use the full
snapshot. These conditions must be re-audited if the renderer starts touching
SILK in a steady-CELT path.

The saved state remains immutably borrowed through the guard lifetime. SILK/CELT histories, seeds,
bandwidth/channel/mode context, redundancy flags, gain, range and duration are
restored together. The working owner is only available through its exclusive
borrow; no references into it escape in the owning PCM result. Production uses
no unwind containment. The guard's destructor performs restoration during
ordinary error returns and Rust unwinding; tests alone use `catch_unwind`.

Error paths now copy the snapshot back rather than discard a modified working
clone. This investigation targets successful calls. No error-throughput,
peak-stack, heap/RSS or memory-superiority claim follows from the source change.
Explicit caller-requested PLC/FEC have native correctness coverage, but the
official normal-packet speed workload does not establish their throughput.

## Failure and reference coverage

Four new tests inject an error or panic after complete rendering and metadata
updates, immediately before disarm. They exercise a Hybrid-to-CELT transition,
a 120ms normal packet, a complete 120ms PLC interval and a 120ms FEC request
with a PLC prefix. A successful control must actually advance state. Each
injection asserts it was reached, compares every owner field against the saved
pre-call state, then verifies resumed PCM/diagnostics and state against control.
Both failure modes start from the same warmed state. Tests pass with the default
thread stack; hooks compile only in unit tests. Steady-CELT normal and 2.5ms
TOC-only loss cases also assert that warmed SILK state stays unchanged while
every metadata field and resumed output retain the expected contract.

The existing CELT synthesis and PVQ late-write faults also exercise the unified
owner through genuine inner rendering failures. All thirteen transaction tests
pass. Independent native normal/gain/long/PLC/FEC/transition and multistream
fixtures pass before the mutations. Five restored mutations are rejected at
runtime: omitted restoration, disarm before actual inner rendering errors,
failure to disarm success, dropping the previous-mode selection condition, and
omitting saved range restoration. An existing PVQ regression detects premature
disarm; independent native fixtures detect failed successful commits. Transition
and short-loss tests detect unsafe selection and missing metadata restoration.

Focused reproduction in a checkout of experimental commit `461081e`:

```sh
cargo test --offline --locked --lib transaction_tests
cargo test --offline --locked --test opus_decoder --test decoder_pcm \
  --test multistream --test multistream_native
```

The [full-snapshot experiment](../benchmarks/decoder-rollback-full-0.2.6.md)
passed all seven gates and 120 unchanged official PCM hashes, but its 300-sample
comparison did not establish aggregate benefit. It is not accepted or tagged.
The revised selective checkpoint passes all seven gates (185 library tests in
debug/release, 44 Python tests), independent native decoder/multistream corpora
and all 120 official cases with every 0.2.5 PCM hash unchanged. The new
[correctness record](results/decoder-rollback-0.2.6-correctness.json) and
[conformance report](results/decoder-rollback-0.2.6-conformance.json) bind these
checks to revised source. The completed [selective comparison](../benchmarks/decoder-rollback-0.2.6.md)
retains 300 samples against stable 0.2.5: ratio 0.9916, 95% interval
[0.9739,1.0075]. It does not establish aggregate benefit and is not accepted
or tagged. No prior check or pilot is substituted for the complete result;
unchanged experiments must not be repeated until a favorable interval appears.
