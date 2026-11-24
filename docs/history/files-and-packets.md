# File workflows, packet framing and reference tools

Historical notes from the milestones leading to v0.2.5. Commands assume the
repository root. For the integrated codec, see the [current support and evidence](../validation/codec-readiness.md).

## Project overview at v0.2.5

Opura is a pure Rust, zero-dependency Opus implementation under development.
**The unified decoder supports SILK, CELT and Hybrid PCM, loss/DTX, in-band FEC
and mode transitions with redundant CELT tails at all five standard output rates.**
The separate SILK/CELT/Hybrid APIs and their component stages remain available.
**The CELT encoder accepts mono/stereo PCM at all five standard input rates,
with CBR, VBR and constrained VBR.
The fixed-bandwidth SILK encoder accepts mono/stereo PCM and emits 10/20/40/60 ms
NB/MB/WB Opus packets.** Both paths are available through the WAV/Ogg CLI.
SILK supports opt-in DTX/FEC and explicit, gradual or automatic bandwidth
selection. Stereo API input can request mono/stereo SILK wire layout while
retaining its live mid/input histories.
**The fixed-band Hybrid encoder accepts mono/stereo24/48kHz PCM for
10/20ms SWB/FB packets through the library and WAV/Ogg CLI, with opt-in DTX/FEC
and VBR.**
**`encoder::OpusEncoder` now automatically encodes single frames through
one PCM/SILK/CELT owner**, including wire-channel changes, bandwidth handshakes,
real prefills and5ms cross-mode tails. It accepts2.5/5/10/20ms at all five input
rates through `encode`; `encode_packet` adds complete packets through120ms.
All five native applications and `--mode auto` WAV/Ogg encoding are available. See
[application/CLI validation](../validation/encoder-applications.md).
**The decoder passes all120 official RFC8251 vector/rate/channel cases.**
The first complete-decoder benchmark is recorded: Opura trails the pinned scalar
libopus reference in all ten configurations. The unified mono/stereo encoder
is implemented; its broader competitive and perceptual acceptance remains open. The accepted direction is a complete decoder first, then an encoder,
prioritizing speed and reliability. See the
[decoder-first decision](../decisions/0002-decoder-first.md) for evaluation
requirements; SOTA performance is an ambition, not a current claim.
The library's [multistream APIs](../validation/multistream.md) add bounded
channel maps, self-delimited framing, persistent mapped PCM decoding with gain,
and generic multistream encoding with whole-packet budgets and PLC/FEC/DTX.
The [mapped file CLI](../validation/mapped-audio-files.md) supports1..8channels
and mapping families0/1/255. Canonical
[surround encoding](../validation/surround-encoder.md) now integrates persistent
masks, native stream rates/policies and LFE processing through the actual common
and CELT owners. Generic mapped APIs remain available. Q24 precision corrections
are explicit; no pristine full-encoder byte identity or SOTA claim follows.

An optional [in-process encoder benchmark](../benchmarks/encoder-0.1.96.md)
compares matched public APIs and independently verifies both packet streams.
The retained 0.1.96 mono/stereo baseline has an Opura/native time ratio of 1.244;
broader competitive acceptance remains open.

