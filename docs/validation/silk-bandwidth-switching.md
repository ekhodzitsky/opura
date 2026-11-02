# SILK packet-boundary bandwidth changes

Opura 0.1.71 adds `bandwidth()` and `set_bandwidth(SilkRate)` to mono/stereo PCM
encoders. A setter forces the internal NB/MB/WB rate for subsequent packets,
preserving the external input rate and channel layout. It migrates existing
history and resampler state rather than replacing the stream with a new encoder.
Calling it with the current rate changes nothing, including pending FEC.

This is an explicit packet-boundary API. The CLI still uses a single bandwidth
per file. Automatic target selection and connecting the native LP taper/handshake
to raw PCM scheduling remain separate work; forced changes are not a claim of
click-free or perceptually optimal adaptation.

## State migration

`ChannelFrameEncoder::change_rate` converts the native `x_buf` history from the
old internal rate to the API rate using a fresh decoder resampler, then from the
API rate to the new internal rate using a fresh encoder resampler. The latter
becomes the live, primed input resampler. Converted history overwrites exactly
the native prefix of the fixed 720-sample buffer, preserving the remaining tail.

The window is twice the previous configured internal frame duration plus 5 ms:
25 ms for a 10 ms frame, 45 ms for a 20 ms frame. Native setup resampling precedes
the packet-size update, so a simultaneous duration change must use the previous
geometry. This also applies to an omitted stereo side: packet geometry is tracked
even when that channel or FEC is not encoded.

Prediction/shape/gain/NSQ restart as in native setup. Frame counters, entropy and
redundant gain history persist. The raw PCM wrappers retain DC rejection, VAD,
mono carry, stereo predictor/sample/width state, DTX clocks, previous activity
and bit reservoir. Incompatible pending FEC is cleared. Existing last-packet
diagnostics retain their meaning until the next successful packet.

Both stereo channels and live input resamplers change atomically. A mismatched
old resampler is rejected; tests force a second-channel failure and verify that
even the first channel's tentative conversion is rolled back. Same-rate calls
are no-ops. Cold setters produce the same packets/state as constructing the
encoder directly at the selected bandwidth.

## Native transition components

`silk::encoder::bandwidth` separately exposes the original bandwidth controller
and variable-cutoff LP state. Selection handles initialization/saved rate,
API/minimum/maximum constraints, activity permission, direction reversals,
switch-ready acknowledgement and the native 5 ms transition bit reservation.
It returns a rate and budget; the enclosing encoder applies the rate change.

The LP state uses 256 internal frame steps, direction -2 down/+1 up, five Q28
coefficient sets and native signed16 interpolation. In-place filtering uses
the original split-coefficient biquad and wrapping/rounding/saturation rules.
VAD must see the unfiltered frame; filtering belongs immediately before channel
buffering. An actual rate change clears filter delay while retaining taper
position/direction. These building blocks do not silently alter fixed streams.

## Independent evidence

The reference is unchanged fixed-point scalar libopus v1.6.1 at
`22244de5a79bd1d6d623c32e72bf1954b56235be`.

- 2400 original controller calls match rate, readiness, reserved bits and every
  LP state field across input rates, constraints, current/saved rates, flags,
  packet durations, taper positions/directions and budget edges.
- 4626 original LP calls cover every position 0..256 at all three rates,
  both internal durations and all directions. Complete PCM fingerprints and
  final filter state match over silence, signed16 endpoints, alternation,
  ramps and deterministic random PCM, including extreme initial delay state.
- All 90 old/new/API/duration combinations match the complete 720-sample history
  fingerprint. Eight subsequent alternating 10/20 ms blocks per case add 720
  PCM fingerprint checks of the primed live resampler. Same-rate cases retain
  their already-running resampler.
- The existing public-native channel golden now performs actual rate migration
  instead of injecting native-converted history. Original main/backup symbols,
  pulses, bytes, NSQ state and packet/recovery PCM still match.

The optional dynamic-band corpus uses all five input rates, all three initial
bands, four base durations, CBR/capped-variable output and both channel layouts.
It includes every directed internal-rate change, simultaneous duration changes,
digital silence/DTX, quiet PCM, correlated/antiphase stereo, FEC/loss toggles and
abrupt budgets. Normal replay decodes every packet; loss replay drops every
third packet and requests recovery from the next before normal decoding.
Each layout/replay has 120 streams and 12000 decoded records. The four corpora
therefore compare 48000 decoded sample counts, complete PCM fingerprints and
final ranges with the unchanged original public decoder, including 7920 FEC
requests. Missing/incompatible backups use normal decoder concealment.

Ordinary tests cover cold/live setters, retained histories, full atomicity,
all six rate changes with continuous PCM decoding at every input rate, TOC
bandwidth, error rollback and queue clearing. Existing fixed-stream behavior
and all seven mandatory offline gates apply. These results establish native
state/bitstream interoperability; quality, latency and CPU advantage require
separate representative measurements.

All seven mandatory offline gates pass on the versioned implementation, including
19 Python tests. The previous 28320 no-FEC packet corpus and 48000 FEC replay
records remain byte-identical. All 480 fixed-band CLI files retain their previous
decoded PCM hashes; report version and SHA-256 match the current executable.
The known builtin FFmpeg short-file tail issue remains independently reproduced
with the reference encoder rather than excluded from the results.

## Reproduction

Use fresh output paths and a clean pinned external checkout:

```sh
python3 tools/silk_encoder_bandwidth_fixtures.py target/reference/opus target/bw-control.txt
cmp tests/silk-encoder-bandwidth-fixtures.txt target/bw-control.txt
python3 tools/silk_encoder_rate_change_fixtures.py target/reference/opus target/bw-history.txt
cmp tests/silk-encoder-rate-change-fixtures.txt target/bw-history.txt
cargo run --offline --locked --release --example silk_bandwidth_vectors -- target/bw-mono-normal.bin 1 normal
cargo run --offline --locked --release --example silk_bandwidth_vectors -- target/bw-mono-loss.bin 1 loss
cargo run --offline --locked --release --example silk_bandwidth_vectors -- target/bw-stereo-normal.bin 2 normal
cargo run --offline --locked --release --example silk_bandwidth_vectors -- target/bw-stereo-loss.bin 2 loss
python3 tools/silk_mono_validation.py target/reference/opus target/bw-mono-normal.bin
python3 tools/silk_mono_validation.py target/reference/opus target/bw-mono-loss.bin
python3 tools/silk_mono_validation.py target/reference/opus target/bw-stereo-normal.bin
python3 tools/silk_mono_validation.py target/reference/opus target/bw-stereo-loss.bin
```

The optional oracles compile unchanged native functions and use original public
decoding; no native source/library is a Cargo dependency. Automatic bandwidth/
mode policy, raw taper scheduling and Hybrid encoding remain unfinished.
