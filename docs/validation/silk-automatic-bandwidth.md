# Automatic SILK bandwidth

Opura 0.1.72 adds `set_automatic_bandwidth(true)` and `automatic_bandwidth()` to
mono/stereo PCM encoders, plus CLI `--mode silk --bandwidth auto`. Fixed bandwidth
remains the default. The full packet byte budget, including TOC, supplies the
nominal wire bitrate for automatic decisions; capped-variable output uses that
same cap as its nominal rate. Channel layout and external PCM rate stay fixed.

Automatic mode clears an external `set_bandwidth_target` request. An external
target disables automatic selection; disabling automatic mode stops the taper
at the current rate. Forced same-rate setters remain no-ops, including the mode;
a changed forced rate returns to fixed operation. Reset retains mode/settings
and clears policy, filter, resampler and stream history. Errors roll back the
entire policy and stream, including FEC queues and pending acknowledgements.

## Original policy and connected pipeline

The policy follows unchanged fixed-point Opus v1.6.1 at
`22244de5a79bd1d6d623c32e72bf1954b56235be`, for the AUDIO default voice estimate
48/128 with SILK selected and fixed channels. It is not global Opus mode/channel
selection or a content-derived voice/music classifier.

Equivalent bitrate includes the original short-frame overhead, CBR penalty,
complexity adjustment, scalar-SILK NSQ penalty and loss adjustment for a known
SILK mode. The unknown-mode half-loss formula is intentionally not substituted.
The original five-band automatic history is retained for hysteresis even though
actual SILK packets are limited to NB/MB/WB. Automatic MB preference is promoted
to WB before API bounds/FEC; actual rate migration can still pass through MB.

Update automatic thresholds on first selection or previous packet activity
permission. Keep automatic-band history separate from the selected target and
actual internal rate. Apply the native WB-LP guard, low-cap WB guard, API bounds,
bandwidth-reducing FEC decision, restricted-SILK WB cap and hard rate limits in
that order. Hard internal maxima are MB below an effective 8 kbit/s and NB below
7 kbit/s; 10 ms budgets first receive the native 2/3 adjustment.

FEC eligibility uses the original outer total equivalent rate and hysteresis,
including narrowing when loss exceeds 5%. It is passed directly to the LBRR
setup rather than recomputed using the previous actual band's per-channel rate.
Fixed/externally targeted operation retains its previous fixed-band eligibility.

The selected desired/max rate feeds the connected native LP handshake and
history migration before current input resampling. VAD sees unfiltered PCM;
side omission pauses its LP. Successful packets, including DTX and PLC output,
advance activity permission and readiness; errors preserve every state. See
[connected transition validation](silk-adaptive-bandwidth.md).

## Explicit budget-overflow concealment

Native channel RDO quotas are soft targets. In automatic mode the physical range
writer bounds each completed candidate; the whole packet is checked against its
byte cap after all frames/channels. If a pending FEC prefix causes overshoot,
retry once from identical post-input state without that prefix. If the complete
primary still exceeds the cap, emit the original one-byte zero SILK payload that
asks the decoder to conceal the packet, with final range zero. CBR pads to the
requested size; variable output is two bytes including TOC.

`packet.used_plc()` and the CLI's PLC fallback count expose this outcome
separately from DTX. Analysis/NSQ, actual encoded-bit reservoir accounting,
activity/handshake and newly generated next-packet backups persist. The lost
prefix is not advertised by `has_fec()`. Fixed/externally targeted streams retain
their previous strict error behavior. A valid tiny budget can thus produce PLC
in automatic mode; successful packet delivery alone does not establish quality.

The unchanged public native restricted-SILK encoder reproduces the minimized
WB stereo case: 48 kHz, 27200 bit/s, 10 ms, complexity 0, 20% loss/FEC, fixed two
channels, deterministic triangular PCM. At packets three/four it uses 273/270
bits against a 264-bit payload cap and sends padded PLC packets with range zero.
The oracle records complete per-channel before/after costs, packet payload
sizes, PLC detection and ranges, including the earlier valid packets. The Rust
regression checks delivery, duration, explicit diagnostics and continuation;
it does not claim byte-identical complete native encoding.

## Independent evidence

The policy oracle extracts the original `compute_equiv_rate`, `decide_fec`,
threshold arrays, automatic selection block, API constraints and desired/max-rate
setup. Its 12480 controlled snapshots cover all input rates/durations/layouts,
26 byte budgets from 3 to 1276, complexity 0/1/2/6/10, CBR/variable mode, six loss
levels, enable/history flags, activity gates, WB-LP status and voice estimates.
It compares equivalent rate, desired/max internal rate, automatic/chosen band
history and FEC eligibility. This is exact controlled-policy evidence rather
than an exact public-native encoder packet comparison.

