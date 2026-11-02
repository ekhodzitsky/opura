# Hybrid DTX

Version0.1.76 extends the existing fixed-band Hybrid encoder with opt-in silence
suppression. `HybridEncoder::set_dtx(true)` or CLI `--mode hybrid --dtx` requests
DTX; `in_dtx()` reports the last successful packet. Reset retains the setting
and clears all histories/counters. DTX is disabled by default. Hybrid FEC, VBR,
general mode/channel policy and competitive evaluation remain unfinished.

Suppressed packets contain one Hybrid TOC byte and public final range zero,
including CBR operation. They retain the configured10/20ms duration, channel
count and SWB/FB bandwidth. The CLI retains312-sample pre-skip, original WAV
duration, frame flushing and the fractional CBR byte-credit clock. Transmitted
non-DTX packets still fill their requested budgets. Tiny budgets can fail before
suppression after bounded SILK RDO, as documented by the underlying encoder.

## State ordering

The two native DTX paths have different effects on high-band history:

- Nonzero quiet input can enter SILK inactivity DTX. SILK still advances its
  input/analysis/quantizer and inactivity state, but its zero-byte return exits
  before CELT, delay-history copying, HB gain and stereo-width fading. Those
  high-band histories remain intact. SILK's encoded-bit reservoir accounts zero
  payload bits for this early return.
- Digital-zero input uses the outer silence hangover/refresh clock. Both SILK
  and CELT process normally, including copying unmodified delay history,
  high-band gain/stereo fading and entropy termination. Only the complete
  packet's visible payload/range are suppressed afterward.

The reused SILK DTX state already has original VAD/outer-clock coverage. Hybrid
stages the complete state, including DTX flags/clocks and both layers. Failure
after entropy termination rolls everything back even for a digitally suppressed
frame. An early SILK suppression does not call the termination callback. Tests
also cover disabling, re-enabling, reset retention and post-DTX frame geometry.

## Evidence

The unchanged reference is scalar fixed-point Opus v1.6.1 revision
`22244de5a79bd1d6d623c32e72bf1954b56235be`, with the same native build flags as
the [normal Hybrid validation](hybrid-pcm-packets.md). The optional oracle uses
the original AUDIO encoder, forces Hybrid/fixed channels/bandwidth, and wraps
original SILK/CELT calls only to observe their order. It replaces no algorithms.
Native codec code is never a Cargo or ordinary-test dependency.

The retained10800 original calls cover all three eligible rate/band pairs,
mono/stereo,10/20ms, complexity0/6/10 and two DTX-toggle profiles. Each stream
contains speech, digital silence, tiny nonzero quiet input and resumed speech,
including refresh and disabling/re-enabling. Every suppression and CELT-call
decision matches:3024 SILK early returns,2340 outer-digital suppressions and5436
normal frames. For every early return, ordinary tests additionally compare full
CELT, delay, gain and width histories to their pre-call state.

Independent unchanged native decoding matches canonical PCM and public final
range for21600 calls across144 streams. The corpus contains10728 transmitted
one-byte DTX packets and288 explicit losses, including isolated/burst/terminal
losses. These are Opura-encoded packets with native decoding, not a native
encoder PCM/byte equality or quality claim. See the
[raw result](results/hybrid-dtx-0.1.76-vectors.json).

Sixteen actual CLI speech/music files preserve exact native PCM and original
sample count over60976 packets, including1284 DTX packets (220 speech,1064
music). They cover SWB/FB,10/20ms and fractional32001/48001/64001bps budgets with
the previously hash-pinned recordings and no LBRR. See the
[file result](results/hybrid-dtx-0.1.76-files.json). Eight additional ordinary CLI
cases exercise2s silence/resume recordings across24/48kHz, mono/stereo and both
durations, checking size reduction, transmitted refresh/resume and exact output
duration. Existing CLI rejection/boundary checks remain active.

The complete5760-record normal Hybrid corpus is byte-identical to0.1.75, including
encoded packets and decoded PCM/range fingerprints. Existing SILK/CELT fixture
expectations remain unchanged. All seven required repository gates pass,
including23 Python tests. No performance, perceptual or SOTA claim is made.

## Reproduction

```sh
python3 tools/hybrid_encoder_dtx_fixtures.py /path/to/opus-v1.6.1 /tmp/hybrid-dtx-fixtures.txt
diff -u tests/hybrid-encoder-dtx-fixtures.txt /tmp/hybrid-dtx-fixtures.txt
cargo run --offline --locked --release --example hybrid_dtx_vectors -- /tmp/new-hybrid-dtx.bin
python3 tools/hybrid_encoder_validation.py /path/to/opus-v1.6.1 /tmp/new-hybrid-dtx.bin /tmp/hybrid-dtx.json --profile dtx
cargo build --offline --locked --release
python3 tools/hybrid_file_validation.py /path/to/opus-v1.6.1 target/release/opura /path/to/hash-pinned-recordings /tmp/new-hybrid-dtx-files --dtx
```

Raw recordings, vectors, native binaries and listening outputs remain local.
Only fixed numerical fixtures and sanitized results are retained in the repo.
