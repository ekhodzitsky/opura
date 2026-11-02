# Fixed-bandwidth SILK FEC packets

Opura 0.1.70 connects the [LBRR channel core](silk-fec-channel.md) to mono/stereo
PCM encoders at all five input rates, fixed NB/MB/WB bandwidths and 10/20/40/60 ms.
Request FEC with `set_inband_fec(true)` and a nonzero loss percentage in existing
encoder options, or `--mode silk --fec 20` in the file CLI. Defaults stay off.
`has_fec()` reports actual prior-packet protection in the last successful packet,
not whether the setting is enabled. Disabling generation can still transmit a
previously queued backup in the next compatible packet.

## Packet and failure contracts

The pending queue has fixed storage for up to three frames per channel. It
retains redundant symbols/pulses and stereo predictor fields, not transformed
PCM. Incompatible packet duration clears the queue, including 10/20 ms changes.
Input rate, internal bandwidth and channel layout remain fixed per encoder.
Reset clears every history/queue/diagnostic while preserving DTX/FEC enablement.
Invalid input/options or an encoding failure leave the entire encoder unchanged.

Previous-packet LBRR masks are written channel-first, followed by frame-first
mid/side redundant data and stereo fields. Conditional coding requires an
adjacent protected frame in that channel. Current main frames then generate
the next queue. Native gain increments, bit-cost moving average and LTP scaling
are retained. Stereo hard limits reserve side capacity from the remaining
budget after the prefix, rather than charging prior-packet bytes twice.

FEC cannot displace a usable primary packet. The first attempt includes any
compatible queued prefix. On a bit/storage budget failure it retries once
without the entire pending prefix, from the same post-resampling snapshot.
VAD, stereo, gain, entropy, quantizer and rate histories are restored together;
input filtering/resampling is not repeated. Non-budget errors are not hidden
by this retry. Each channel search stays bounded at seven iterations and the
packet has at most two attempts. The existing non-FEC strict-bound path remains.
Explicit byte caps and CBR packet length hold in either attempt.

DTX can suppress the whole payload, producing the same one-byte TOC/final-range
zero contract as before. Such a packet reports no FEC. The full analysis path
still runs, so neither FEC nor DTX implies a CPU improvement.

## Fixed-bandwidth policy

Native FEC threshold/hysteresis constants are evaluated at the per-channel
packet payload rate, with nonzero estimated loss and activity gates. A clean
original policy oracle verifies 576 threshold/gain cases, each checked for mono
and stereo. It extracts the original `decide_fec` and `silk_setup_LBRR` functions
and original constants from pinned libopus v1.6.1 revision
`22244de5a79bd1d6d623c32e72bf1954b56235be`.

These APIs preserve the requested bandwidth. If native automatic policy would
lower bandwidth to enable FEC, this fixed-mode policy leaves the bandwidth alone
and declines new backups. This is not a replica of the full native automatic
mode/bandwidth/equivalent-rate controller. Packet budget admission can also omit
previous backups; FEC is requested protection, not a guarantee on every packet.
The channel record retains an original internal entropy overflow and its actual
padded PLC response. Opura's admission retry addresses that class of oversize
prefix without substituting a corrupt/PLC primary packet.

## Evidence

The optional Rust corpus has 120 streams per channel layout and replay mode:
all 15 input/internal rate pairs, four durations, CBR/capped-variable output and
complexities 0/6/10. Each stream encodes 100 packets. Normal replay decodes every
packet; loss replay drops every third packet and requests recovery from the next
before normal decoding. Each replay has 12000 decoded records; both layouts and
both modes total 48000 independent sample-count, full PCM fingerprint and final
range comparisons against the unchanged original public decoder.

The final corpus contains 4345 protected mono packets and 4587 protected stereo
packets per replay, with 3600 DTX packets per layout. The two loss replays make
7920 recovery calls in total. Suppression/protection counts describe emitted
packets; missing-prefix recovery still follows the normal concealment path.

The workload includes speech, digital silence/DTX, nonzero quiet PCM, resumed
speech, correlated/antiphase stereo, FEC and zero-loss toggles, duration changes
while speech is active and abrupt packet budget reductions while backups are
pending. Missing/partial LBRR recovery falls back to the decoder's standard
concealment. This verifies protocol/state continuity, not a quality advantage.

Ordinary tests check all geometries, sparse stereo masks and diagnostics against
parsed headers. Mono/stereo oversized-prefix tests compare the retry's bytes
and subsequent full state with encoding the same input from an explicitly
cleared queue. Other tests cover DTX, geometry invalidation, reset settings and
complete rollback after invalid input and late budget errors. The original
channel/stereo/DTX fixtures remain normal-test dependencies only as inert data.

The optional CLI matrix checks 60 cases per channel layout and DTX setting with
FEC enabled: 240 files independently decoded by FFmpeg/libopus. A separate
external libopus `opus_packet_has_lbrr` probe must find protection in every case.
Reports retain its version, per-case protected packet counts, requested loss,
bitrate, exact duration/pre-skip, fractional CBR accounting and PCM hashes,
plus binary/version and FFmpeg provenance. No-FEC and DTX regression matrices
retain their separate short-file/default-FFmpeg tail-trimming check.
All 240 no-FEC regression PCM hashes match 0.1.69, and all 28320 no-FEC/default
raw packets are byte-identical. Separate default-bitrate mono/stereo CLI probes
also detect FEC and preserve exact native decoded duration. All seven mandatory
offline gates pass, including 19 Python tool tests; both native fixtures reproduce.

## Reproduction

Use fresh output paths and the unchanged pinned external source:

```sh
python3 tools/silk_encoder_fec_policy.py target/reference/opus target/fec-policy.txt
cmp tests/silk-encoder-fec-policy-fixtures.txt target/fec-policy.txt
cargo run --offline --locked --release --example silk_fec_vectors -- target/fec-mono-normal.bin 1 normal
cargo run --offline --locked --release --example silk_fec_vectors -- target/fec-mono-loss.bin 1 loss
cargo run --offline --locked --release --example silk_fec_vectors -- target/fec-stereo-normal.bin 2 normal
cargo run --offline --locked --release --example silk_fec_vectors -- target/fec-stereo-loss.bin 2 loss
python3 tools/silk_mono_validation.py target/reference/opus target/fec-mono-normal.bin
python3 tools/silk_mono_validation.py target/reference/opus target/fec-mono-loss.bin
python3 tools/silk_mono_validation.py target/reference/opus target/fec-stereo-normal.bin
python3 tools/silk_mono_validation.py target/reference/opus target/fec-stereo-loss.bin
cargo build --offline --locked --release --bin opura
python3 tools/silk_cli_validation.py target/release/opura target/fec-mono-cli --fec 20
python3 tools/silk_cli_validation.py target/release/opura target/fec-stereo-cli --fec 20 --channels 2
python3 tools/silk_cli_validation.py target/release/opura target/fec-dtx-mono-cli --fec 20 --dtx
python3 tools/silk_cli_validation.py target/release/opura target/fec-dtx-stereo-cli --fec 20 --channels 2 --dtx
```

All seven mandatory offline gates apply. External tools/libraries are optional
validation, not Cargo dependencies. Adaptive bandwidth, Hybrid encoding and
complete automatic policy remain work. Representative quality, latency, memory
and speed comparisons are still required; no SOTA claim is established.
