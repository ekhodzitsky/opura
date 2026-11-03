# Native encoder budget admission and PLC

Version0.1.93 completes early insufficient-budget signalling and completed-SILK
overflow handling in the shared encoder. `encode` accepts1..1276-byte caps;
`encode_packet` accepts1..7656 for long packets. Normal useful budgets keep
encoding PCM. Other budgets produce native loss-concealment packets rather
than pretending to encode audio. `EncodedPacket::used_plc` reports whether any
component deliberately signals PLC. A100ms packet needs at least2 bytes; an
impossible framing request remains an atomic error.

## Admission order and retained state

Positive user bitrate is first clamped to at least500bps for the whole stream
and at most750000bps per immutable API channel, then limited by physical
capacity and native whole-packet CBR rounding.
Raw stereo-width analysis runs before budget admission, except on forced mono,
where native preserves all five width memories. Invalid controls remain errors.

Early admission uses the previous actual selected mode, band and wire layout.
That actual mode is separate from logical previous mode after an outgoing CELT
tail. The initial native mode is Hybrid/fullband. Native duration and bandwidth
rules may change the PLC packet's format without changing the retained owner.
The resulting packet has empty compressed frames and zero final range, with
whole CBR padding as needed.100ms/one-byte rejection precedes analysis.

Early PLC preserves PCM/DC/HP/delay, resamplers, speech, FEC, DTX, transform,
prefill, bandwidth, fades and selection histories. It does not consume a pending
FEC prefix or advance audio counters. Raw width is the deliberate exception
above. A subsequent useful-budget encode resumes the retained live histories.

## Completed speech overshoot

SILK frame search keeps its normal soft RDO target while the outer owner allows
the completed search to exceed it. The borrowed packet engine reports actual
overflow; fixed public owners retain their old hard-cap behavior. After actual
speech processing and outer state/history completion, an overflowing primary
is replaced by a payload containing one zero byte and zero range. CBR transport padding uses
the original cap. A pending FEC capacity failure still has one bounded retry
without that prefix; allocation-budget failures participate in that retry.
Arbitrary errors do not become successful PLC and still discard staged state.

The effective physical bitrate also limits the SILK internal-rate ceiling.
For forced Hybrid VBR with a requested high-band target at most500bps, native's
CELT setter is rejected and leaves BITRATE_MAX in place; the shared path retains
that physical-budget high-band mode while outer/SILK processing remains VBR.

## File path

Automatic WAV/Ogg encoding accepts positive nominal rates down to1 (native
normalization still applies) and caps down to1, including CBR caps. Deliberate
PLC packets retain exact output duration and application-specific pre-skip.
100ms/one-byte files are rejected before creation. Legacy fixed-mode flag
validation remains unchanged. A cap can reduce effective bitrate below the
nominal target; this must be considered in later quality/rate comparisons.

## Independent reference evidence

The optional generator uses unchanged Opus1.6.1 revision
`22244de5a79bd1d6d623c32e72bf1954b56235be`, scalar fixed fast64/Q24, float analysis
disabled. It only wraps original calls and reads their states. For negative
calls, the wrapper restores the complete native allocation and observation
counter to model the Rust owner's stronger transactional error contract;
positive calls use original native behavior without restoration.

The7,200 original calls cover all five rates, API layouts, applications and
supported durations, CBR/unconstrained VBR, tiny caps, low/clipped nominal rates,
mode/wire requests and ordinary resume calls. Complexity is zero. Rust tests
compare5,630 early PLC packets and126 completed-overflow packets byte-for-byte
and by range, plus100 unrepresentable100ms/one-byte errors. All1,470 normal
admission calls check live mode/band/wire, logical previous mode, active API
resamplers, cumulative complete mid PCM, HP/delay, every actual CELT call and
all five raw width memories. Early PLC also checks the complete owner against
its prior state, accounting only for permitted width updates. A local pressure
regression now requires legal small budgets to succeed and invalid caps to
preserve the whole owner. Both original fixture files regenerate identically.

Five isolated wrong implementations are detected: advancing retained audio on
early PLC, wrong initial retained mode, omitting completed PLC replacement,
omitting user-rate clamping, and updating width on forced mono. Source was
restored before final checks.

```sh
python3 tools/opus_budget_fixtures.py target/reference/opus target/budget.txt \
  --calls-output target/budget-calls.txt
cargo test --offline --locked --lib encoder::stream
```

The separate `budget_encoder_vectors` example exercises actual emitted packets
with complexity0/6/10, DTX/FEC/loss and changing budgets over100 streams/9,600
received calls. It includes800 explicit PLC and800 FEC calls, plus7,955 encoded
packets with deliberate PLC in at least one component. Native decoding compares
full PCM hashes and final range, independently of the encoder's implementation.
The final complete corpus repeats byte-identically.

```sh
cargo run --offline --locked --release --example budget_encoder_vectors -- target/budget.bin
python3 tools/silk_mono_validation.py target/reference/opus target/budget.bin
```

The replay tool validates recorded PCM counts against its allocated C buffer
before calling native decoding. A malformed oversized FEC-count record is
rejected; valid final replay was repeated after this guard. This optional
native tool remains outside Cargo dependencies.

The CLI adds85 tiny CBR/VBR file cases across all applications and20..120ms
at48kHz stereo, checking legal caps, empty compressed frames and exact duration;
unrepresentable files are refused before creation. Prior300 short empty/partial
and250 long file cases remain. See the [versioned report](results/encoder-budgets-0.1.93.json)
for final corpus identity and offline checks.

All seven required offline gates pass, including158 library and23 Python
tests; the Python gate and native replay were repeated after the count guard.
The previous complete9,600-call short and9,600-call long automatic corpora match
committed0.1.92 hashes byte-for-byte. All24 fixed SILK/Hybrid corpora also match
prior hashes, byte counts and record statistics:1,384,320 records in5,088
streams,220,896 FEC requests and768 PLC calls. Fixed-native replay was not
repeated; these preserve previously native-validated bytes. See the
[fixed-path report](results/encoder-budgets-0.1.93-fixed.json).

## Remaining goal

Remaining native quality/complexity/control review and representative real-audio
quality/speed evaluation are still required. Budget/interoperability agreement
is not proof of general encoder byte parity, perceptual superiority or SOTA.
