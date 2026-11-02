# SILK encoder DTX

Opura 0.1.69 adds opt-in DTX to fixed-bandwidth mono/stereo SILK encoders.
Use `set_dtx(true)` or `opura encode input.wav output.opus --mode silk --dtx`.
`in_dtx()` reports whether the last successful encode suppressed its payload;
changing the setting does not rewrite that diagnostic. Reset retains configured
rates and DTX enablement, while clearing all stream history. Default output
remains unchanged. FEC and adaptive bandwidth are still separate work.

DTX emits a one-byte TOC packet with final range zero, even under CBR. It
preserves the configured packet duration/channel count/bandwidth. All input,
analysis, stereo and quantizer histories continue to advance; DTX saves wire
bytes and makes no promise of reduced encoder CPU. Invalid geometry/options or
a late budget failure preserve all codec state, DTX clocks and last diagnostic.
Encoding still requires the normal 3..=1276-byte budget for the full pipeline.

## Native decisions and recovery

The pinned reference is unchanged fixed-point scalar libopus v1.6.1 at
`22244de5a79bd1d6d623c32e72bf1954b56235be`, built with `DISABLE_FLOAT_API`.
This implementation follows that fixed-point policy; it does not implement
the float reference's optional external activity analysis.

SILK counts actually analyzed inactive internal frames per channel, even when
DTX is disabled. Ten frames precede suppression, and after 20 consecutive
suppression opportunities a full refresh is required. A transmit decision in
any internal frame sticks for the entire packet. An omitted stereo side keeps
its counter and packet flag; a side resumption does not reset its VAD/DTX history.
Both channels must permit suppression. The packet reservoir accounts for zero
payload when SILK suppresses it.

Digital silence (every original PCM16 sample zero) uses a separate outer clock:
200 ms hangover, at most 400 ms suppression before a full refresh. Clock updates
use actual packet duration, including 10/40/60 ms. During this silence VAD
probability is capped below the inactive threshold, preventing delayed filter
tails from restarting activity. The outer suppression leaves SILK's already
updated bit reservoir intact. Native SILK suppression returns before this
outer clock is updated. These distinctions affect refresh and resume behavior.

The retained original oracle verifies:

- 4220 VAD/inactive-counter/packet-flag transitions from unchanged public calls;
- 400 direct original outer clock decisions at all four durations;
- all 2400 complete mono/stereo public encoder suppression decisions over
  speech, digital silence, nonzero quiet PCM, resumed speech and DTX toggles.

Long stereo silence exposed a separate near-zero modified-Burg bug: the Rust
code rejected a negative residual estimate although native fixed-point
conditioned subtraction can round below zero. The minimal input has 100 samples,
the first three -1 and the rest zero, NB/10 ms, minimum inverse gain 63388.
Original Burg returns energy -4 at Q16 and the exact retained coefficient
vector. Rust now preserves this signed estimate for interpolation comparisons.
It is not clamped or substituted with another predictor. The probe is retained
alongside native DTX evidence, and long continuous packet streams cover the fix.

## End-to-end evidence

The optional Rust corpus emits 120 streams / 12000 packets for each channel
layout: all five input rates, three bandwidths, four durations and CBR/variable
output. Each stream includes speech, digital silence, quiet nonzero PCM,
resumed speech and DTX disable/re-enable. Complexities are 0/6/10 for NB/MB/WB,
with 20% loss scaling. Each layout contains 6720 suppressed packets. The unchanged
public libopus decoder matches every packet's decoded sample count, full PCM
fingerprint and final range through suppression, refresh and recovery.

Independent FFmpeg/libopus also decodes 60 mono and 60 stereo mixed speech/pause
CLI files with exact source duration, channel count and 312-sample pre-skip.
Packet counts include every duration slot; unsuppressed packets retain the
fractional 32101 bits/s CBR budget and suppressed packets are one byte. The
tool retains individual PCM hashes, packet/byte counts, binary hash/version and
external tool version. The builtin FFmpeg decoder passes these DTX files; its
separately documented short-file tail issue remains checked by no-DTX runs.
Ordinary CLI tests additionally verify byte savings on pure silence, exact
duration and unsupported/repeated flags. Rust tests cover error rollback during
DTX, reset setting retention and original schedule/state behavior.

## Reproduction

Use fresh output paths and a clean pinned external checkout:

```sh
python3 tools/silk_encoder_dtx_fixtures.py target/reference/opus target/dtx-native.txt
cmp tests/silk-encoder-dtx-fixtures.txt target/dtx-native.txt
cargo run --offline --locked --release --example silk_dtx_vectors -- target/dtx-mono.bin 1
cargo run --offline --locked --release --example silk_dtx_vectors -- target/dtx-stereo.bin 2
python3 tools/silk_mono_validation.py target/reference/opus target/dtx-mono.bin
python3 tools/silk_mono_validation.py target/reference/opus target/dtx-stereo.bin
cargo build --offline --locked --release --bin opura
python3 tools/silk_cli_validation.py target/release/opura target/dtx-mono-cli --dtx
python3 tools/silk_cli_validation.py target/release/opura target/dtx-stereo-cli --channels 2 --dtx
```

All seven mandatory offline gates apply. External compilers/libraries/tools are
optional validation only; ordinary Cargo builds/tests have zero dependencies.
This establishes synthetic DTX interoperability and state continuity, not
representative perceptual quality, overall encoder completeness or SOTA speed.
