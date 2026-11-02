# Connected SILK bandwidth transitions

The Opura 0.1.72 SILK PCM frontends connect the native controller and LP taper to
actual mono/stereo encoding. `set_bandwidth_target(Some(rate))` supplies an
external desired NB/MB/WB rate; `bandwidth_target()` returns that request while
`bandwidth()` reports the actual rate. Selection caps the desired rate by the
external PCM rate. `None` stops the taper and retains the current bandwidth.
Reset retains the request and clears stream/filter/handshake history.

This API supplies the inner SILK adaptation pipeline. Version 0.1.72 also
connects [automatic fixed-point AUDIO bandwidth](silk-automatic-bandwidth.md)
through the API and CLI. Global Opus mode decisions and voice/music signal
classification remain unfinished. Cross-mode CELT transition redundancy and Hybrid encoding
remain separate unfinished work. Valid transitions do not establish click-free
quality or a quality/CPU advantage.

## Packet and frame ordering

Each packet stages the entire stream before selecting the internal rate. The
controller uses previous successful packet activity permission and readiness.
An actual change migrates buffered channel history and primes new live input
resamplers before processing current external PCM. Incompatible pending FEC is
cleared; DC/VAD/stereo/DTX/reservoir histories persist.

Stereo runs native control on both LP states, including each reservation of
5 ms of the remaining entropy budget, then forces actual side rate to the mid.
Nominal bitrate and reservoir accounting retain the full packet budget; only
the channel entropy caps use the reduced transition budget. The LP delay resets
on actual rate changes while direction and taper position persist.

For every internal frame, VAD observes unfiltered PCM. The LP filter runs just
before channel history insertion/analysis/NSQ. Stereo M/S prediction precedes
this filtering; an omitted side frame advances neither side VAD nor side LP.
The side LP can consequently lag the mid even though their actual rates agree.

Only complete successful packets update native switching permission from final
mid activity and acknowledge readiness, including one-byte DTX output. The
threshold is `13 + mul_wb(3188, elapsed_ms)`; a value below it permits switching
and resets the clock, otherwise the clock advances by the packet duration.
Failed packets preserve selection, migration, filter, clock and acknowledgements
along with ordinary codec state. A bounded FEC retry starts from the identical
post-input snapshot and replays the taper once, without the old prefix.

The existing forced `set_bandwidth` keeps same-rate calls as complete no-ops,
including the target. Changing the forced rate clears gradual targeting; this
allows existing fixed streams and their exact packet bytes to remain stable.

## Native scheduling evidence

The optional oracle compiles unchanged original `control_audio_bandwidth.c`,
`LP_variable_cutoff.c`, `biquad_alt.c` and `tables_other.c` from pinned libopus
v1.6.1 at `22244de5a79bd1d6d623c32e72bf1954b56235be`. Its driver schedules
original calls and uses the original activity formula/macros/constants from
`enc_API.c`. This is controlled scheduling evidence, not a trace of the complete
public native encoder or a comparison of its final encoded bytes.

Sixty sequences of 900 packets cover all five input rates, all three initial
bands, both channel layouts, fixed/variable duration schedules, five bit budgets,
quiet/activity changes, direction reversals and side omission. Aggregate hashes
include every filtered PCM sample, selected rate, readiness, entropy budget,
activity clock and complete LP delay/direction/position after every packet.
All 54000 packets match; they include 176 rate changes, 164 readiness events and
31635 omitted side frames. Existing independent controller/filter/history
fixtures continue to cover individual arithmetic boundaries and all migrations.

Ordinary tests exercise actual down/up PCM streams through all three rates,
DTX/FEC, independent decoder sample counts/ranges, delayed side filtering,
failed-packet rollback and retained target settings on reset.

## Reproduction

Use fresh output paths and the unchanged pinned external checkout:

```sh
python3 tools/silk_encoder_bandwidth_schedule.py target/reference/opus target/bw-schedule.txt
cmp tests/silk-encoder-bandwidth-schedule-fixtures.txt target/bw-schedule.txt
cargo run --offline --locked --release --example silk_adaptive_vectors -- target/adaptive-mono-normal.bin 1 normal
cargo run --offline --locked --release --example silk_adaptive_vectors -- target/adaptive-mono-loss.bin 1 loss
cargo run --offline --locked --release --example silk_adaptive_vectors -- target/adaptive-stereo-normal.bin 2 normal
cargo run --offline --locked --release --example silk_adaptive_vectors -- target/adaptive-stereo-loss.bin 2 loss
python3 tools/silk_mono_validation.py target/reference/opus target/adaptive-mono-normal.bin
python3 tools/silk_mono_validation.py target/reference/opus target/adaptive-mono-loss.bin
python3 tools/silk_mono_validation.py target/reference/opus target/adaptive-stereo-normal.bin
python3 tools/silk_mono_validation.py target/reference/opus target/adaptive-stereo-loss.bin
```

Each corpus has 120 streams and 108000 decoded records across all input rates,
initial bands, four base durations and CBR/capped-variable output. Streams change
desired rate every 300 packets while repeating speech, silence, quiet PCM,
antiphase stereo, FEC/loss toggles, simultaneous duration changes and byte-budget
drops. Loss replay drops every third packet and requests recovery from the next
before normal decoding. Every normal/loss record is compared with original
public native decoding for sample count, complete PCM fingerprint and final
range. These are interoperability checks, not perceptual quality measurements.

The corpus also checks the actual encoder bandwidth after every packet and
requires at least ten changes in each adjacent direction (NB↔MB and MB↔WB).
This guards against workloads whose requested target changes before a transition
can complete. An earlier 100-packet target schedule passed decoding but never
reached MB→NB; its diagnostic output is retained locally and is not the final
transition coverage evidence.

The strengthened final workload passes all four unchanged public native decoder
replays: 432000 records, including 72000 FEC requests. Per replay mono produces
43644 protected/33750 DTX packets and stereo 46298 protected/33748 DTX packets.
Mono completes 64 NB→MB, 64 MB→NB, 36 MB→WB and 36 WB→MB changes; stereo completes
64, 64, 48 and 48 respectively. These counts exclude first-packet selection;
initial rate choices and API caps are separate from gradual transitions.

All seven mandatory offline gates pass on the final implementation, including
19 Python tests. Twelve previous corpora remain byte-identical: 28320 no-FEC
packets, 48000 FEC replay records and 48000 forced-band replay records. The
optional schedule fixture regenerates exactly. The connected pipeline is included in v0.1.72 alongside the automatic target
policy.
