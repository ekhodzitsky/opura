# Native long-packet scheduling

This records the0.1.92 boundary. Version0.1.93 adds native budget/overflow PLC;
see [budget validation](encoder-budgets.md).

Version 0.1.92 adds `encoder::PacketSize` and `OpusEncoder::encode_packet` for
complete 2.5/5/10/20/40/60/80/100/120ms packets. The existing `encode(FrameSize,...)`
API delegates through the same owner. All five applications remain supported;
restricted SILK requires at least10ms. Caps are3..1276 for short packets and
3..7656 for long packets. Insufficient admission/coding budgets may return an
error; native tiny-budget PLC is still separate unfinished work.

## One policy, native component boundaries

Raw stereo-width analysis, bitrate/CBR normalization and mode/layout/band/FEC
selection run once over the complete packet. Long packets do not re-run the
selection policy on independently encoded chunks. SILK40/60ms remain single
speech payloads containing two/three20ms internal frames. SILK80/120ms use two
40/60ms units; SILK100ms uses five20ms units. CELT/Hybrid use20ms components.

The owner reserves the worst-case packet header before dividing the available
bytes between components. CBR rounding uses the complete duration, including
non-power-of-two60/100/120ms durations. Per-component entropy caps and original
padding caps are separate. Repacketization then writes the complete canonical
or padded packet under the original whole budget. Final range is that of the
final component, including its transition-range XOR, as in native decoding.

Incoming redundancy is requested only on the first component; outgoing
speech-to-CELT redundancy only on the final component. Logical previous mode
stays speech until that outgoing component completes. Bandwidth readiness may
be acknowledged only on a final component. A multi-component toMono request
is retained across the whole call while component speech calls suppress their
individual toMono request, matching native scheduling.

Native incoming prefills repeat across components. Each reinitializes actual
channel/VAD/resampler state while preserving outer stereo and bit-reservoir
history; initial entry alone resets the whole speech owner. Mono resampling
uses individual internal frame boundaries, including overwritten delay slots.
Changing rate migrates actual coding channels and retains inactive side state;
returning to stereo resets the side LP handshake before its rate control.

The complete owner is staged once for the public packet transaction. A pending
FEC capacity failure has one bounded prefix-free component retry. Every error,
including a late component or final repacketization failure, discards all input,
layer, selection, prefill and transport work from that public call.

## File path

Automatic WAV/Ogg encoding uses this API directly, including application-specific
lookahead, zero flushing and exact final granules. `--packet-ms` must be a
standard duration at least as long as and divisible by `--frame-ms`; it selects
one actual complete packet. Alternatively use `--frame-ms 40|60|80|100|120`.
Caps apply to the whole packet. The automatic default allows1276 bytes per20ms
window, at most7656. Large PCM windows use bounded WAV reads.

```sh
opura encode input.wav output.opus --mode auto --packet-ms 120 --vbr --bitrate 64000
```

## Independent evidence

The optional `tools/opus_long_input_fixtures.py` wraps unchanged Opus1.6.1
revision `22244de5a79bd1d6d623c32e72bf1954b56235be`, scalar fixed fast64/Q24 with
float analysis disabled. Reference C is not linked into Cargo or a dependency.

The two tracked fixtures contain5,248 original packet calls. All five rates,
API layouts and applications use mixed20..120ms mode/wire schedules; additional
16/48kHz long sequences alternate narrow/wide speech bandwidth with silence and
resume. Complexity is zero, with CBR and unconstrained VBR. Normal Rust tests
compare active API resampler fingerprints, complete cumulative speech PCM,
outer delay/HP state, packet wire/toMono decisions, LP position/direction,
permission/readiness/final acknowledgement, pending nonrestricted bandwidth
prefill and every actual CELT call's duration/band/prediction/capacity/PCM hash.

These independent observations exposed repeated prefill resets, mono resampler
call geometry, inactive side migration and side LP initialization; regressions
retain the original data. Four isolated wrong schedules are detected: early
bandwidth acknowledgement, reversed incoming/outgoing positions, repeated full
speech reset and omitted framing reservation. Source was restored.

```sh
python3 tools/opus_long_input_fixtures.py target/reference/opus target/long-input.txt \
  --calls-output target/long-calls.txt
cargo test --offline --locked --lib encoder::stream
```

The separate `long_encoder_vectors` example records9,600 received calls in100
actual streams over all five applications/rates, short and long durations,
CBR/VBR/CVBR, complexity0/6/10, automatic/explicit mode/wire decisions, DTX,
800 PLC calls and800 FEC requests. Independent unchanged-native decoding checks
full PCM hashes and final ranges. The final corpus is regenerated identically.

```sh
cargo run --offline --locked --release --example long_encoder_vectors -- target/long.bin
python3 tools/silk_mono_validation.py target/reference/opus target/long.bin
```

The CLI additionally checks250 long files across all five rates, layouts,
applications and long durations. Both direct `--frame-ms` and `--packet-ms`
paths preserve exact partial-file duration; CBR uses the whole-packet byte
budget. Prior300 empty/partial short cases and control cases remain. A pressure
regression compares complete owner state after packet errors and checks legal
successful framing. See the [versioned evidence report](results/encoder-long-packets-0.1.92.json)
for corpus identity, repeated original fixtures and final offline gates.

The short9,600-call corpus matches committed0.1.91 byte-for-byte (SHA256
`8f5e9f429ca5dd1df9cb55032bd53ba8050cdfa896d673b4aec99f8627f1865d`).
All seven required offline gates pass, including157 library and23 Python tests.

All24 complete fixed SILK/Hybrid corpora match committed0.1.91 hashes,
byte counts and record statistics:1,384,320 records/5,088 streams,
220,896 FEC requests/768 PLC calls. This repeats previously native-validated
bytes; native replay of those fixed corpora was not repeated. See the
[fixed-path report](results/encoder-long-packets-0.1.92-fixed.json).

## Remaining scope

Native insufficient-budget PLC admission and completed-speech overflow handling,
remaining quality/complexity/rate-control review and matched-rate/latency
competitive evaluation remain open. These tests prove the stated native state,
input, framing and decode contracts; they do not prove encoder byte identity,
perceptual superiority or SOTA performance. Full-codec completion is not claimed.
