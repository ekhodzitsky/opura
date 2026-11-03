# Unified AUDIO packet owner

This records the0.1.90 boundary. Version0.1.91 adds all applications, embedded
CBR silence and the automatic file CLI; see [application validation](encoder-applications.md).

Version0.1.90 adds `encoder::OpusEncoder`: actual automatic single-frame AUDIO
encoding over one PCM/DC/delay state, capacity-two speech/resampler/bandwidth
state, CELT state and gain/width/selection history. It calls the borrowed SILK
and Hybrid engines and embedded CELT directly. It does not select independently
primed fixed encoder instances. Existing fixed APIs and CLI defaults remain.

## Contract and current bounds

The public owner accepts signed16 mono/stereo API input at8/12/16/24/48kHz and
2.5/5/10/20ms. Scalar policy selects SILK, Hybrid or CELT, Nyquist-limited band,
wire layout and FEC. Short frames select CELT. Controls include nominal rate,
CBR/VBR, CELT constrained VBR, signal/mode/band/channel hints, complexity, loss,
DTX and FEC. The TOC-inclusive physical cap is3..=1276 bytes; CBR rounds the
nominal target to bytes. AUDIO lookahead is6.5ms. Invalid or insufficient budgets
can return errors; native tiny-budget PLC admission is not implemented yet.

Only AUDIO is currently accepted. VOIP/restricted applications, long-packet
scheduling and automatic WAV/Ogg CLI encoding remain work. The fixed file modes
remain usable. This release is not completion of the whole adaptive encoder.

Every encode stages the complete owner and commits after entropy termination,
redundancy, final-range calculation and padding. A pending-FEC capacity failure
can retry once from identical complete pre-packet state without its prefix.
Errors leave all committed input, layer and selection histories unchanged.

## Connected order

Raw stereo width and digital silence feed the scalar controller. Common AUDIO
DC rejection precedes resampling. The owner derives4ms compensated CELT input
from the previous10ms retained PCM before changing that history. Hybrid consumes
already filtered input, so AUDIO filtering occurs once. API and wire layouts
are separate; resampler history follows the previous actual speech call,
including prefills and early SILK DTX, rather than merely the previous packet.

Entering speech from CELT resets and prefills the actual speech histories. The
native10ms prefill retains the onset at3.5ms with a2.5ms ramp. Bandwidth switches
preserve LP transition state across the channel reset/prefill. Original activity
permission, readiness, acknowledged transitions and actual selected rates feed
subsequent decisions. Digital silence caps speech activity even with DTX/FEC
disabled. Early SILK DTX omits PCM completion and CELT; outer digital-silence
suppression follows full processing and advances the committed mode history.

The live speech writer receives transition flag/direction/length syntax before
Hybrid CELT or SILK termination. Incoming5ms redundancy uses previous live CELT
before reset. A new non-SILK primary performs a real2.5ms dummy encode before
prediction is disabled. Outgoing redundancy uses reset CELT, prediction zero,
a real2.5ms prefill and the last5ms of compensated/faded input. The next packet
retains that tail state. Short direct speech-to-CELT switches use primary
prefill rather than a delayed outgoing speech packet. Final range is primary
range XOR redundant range. Tail bytes follow the actual primary length after
VBR shrink, before transport padding.

CELT receives complexity/loss/prediction independently of its start band.
Embedded CELT has native signalling disabled; it does not subtract a second
TOC from its rate, and VBR dummy encodes advance the same rate history. Fixed
CELT packets keep their signalling-enabled rate contract. Complexity zero skips
temporal analysis while retaining tone detection. SILK-only incoming tails
retain the preceding CELT prediction control, including bandwidth transitions.

## Independent evidence

The optional generator uses unchanged Opus1.6.1 revision
`22244de5a79bd1d6d623c32e72bf1954b56235be`, scalar fixed fast64/Q24 with float
analysis disabled. Wrappers observe original public encoder calls, original
SILK VAD input, resamplers and CELT input state; they do not replace algorithms.
Only active API channels are read. Unconstrained VBR is selected explicitly.