The library parses externally delimited Opus packets according to
[RFC 6716 section 3](https://www.rfc-editor.org/rfc/rfc6716.html#section-3),
returning borrowed frame slices and TOC metadata without allocation. It checks
frame lengths, counts, padding and the 120 ms packet-duration limit. Empty
frames are accepted. Parsing does not validate compressed audio contents.
RTP is outside the packet API. Self-delimited prefixes and mapped streams are
available through the [multistream APIs](../validation/multistream.md).
The audio command handles the supported Ogg container subset described above.

```rust
use opura::Packet;

let packet = Packet::parse(&[0x98, 0x12, 0x34])?;
assert_eq!(packet.duration_samples(), 960); // 20 ms on a 48 kHz clock
assert_eq!(packet.frames(), &[&[0x12, 0x34][..]]);
```

Durations count samples per channel on a 48 kHz clock, independent of encoded
bandwidth. Pass exactly one packet with transport/container headers removed.
The parser uses fixed storage for up to 48 frames. Padding-length processing
is linear in its header length; callers remain responsible for bounding their
transport's packet size.

## Audio command

Build the native, dependency-free file converter and encode/decode a recording:

```sh
cargo build --offline --locked --release --bin opura
target/release/opura encode input.wav output.opus --bitrate 64000 --frame-ms 20
target/release/opura decode output.opus decoded.wav
```

Encoding accepts little-endian RIFF/WAVE PCM16 at8/12/16/24/48kHz.
AUTO supports1..8channels: canonical WAVE extensible speaker masks select
positioned family1 layouts and native surround/LFE coding; unpositioned PCM selects
generic family255 coding. Version0.2 preserves stereo in coupled surround streams
and rejects `--channels 1` for positioned multichannel input. Mono/stereo keeps
its existing channel override; generic custom library maps retain their controls.
Fixed modes require positioned mono/stereo input. The default CELT path enables
all six adaptive CELT analyses. Frame durations
are 2.5, 5, 10 or 20 ms; defaults are 20 ms and 64,000 bits/s. Bitrate includes
Opus packet TOC bytes and excludes Ogg overhead; fractional byte budgets carry
between CBR packets. Add `--vbr` for unconstrained variable rate or `--cvbr`
for native constrained capacity/reservoir/drift feedback. Their separate hard
packet cap is `--max-packet-bytes` (default1276 for one frame, including TOC);
single-frame silence can use three-byte packets. CBR remains the default. Library users call
`CeltEncoder::encode_vbr` with `VbrOptions`, whose payload cap excludes TOC.
The nominal rate includes TOC and can be limited by channel/capacity bounds;
unattainable budgets fail explicitly. See [CELT VBR validation](../validation/celt-vbr.md).

The CELT library also supports stereo PCM emitting mono or stereo packets via
`set_encoded_channels(1|2)`, retaining both input filter/overlap/pitch histories
and rate feedback across changes. `channels()` remains the fixed PCM input
layout; `encoded_channels()` selects the wire layout. Reset retains selection.
See [channel-layout validation](../validation/celt-channel-layout.md).
Automatic mode/channel selection is available through the bounded AUDIO
`encoder::OpusEncoder` API described below.

Fixed SILK mono/stereo and Hybrid packet engines reuse the same internal
speech-state type, frame analysis and AUDIO input filtering/history code.
The common input state retains 10ms of filtered PCM, including the native
early-DTX update order, and supplies Hybrid's compensated CELT input.
Each fixed encoder remains an independent stream. The common owner now
connects these histories with native prefills/tails and application-specific
filter/delay processing. See
[input-history validation](../validation/encoder-pcm-history.md).

For SILK stereo API input, `StereoEncoder::set_encoded_channels(1)` requests
native width collapse in one successful stereo packet before the next packet
changes to mono. A fresh stream may start mono immediately. Requesting `2`
resumes stereo with the retained mid history and a newly initialized side;
input stays interleaved stereo. `encoded_channels()` reports the last wire
layout (initially two), and `encoded_channels_target()` reports the request.
Reset retains the request. This library control preserves fixed defaults;
automatic AUDIO mode/channel selection and Hybrid wire changes use the new
`encoder::OpusEncoder` owner. See [speech-layout validation](../validation/silk-channel-layout.md).

The fixed packet APIs now delegate to borrowed internal engines: one SILK
payload implementation serves mono/stereo, and Hybrid preparation/finalization
borrows the caller's PCM, speech, CELT and fade histories. Public owners retain
whole-stream transactions and retries. The new AUDIO owner borrows these same
engines for its connected native transitions. Full application/duration/CLI
integration remains in progress. See
[packet-engine validation](../validation/borrowed-packet-engines.md).

CELT `set_bandwidth` selects NB/WB/SWB/FB up to the input-rate default without
resetting signal histories; `bandwidth()` reports the selection. Packet TOC and
spectral coding change together, and completed packets clear inactive energy
histories in both API channels. MB and above-input bands are rejected, and reset
retains selection. See [bandwidth validation](../validation/celt-bandwidth.md).
The encoder flushes its 120-sample delay at 48 kHz; pre-skip
and the final granule preserve the exact input duration after resampling.

Add `--packet-ms 120` to assemble compatible frames into longer packets.
It must be a positive multiple of `--frame-ms` and at most120ms. The final
group and a SILK bandwidth change can close a shorter packet. CBR counts the
complete packet header and padding; all-DTX groups stay compact. VBR/CVBR
`--max-packet-bytes` caps the whole group, defaulting to1276 bytes per planned
component frame (at most61248 bytes). Tight caps can reduce the nominal rate
or fail admission. Existing one-frame defaults retain their audio bytes.
The library's allocation-free `repacketizer::Repacketizer` assembles borrowed
compatible frames, selects ranges and writes exact padded lengths with atomic
append/write errors. See [multi-frame validation](../validation/grouped-packets.md).

Speech can be encoded through SILK:

```sh
target/release/opura encode speech.wav speech.opus --mode silk --bitrate 24000 --frame-ms 20
```

SILK supports mono/stereo input, 10/20/40/60 ms packets and `--bandwidth nb|mb|wb`.
Bandwidth defaults to NB for 8 kHz, MB for 12 kHz and WB for higher input rates.
Its default bitrate is 24,000 bits/s for mono and 48,000 bits/s for stereo. Pre-skip is 312 samples at 48 kHz (6.5 ms);
lookahead flushing and final granules preserve exact source duration.
Add `--dtx` to suppress silence while retaining timing. Suppressed single-frame
packets are one byte even in CBR; all-DTX groups use at most two bytes.
Periodic full packets refresh decoder state. DTX is off by
default and still runs full encoder analysis. Library users call `set_dtx(true)`
on the mono/stereo encoder; `in_dtx()` describes its last successful packet.
See the [DTX validation record](../validation/silk-dtx.md).

Add `--fec 20` to request in-band FEC with a 20% loss estimate (0 disables it).
The library equivalent is `set_inband_fec(true)` with
`MonoOptions::packet_loss_percent` / `StereoOptions::packet_loss_percent`.
Protection needs sufficient bitrate and speech activity. It carries prior audio
in the next packet and can combine with DTX; `has_fec()` describes the last
successful packet. Explicit bandwidth stays fixed. If pending redundancy cannot
fit alongside primary audio, it is omitted; CBR bytes/duration remain intact.
The [FEC validation record](../validation/silk-fec-packets.md) describes loss
recovery, queue transitions, bounded retry and independent decoding.

Library streams can call `set_bandwidth(SilkRate::Khz8/Khz12/Khz16)` between
packets and inspect `bandwidth()`. Input history and live resamplers migrate;
VAD/stereo/DTX state persists and incompatible pending FEC is cleared. This is a
forced change. Automatic target policy remains separate;
the CLI still uses one selected bandwidth per file. See
[bandwidth switching validation](../validation/silk-bandwidth-switching.md).

`set_bandwidth_target(Some(rate))` connects native activity-permitted
LP transitions to mono/stereo PCM. `bandwidth()` reports the actual rate;
`set_bandwidth_target(None)` stops the taper. Targets are capped by the input
rate, reset retains the request, and failed packets roll back all transition
state. This supplies the inner SILK handshake; cross-mode redundancy remains
unfinished. See
[connected transition validation](../validation/silk-adaptive-bandwidth.md).

Use `--mode silk --bandwidth auto` or `set_automatic_bandwidth(true)` to select
NB/MB/WB from packet bitrate, complexity, loss and native AUDIO thresholds. FEC
can lower the target, while channel layout stays fixed. Reset retains the mode.
Automatic-mode packet overflow follows native PLC signaling; `used_plc()` and the
CLI PLC packet count distinguish this from DTX (component-frame count when grouping). The
[automatic bandwidth record](../validation/silk-automatic-bandwidth.md) retains
stress-case concealment counts and limitations. General mode/channel selection
and quality/performance
leadership remain unfinished.


Decoding accepts SILK, Hybrid and CELT in one Ogg Opus logical stream with
1..8channels, mapping families0/1/255 and signed header output gain. Output is
PCM16 WAVE at48kHz with explicit WAVE speaker order/masks for mapped layouts.
Unpositioned family255 preserves channel order and writes a zero speaker mask.
Chained/multiplexed streams, unknown mappings and packets larger than16MiB
(including metadata) reject explicitly. CRC, sequence, continuation and granule
inconsistencies are errors. Output is limited to RIFF32 size. Existing paths are
never overwritten; errors may leave a new partial output, which must be discarded.
See [mapped file contracts and independent validation](../validation/mapped-audio-files.md).

The [interoperability record](../validation/audio-cli-0.1.65.md) covers
independent decoding, exact durations, packet budgets and original native
mode files. It does not establish perceptual quality or performance leadership.
The [initial real-recording baseline](../benchmarks/audio-0.1.66.md) reports
speech/music signal diagnostics and Opura file wall time with explicit limits.
Broader subjective/competitive comparisons remain work.
Fixed-bandwidth SILK library paths are available under
`silk::encoder::mono` and `silk::encoder::stereo`: PCM at any standard input rate to NB/MB/WB packets of
10/20/40/60 ms, with exact CBR byte budgets or capped variable length. See the
[mono interoperability record](../validation/silk-mono-packets.md) and
[stereo validation](../validation/silk-stereo-packets.md).
The validation record also
retains a reproduced default-FFmpeg decoder tail-trimming issue; FFmpeg/libopus
and the native decoder preserve exact duration.
Library packet parsing itself does not parse Ogg containers.

The [SILK recording baseline](../benchmarks/silk-0.1.72.md) adds 88 matched
speech/music file cases and 96 deterministic loss scenarios. Unprotected/fixed
SILK controls match native encoder PCM; protected profiles retain every SNR loss,
backup-coverage tradeoff and PLC count. These are two-recording waveform
checks, with process timings and no perceptual or native speed advantage claim.
These fixed-mode results do not establish unified-encoder quality.

Shared frontend primitives are available in `encoder::stereo_width` and
`encoder::control`, with independently checked native scalar analysis and
mode/channel/band/FEC decisions. `encoder::OpusEncoder` connects them to a single
packet owner for2.5/5/10/20ms input and all five applications. The API accepts CBR/VBR/CELT CVBR,
nominal rate, hard cap, mode/signal/band/channel hints, loss, DTX and FEC. Errors
preserve all PCM, speech, CELT and selection histories. `PacketSize` and
`encode_packet` select one policy for complete packets through120ms, including
native component scheduling. Native tiny-budget and completed-SILK-overflow
PLC are connected, with `EncodedPacket::used_plc` reporting loss signalling.
Native CELT complexity-zero spreading and tone-only analysis are connected.
The missed-transient detector preserves Q24 differences, intentionally fixing
an int16 narrowing in the pinned fixed-point reference. Its corrected encoder
oracle matches PCM/range and temporal VBR history over10,560 frames; the
reference decoder remains unchanged. See [CELT quality controls and recording
diagnostics](../validation/celt-quality.md).
The shared CELT owner retains the realized allocation intensity boundary for
subsequent hysteresis, including mono resets. [Rate-history validation](../validation/celt-rate-history.md)
covers5,132 actual native CBR/VBR/CVBR calls with byte-identical packets,
PCM/ranges and live feedback histories, using the disclosed Q24 correction.
[Prediction/loss/band validation](../validation/celt-controls.md) additionally
matches6,144 native control-history packets, including active pitch filtering.
See [current readiness and open work](../validation/codec-readiness.md) for
the integrated scope, documented exclusions and comparative evidence still needed.
All applications and automatic file packets through120ms are connected.
Existing fixed library/CLI defaults are unchanged.

```rust
use opura::{celt::{FrameSize, input::InputRate}, encoder::{EncodeOptions, OpusEncoder}};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut encoder = OpusEncoder::new(InputRate::Khz48, 2)?;
    let pcm = [0_i16; 1920]; //20ms interleaved stereo
    let packet = encoder.encode(FrameSize::Ms20, &pcm, 320, EncodeOptions {
        bitrate: 64000,
        ..EncodeOptions::default()
    })?;
    let bytes = packet.as_bytes();
    Ok(())
}
```

Automatic file encoding uses the same owner:

```sh
opura encode input.wav output.opus --mode auto --application voip --signal voice --bitrate 32000 --vbr --fec 20 --dtx
opura decode output.opus output.wav
```

`--application` accepts `audio`, `voip`, `lowdelay`, `restricted-silk` and
`restricted-celt`. `--complexity 0..10`, `--channels 1|2` and all supported band
hints apply to auto mode. `lowdelay`/`restricted-celt` pre-skip is120 samples;
AUDIO/VOIP/restricted-SILK use312. Restricted SILK requires at least10ms.
Automatic `--packet-ms` selects a complete native-scheduled packet through120ms,
with one mode/layout decision and correct first/final transition placement.
It must be a standard duration and a multiple of `--frame-ms`; alternatively
set `--frame-ms` directly to40/60/80/100/120. Caps apply to the whole packet;
the automatic default is1276 bytes per20ms window, at most7656. Caps can be as
small as1 byte; unusable audio budgets signal native PLC while preserving file
length. A100ms packet requires at least2 bytes. Positive nominal rates are
clamped to at least500bps total and at most750000bps per API channel before
the physical cap.
See [budget validation](../validation/encoder-budgets.md).
See [long-packet validation](../validation/encoder-long-packets.md).

See [unified AUDIO validation](../validation/unified-audio-owner.md) for the
connected state/transition evidence and remaining bounds, and the
[frontend validation](../validation/encoder-frontend.md) for scalar policy.

`hybrid::encoder::HybridEncoder` emits fixed-band CBR Hybrid packets using a
single shared range writer, WB SILK and CELT bands17..19(SWB)/21(FB).24kHz PCM
accepts SWB;48kHz accepts SWB/FB. Lookahead is312 samples on the48kHz clock.
All input/filter/resampler/stereo/predictor/entropy state rolls back on errors.
Tiny budgets can fail explicitly after bounded search. The CLI defaults to32kbps
mono/64kbps stereo and supports `--bandwidth swb|fb` with `--frame-ms 10|20`.
Use `set_dtx(true)` or CLI `--mode hybrid --dtx` for native silence suppression.
Suppressed packets contain one TOC byte and range zero; `in_dtx()` reports the
last successful result, and reset retains the setting. SILK inactivity preserves
CELT history, whereas outer digital silence advances both layers. See
[Hybrid DTX validation](../validation/hybrid-dtx.md).
Use `set_inband_fec(true)` with `Options::packet_loss_percent` or CLI
`--mode hybrid --fec 20` to request previous-frame SILK-band protection. High-band
recovery uses concealment. Fixed SWB/FB is retained; insufficient bitrate/activity
can leave FEC unavailable. `has_fec()` reports actual carried LBRR, and queued
backups may still be carried once after disabling new protection. Budget errors
retry once without the old backup, preserving primary encoding and atomic state.
See [Hybrid FEC validation](../validation/hybrid-fec.md).
Use `encode_vbr(duration, pcm, bitrate, maximum_packet_bytes, options)` for
unconstrained Hybrid VBR with separate nominal TOC-inclusive bitrate and hard
packet cap. CLI `--mode hybrid --vbr --bitrate 48000 --max-packet-bytes 200`
exposes the same behavior. CBR defaults remain unchanged. VBR retains the
original temporal/SILK-offset target and conservative shared-entropy length
floors; DTX/FEC and error/reset contracts still apply. See
[Hybrid VBR validation](../validation/hybrid-vbr.md).
Version0.1.80 corrects its high-band dynamic-analysis policy; see the
[policy correction evidence](../validation/hybrid-vbr-dynalloc.md).
Use `encoder::OpusEncoder` for cross-mode switching and general mode/channel
selection; `HybridEncoder` retains its fixed-mode contract. See
[Hybrid PCM validation](../validation/hybrid-pcm-packets.md).

```sh
cargo run --release --offline --locked -- encode input.wav output.opus --mode hybrid --bandwidth fb --bitrate 64000
```

Hybrid encoder control calculations are also available in `hybrid::encoder`:
`RateAllocation` reproduces native SILK/high-band rate splitting and the SILK
CBR/VBR bit cap, including selected FEC, bandwidth and stereo corrections.
The target bitrate excludes TOC/transition costs; the payload capacity is shared
by both layers. This control API does not encode PCM or certify that a frame fits.
CELT packet termination is separated from its frame engine; Hybrid passes its
live writer to that engine. Existing fixed-mode packet bytes and rollback
contracts are preserved.
See [Hybrid rate validation](../validation/hybrid-encoder-rates.md).

## Development

Rust 1.87.0 is the minimum supported and pinned toolchain. With that toolchain
installed, the library and its tests run offline:

```sh
cargo test --offline --locked
cargo test --offline --locked --release
cargo fmt --all -- --check
cargo clippy --offline --locked --all-targets -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --offline --locked --no-deps
python3 tools/check_dependencies.py
python3 -m unittest discover -s tools -p 'test_*.py'
```

There are no third-party normal, build, development, optional or target-specific
Cargo dependencies. Unsafe Rust is forbidden. Python is used only by developer
checks and optional fixture generation; it is not needed to build or test the
library. Package publication is disabled during this initial stage.

## Reference checks

The detailed component sections below retain development-stage notes. Statements
about later stages being future work describe those earlier component milestones;
current integrated capabilities are summarized above and in the
[readiness record](../validation/codec-readiness.md).

Tests cover all TOC configurations, frame codes 0–3, zero-length frames,
one/two-byte lengths, CBR/VBR, padding, duration limits and malformed packets.
They also exercise every one- and two-byte input, deterministic longer inputs,
and long padding headers. These are regression checks, not a proof of safety
or full audio-decoder conformance.

[Reference fixtures](../../tests/packet-fixtures.txt) were generated with the external
`libopus 1.6.1` packet parser and contain synthetic payloads. Expected results
include acceptance/rejection, duration, channels, bandwidth and frame offsets
and lengths. Cargo tests read these fixtures without loading libopus.

To independently regenerate them (optional, requires Python and libopus):

```sh
python3 tools/packet_fixtures.py /path/to/libopus.so.0 > /tmp/opura-fixtures.txt
diff -u tests/packet-fixtures.txt /tmp/opura-fixtures.txt
```

The fixture format is documented in its header; `byte*count` encodes a run of
identical bytes, and commas concatenate hex chunks. The generator reports the
reference version. Review differences before replacing fixtures.

See the [pure Rust decision](../decisions/0001-pure-rust-zero-dependencies.md)
for project constraints. These packet fixtures validate framing only;
complete audio decoding and official conformance have separate validation.

## Range decoding

`opura::range::RangeDecoder` borrows one compressed frame and supports general
CDFs, 8-bit inverse CDFs, probability-coded bits, raw bits and uniform integers.
It exposes the normalized range and bit-exact whole/eighth-bit usage counters.
Its operations use fixed storage, with no allocation or unsafe Rust.

- Range input uses zero extension, even after the frame is exhausted.
- Raw bits come from the frame end and may overlap range input; requesting
  more raw bits than the frame holds returns an error.
- All errors leave decoder state unchanged, including an out-of-alphabet
  uniform integer or a request that exhausts the accounting counter.
- Callers supply the probability models and check codec-specific bit budgets.
  Successful primitive operations do not establish valid audio. The constructor
  accepts empty input, but the audio layer must decide how to handle lost frames.

CDF boundaries are nondecreasing `u16` values, starting at zero with a positive
total. ICDF precision is 1–8 bits; log-probability bits support logp 1–15;
raw reads support 0–25 bits; uniform totals range from 2 through `u32::MAX`.
Zero-probability symbols are supported. Bit accounting returns a typed error
before its 32-bit eighth-bit counter can overflow.

[Range fixtures](../../tests/range-fixtures.txt) check 1539 intermediate states against
Opus v1.6.1, including symbols, range, `tell` and `tell_frac`. These synthetic
entropy tests complement the packet fixtures; they are not audio test vectors.
Normal Cargo tests use the saved data and need neither C nor libopus.

Optional regeneration requires a clean external source checkout at the exact
revision shown below, Python, Git and a C compiler. The generator compiles only
the reference entropy routines into a temporary library that is deleted on exit.
The checkout can live under ignored `target/reference/opus`:

```sh
git clone --branch v1.6.1 --depth 1 https://github.com/xiph/opus.git target/reference/opus
git -C target/reference/opus rev-parse HEAD
# Must be 22244de5a79bd1d6d623c32e72bf1954b56235be
python3 tools/range_fixtures.py target/reference/opus > /tmp/opura-range-fixtures.txt
diff -u tests/range-fixtures.txt /tmp/opura-range-fixtures.txt
```

See [third-party notices](../../THIRD_PARTY_NOTICES.md) for the adapted entropy
algorithm's attribution and redistribution terms. There are no runtime or
Cargo dependencies on the reference implementation.

## Versions and history

See [CHANGELOG.md](../../CHANGELOG.md) for versioned changes. Development milestones
use annotated `vX.Y.Z` Git tags, with matching Cargo package versions. Before
1.0, incompatible API changes increment the minor version; compatible additions
and fixes increment the patch version. Tags record tested development milestones,
not crates.io publication or full codec conformance.

## Comparison goals

The [competitive landscape and benchmark contract](../benchmarks/competitive-landscape.md)
records the GitHub comparison set, pinned evidence and measurable performance,
quality, reliability and usability targets. Competitor claims have not yet been
reproduced by Opura; no overall leadership is claimed.

[Local measurements and gates](../benchmarks/packet-0.1.5.md) record the first
packet-framing comparison with libopus, before/after raw samples and remaining
failures. The full-codec competitive target is not met.
