# Stateful speech wire layout

Version 0.1.87 connects mono/stereo wire requests to the actual stereo-input
SILK packet owner, using one live speech state and one stereo PCM input state.
The input rate and two interleaved API channels stay fixed. Existing fixed
stereo defaults and APIs remain compatible; no independent mono encoder bank
is created or switched in.

## Requests and histories

`StereoEncoder::set_encoded_channels(1|2)` sets the desired wire layout.
`encoded_channels_target()` returns that request. `encoded_channels()` reports
the last successfully encoded layout, initially two. A fresh stream can start
mono immediately. A live stereo-to-mono request first emits one successful
stereo packet with native `toMono` width/prediction collapse, then switches to
mono on the following successful packet. Invalid requests and packet failures
do not consume a transition. Reset retains the request but clears stream history.

Input DC rejection and the common 10ms PCM history always use both API channels.
Mono input is rounded L/R mixing before resampling. On the first mono internal
frame, both departing resampler states process that mix, and their outputs are
averaged with native floor shifting. Later frames advance the mid resampler
alone. On stereo return, clone the live mid resampler before either channel
consumes new input, and initialize the side channel/VAD. Resampler calls follow
10/20ms internal frame boundaries even inside 40/60ms packets, reproducing
native overwritten delay-buffer slots as well as future-relevant histories.

Mono and stereo analysis now use the same two-sample mid carry. Stereo return
retains that mid memory and the side-silence clock, while resetting native
prediction, side, amplitude and width fields. Main prediction/VAD/rate history
remains live. Wire counts control entropy headers, coded-channel loops, DTX,
FEC eligibility and active LP scheduling. Shape changes clear incompatible
queued redundancy; the ramp packet keeps its compatible stereo backups.
FEC policy uses actual wire channels rather than the state's capacity of two.

All operations remain within the existing staged whole-packet transaction.
FEC retries reuse the same filtered/resampled input; errors preserve the phase,
both filters/resamplers, stereo, VAD, DTX/FEC, channel and bandwidth histories.
The existing late-error/retry assertions are retained. New tests cover invalid
requests, cancellation, fresh mono startup, failed ramp/mono packets, reset and
composition with existing automatic bandwidth in both CBR and capped VBR.

## Independent native evidence

All external references require a clean original Opus 1.6.1 checkout at
`22244de5a79bd1d6d623c32e72bf1954b56235be`, scalar fixed-point fast64/Q24.
Their C code never participates in ordinary Cargo builds/tests.

- [4,608 public input snapshots](../../tests/silk-channel-layout-fixtures.txt)
  observe actual original AUDIO/SILK encoding, including forced mono/stereo,
  the ramp flag, mid carry, both resamplers and cumulative complete mid PCM on
  entry to native VAD. All five rates, native bands up to input Nyquist,
  10/20/40/60ms, CBR/VBR, DTX/FEC and repeated layout changes are covered.
  Resampler hashes serialize 103 native state/configuration values. A zero
  side hash means the native side resampler has never been initialized; that
  inactive state is not compared. The VAD wrapper observes input only and
  forwards every call to the unchanged implementation.
- [32 partial initializer snapshots](../../tests/silk-channel-restart-fixtures.txt)
  execute the unchanged native mono-to-stereo initialization block on seeded
  state, checking all 13 tracked stereo fields, including retained mid/silence.
- [120 stereo analysis snapshots](../../tests/silk-channel-analysis-fixtures.txt)
  capture original PCM, controls, before/after histories, mid/side output,
  rate/predictor decisions and entropy bytes across collapse and stereo return.
  Six cases request `toMono`. The original 144 fixed-layout snapshots and all
  their expectations remain unchanged.
- The existing 576 original FEC policy/gain cases additionally check a capacity
  two state encoding mono, without substituting a stereo rate threshold.