The automatic packet corpus covers all five input rates, three initial rates,
four base durations, CBR/capped-variable output and both layouts. It changes
wire budgets over long intervals, repeats speech/silence/quiet/antiphase stereo,
toggles FEC/loss and changes duration. Guards require at least ten completed
transitions in every adjacent direction and exclude cold first-packet choices.
Each normal/loss replay contains 120 streams and 108000 decoded records. Loss
replay requests 36000 recoveries. Every record is checked using unchanged public
native decoding for sample count, complete PCM fingerprint and final range.

The deliberate 14 kbit/s total two-channel stress interval is not representative
quality evaluation. Final normal/loss replay counts 9 mono and 5664 stereo
PLC packets; these outcomes are retained and reported rather than hidden as DTX.
Typical CLI rates and real-recording quality require separate measurements.
General mode/channel adaptation, cross-mode CELT transition redundancy, Hybrid
encoding and competitive quality/speed claims remain unfinished.

## Reproduction

Use fresh output paths and the unchanged pinned external checkout:

```sh
python3 tools/silk_encoder_automatic_policy.py target/reference/opus target/automatic-policy.txt
cmp tests/silk-encoder-automatic-policy-fixtures.txt target/automatic-policy.txt
python3 tools/silk_encoder_automatic_overflow.py target/reference/opus target/automatic-overflow.txt
cmp tests/silk-encoder-automatic-overflow-fixtures.txt target/automatic-overflow.txt
cargo run --offline --locked --release --example silk_automatic_vectors -- target/automatic-mono-normal.bin 1 normal
cargo run --offline --locked --release --example silk_automatic_vectors -- target/automatic-mono-loss.bin 1 loss
cargo run --offline --locked --release --example silk_automatic_vectors -- target/automatic-stereo-normal.bin 2 normal
cargo run --offline --locked --release --example silk_automatic_vectors -- target/automatic-stereo-loss.bin 2 loss
python3 tools/silk_mono_validation.py target/reference/opus target/automatic-mono-normal.bin
python3 tools/silk_mono_validation.py target/reference/opus target/automatic-mono-loss.bin
python3 tools/silk_mono_validation.py target/reference/opus target/automatic-stereo-normal.bin
python3 tools/silk_mono_validation.py target/reference/opus target/automatic-stereo-loss.bin
cargo build --offline --locked --release --bin opura
python3 tools/silk_cli_validation.py target/release/opura target/automatic-cli --automatic
python3 tools/silk_cli_validation.py target/release/opura target/automatic-stereo-cli --automatic --channels 2 --fec 20 --dtx
```

Reference compilers/libraries and FFmpeg are optional validation tools. Ordinary
Cargo builds/tests retain zero third-party dependencies and need no C compiler.

## Final verification

All seven mandatory offline gates pass, including 19 Python tests. Both new
policy and overflow fixtures and the existing long scheduling fixture regenerate
exactly. All four versioned automatic corpora match original native decoding:
432000 decoded records and 72000 FEC calls. Per replay mono has 14819 protected,
33750 DTX and 9 PLC packets; stereo has 13519 protected, 34600 DTX and 5664 PLC
packets. Completed adjacent transitions exclude first-packet selection: mono
80 NB→MB / 72 MB→NB / 21 MB→WB / 15 WB→MB; stereo 100 / 88 / 69 / 69.

Sixteen previous corpora remain byte-identical: 28320 no-FEC packets, 48000 FEC
replay records, 48000 forced-band records and 432000 externally targeted records.
Only the external-target example's diagnostic counters changed to exclude cold
selection; the encoded corpus did not change.

At the explicit test rates of 32101 bit/s for mono and unprotected stereo,
and 64101 bit/s for protected stereo, all 640 CLI files
(480 fixed and 160 automatic, each layout with/without DTX/FEC) have exact duration,
312-sample pre-skip and packet byte accounting. All emit zero PLC fallbacks at
these tested rates. Every report version/SHA matches the release executable;
all 480 fixed regression PCM hashes match v0.1.71.

`silk_cli_native_validation.py` independently decodes all 640 retained Ogg files
with the unchanged pinned fixed-point original and compares every PCM byte after
pre-skip/granule trimming. All pass exactly. The separate FFmpeg-embedded libopus
build differs on all 320 DTX cases (maximum absolute signed16 difference 1144),
including unchanged v0.1.71 cases. The exact cause across those builds has not
been isolated; do not assume bit-identical noise output from that comparator.
Its hashes, differing sample counts and maximum deltas remain in CLI reports.
The builtin FFmpeg short-file tail issue is still independently reproduced.
These diagnostics are not erased or described as exact FFmpeg PCM agreement.

For exact retained CLI comparison, run after the CLI validation commands:

```sh
python3 tools/silk_cli_native_validation.py target/reference/opus target/native-cli-report.json target/automatic-cli/report.json target/automatic-stereo-cli/report.json
```

Four additional no-`--bitrate` automatic CLI probes exercise the actual defaults
(24000 mono / 48000 stereo), with DTX and FEC off/on. Exact native PCM, duration,
packet budgets, zero PLC counts and independent `opus_packet_has_lbrr` detection
pass; every supplementary report matches the current binary SHA.
