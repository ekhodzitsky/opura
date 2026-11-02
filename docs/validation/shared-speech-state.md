# Shared speech state and frame analysis

Version 0.1.85 extracts one private speech-state implementation and actual frame
analysis used by the mono SILK, stereo SILK and Hybrid PCM packet engines.
Each independently constructed encoder still owns its stream. Public APIs and
fixed-mode defaults are unchanged. The unified adaptive PCM owner and native
cross-mode transition processing remain unfinished.

## Ownership and processing

`silk::encoder::state::SpeechState<CHANNELS>` owns the input resamplers, channel
encoders, VAD histories, stereo prediction state, mono sample memory, previous
side/activity state, excess-bit feedback, DTX and queued FEC. Const channel
capacity preserves existing mono/stereo FEC eligibility and queue semantics.
Standalone mono uses capacity 1; stereo SILK and existing Hybrid use capacity 2.
This is a private reusable state type, not a global shared instance.

The common mono/stereo analysis produces bounded internal PCM, per-channel
activity/rates and optional stereo prefix metadata. It runs native stereo
analysis, side prediction restart, VAD, silence probability limits and per-frame
DTX activity policy. The packet engine retains budget decisions, LP filtering,
entropy ordering, conditional pitch coding, redundancy recording and final
commit. A shared completion operation advances previous activity/side state
after encoding. Rate migration and reset reuse the same state implementation.

Mono input buffering moves from a whole-packet preprocessing loop into staged
frame analysis. One persistent sample is carried across all internal frames;
splitting 40/60 ms packets never introduces an extra delay at component boundaries.
Prefix-free FEC retries start from the saved state and the same undelayed
resampled input, so each successful attempt advances the delay exactly once.
Hybrid mono uses this same operation instead of its former separate loop.

Outer DC rejection and Hybrid input delay/high-band gain/width history remain
with their existing packet owners. SILK bandwidth permission/LP scheduling also
remains with its packet owner. Native API/encoded-channel resampler handshakes,
common outer input ownership, real mode-switch prefills and 5 ms tails must still
be connected before the complete automatic encoder can be advertised.

## Verification scope

Existing native component fixtures and expected packet/PCM values are retained.
Tests referencing moved private fields now follow the speech-state member;
their assertions, injected failures and expected data are unchanged. They check
rate migration, omitted-side restart, DTX/FEC clocks, bounded packet retries and
complete-state rollback. No production lint/dependency gate is relaxed.

The optional [regression report](results/speech-state-0.1.85-regressions.json)
compares full raw corpora from baseline 0.1.84 and the refactored engines. Each
corpus is generated independently by both builds, compared byte-for-byte and
then replayed by the existing unchanged-native decoder tools. Raw records carry
packet bytes, sample counts and decoder PCM/final-range fingerprints, including
normal, concealment and FEC calls where the profile requests them. Report
`lost` counts zero-length PLC records; `fec_requests` separately counts recovery
calls using a received packet. Neither count is a network-loss-rate estimate.

The matrix covers mono/stereo SILK at all five input rates, all three internal
bands and 10/20/40/60 ms packets, fixed/gradual/automatic bandwidth, DTX, FEC and
normal/recovery scenarios. Hybrid covers both layouts, 24/48 kHz, SWB/FB,
10/20 ms, CBR/VBR, DTX/FEC, silence/resume and loss/recovery. The reference tools
require original Opus 1.6.1 revision
`22244de5a79bd1d6d623c32e72bf1954b56235be`; native source is never part of Cargo
builds or ordinary tests. This proves compatibility of the refactoring and
the recorded reference decoding, not full native encoder identity, subjective
quality, cross-mode waveform continuity or competitive performance.

All 24 corpora pass exact baseline comparison and requested native replay:
1,384,320 records across 5,088 streams, 220,896 FEC requests, 768 zero-length PLC
records and 387,082 single-byte record payloads. The latter includes recovery
records and is not a count of unique transmitted DTX packets. All seven final
repository gates pass: formatting, debug/release offline locked tests,
all-targets Clippy with warnings denied, warning-free rustdoc, the dependency
checker and 23 Python tests. The encoder-only extraction leaves decoder code
unchanged; existing decoder/audio CLI regressions remain in the normal suites.

## Reproduction

Build the same examples with the pinned Rust toolchain in separate baseline
and candidate source trees, retaining both executable directories. Use the
annotated 0.1.84 baseline and the current refactor milestone. Raw corpus formats
contain no package-version strings; final version/vendor changes in the audio
CLI are separate from these records. Then run with new output paths:

```sh
cargo build --offline --locked --release --examples
python3 tools/speech_state_regressions.py BASELINE_EXAMPLES CANDIDATE_EXAMPLES target/speech-state-check target/speech-state-report.json --reference target/reference/opus
```

The tool has 24 fixed corpus cases, refuses to overwrite an existing output
directory, checks truncated/invalid record boundaries and writes incremental
progress. It marks the report passing only after both generation commands,
exact comparison and requested native replay succeed for every case. Logs and
binary vectors remain under the supplied local output directory; the tracked
report contains sanitized case names, arguments, sizes, counts and hashes.
