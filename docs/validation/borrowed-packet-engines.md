# Borrowed packet engines

Version 0.1.88 removes packet coding's dependence on a particular fixed PCM
owner. The actual public mono SILK, stereo-input SILK and Hybrid paths now
delegate their coding to private engines borrowing the caller's live histories.
Each public encoder still represents an independent stream. The complete
adaptive SILK/CELT/Hybrid owner and native cross-mode processing remain work.

## SILK payload

`silk::encoder::packet` contains one generic payload implementation over
`SpeechState<N>` and `PacketBandwidth<N>`. Typed mono/stereo entry points supply
statically dispatched analysis and prefix callbacks, preserving the existing
public error types. No alternate encoder instance is initialized by the engine.

The shared body handles FEC header/prefix ordering, frame/rate budgets,
mono/stereo analysis, active side omission, LP filtering, channel quantization,
conditional pitch coding, header patching, overflow/PLC, DTX and packet padding.
It uses actual wire count for FEC rather than const state capacity. The existing
owners retain input filtering/resampling, layout requests, whole-state staging,
prefix-free retries and successful input-history commit. Their public input,
error, reset and default packet contracts are unchanged.

The mono entry can borrow a capacity-two speech state. This is needed by a
single layer owner handling API1/coded1 or API2/coded1 without creating a second
mono bank. A paired test alternates coded mono/stereo on the same histories,
comparing 128 packet/decision/complete-state results against the existing stereo
entry: 64 use the capacity-two mono entry and 64 the stereo entry. It includes
all SILK packet durations, CBR/VBR, DTX/FEC and layout changes. It tests the
borrowed payload contract, not an additional independent native algorithm oracle.

A separate 15.6kbps NB mono case at 20% loss checks that a capacity-two owner
uses the native mono FEC threshold. An isolated call-site mutation substituting
capacity for wire count fails that assertion. Source is restored and all three
borrowed-payload tests pass. Existing 576 native FEC policy cases, native input,
stereo-analysis and decoder evidence remain in the normal suites unchanged.

Invalid borrowed SILK geometry/control inputs are rejected before history
changes. Late coding failures may mutate the borrowed staged state; the caller
must discard the entire failed transaction, as the public owners already do.

## Hybrid frame

`HybridFrameEncoder` borrows PCM, `SpeechState<2>`, CELT, high-band gain and width
histories. Its preparation and finalization contain the existing shared-writer
low/high-band coding, DTX ordering, fades, VBR shrink and packet formatting.
`PreparedFrame` retains the unfinished range writer and the actual DTX decision.
The public `HybridEncoder` keeps configuration validation, cloning, its bounded
FEC retry and commit. It delegates through temporary references to the same
owned fields on each call.

The borrowed Hybrid entry checks frame geometry, byte/rate/options bounds and
matching PCM/SILK/CELT configurations before mutation. Tests reject invalid
rate/band/channel metadata, PCM length, budget, nominal rate and mismatched CELT
wire layout with histories unchanged. Existing late-finalization, tiny-budget,
FEC retry, DTX layer-order and complete-state rollback assertions remain intact.
The borrowed engine requires staged histories; it does not provide independent
rollback for late failures.

## End-to-end evidence

The layout example still produces the exact 0.1.87 raw corpus, SHA256
`a261e8e8c5ea8a50dc413896b1ea6748a3a346aca2fed8204cc9ab08fce2596f`.
A fresh replay through unchanged scalar Opus 1.6.1 revision
`22244de5a79bd1d6d623c32e72bf1954b56235be` matches all PCM fingerprints and final
ranges: 11,520 calls across 120 streams, including 1,620 encoded wire changes,
1,080 explicit PLC calls and 960 FEC calls. The original encoded corpus contains
4,980 mono/6,540 stereo packets, 1,530 DTX packets and 4,135 FEC-carrying packets.
As before, encoded and received/recovery counts describe different schedules.

The [fresh layout replay report](results/borrowed-packet-engines-0.1.88-layout.json)
records that byte identity and unchanged-native decoding. All 24 existing fixed
corpora also [match the committed 0.1.87 baseline](results/borrowed-packet-engines-0.1.88-fixed.json)
byte-for-byte: 1,384,320 records across 5,088 streams, with 220,896 FEC requests
and 768 zero-length PLC records. Baseline examples were built from an isolated
archive of the annotated commit. The prior matrix is compared exactly rather
than replayed in native code again, so its report records `native_replay: false`.
The separate layout corpus has fresh native replay.

All seven final offline gates pass: formatting, debug/release locked tests,
all-targets Clippy with warnings denied, warning-free rustdoc, the zero Cargo
dependency checker and 23 Python tests. The isolated mutation was restored
before the final gates. Existing golden values, rollback assertions, unsafe and
dependency policies remain intact.

The optional reference remains outside Cargo builds and ordinary tests.
Reproduce the fresh layout replay with new output paths:

```sh
cargo run --offline --locked --release --example silk_layout_vectors -- target/borrowed-layout.bin
python3 tools/silk_mono_validation.py target/reference/opus target/borrowed-layout.bin
```

## Open payload phases (0.1.89)

Private SILK preparation now returns the live range writer, speech DTX decision,
TOC and overflow outcome before termination. The fixed entry points immediately
finish that prepared payload as before; outer integration can first append
transition symbols. Packet final range is captured after those writes. A boundary
test appends a direction symbol and checks that the resulting range, rather than
its pre-symbol value, reaches the packet. This does not exercise a complete
redundancy packet: CELT tail bytes and their final-range XOR are still absent.

Hybrid preparation now has distinct low-band and high-band phases. After the
low-band phase, prior CELT state, committed 10ms PCM history and gain/width fades
are unchanged. The fixed path writes its no-redundancy syntax at this boundary,
then completes CELT. A mono/stereo 10/20ms sequence checks both this ordering and
complete packet/state equivalence with the ordinary entry. Existing native DTX
fixtures retain early-SILK-return and outer-suppression coverage. Both phases
require the same staged owner and configuration throughout the transaction;
late failures require discarding the complete staged state.

All seven required offline gates pass on 0.1.89, including 23 Python tests.
The [complete fixed-mode comparison](results/open-payload-phases-0.1.89-fixed.json)
matches an isolated archive of committed 0.1.88 byte-for-byte: all 24 corpora,
1,384,320 records, 5,088 streams, 220,896 FEC requests and 768 zero-length PLC
records. Every corpus hash and coverage count also matches the earlier report.
This comparison does not rerun native decoding; it records `native_replay: false`.
The unchanged original fixtures and rollback assertions pass in ordinary tests.

These changes expose where native transition processing must occur. They do not
add an automatic encoder, prefill, redundant tail or a new public API.

## Remaining integration at0.1.89

Version0.1.90 connects a bounded single-frame AUDIO owner; see
[unified AUDIO validation](unified-audio-owner.md). The following describes
the boundary at the original borrowed-engine milestone.

Borrowing makes the actual coding paths reusable by one PCM/speech/CELT owner;
that adaptive owner has not been implemented here. Hybrid still has a fixed API
and wire layout and its preparation includes normal AUDIO input processing.
The fixed SILK entry finishes its packet; the private preparation entry now
keeps its writer open. Native mode transitions require
connecting root policy, application-specific preprocessing, explicit band/FEC
control and acknowledgement, live-writer transition syntax, true SILK/CELT
prefills, 5ms redundancy tails and long-packet placement/final-range XOR. Those
must be implemented before complete automatic library/CLI encoding is claimed.
No encoder byte identity with original libopus, perceptual parity or competitive
performance claim follows from this refactoring.
