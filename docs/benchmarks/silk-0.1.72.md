# Real-recording SILK baseline: Opura 0.1.72

This baseline validates the integrated SILK path on two fixed recordings and
retains signal diagnostics, including losing cases. It establishes neither
perceptual/SOTA superiority nor competitive native codec speed. The reference
is restricted to SILK with forced source channels, rather than ordinary Opus
automatic mode/channel selection. Music results here must not be generalized
into a comparison against unrestricted libopus.

## Workload and provenance

The same hash-pinned Xiph examples used by the [CELT baseline](audio-0.1.66.md)
provide 10.8 seconds of mono speech and 90.796 seconds of stereo music, both
PCM16/48 kHz. Recordings and listening artifacts remain local. Source URLs,
hashes, sample counts, peaks and original clipping are retained in each report.
Only two original recordings are present; parameter cases are not independent
recordings or a representative listening corpus.

All measurements use the frozen 0.1.72 release executable. Validation tooling
and this report ship in 0.1.73; Rust codec source is unchanged. The original
0.1.72 binary is retained locally as `target/opura-baseline-0.1.72`. Binary,
tool, codec-source and corpus hashes identify the measured state. The reference
is unchanged scalar fixed-point fast64 libopus v1.6.1 at
`22244de5a79bd1d6d623c32e72bf1954b56235be`, built with the reported flags.

## Matched controls and checks

Both encoders use CBR, complexity 10, fixed source channels and 312-sample
lookahead. Automatic speech cases use 12/16/24 kb/s; stereo music uses 24/32/48
kb/s, each at 10/20/40/60 ms. Three profiles independently exercise normal,
FEC with 20% loss estimate, and FEC plus DTX. Additional fixed controls use
NB/MB/WB at 24 kb/s mono and WB at 48 kb/s stereo. There are 88 SILK cases.
Four CELT speech regressions preserve the previous PCM hashes and diagnostics.

Actual Opura Ogg output is decoded by the pinned original library and must
match the CLI WAV exactly, after pre-skip/granule trimming. Independent native
packet parsing checks actual bandwidth, DTX, PLC and LBRR; packet counts,
budgets and delay must agree with the configured controls. PLC counters must
match the CLI diagnostic. One unmeasured full warmup precedes three measured
encode/decode process pairs; encoded/decoded hashes and PLC counts must repeat.
No build/test process from this task runs alongside retained measurements.

## Ordinary decoding results

All 88 SILK cases pass exact independent decoding and duration/budget checks.
All 40 unprotected automatic/fixed controls also produce exactly the same PCM
as separate native encoding. Packet bytes differ; complete encoder bitstream
identity is not claimed. Opura emits zero PLC in every recording case.

| Recording | kb/s | Frame ms | SNR Opura dB | SNR native dB | Encode RTF | Decode RTF |
|---|---:|---:|---:|---:|---:|---:|
| speech | 12 | 20 | 9.688 | 9.688 | 0.02422 | 0.00131 |
| speech | 16 | 20 | 10.490 | 10.490 | 0.02588 | 0.00153 |
| speech | 24 | 20 | 11.152 | 11.152 | 0.02533 | 0.00154 |
| music | 24 | 20 | 8.323 | 8.323 | 0.03640 | 0.00163 |
| music | 32 | 20 | 9.905 | 9.905 | 0.03647 | 0.00153 |
| music | 48 | 20 | 11.030 | 11.030 | 0.04180 | 0.00189 |

Protected profiles expose an admission tradeoff. Across the 24 FEC-only cases
Opura emits zero PLC versus 1077 in the restricted native reference; with
FEC+DTX the totals are zero versus 1083. These totals span repeated recordings
under multiple settings. Preserving primary packets can reduce backup coverage;
fewer PLC packets alone does not establish better recovery or listening quality.
All FEC/DTX/band/PLC counts and losing SNR cases remain in the reports. For
example, protected music at 48 kb/s/10 ms has lower SNR in Opura than native.
Some forced SILK music settings have poor waveform diagnostics in both encoders;
a complete Opus mode selector can choose CELT instead. No cause is attributed
solely to DTX: a separate FEC-only profile is retained.

## Deterministic loss replay

