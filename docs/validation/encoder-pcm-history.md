# Common encoder PCM input history

Version 0.1.86 connects one private AUDIO input implementation to the actual
mono SILK, stereo SILK and Hybrid PCM packet paths. Filtering and the native
10ms input window now have a common owner type, `encoder::pcm::PcmState`.
Independent encoder instances still own independent streams. This is one
required part of the unified encoder; mode/channel transitions remain unfinished.

## Processing and transactions

Input filtering works on immutable API channels, before resampling or any
encoded-channel downmix. The original fixed-point DC rejection arithmetic and
symmetric output saturation remain unchanged. The prior planar DC fixtures
still check the same sample kernel with the original expected values.

The input state retains the latest 10ms of unmodified filtered PCM. Short
frames shift previous history; frames of at least 10ms replace it. Hybrid CELT
receives the previous 4ms followed by current filtered input, truncated to one
frame. Consequently, a 2.5ms frame would consume only part of retained input;
it cannot be implemented by subtracting 4ms from the current frame length.
Hybrid's existing 10/20ms output remains the same. High-band gain and stereo
fades happen after history capture and cannot alter the retained speech input.

Filtering and history commit are separate operations. Native SILK-DTX returns
after DC/resampler/analysis advancement but before updating outer delay history.
Digital-silence DTX continues through the layers and does update this history.
The actual mono/stereo packet engines return their existing private DTX decision
to their input owner, which commits history only for the appropriate decision.
Hybrid retains the same early-return boundary before alignment/history/fades.

All input state belongs to the existing staged packet transaction. Mono/stereo
FEC retries reuse the post-filter/resampler snapshot and the same filtered PCM;
only the successful attempt completes input history. Hybrid retries still
restore the entire pre-packet state before filtering. Invalid geometry, budget
and finalization failures preserve the original stream. Reset clears input
filters/history while retaining the existing encoder control contracts.

## Independent evidence

The optional oracle wraps the original `dc_reject` and the unchanged outer
`delay_buffer` update block from Opus 1.6.1 revision
`22244de5a79bd1d6d623c32e72bf1954b56235be`. The reference checkout must be clean;
the temporary scalar fixed-point build enforces fast64/Q24. This is an external
validation build, never a Cargo or ordinary test dependency.

[The numerical fixture](../../tests/opus-encoder-pcm-fixtures.txt) contains
1,440 sequential snapshots: all five input rates, mono/stereo API layouts,
all nine standard durations from 2.5ms through 120ms, alternating short/long
frames, repeated short frames, digital zero, full-scale extremes and seeded
noise. Each row checks both native DC memories and filtered/aligned/history
PCM fingerprints. The 330 skipped commits continue filtering while retaining
history. Their schedule is supplied to the primitive oracle; this alone is
not evidence that a packet engine makes the correct DTX decision.

Actual packet-owner DTX ordering is checked separately against the existing
unchanged-native public mono/stereo DTX schedules. These tests derive the early
return from native expected suppression plus the original digital-silence input,
then compare the entire input state after every packet. They exercise early
SILK-DTX, outer silence suppression and transmitted packets for both layouts.
The existing native Hybrid DTX test still checks layer calls, unchanged retained
history on the early return and fade-state ordering. Existing complete-state
FEC retry/failure/reset assertions are retained.

Five isolated source mutations are detected: wrong DC cutoff, wrong compensation
length, failure to shift old history for short frames, and unconditional history
advancement during early SILK-DTX in each of the actual mono/stereo packet owners.
The first three fail native fixture assertions; the latter two fail native public
DTX schedule assertions. All source changes are restored and both owner tests
pass again. No fixture expectations or existing fault-injection assertions were
weakened. Repeated optional fixture regeneration is byte-identical, SHA256
`5a062c9fbd7380287dfe65b1ee7559a760ae22032fa910cd9b7d54954ed87169`.

Regenerate the numerical fixture with an external original source checkout:

```sh
python3 tools/opus_pcm_fixtures.py target/reference/opus target/pcm-history-repeat.txt
cmp tests/opus-encoder-pcm-fixtures.txt target/pcm-history-repeat.txt
```

## Whole-packet preservation

[The regression report](results/pcm-history-0.1.86-regressions.json) compares
all 24 fixed SILK/Hybrid corpora against 0.1.85 byte-for-byte: 1,384,320 records
across 5,088 streams, 220,896 FEC requests, 768 zero-length PLC records and
387,082 single-byte record payloads. The single-byte count includes recovery
records and is not a count of unique transmitted DTX packets. All per-corpus
sizes, hashes and geometry counts also match the tracked
[0.1.85 native-replayed report](results/speech-state-0.1.85-regressions.json).
Raw records include packets and decoder PCM/final-range fingerprints.

This milestone runs a fresh unchanged-native PCM primitive oracle and exact
whole-corpus comparison; it does not repeat the previous full native packet
replay. The report therefore correctly records `native_replay: false`.
Compatibility with that prior replay follows from identical complete corpus
bytes, rather than a new native-replay run. Corpus generation contains no
package-version/vendor metadata; changing release metadata does not change it.

Reproduce with separate executable directories built from the annotated
baseline and candidate revisions:

```sh
cargo build --offline --locked --release --examples
python3 tools/speech_state_regressions.py BASELINE_EXAMPLES CANDIDATE_EXAMPLES target/pcm-history-check target/pcm-history-report.json
```

All seven final repository gates pass: formatting, debug/release offline locked
tests, all-targets Clippy with warnings denied, warning-free rustdoc, the zero
Cargo dependency checker and 23 Python tests. The temporary mutation edits were
restored and both real packet-owner history tests passed again. No existing
fixture expectations, failure assertions, unsafe policy or dependency gates
were relaxed.

## Remaining complete-encoder requirements

The common input type currently implements the normal AUDIO DC/4ms/10ms path.
VOIP's adaptive high-pass smoothing, restricted-application buffering, native
input/coded-channel resampler handshakes, LP readiness, SILK/CELT prefills and
5ms redundancy tails must still be connected to a single adaptive packet owner.
No automatic cross-mode library/CLI API or perceptual/performance claim follows
from this milestone. Standalone CELT's existing input contract is unchanged.