Normal offline tests compare1,408 cross-mode/short-switch snapshots across all
five rates and mono/stereo API input, plus3,072 longer bandwidth snapshots at
16/48kHz. They check wire/ramp decisions, actual internal/speech layout, both
active resampler fingerprints and cumulative complete mid PCM. The bandwidth
sequences exercise upward and downward handshakes and reset-with-LP-prefill.

For all4,480 original packet calls, fixtures also check every actual CELT call:
duration, start band, prediction, entropy capacity and complete input-PCM hash.
The1,408 cross-mode snapshots additionally compare CELT preemphasis, filtered
120-sample overlap and VBR count. These are independent checks of actual dummy
and tail calls, rather than merely checking that emitted packets decode. The
original reference scope is complexity zero; it does not establish full encoder
byte identity or quality-control parity at other complexities.

Four isolated mutations are detected: shifting the SILK ramp, omitting a real
CELT dummy call, resetting prediction on a SILK incoming tail and omitting
final-range XOR. Source was restored. The short direct-switch cases were added
after mutation testing exposed an unexercised primary-prefill branch. All five
fixture files regenerate identically; prior cross-mode rows were retained when
adding these short cases.

Reproduce the original input/call evidence outside Cargo dependencies:

```sh
python3 tools/opus_stream_input_fixtures.py target/reference/opus target/stream-input.txt \
  --band-output target/stream-band-input.txt --celt-output target/stream-celt-input.txt \
  --call-output target/stream-calls.txt --band-call-output target/stream-band-calls.txt
cargo test --offline --locked --lib encoder::stream
```

The separate `unified_encoder_vectors` example exercises actual automatic and
explicit mode/layout/band/rate controls, short frames, CBR/VBR/CVBR, DTX, FEC,
loss and recovery. It records packet ranges and decoded PCM for independent
replay through unchanged native decoding:

```sh
cargo run --offline --locked --release --example unified_encoder_vectors -- target/stream.bin
python3 tools/silk_mono_validation.py target/reference/opus target/stream.bin
```

The [0.1.90 evidence report](results/unified-audio-owner-0.1.90.json) records
fresh native replay of11,520 received calls in120 streams,960 explicit PLC and
960 FEC calls. The encoded schedule contains3,148 SILK,1,372 Hybrid and7,000
CELT packets,920 wire changes and880 encoded DTX packets; encoded and received
schedules differ. Corpus regeneration is identical, SHA256
`d7e895958e699d484ca5daa6ec0c6cbabdfe0f02a4b5265639972cb48fed1794`.
The independent original call traces contain1,136 CELT calls, including160 real
2.5ms dummy encodes. All seven required offline gates pass, including23 Python
tests. No original fixture assertion, unsafe gate or dependency check is weakened.

All24 fixed-mode corpora also
[match the isolated committed0.1.89 baseline](results/unified-audio-owner-0.1.90-fixed.json)
byte-for-byte:1,384,320 records/5,088 streams/220,896 FEC requests/768 PLC calls.
The fixed matrix compares previously native-validated bytes; it does not repeat
native decoding. After the final root-only early-DTX correction, all four
complete Hybrid corpora were regenerated from the final build and still match.
The fixed SILK paths were unchanged by that correction. All seven gates were
rerun, and the final rebuilt unified corpus repeated identically and passed
fresh native replay again.

A successful early-DTX regression uses nonzero quiet PCM through a wire change.
It reproduces the original erroneous CELT metadata update at frame33, then checks
that the corrected owner preserves the complete CELT state, committed PCM and
logical previous mode/channels whenever SILK returns early. The filtered Hybrid
low-band path defers wire validation/configuration to actual CELT processing;
the legacy fixed Hybrid geometry checks are retained.

## Remaining completion requirements

The complete owner still needs VOIP filtering and restricted applications,
long-packet decisions/prefill/tail placement, native insufficient-budget PLC,
automatic file CLI integration and a full control/quality review. That review includes the embedded CELT CBR
silence path and all complexity gates; the input/call snapshots do not prove
those encoding decisions match original libopus. End-to-end
robustness, quality, latency, throughput and memory comparisons must follow on
representative real audio at matched rates and constraints. Input-history and
interoperability evidence do not prove perceptual parity, whole-codec readiness
or state-of-the-art performance.