Each retained normal and FEC-only stream is first decoded without loss; it must
reproduce its own baseline PCM exactly. The same native decoder then replays
identical isolated 10% losses and two-packet bursts at about 10%. Both encoders
get a one-packet playout wait for FEC. Earlier losses within a burst use direct
PLC; only its last missing packet can recover from the next received packet.
Terminal losses use PLC. Every time slot yields exactly one output frame.

The report records actual loss ratios/mask hashes, received bytes, direct PLC
calls, FEC requests and requests whose next packet has any LBRR. That last
counter does not promise complete multi-frame recovery. Per-frame PCM counts
and final trimmed length are checked. Added playout wait is the packet duration
(10/20/40/60 ms), equally for both encoders; this is separate from codec delay.

All 48 normal loss scenarios match native PCM signal diagnostics. In 48 protected
scenarios the SNR difference spans -0.376 to +5.031 dB: six lower, eighteen equal
and twenty-four higher. The lowest difference is music, 48 kb/s/10 ms, isolated
loss; the highest is speech, 24 kb/s/60 ms, isolated loss. These are waveform
metrics on two examples and periodic masks, without a subjective trial,
perceptual metric, statistical confidence interval or representative network.

## Timing limits and retained data

Measurements use AMD Ryzen AI 9 HX 370 w/ Radeon 890M, rustc 1.87.0 (17067e9ac 2025-05-09). Across the
88 SILK cases the retained encode process RTF ranges from
0.00761 to 0.08069; decoder process RTF ranges from
0.00078 to 0.00262. RTF is process seconds/audio seconds, including
startup and WAV/Ogg I/O. Allowed CPU affinity is recorded; placement/frequency are not controlled.
The native encoder is not timed. No native speed ratio follows from these data.

Raw reports:

- [normal](results/silk-0.1.72-normal.json)

- [fec](results/silk-0.1.72-fec.json)

- [fec-dtx](results/silk-0.1.72-fec-dtx.json)

- [fixed-nb](results/silk-0.1.72-fixed-nb.json)

- [fixed-mb](results/silk-0.1.72-fixed-mb.json)

- [fixed-wb](results/silk-0.1.72-fixed-wb.json)

- [fixed-stereo-wb](results/silk-0.1.72-fixed-stereo-wb.json)

- [loss normal](results/silk-0.1.72-loss-normal.json)

- [loss fec](results/silk-0.1.72-loss-fec.json)

## Reproduction

Use the current validation tools and a selected release binary. For exact
0.1.72 provenance, build the `v0.1.72` checkout separately or use the retained
binary above. The tools record the actual version and hashes of any new run.
Output directories must be new; without `--download`, the hash-verified
original recordings must already exist.

```sh
python3 tools/audio_baseline.py target/opura-baseline-0.1.72 target/reference/opus target/audio-recordings target/silk-normal --mode silk --rounds 3
python3 tools/audio_baseline.py target/opura-baseline-0.1.72 target/reference/opus target/audio-recordings target/silk-fec --mode silk --rounds 3 --fec 20
python3 tools/audio_baseline.py target/opura-baseline-0.1.72 target/reference/opus target/audio-recordings target/silk-fec-dtx --mode silk --rounds 3 --fec 20 --dtx
python3 tools/audio_baseline.py target/opura-baseline-0.1.72 target/reference/opus target/audio-recordings target/silk-fixed-nb --mode silk --recording speech_orig --bitrate 24000 --bandwidth nb --rounds 3
python3 tools/audio_loss_diagnostics.py target/reference/opus target/audio-recordings target/silk-normal target/silk-normal-loss
python3 tools/audio_loss_diagnostics.py target/reference/opus target/audio-recordings target/silk-fec target/silk-fec-loss
```

Repeat the fixed command with MB/WB and stereo WB/48000 as needed. The ordinary
Python suite checks rounding, DTX budget exceptions, missing/nonzero PLC
diagnostics, burst/terminal replay ordering and truncated packet records. The
codec retains zero third-party dependencies; C and native reference sources
are optional validation tools. Recordings, packet artifacts and listening WAVs
stay local. The next missing encoder mode is Hybrid; general mode/channel
adaptation, broader quality evidence and native core performance remain work.