Eight isolated mutations are detected: wrong input-mix rounding, omitted first
mono averaging, fresh rather than cloned stereo resampling, incomplete stereo
initialization, wrong mid carry, skipped ramp, capacity-based FEC eligibility,
and ignored width-collapse control. All mutation edits are restored. No
fixture expectations, failure assertions, dependency or unsafe gates are relaxed.

The [actual packet report](results/silk-channel-layout-0.1.87-vectors.json)
records a fresh unchanged-native decoder replay: 120 streams and 11,520 decode
calls match PCM fingerprints and final ranges. The generated input uses all
five rates, all three existing fixed bands, all four packet durations, CBR/VBR,
silence/resume, frame-duration changes, losses and recovery. It encodes 4,980
mono and 6,540 stereo packets, makes 1,620 wire changes, emits 1,530 DTX packets
and carries FEC in 4,135 packets. The decode schedule contains 1,080 explicit
PLC calls and 960 recovery calls; skipped recovery losses are not first
decoded as PLC. The 1,440 single-byte record payloads include recovery calls
and exclude unreceived packets, so they are not the encoded DTX count.

The replay validator supplies the requested duration for explicit zero-length
PLC calls, as already done for FEC. A maximum output capacity is appropriate
for received packets whose TOC carries duration, not for a requested PLC time.
The PCM/range comparisons and corpus-count checks remain strict.

## Fixed behavior and final gates

[All 24 prior corpora](results/silk-channel-layout-0.1.87-fixed.json) match the
committed 0.1.86 baseline byte-for-byte: 1,384,320 records across 5,088 streams,
220,896 FEC requests and 768 zero-length PLC records. All sizes, hashes and
counts also match the earlier tracked reports. Baseline examples were built
from an isolated archive of the annotated 0.1.86 commit. This comparison does
not rerun full native decoding of the prior corpora; its report correctly has
`native_replay: false`. The new layout corpus separately has fresh native replay.

All seven final offline gates pass: formatting, debug/release locked tests,
all-targets Clippy with warnings denied, warning-free rustdoc, the zero Cargo
dependency checker and 23 Python tests. The test-only mid-input observation and
native-state serialization add no production processing or dependency. Full
state rollback/reset checks and all original fixture expectations are retained.

## Reproduction and boundaries

```sh
python3 tools/silk_channel_layout_fixtures.py target/reference/opus target/layout-fixtures.txt --restart-output target/layout-restarts.txt
python3 tools/silk_encoder_stereo_fixtures.py target/reference/opus target/layout-analysis.txt --channel-transitions
cargo run --offline --locked --release --example silk_layout_vectors -- target/layout-packets.bin
python3 tools/silk_mono_validation.py target/reference/opus target/layout-packets.bin
```

Fixture and actual packet-corpus regeneration are byte-identical. Fixture
SHA256 values are `985fab133a65c3528d4a832c86b283da43fc77b5077d4f6999913913c8168400`
(input histories), `584b922331c961079629b332e9314f7bc6dece52c25424993b98886c095b1694`
(partial initializer) and `cc89bc19a7ce4ec65c8821d93d8554aefe218d814696099b87ee9a1a86dc5a92`
(stereo analysis). The actual
packet corpus SHA256 is
`a261e8e8c5ea8a50dc413896b1ea6748a3a346aca2fed8204cc9ab08fce2596f`.

Native input/history matching uses fixed bands within each reference stream.
The automatic-bandwidth composition test verifies packet behavior and rollback,
not a complete original-encoder trace of simultaneous channel/band handshakes.
The unchanged-native decoder replay proves packet/decoder compatibility; it
does not establish original encoder byte identity or perceptual/speed parity.

Hybrid still has a fixed input/wire layout. A single adaptive SILK/CELT/Hybrid
packet owner, VOIP high-pass, restricted application behavior, cross-mode
prefills/5ms tails and the automatic library/CLI path remain required. This
milestone supplies real speech channel ownership for that integration; it does
not complete the broader codec goal or support SOTA claims.
