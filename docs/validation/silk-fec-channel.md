# SILK LBRR channel-core checkpoint

This record describes the lower-rate redundant quantization/serialization
checkpoint in commit `fdb7297`, under `silk::encoder::channel`. Its evidence is
limited to the channel core. Opura 0.1.70 subsequently connects the raw PCM
frontends and CLI; see [integrated FEC validation](silk-fec-packets.md).
Adaptive bandwidth and broader encoder evaluation remain unfinished.

`ChannelFrameEncoder::encode_with_redundancy` optionally generates a saved
`RedundantFrame` before the normal bitrate-search loop. Speech activity must
exceed the native Q8 threshold of 77. The redundant NSQ starts from a copy of
the normal pre-search state, using original analyzed predictors, quantized gain
symbols and a separate redundant gain history. Native first/after-gap gain
increments and conditional gain reconstruction are retained. The redundant
quantizer's winning seed and pulses are saved; its state never replaces the
main quantizer. The caller carries that frame in the following packet and uses
`encode_redundant` after that packet's masks/stereo fields. Normal and redundant
entropy traversal share the channel's prediction context.

The existing `encode` API preserves its strict `max_bits` bound. The extended
API distinguishes a per-frame RDO target from an explicit hard packet bound.
LBRR can already exceed a partial first-frame quota; native final damage control
then emits held gains and cleared pulses above that quota. It must still fit
the hard packet limit. Errors preserve both writer and every main/redundant
channel history. Geometry/rate mismatches and unsupported redundant coding
contexts are rejected. Supplying a redundant frame does not automatically write
packet headers or decide what to retain under a constrained bitrate.

## Unchanged native evidence

The optional oracle uses clean fixed-point scalar libopus v1.6.1 at
`22244de5a79bd1d6d623c32e72bf1954b56235be`. It wraps existing public frame calls
and uses original public packet decoding without editing reference source.
The workload retains the previous channel corpus: NB/MB/WB, 10/20/40/60 ms,
CBR/VBR, complexities 0/6/10, initial silence, speech-like periodic PCM, loud
noise, changing bitrate and FEC toggles, with 20% configured loss.

The golden corpus retains 483 valid main frames, 243 generated redundant frames
and 281 complete packets across 24 streams. Every main/backup index and pulse
matches, along with entropy range/tell/complete bytes, selected main gain state,
RDO call counts and full main NSQ fingerprints. It covers 202 conditional main
frames and native backup gain-history changes. Own packets are independently
decoded by the unchanged public reference; complete normal PCM fingerprints
and final ranges match. Tests also match 280 loss/recovery/continuation outputs:
93 FEC requests, including 46 with LBRR and 47 without it, followed by normal
decoding where applicable. These are interoperability checks, not a measured
speech-quality advantage over PLC.

## Retained reference limitation

One stream exposes an internal native entropy overflow: WB, 60 ms, CBR32 kb/s,
the sixth packet, first internal frame (counter 15). Entropy tell reaches 2257
bits against a 764-bit partial quota and 239-byte storage (1912 bits). The
original public encoder returns a 240-byte padded packet whose compressed frame
is one zero byte, requesting decoder PLC. This occurs at constant bitrate, not
only on rate reductions. The actual packet and all diagnostic values are kept
in the fixture's `overflow` record; tests verify its framing and PLC payload.

The oracle buffers traces per packet. A packet with invalid native entropy is
not treated as a valid frame golden; its diagnostic/actual response is retained
and that stream ends. Thus 7 packets / 21 internal frames after the failure are
outside this golden comparison. The workload is not altered to avoid the failure.
At this checkpoint frontend integration still needed to admit pending FEC
against the full current packet budget and preserve a usable primary frame.
The subsequent [0.1.70 frontend record](silk-fec-packets.md) verifies that
policy and constrained raw-PCM recovery streams.

Ordinary Rust tests additionally verify that redundant quantization leaves main
NSQ/gains/bytes unchanged, errors roll back gain/NSQ/entropy state, finished
writers stay unchanged and a conditional redundant frame requires history.
The pre-existing 504-frame/288-packet no-FEC corpus remains unchanged and passes.

## Reproduction

```sh
python3 tools/silk_encoder_fec_fixtures.py target/reference/opus target/fec-channel.txt
cmp tests/silk-encoder-fec-fixtures.txt target/fec-channel.txt
cargo test --offline --locked --lib silk::encoder::channel::tests
```

All seven mandatory offline checks apply to this checkpoint. Native C tools
are optional validation only; ordinary builds/tests remain pure safe Rust with
zero dependencies. No complete FEC frontend, quality or SOTA claim follows.
