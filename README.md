# opura

Opura is a pure Rust, zero-dependency Opus implementation under development.
**Mono/stereo SILK packets support final PCM output, explicit loss concealment and in-band FEC.**
Packet framing, range decoding and the SILK channel primitives are also available.
CELT synthesis from decoded spectra is verified; its compressed-frame path remains
under development.
Implicit DTX handling, CELT/Hybrid decoding and encoding
are not implemented. The accepted direction is a complete decoder first, then
an encoder, prioritizing speed and reliability. See the
[decoder-first decision](docs/decisions/0002-decoder-first.md) for evaluation
requirements; SOTA performance is an ambition, not a current claim.

The library parses externally delimited Opus packets according to
[RFC 6716 section 3](https://www.rfc-editor.org/rfc/rfc6716.html#section-3),
returning borrowed frame slices and TOC metadata without allocation. It checks
frame lengths, counts, padding and the 120 ms packet-duration limit. Empty
frames are accepted. Parsing does not validate compressed audio contents.
Ogg, RTP, multistream and self-delimiting framing are not supported yet.

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

Tests cover all TOC configurations, frame codes 0–3, zero-length frames,
one/two-byte lengths, CBR/VBR, padding, duration limits and malformed packets.
They also exercise every one- and two-byte input, deterministic longer inputs,
and long padding headers. These are regression checks, not a proof of safety
or full audio-decoder conformance.

[Reference fixtures](tests/packet-fixtures.txt) were generated with the external
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

See the [pure Rust decision](docs/decisions/0001-pure-rust-zero-dependencies.md)
for project constraints. Complete audio-layer decoding remains future work; no audio compatibility or performance
claims are made by the current primitives.

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

[Range fixtures](tests/range-fixtures.txt) check 1539 intermediate states against
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

See [third-party notices](THIRD_PARTY_NOTICES.md) for the adapted entropy
algorithm's attribution and redistribution terms. There are no runtime or
Cargo dependencies on the reference implementation.

## SILK headers

`opura::silk::decode_header(&packet, frame_index)` reads VAD and LBRR flags
for a SILK or Hybrid Opus frame and returns its positioned `RangeDecoder`.
For 40/60 ms Opus frames, each channel has two/three internal 20 ms SILK frames.
Stereo flags refer to mid and side channels, not left and right.

The function uses packet metadata to avoid mismatched duration/channel settings.
It rejects CELT-only input, missing indices, empty loss/DTX frames and headers
that exceed the frame bit budget. A successful header parse does not validate
or decode the rest of the compressed audio. LBRR payload, when present, must
be consumed before regular frame data; header parsing does not skip it.

The [640 header fixtures](tests/silk-header-fixtures.txt) use header loops and
probability tables extracted from the actual pinned upstream SILK source.
They verify VAD/LBRR flags, range and whole/fractional bit counters, covering
all LBRR masks. Regenerate with the same clean checkout used above:

```sh
python3 tools/silk_header_fixtures.py target/reference/opus > /tmp/opura-silk-fixtures.txt
diff -u tests/silk-header-fixtures.txt /tmp/opura-silk-fixtures.txt
```

Header tests also enumerate all two-byte payloads across six duration/mode
configurations in mono and stereo. They exercise frame isolation and ensure
that subsequent decoding resumes from the returned entropy state.

## SILK gain side information

`opura::silk::gains::GainState` reads the signal type, quantization offset
selector and gain indices, then reconstructs Q16 scale factors for each 5 ms
subframe. A typed 10/20 ms internal-frame duration selects two/four subframes.
Inactive, active and redundant contexts use the appropriate probability model;
conditional first gains require previous history. Independent gains still honor
the prior-index drop limit when history is available.

Each channel needs its own gain state. Decoding either commits both entropy and
gain history or returns an error with both unchanged. A reset clears history.
This primitive stops before NLSF data. The caller must position the entropy
cursor after any stereo-prediction fields and handle LBRR ordering. It cannot
be blindly called immediately after every header. Future full-frame decoding
must commit gain history only after all of that frame's data has succeeded.

[Reference sequences](tests/silk-gain-fixtures.txt) compare 264 decoding steps
against the pinned upstream signal/gain prefix and original gain reconstruction.
[Exhaustive reference fingerprints](tests/silk-gain-dequant-fixtures.txt) cover
all 64 linear gain values and all 6720 prior/encoded-index transitions. The
fingerprints use FNV-1a over little-endian result pairs to keep fixtures compact;
they are regression checks, not cryptographic proofs.

Optional regeneration uses the same clean source checkout and external tools:

```sh
python3 tools/silk_gain_fixtures.py target/reference/opus > /tmp/opura-gains.txt
diff -u tests/silk-gain-fixtures.txt /tmp/opura-gains.txt
python3 tools/silk_gain_fixtures.py target/reference/opus --dequant > /tmp/opura-dequant.txt
diff -u tests/silk-gain-dequant-fixtures.txt /tmp/opura-dequant.txt
```

These are synthetic primitive tests. They do not establish full decoder
conformance or performance; full Opus orchestration remains incomplete.

## SILK spectral parameters

`opura::silk::nlsf::NlsfState` consumes the NLSF symbols immediately after
gains. It supports order 10 (8/12 kHz SILK) and order 16 (16 kHz, including
Hybrid), reconstructs stabilized Q15 spectral frequencies, and interpolates
the first half of 20 ms frames using per-channel history. Ten-millisecond
frames do not interpolate. Default/reset and order changes disable interpolation
for the first frame. The caller must also reset on sample-rate changes that
preserve order, such as 8↔12 kHz. Use `FrameNlsf::to_lpc()` for the subsequent LPC conversion.

The decoder uses fixed storage without allocation. Failure leaves spectral
history and entropy unchanged. Like gains, this primitive must participate in
the channel transaction below before its history is used for another frame.
Successful side-information decoding does not validate the remaining payload.

[Reference sequences](tests/silk-nlsf-fixtures.txt) cover 3408 decoding steps:
3402 match upstream indices, Q15 spectra, interpolation and entropy counters;
six exceed the supplied bit budget and are rejected transactionally. Tests
exercise every first-stage codebook entry, all residual indices from -10 to 10,
and all interpolation factors. Rare residual escapes are deliberately encoded
with the external reference encoder. Another [266 adversarial vectors](tests/silk-nlsf-stabilize-fixtures.txt)
check the stabilizer against upstream, including its bounded fallback. Tests
also enumerate all two-byte inputs across both orders, durations and signal types.

The tables and fixtures can be independently regenerated from the clean pinned
checkout. Only the optional generator compiles external C; Cargo never does.

```sh
python3 tools/silk_nlsf_fixtures.py target/reference/opus > /tmp/opura-nlsf.txt
diff -u tests/silk-nlsf-fixtures.txt /tmp/opura-nlsf.txt
python3 tools/silk_nlsf_fixtures.py target/reference/opus --stabilize > /tmp/opura-nlsf-stabilize.txt
diff -u tests/silk-nlsf-stabilize-fixtures.txt /tmp/opura-nlsf-stabilize.txt
python3 tools/silk_nlsf_fixtures.py target/reference/opus --tables > /tmp/opura-nlsf-tables.rs
rustfmt --edition 2024 /tmp/opura-nlsf-tables.rs
diff -u src/silk/nlsf_tables.rs /tmp/opura-nlsf-tables.rs
```

## Versions and history

See [CHANGELOG.md](CHANGELOG.md) for versioned changes. Development milestones
use annotated `vX.Y.Z` Git tags, with matching Cargo package versions. Before
1.0, incompatible API changes increment the minor version; compatible additions
and fixes increment the patch version. Tags record tested development milestones,
not crates.io publication or full codec conformance.

## SILK prediction filters

`FrameNlsf::to_lpc()` returns `FrameLpc` with Q12 prediction coefficients for
both halves of the frame. It uses the fixed-point cosine table and polynomial
ordering, coefficient fitting, and inverse-prediction-gain stability correction
from Opus 1.6.1. Conversion uses fixed storage and bounded loops. It consumes no
entropy and does not alter spectral history. For 10 ms frames, or when
interpolation is disabled, both filters are identical.

[2281 independent reference spectra](tests/silk-lpc-fixtures.txt) verify exact
Q12 coefficients and Q30 inverse gains, including clustered and boundary
spectra. Integration tests also check both returned filters for every accepted
NLSF reference step. Post-loss bandwidth expansion remains unimplemented.
The separate synthesis stage below consumes these filters to produce internal-rate PCM.

Optional regeneration uses the same clean upstream checkout:

```sh
python3 tools/silk_lpc_fixtures.py target/reference/opus > /tmp/opura-lpc.txt
diff -u tests/silk-lpc-fixtures.txt /tmp/opura-lpc.txt
python3 tools/silk_lpc_fixtures.py target/reference/opus --table > /tmp/opura-lpc-table.rs
rustfmt --edition 2024 /tmp/opura-lpc-table.rs
diff -u src/silk/lpc_table.rs /tmp/opura-lpc-table.rs
```

## SILK pitch and long-term prediction

`opura::silk::pitch::PitchState` consumes the side information immediately after
NLSF symbols: pitch lag and contour, LTP periodicity and vector indices, scaling,
and the excitation seed. `FramePitch` exposes sample lags and five signed Q14
LTP coefficients per 5 ms subframe. `SilkRate` selects the internal 8/12/16 kHz
rate; `FrameDuration` selects a 10/20 ms internal frame.

`PitchCoding` distinguishes independent coding, independent coding without LTP
scaling, and conditional coding. Conditional lags use previous voiced history;
without it, lags are absolute. Inactive/unvoiced frames consume only the seed,
produce zero pitch/LTP parameters, and clear voiced history. Sampling-rate
changes reset history automatically. Implicit scale index zero on a voiced
frame means Q14 scale 15565, not zero.

Entropy and history commit together only on success. As with the other
primitives, a caller must defer history commits until all
excitation data succeeds. Malformed delta chains preserve the reference's signed
16-bit raw index behavior while reconstructed sample lags clamp to 2–18 ms.
This stage stops before excitation samples; the synthesis and channel stages
below provide PCM. Concealment remains future work.

[3618 reference steps](tests/silk-pitch-fixtures.txt) use the actual pinned
upstream index-decoding suffix, parameter reconstruction and pitch decoder.
Coverage assertions require every pitch contour, every LTP codebook vector,
all scale indices and all seed values. Tests also exercise all two-byte inputs
across 18 rate/duration/coding combinations, unvoiced one-byte frames, rate
changes, and long malformed delta chains.

```sh
python3 tools/silk_pitch_fixtures.py target/reference/opus > /tmp/opura-pitch.txt
diff -u tests/silk-pitch-fixtures.txt /tmp/opura-pitch.txt
python3 tools/silk_pitch_fixtures.py target/reference/opus --tables > /tmp/opura-pitch-tables.rs
rustfmt --edition 2024 /tmp/opura-pitch-tables.rs
diff -u src/silk/pitch_tables.rs /tmp/opura-pitch-tables.rs
```

## Comparison goals

The [competitive landscape and benchmark contract](docs/benchmarks/competitive-landscape.md)
records the GitHub comparison set, pinned evidence and measurable performance,
quality, reliability and usability targets. Competitor claims have not yet been
reproduced by Opura; no overall leadership is claimed.

[Local measurements and gates](docs/benchmarks/packet-0.1.5.md) record the first
packet-framing comparison with libopus, before/after raw samples and remaining
failures. The full-codec competitive target is not met.

## SILK excitation

`opura::silk::excitation::decode_excitation` reads from the cursor after the
pitch/LTP seed field. Pass the signal type and offset selector from gains and
the seed from pitch. It decodes the rate level, pulse sums, shell splits,
LSB extensions and signs, then reconstructs the Q14 excitation with the
reference quantization offsets and wrapping pseudo-random generator.

The result contains signed pulses and excitation for 80, 120, 160, 240 or 320
samples. A 120-sample frame consumes the entire 128-sample shell representation,
including padding bits and signs, but does not expose the padding as audio.
Escape chains stop at ten LSBs. Output uses fixed buffers and all work is
bounded; errors leave the caller's entropy unchanged. Offset selectors outside
0..=1 and seeds outside 0..=3 are rejected before reading entropy.

[Independent reference fingerprints](tests/silk-excitation-fixtures.txt) cover
1320 unique length/context cases, exercised through 1584 rate/duration
configurations. They compare signed pulses, Q14 samples and entropy counters.
FNV-1a over little-endian signed-sample bit patterns keeps the corpus compact;
these fingerprints are regression evidence, not cryptographic proofs. Tests
also cover 46080 short/seeded input cases and detect changes to escape limits,
padded-block consumption, PRNG pulse feedback and error transactions.

```sh
python3 tools/silk_excitation_fixtures.py target/reference/opus > /tmp/opura-excitation.txt
diff -u tests/silk-excitation-fixtures.txt /tmp/opura-excitation.txt
python3 tools/silk_excitation_fixtures.py target/reference/opus --tables > /tmp/opura-excitation-tables.rs
rustfmt --edition 2024 /tmp/opura-excitation-tables.rs
diff -u src/silk/excitation_tables.rs /tmp/opura-excitation-tables.rs
```

This is excitation before LTP/LPC filtering and gain scaling, not PCM.
`ChannelState` below supplies the enclosing state transaction and synthesis;
stereo processing and resampling are separate stages below.


## SILK synthesis

`opura::silk::synthesis::SynthesisState::synthesize` consumes the rate and
existing `FrameGains`, `FrameNlsf`, `FramePitch` and `FrameExcitation` for one
channel of the same internal frame. It converts NLSFs to LPC coefficients,
rewhitens pitch history, applies five-tap LTP and 10/16-tap LPC prediction,
adjusts history for gain changes, and returns signed 16-bit `PcmFrame` samples.
Supported internal rates are 8/12/16 kHz and durations 10/20 ms. Buffers are
fixed-size; no allocation, dependency or unsafe Rust is introduced.

State persists between frames. `reset()` clears all history. A rate change
clears PCM/LPC history while preserving the previous gain, matching the reference
rate-switch behavior. Geometry and voiced-pitch mismatches return a typed error
without changing synthesis state. Parameter provenance remains the caller's
responsibility: shape validation cannot establish that all components came from
the same entropy stream. The channel integration below transacts the entropy,
gain, NLSF and pitch states around synthesis.

[192 independent reference frames](tests/silk-synthesis-fixtures.txt) contain
literal excitation, PCM and persistent histories from pinned Opus 1.6.1
`silk_decode_core`, with the normal `decode_frame` history update. Tests compare
every PCM sample and all retained LPC/PCM state values, not just hashes. The
sequences cover both durations, all three rates, reset/rate changes, voiced and
unvoiced transitions, interpolation, pitch extremes, constant/changing gains,
quiet/strong excitation and clipping. Separate integration tests feed 192 sets
of decoded typed parameters and verify reset/error behavior. Targeted mutation
checks detect ordinary division replacing reference gain refinement, missing
mid-frame rewhitening, reversed LTP taps and incorrect saturating shifts.

```sh
python3 tools/silk_synthesis_fixtures.py target/reference/opus > /tmp/opura-synthesis.txt
diff -u tests/silk-synthesis-fixtures.txt /tmp/opura-synthesis.txt
```

This produces **loss-free internal-channel PCM**, before CNG, PLC glue, stereo
reconstruction, output-rate resampling and CELT/Hybrid mixing. It is not a
complete Opus decoder, does not handle packet loss, and does not establish audio
quality or performance superiority over competitors. Mono SILK packet
orchestration is described below.

## SILK channel integration

`opura::silk::channel::ChannelState::decode` combines gains, NLSF, pitch/LTP,
excitation and synthesis into one transaction for a 10/20 ms internal channel
frame at 8/12/16 kHz. It takes a range cursor positioned at signal-type symbols,
plus rate, duration, activity and `PitchCoding`. Both independent coding modes
use absolute first gains; conditional coding needs prior history at the same
rate. The caller must first consume headers, any LBRR payload ordering and
stereo fields. This API does not parse packets or perform those steps.

Success commits entropy and all histories together and returns `PcmFrame`.
Failure returns a stage-specific `ChannelError` and preserves every history
and the entropy cursor for retry. `reset()` clears all history. Rate changes
clear gain, NLSF and pitch history, including 8↔12 kHz switches with unchanged
LPC order. Synthesis clears PCM/LPC history but retains its previous linear
gain, as upstream does. Duration changes retain history. Separate channels
need separate state instances.

[Independent reference sequences](tests/silk-channel-fixtures.txt) compare
all PCM samples and range/tell/tell_frac after 144 consecutive compressed
internal frames, using the complete pinned upstream index, pulse, parameter
and core synthesis routines. They cover both durations, all rates, all
activity and coding modes, interpolation, reset and rate changes. Separate
tests truncate inputs to provoke failure at each entropy stage, verify full
rollback and successful retry, and reject conditional starts after reset or
rate change. Targeted mutation checks detect missing spectral-history reset
and premature history commits before excitation validation. The synthetic
inputs are not complete Opus conformance vectors;
output precedes CNG/PLC glue, stereo conversion and resampling. Complete
Opus support and competitive PCM performance remain future work. Mono SILK
packet integration is described below.

Optional fixture regeneration uses the same clean pinned reference checkout:

```sh
python3 tools/silk_channel_fixtures.py target/reference/opus > /tmp/opura-channel.txt
diff -u tests/silk-channel-fixtures.txt /tmp/opura-channel.txt
```

## Mono SILK packets to PCM

`opura::silk::mono::MonoDecoder::decode(&packet_bytes)` parses an externally
delimited mono SILK-only Opus packet and returns `MonoPcm`. It handles 10/20/40/60 ms
Opus frames, packet codes 0–3, framing padding and multi-frame packets up to 120 ms.
Output uses fixed storage for at most 1920 signed 16-bit samples, with no allocation,
unsafe code or dependencies. `sample_rate()` reports the encoded 8/12/16 kHz rate;
`samples()` exposes the used portion, and `final_range()` matches the public
libopus final range of the last Opus frame for supported packets.

Each Opus frame has its own entropy cursor and SILK header. LBRR is consumed
before regular audio, with conditional coding only for adjacent redundant
frames. Scratch states keep skipped LBRR out of regular gain/NLSF/synthesis
history. The first regular internal frame is independent; later internal frames
are conditional. Histories persist across Opus frames and packets, with channel
rate-switch behavior. `reset()` starts a fresh stream.

The entire packet is a transaction: any `MonoError` discards all output and
history updates, including earlier successfully decoded frames. Empty/one-byte
loss or DTX frames, stereo, CELT, Hybrid and SILK/CELT transition redundancy
return explicit errors. LBRR is skipped for normal playback; use `decode_fec` below for recovery. Transport/container parsing remains the caller's responsibility.

SILK encoders may strip trailing zero bytes. Packet entropy therefore uses zero
extension with a virtual budget of 1275 bytes, the maximum original Opus frame
size, while existing public channel/parameter primitives retain their strict
input-length budgets. Some truncated or modified payloads are indistinguishable
from valid zero-trimmed data; successful decoding and final range do not provide
an integrity guarantee. All decoding loops and output sizes remain bounded.

These are **raw internal-channel samples** from loss-free synthesis, before
Opus mono delay buffering and output-rate resampling. They are not sample-aligned
with `opus_decode` output. CNG/PLC and stereo output are described below.
CELT/Hybrid mixing remains future work. This API establishes a limited packet-to-PCM
path, not complete Opus conformance or competitive audio performance.

[Audio packet fixtures](tests/silk-packet-fixtures.txt) come from the pinned
Opus 1.6.1 scalar fixed-point encoder, using deterministic periodic/noise audio
and silence, both with and without in-band FEC. The full upstream decoder runs
normally; observational hooks copy original `silk_decode_core` PCM and identify
transition tails. Tests compare every supported PCM sample and public
`OPUS_GET_FINAL_RANGE`, and verify explicit rejection and rollback for transitions.
The corpus includes rate/duration changes and packets repacked by the reference
repacketizer. Separate synthetic symbol packets encoded with upstream primitives
exercise every LBRR mask, including gaps and adjacent conditional frames. Additional checks exercise packet codes, padding, the 120 ms output
boundary, zero-byte trimming, malformed inputs and whole-packet rollback/retry.

Optional regeneration requires the same clean pinned checkout, Python and a C
compiler. It compiles external source manifests into a temporary reference
library; normal Cargo builds and tests use only the saved fixtures.

```sh
python3 tools/silk_packet_fixtures.py target/reference/opus > /tmp/opura-silk-packets.txt
diff -u tests/silk-packet-fixtures.txt /tmp/opura-silk-packets.txt
```

## SILK comfort noise

`opura::silk::cng::CngState` estimates background noise from decoded inactive
frames. `update(rate, gains, nlsf, excitation)` observes typed channel parameters;
active frames retain the estimates and clear the noise filter history. Channel
transactions now retain these estimates automatically without modifying loss-free
PCM. Separate channels require separate state instances.

`mix(rate, duration, noise_scale_q14, previous_gain_q16, pcm)` adds CNG to a
10/20 ms frame already produced by PLC. The scale and previous gain come from
PLC after concealment; they are not volume controls. It compensates for the
random-noise energy already supplied by PLC, selects excitation from its bank
with the reference PRNG, and applies fixed-point LPC synthesis and saturation.
Invalid geometry, a scale outside 0..=16384 or a nonpositive prior gain leaves
history and samples unchanged. All buffers and loops are bounded. Rate switches
reset smoothed estimates/seed while retaining bank/filter memory as upstream
does; explicit `reset()` clears all history.

[192 CNG reference cases](tests/silk-cng-fixtures.txt) compare literal PCM and
all retained state fields against pinned `silk_CNG`: inactive/active frames,
first loss before training, burst losses, recovery, all rates/durations, switches
during loss, both gain-energy branches and clipping. Tests also check typed
integration and transactional errors. Targeted mutations detect wrong excitation
masks, PLC noise compensation and missing good-frame filter reset.

```sh
python3 tools/silk_cng_fixtures.py target/reference/opus > /tmp/opura-cng.txt
diff -u tests/silk-cng-fixtures.txt /tmp/opura-cng.txt
```

This stage does not itself supply PLC; the channel loss engine below combines
it with concealment. The packet API requires explicit loss requests, as described below, and rejects
implicit DTX payloads. Complete Opus compatibility and competitive performance remain
unestablished.

## SILK channel loss concealment

`ChannelState::conceal(rate, duration)` advances one missing 10/20 ms internal
frame through PLC, CNG and loss-energy bookkeeping, returning `PcmFrame` without
consuming entropy. A fresh channel produces silence. Separate channel instances
retain their own histories; explicit `reset()` starts a new stream. Errors leave
all state unchanged. No allocation or unsafe Rust is used.

PLC retains the last good excitation and filter, estimates the harmonic/noise
mixture, rewhitens PCM history, drifts pitch and attenuates both components during
loss. Concealed PCM updates shared synthesis history before CNG, as upstream does.
The next good frame applies post-loss filter bandwidth expansion and temporary
voiced prediction after voiced loss; energy glue fades a stronger recovery onset.
Concealment resets the gain index to 10, matching dec_API. Entropy context and
first-frame/conditional scheduling remain the caller's responsibility, including
independent first regular frames after configuration changes.

[240 independent loss/recovery steps](tests/silk-plc-fixtures.txt) run the original
pinned `silk_decode_frame`, including original PLC, CNG and glue. Tests compare
every PCM sample, entropy counters for good frames, gain indices, shared LPC/PCM
history and all retained PLC fields. Sequences include loss before training,
consecutive losses, recovery, all rates/durations, changes during loss, inactive,
unvoiced and voiced signals, and clipping. Public tests also exercise long bursts,
reset and transactional failure/retry during recovery. Targeted mutations detect
missing PLC expansion, energy fade, post-loss expansion and recovery prediction.

```sh
python3 tools/silk_plc_fixtures.py target/reference/opus > /tmp/opura-plc.txt
diff -u tests/silk-plc-fixtures.txt /tmp/opura-plc.txt
```

The mono packet API rejects empty/one-byte loss or DTX payloads; use the explicit
packet-stream loss method below. FEC recovery remains separate work. Channel
PCM precedes mono buffering/resampling and stereo. Full Opus conformance and
competitive performance remain unestablished.

## Mono packet-stream loss handling

After a successful packet, `MonoDecoder::conceal(duration_ms)` fills an explicitly
missing interval. Durations are 10..=120 ms in multiples of 10; the last good packet
establishes the rate. It schedules 10 ms internal frames after a 10 ms Opus frame,
or 20 ms frames plus a final 10 ms remainder after longer Opus frames, matching
libopus. Output includes PLC/CNG and loss-energy bookkeeping, with final range
zero. All histories and samples commit as one transaction. Invalid durations or
missing stream history return typed errors without changing state. `reset()` also
clears the last-rate/duration history.

[288 real packet-stream reference steps](tests/silk-packet-loss-fixtures.txt)
include 120 scheduled losses across every supported duration, burst losses,
silence, FEC-bearing packets and subsequent good audio at all three SILK rates.
Observational hooks copy the completed original `silk_decode_frame` output after
PLC/CNG/glue; tests compare every internal PCM sample and public final range.
The default packet fixture mode remains byte-identical. Additional tests cover
invalid requests, reset, failed recovery rollback/retry, and mutations of loss
scheduling and history advancement.

```sh
python3 tools/silk_packet_fixtures.py target/reference/opus --loss > /tmp/opura-packet-loss.txt
diff -u tests/silk-packet-loss-fixtures.txt /tmp/opura-packet-loss.txt
```

Missing intervals must be explicit; `decode` still rejects empty/one-byte DTX or
loss payloads. In-band FEC is consumed during normal decoding and can be used through
the explicit recovery method below. PCM remains before mono delay buffering/resampling.
Stereo output uses the channel-general API below; CELT/Hybrid remain unimplemented. Full Opus conformance and competitive performance
remain unestablished.

## Mono in-band FEC recovery

`MonoDecoder::decode_fec(&received_packet)` recovers the preceding interval from
LBRR in the first compressed Opus frame, matching `opus_decode` with FEC enabled
and a buffer of that frame's duration. In multi-frame packets it recovers only
the first frame interval. Decode the same received packet normally afterwards
to obtain its primary audio and continue with the recovered histories. Earlier
missing intervals can be filled explicitly with `conceal` before FEC recovery.

Present LBRR frames use their active probability context and conditional coding
only after an adjacent present frame. Gaps use PLC/CNG without the gain reset
performed for explicit packet loss. A fresh decoder takes its configuration from
the received packet; absent FEC then produces cold concealment rather than an
error. FEC does not consume primary audio or interpret a CELT transition tail.
Its final range describes the redundant entropy prefix; all state/output commit
atomically. Unsupported modes/channels and missing first payloads return errors.

[328 FEC/reference regression cases](tests/silk-fec-fixtures.txt) include 111 FEC
and 215 normal PCM results from the complete original decoder, a real unsupported
primary transition, and an encoded over-budget prefix that fails after an earlier
internal frame. They cover real encoder FEC/no-FEC streams, silence, rate changes,
subsequent primary decoding and synthetic packets for every sparse LBRR mask.
Tests compare all PCM samples and public final range, and verify multi-frame
first-frame semantics, reset, unsupported input and late-error rollback. Targeted
mutations detect incorrect sparse coding, premature commits and FEC gain reset.

```sh
python3 tools/silk_packet_fixtures.py target/reference/opus --fec > /tmp/opura-fec.txt
diff -u tests/silk-fec-fixtures.txt /tmp/opura-fec.txt
```

As with normal and loss decoding, output precedes mono delay buffering/resampling.
CELT/Hybrid and implicit DTX still require implementation; no full-codec
conformance or competitive performance claim follows from these stage checks.

## SILK output resampler

`opura::silk::resampler::ResamplerState::new(OutputRate)` selects one of the standard
8/12/16/24/48 kHz outputs. `process(input_rate, input, output)` accepts internal
8/12/16 kHz PCM and writes converted samples into a caller-owned buffer, returning
the count written. Input must contain 1..=120 whole milliseconds. A short output
buffer or invalid input geometry changes neither state nor output; any unused
output tail remains untouched. Buffers and loops are bounded and allocation-free.

The stage includes reference input-delay compensation, x2 allpass upsampling,
fractional FIR interpolation and AR/FIR downsampling for all 15 supported rate
pairs. Filter history persists across calls; an input-rate change resets it, while
`reset()` clears history but retains the output rate. Output is invariant under
whole-ms chunking. The separate Opus mono/stereo buffering delay must still be
supplied by the packet output integration.

[360 reference cases](tests/silk-resampler-fixtures.txt) compare every PCM sample,
active IIR/FIR state, delay buffer and configuration value with the original pinned
resampler. Inputs include silence, impulses, positive/negative DC limits,
alternating signed limits, random samples and clipping, across variable chunk
sizes, explicit resets and input/output rate pairs. Additional tests check split
processing, follow-up state behavior and transactional validation. Mutations detect
a missing allpass leading term, incorrect FIR mirroring and dropped input delay.

```sh
python3 tools/silk_resampler_fixtures.py target/reference/opus > /tmp/opura-resampler.txt
diff -u tests/silk-resampler-fixtures.txt /tmp/opura-resampler.txt
python3 tools/silk_resampler_fixtures.py target/reference/opus --tables > /tmp/opura-resampler-tables.rs
rustfmt --edition 2024 /tmp/opura-resampler-tables.rs
diff -u src/silk/resampler_tables.rs /tmp/opura-resampler-tables.rs
```

Packet output integration and direct public-Opus PCM comparisons are described below.
CELT/Hybrid remain incomplete; no competitive performance claim is made.

## Final-rate mono SILK PCM

`opura::silk::output::MonoOutputDecoder::new(OutputRate)` supplies the final mono
output path at 8/12/16/24/48 kHz. It combines packet/channel decoding, PLC/CNG/FEC,
the dec_API mono delay and the output resampler. Use `decode(packet)`,
`conceal(duration_ms)` or `decode_fec(packet)` with the same scheduling and subset
restrictions as `MonoDecoder`. FEC is followed by normal decoding of the same
received packet. `OutputPcm` exposes final `samples()`, `sample_rate()` and
`final_range()`, with fixed storage for at most 5760 samples (120 ms at 48 kHz).

Every operation commits packet, predictor, delay and resampler state together.
Errors preserve all histories. Input-rate switches reset resampling filters but
retain the preceding mono channel sample, matching upstream buffering semantics.
`reset()` clears the stream while retaining the selected output rate. All work
is bounded and allocation-free.

[260 direct public-decoder cases](tests/silk-output-fixtures.txt) contain the actual
signed-16 samples returned by pinned fixed-point `opus_decode`, rather than an
internal observation point. Tests match every sample and public final range on
255 successful normal/loss/FEC results across all 15 input/output rate pairs,
durations, 120 ms repacketized inputs, recovery and input-rate switches. Five real
transition packets reject transactionally. Additional tests verify invalid input,
late FEC resource-error rollback, output-history recovery and reset. Mutations
detect missing mono delay and accidental resampler reset between packets.

```sh
python3 tools/silk_packet_fixtures.py target/reference/opus --public-output > /tmp/opura-output.txt
diff -u tests/silk-output-fixtures.txt /tmp/opura-output.txt
```

The lower-level APIs remain available for raw internal PCM and individual stages.
This final-rate path supports mono SILK, not stereo, CELT/Hybrid, implicit DTX or
CELT transition reconstruction. It has not yet passed complete official Opus
conformance, and no full-codec performance superiority is claimed.

## SILK stereo reconstruction primitives

`opura::silk::stereo::StereoPrediction::decode(&mut entropy)` reads the joint,
coarse and fine stereo predictor symbols transactionally. `decode_mid_only` reads
the optional flag separately; callers determine its presence from side VAD/LBRR
context. Failed symbol decoding leaves entropy unchanged. Default prediction is
zero, suitable before the first stereo frame; the optimized Q13 pair is exposed
by `coefficients_q13()`.

`StereoState::reconstruct(rate, mid, side, prediction)` converts equal-length
10/20 ms raw mid/side frames at 8/12/16 kHz into `StereoFrame` left/right samples.
It uses two previous raw samples per channel and 8 ms predictor interpolation,
including reference signed-16 difference truncation at extreme predictor changes.
Side history retains original samples before prediction. Invalid geometry leaves
all history unchanged; `reset()` clears it. Rate changes preserve stereo history,
with packet channel-transition policy left to orchestration.

`StereoFrame` includes the one-sample buffering delay and is ready for per-channel
resampling. Do not apply the separate mono buffering delay again. Output uses
fixed buffers and signed saturation with no allocation or unsafe Rust. Previous
prediction can be reused during losses through `previous_prediction()`.

[All 5625 predictor combinations](tests/silk-stereo-entropy-fixtures.txt) compare
values, both mid-only outcomes and entropy states with original pinned routines.
[192 reconstruction cases](tests/silk-stereo-fixtures.txt) compare both channels
and every retained state value across quiet, extreme, correlated/opposed inputs,
clipping, resets, rate changes and signed-16 interpolation wrap. Tests cover every
two-byte prefix and invalid geometry transactions. Mutations detect incorrect
predictor subtraction, interpolation width and side-history retention.

```sh
python3 tools/silk_stereo_fixtures.py target/reference/opus --entropy > /tmp/opura-stereo-entropy.txt
diff -u tests/silk-stereo-entropy-fixtures.txt /tmp/opura-stereo-entropy.txt
python3 tools/silk_stereo_fixtures.py target/reference/opus > /tmp/opura-stereo.txt
diff -u tests/silk-stereo-fixtures.txt /tmp/opura-stereo.txt
python3 tools/silk_stereo_fixtures.py target/reference/opus --tables > /tmp/opura-stereo-tables.rs
rustfmt --edition 2024 /tmp/opura-stereo-tables.rs
diff -u src/silk/stereo_tables.rs /tmp/opura-stereo-tables.rs
```

The channel-general packet API below composes these primitives with side-channel
restarts and mono/stereo stream transitions. CELT/Hybrid and complete Opus
conformance remain future work.


## Final-rate mono/stereo SILK PCM

`opura::silk::decoder::SilkDecoder::new(OutputRate, PcmChannels)` accepts either
encoded channel count and produces one mono or two interleaved output channels
at 8/12/16/24/48 kHz. Output geometry is fixed for a stream. Encoded stereo to
mono uses the decoded mid channel; mono to stereo duplicates output after the
reference transition filtering. `decode(packet)`, `conceal(duration_ms)` and
`decode_fec(packet)` follow the scheduling described above. Decode a received
packet normally after recovering its preceding first-frame interval with FEC.

`DecodedPcm` exposes samples, rate, channel count, samples per channel and final
range. Storage is bounded to 11520 interleaved samples (120 ms at 48 kHz stereo).
Operations commit both channel, stereo, resampler and configuration histories
together. Reset retains output geometry. Mid-only side suppression, side restart,
independent/conditional side coding, stereo predictor reuse during loss/FEC,
LBRR ordering, internal-rate and mono/stereo transitions follow pinned dec_API.

[1768 public reference cases](tests/silk-decoder-fixtures.txt) compare every final
PCM sample and public final range: 1194 normal, 464 FEC and 80 loss results,
plus 30 rejected transition packets. They cover all 15 rate pairs, both output
layouts and encoded channel counts, 10–120 ms intervals, repacketized frames,
mid-only/side return (including within a payload), silence, loss recovery and
all 64 independent mid/side LBRR mask pairs at each internal rate/output layout.
Reference observation flags verify side restart/suppression and resampler
transition branches were exercised. Additional tests retain all 260 earlier
mono results, check reset, malformed input, late internal/Opus-frame rollback
and 1024 bounded adversarial decode/FEC calls. Six mutations detect wrong side
coding, FEC side presence, side-state reset, omitted stereo LBRR fields and
incorrect resampler history on both directions of channel transition.

```sh
python3 tools/silk_packet_fixtures.py target/reference/opus --stereo-output > /tmp/opura-decoder.txt
diff -u tests/silk-decoder-fixtures.txt /tmp/opura-decoder.txt
```

The optional oracle observes original functions and public output without
replacing reference decoding algorithms. Ordinary builds/tests need no C or
libopus. CELT/Hybrid, CELT transition tails and implicit empty-payload DTX remain
unsupported and reject transactionally. Complete official Opus conformance and
competitive performance are not established.


## CELT fixed-point math

`opura::celt::math` supplies the scalar math needed for forthcoming CELT energy,
PVQ and loss reconstruction. `integer_sqrt(u32)` returns the exact floor of the
root (including zero) in at most 16 steps. Other functions reproduce the pinned
reference's integer approximations, rather than correctly rounded real math:
`sqrt`, `rsqrt_normalized`, `rsqrt_normalized_q31`, `cos_norm`, `log2`, `exp2`, `reciprocal`,
`fractional_divide_q29` and saturated-Q31 `fractional_divide`.

Rustdoc specifies Q formats, saturation, zero sentinels and valid input domains.
Invalid inputs return `MathError` before arithmetic. Fractional division accepts
positive denominators with numerator magnitude no greater than the denominator;
negative Q31 saturation uses -2147483647. All operations use bounded fixed-width
integer arithmetic without allocation or dependencies. Arithmetic follows the
reference scalar fast-64-bit multiplication path on every Rust target.

[Reference evidence](tests/celt-math-fixtures.txt) includes 624 FNV-1a blocks
covering 638975 individual results and 29164 boundary/wide/division cases.
Exhaustive blocks include signed Q10 exponents, normalized Q16 reciprocal roots,
two Q15 cosine periods and small integer sqrt/log/reciprocal domains. Each hash
covers four little-endian bytes per signed result. Separate tests independently
check integer-sqrt inequalities around every square up to 65535² and 65536 wide
values. Five mutations detect rounding, normalization and Newton corrections.

Optional regeneration compiles original external reference routines into a
temporary library, requiring the same clean pinned source, Python and C compiler:

```sh
python3 tools/celt_math_fixtures.py target/reference/opus > /tmp/opura-celt-math.txt
diff -u tests/celt-math-fixtures.txt /tmp/opura-celt-math.txt
```

These primitives do not yet decode CELT/Hybrid packets or establish full-codec
conformance/performance. CELT transform and band decoding are the next stages.


## CELT band energies

`opura::celt::energy::EnergyState` retains 21 Q24 log-energy values per channel.
`BandLayout::new(channels, start, end)` validates one/two encoded channels and a
half-open interval within 0..21 (including empty intervals). Unselected bands
and channels retain their histories. `FrameSize` selects 2.5/5/10/20 ms at the
internal 48 kHz clock; its `samples()` reports 120/240/480/960 respectively.

Call `coarse(entropy, layout, size, intra)` after reading the intra flag. It uses
Laplace, three-symbol, one-bit or no-bit delta fallbacks according to remaining
physical bits, then applies temporal/inter-band prediction and reference clamps.
`fine(entropy, layout, bits)` reads zero through eight raw refinement bits per
band, skipping a whole band when insufficient bits remain. After shape decoding,
`finalise(entropy, layout, bits, priorities, bits_left)` refines eligible bands in
priority order and returns unused bits. Its budget cannot exceed the remaining
physical input. These stages must be positioned around allocation/PVQ by the
future packet decoder; this API does not consume a complete CELT frame itself.

All operations commit energies and entropy together. Invalid layouts/quantizers,
invalid final budgets and arithmetic overflow reject without changing history.
`reset()` zeros both channel histories; `channel(index)` inspects all 21 energies.
The classic Opus path is supported; QEXT/RES24 refinement is excluded.

`RangeDecoder::decode_laplace(zero_frequency, decay)` exposes the bounded classic
CELT entropy distribution with validated Q15 zero frequency 1..32736 and Q14
decay 0..11456. Like other range primitives it zero-extends compressed input;
the energy stages handle physical budgets. Invalid models leave entropy unchanged.

[Reference sequences](tests/celt-energy-fixtures.txt) use original pinned
`unquant_coarse_energy`, `unquant_fine_energy`, `unquant_energy_finalise` and
`ec_laplace_decode`, recording all energies and entropy diagnostics after each
stage. Tests cover 768 chained frames across all LM/channel/intra contexts,
selected/empty intervals and 0–200 byte payloads, plus 2688 standalone symbols.
Every two-byte prefix is checked with three boundary Laplace models. Injected
second-channel refinement overflow verifies rollback after earlier samples and
raw bits have advanced. Five mutations detect accumulator truncation, temporal
clamp, inter-band prediction, fine midpoint and final priority errors.

```sh
python3 tools/celt_energy_fixtures.py target/reference/opus > /tmp/opura-celt-energy.txt
diff -u tests/celt-energy-fixtures.txt /tmp/opura-celt-energy.txt
python3 tools/celt_energy_fixtures.py target/reference/opus --tables > /tmp/opura-celt-energy-tables.rs
rustfmt --edition 2024 /tmp/opura-celt-energy-tables.rs
diff -u src/celt/energy_tables.rs /tmp/opura-celt-energy-tables.rs
```

CELT shape decoding, transform synthesis and complete Opus conformance remain
incomplete. No end-to-end CELT audio or performance claim follows from these tests.


## CELT integer pulse vectors

`opura::celt::pulses::PulseCodebook::new(dimensions, pulses)` accepts 2..176
coordinates and 0..128 pulses when the signed codebook fits a `u32` index.
`cardinality()`, `dimensions()` and `pulses()` expose the validated geometry.
`decode_index(index)` returns a fixed-storage `PulseVector` in canonical CWRS
coordinate order; `values()` exposes its signed coordinates and `squared_norm()`
the exact sum of squares. Absolute coordinate values sum to the pulse count.
Zero pulses yields a single all-zero vector.

`decode(entropy)` reads the uniform index and commits entropy only on success.
Large indices use raw suffix bits, which must be present physically; range input
retains the usual zero-extension semantics and the future band decoder enforces
codec bit budgets. Invalid geometry, overflowing cardinalities and out-of-range
indices return typed `PulseError` values. The one-dimensional sign case belongs
to band orchestration and is excluded from this primitive.

Compile-time symmetric U recurrence rows use capped counts and checked V sums.
No runtime allocation or table generation is needed; descending-dimension
ordering takes at most dimensions+pulses search/coordinate iterations. Capped
values reject overflowing codebooks and cannot change any valid index ordering.

[Original generic CWRS evidence](tests/celt-pulse-fixtures.txt) covers all 22575
configuration outcomes (1834 fit), 70423 indices exhaustively across 57 small
codebooks, 13401 wide/boundary vectors, and 566 entropy steps across 73 streams.
Exhaustive fingerprints hash little-endian signed-32 squared norm followed by
every coordinate for each index. Separate tests independently count codewords
using signed occupancy/binomial sums and check pulse/norm invariants. Invalid
requests and raw suffix exhaustion preserve entropy, including after a preceding
range symbol. Four mutations detect sign boundary, coordinate order, U recurrence
and index-interval errors.

```sh
python3 tools/celt_pulse_fixtures.py target/reference/opus > /tmp/opura-celt-pulses.txt
diff -u tests/celt-pulse-fixtures.txt /tmp/opura-celt-pulses.txt
```

The optional oracle compiles original `cwrs.c` with `SMALL_FOOTPRINT` so arbitrary
valid geometries can be checked without the optimized standard-mode table's
shape restrictions. CELT normalized bands, rotations, allocation and inverse
transforms remain future work; these integer vectors do not provide CELT audio.


## Normalized CELT PVQ bands

`opura::celt::pvq::decode_band(entropy, codebook, spread, blocks, gain)` composes
positive-pulse CWRS decoding, Q24 normalization and inverse spreading. Gain is a
nonnegative Q31 value, and `Spread` selects None/Light/Normal/Aggressive. Blocks
must be 1/2/4/8 and divide the coordinate count. `NormalizedBand::values()` exposes
Q24 spectral coordinates; `collapse_mask()` marks contiguous raw-pulse blocks
with nonzero entries before spreading, independently of gain.

Refined normalization uses `math::rsqrt_normalized_q31`, which maps Q31 input
in [2^29,2^31) to a Q29 reciprocal root with the reference Newton step. Spreading
preserves the original two-pass rotation order, secondary stride and deliberate
Q14 rounding between rotations. Output and intermediate storage are bounded;
input validation and pulse/raw-bit failures leave entropy unchanged. The caller
still enforces the surrounding CELT budget and handles zero-pulse folding,
one-dimensional signs, recursive splits and stereo orchestration.

[Original alg_unquant evidence](tests/celt-pvq-fixtures.txt) matches all spectral
coordinates, masks and range/tell/tell_frac in 28464 bands across 4116 streams.
It covers sparse/dense and boundary codebook indices, dimensions 2–176, zero,
half and full gains, every spread/block combination, including one-coordinate
short blocks. Another 196608 normalized reciprocal-root inputs are hashed from
original mathops.c, including fractional boundaries between Q16 seed values.
Five mutations detect missing root refinement, Q14 rounding, reverse rotation,
secondary-stride coefficients and incorrect collapse masks.

```sh
python3 tools/celt_pvq_fixtures.py target/reference/opus > /tmp/opura-celt-pvq.txt
diff -u tests/celt-pvq-fixtures.txt /tmp/opura-celt-pvq.txt
```

This is a normalized-band primitive, not CELT packet or audio output. Allocation,
recursive bands, transform synthesis, Hybrid and full conformance remain future work.


## Standard CELT complex FFT

`opura::celt::fft::FftPlan::new(FrameSize)` selects the standard 60/120/240/480
complex-point FFT used by a 120/240/480/960-sample CELT frame. `Complex` has signed
32-bit `real` and `imag` coordinates; callers choose signal scale/headroom.
`points()` reports the exact input length. Output needs at least that many
entries; longer output tails stay unchanged. Invalid geometry or downshift
returns `FftError` without modifying any output.

`forward(input, output, downshift)` transforms natural input to natural output
order with a negative exponent and no initial scale, distributing a 0..31-bit
shift across the reference radix stages. `forward_scaled(input, output)` matches
`opus_fft_c`, approximately dividing by N. `inverse(input, output)` matches the
unscaled positive-exponent `opus_ifft_c`. Arithmetic intentionally wraps at
signed-32 boundaries, matching the canonical scalar reference; avoid clipping by
providing suitable headroom/downshift. Stage rounding means these integer
transforms are not correctly rounded real DFTs or exact floating inverses.

Plans, shared Q15 twiddles, factors and bit-reversal permutations are static.
Radix 2/3/4/5 butterflies and downshift scheduling need no allocation or runtime
planning. The [1632 original FFT cases](tests/celt-fft-fixtures.txt) compare every
complex output for all sizes, all 32 shifts, canonical wrappers, impulses,
DC/Nyquist inputs, full-range signed extremes and deterministic arrays. Separate
floating DFT checks verify sign and natural frequency ordering at low amplitude;
exact impulse tests independently verify unscaled and 1/N forward amplitudes.
Five mutations detect bit reversal, plan twiddle stride, radix-3 sign, radix-5
coefficient and one-bit rounding errors.

```sh
python3 tools/celt_fft_fixtures.py target/reference/opus > /tmp/opura-celt-fft.txt
diff -u tests/celt-fft-fixtures.txt /tmp/opura-celt-fft.txt
python3 tools/celt_fft_fixtures.py target/reference/opus --tables > /tmp/opura-celt-fft-tables.rs
rustfmt --edition 2024 /tmp/opura-celt-fft-tables.rs
diff -u src/celt/fft_tables.rs /tmp/opura-celt-fft-tables.rs
```

The optional oracle observes original standard plans and scalar routines. FFT
alone does not synthesize CELT audio; inverse MDCT/window overlap and output
stages are described below. Recursive bands/allocation and full packet
orchestration remain incomplete.


## CELT inverse MDCT and overlap

`opura::celt::mdct::MdctState` stores the pending 60-sample overlap for one channel.
`synthesize(size, input, stride)` applies original inverse rotations/headroom
control, the verified FFT and the standard 120-sample TDAC window. It returns a
`TimeFrame` containing 120/240/480/960 signed-32 internal signal values through
`samples()`. These precede CELT postfiltering, de-emphasis and signed-16 PCM.
The caller supplies the reference spectral signal scale after denormalization.

Input bin i is read at `input[i*stride]`. Stride is 1/2/4/8, the selected
size*stride must not exceed 960, and input must contain all selected bins in at
most 960 entries. Other entries are ignored. Interleaved short blocks use
successive slices offset by block index, retaining overlap on each call.
State and output commit together; invalid geometry leaves overlap unchanged.
`overlap()` inspects the pending 60 samples; `reset()` clears them. Frame-size
changes preserve overlap, matching upstream synthesis buffers.

[398 original inverse-MDCT cases](tests/celt-mdct-fixtures.txt) match every time
sample and overlap entry (198360 values). Chained cases cover all size/stride
geometries, resets, silence, tiny signals, impulses, DC, full-scale signed limits,
alternating extremes and deterministic spectra. Additional sequences process
all 2/4/8 short blocks from a shared interleaved spectrum. Original headroom
observations cover pre/FFT shift extrema 0 and 29 and post-shift extrema 0 and
13, including pathological overflow behavior. Invalid requests preserve
state; five mutations detect lost overlap, wrong window complement, omitted FFT
shift, real/imaginary swap and incorrect saved tail.

```sh
python3 tools/celt_mdct_fixtures.py target/reference/opus > /tmp/opura-celt-mdct.txt
diff -u tests/celt-mdct-fixtures.txt /tmp/opura-celt-mdct.txt
python3 tools/celt_mdct_fixtures.py target/reference/opus --tables > /tmp/opura-celt-mdct-tables.rs
rustfmt --edition 2024 /tmp/opura-celt-mdct-tables.rs
diff -u src/celt/mdct_tables.rs /tmp/opura-celt-mdct-tables.rs
```

These are internal transform outputs; the complete synthesis backend is below.
CELT allocation/recursive band assembly, full packet decoding and complete
conformance remain incomplete; no end-to-end CELT audio or competitive performance claim is made.


## CELT spectral scaling and output de-emphasis

`opura::celt::output::denormalize(size, input, energies, bands, rate, silence)`
converts one normalized Q24 spectrum using 21 Q24 log energies plus the standard
band means. `Spectrum::values()` supplies reference-scale inverse-MDCT input.
The validated band range lies within 0..21, and the input exactly matches frame
size. Silence zeros the whole spectrum. Bins below selected bands and above the
output-rate cutoff are zeroed; reference gain underflow, caps, fractional energy
rounding and signed wrapping are preserved. The shared fractional exponent
polynomial includes the rounded Q10 endpoint needed by Q24 energies.

`Decimation::{By1, By2, By3, By4, By6}` selects 48/24/16/12/8 kHz respectively;
`factor()` and `sample_rate()` report its geometry. `DeemphasisState::process`
accepts a 120/240/480/960-sample 48 kHz left signal, an optional equal-length right
signal and decimation. It filters all internal samples before selecting output,
then rounds/saturates to signed-16 mono or interleaved stereo `PcmFrame` samples.
The frame reports channels, rate and samples per channel with at most 1920 values.

Feedback uses the canonical coefficient and clipping. Inactive right-channel
memory and both memories across rate changes are retained; `memory()` inspects
them and `reset()` clears them. Invalid signals leave both memories unchanged.
Comb postfiltering, when enabled, must precede de-emphasis. This API does not
perform Hybrid accumulation or supply a complete CELT packet decoder.

[Reference evidence](tests/celt-output-fixtures.txt) contains 640 original
spectral-scaling cases and 384 chained original de-emphasis calls, comparing
all spectra/PCM/memory values across sizes, rates, channel changes, bands,
silence, clipping, signed extremes and fractional energy boundaries. The
optional oracle extracts the original function bodies verbatim into a temporary
probe with original headers/constants; it does not substitute implementations.
Five mutations detect wrong mean-energy format, mantissa rounding, rate cutoff,
feedback gain and PCM rounding. Invalid geometry and reset preserve contracts.

```sh
python3 tools/celt_output_fixtures.py target/reference/opus > /tmp/opura-celt-output.txt
diff -u tests/celt-output-fixtures.txt /tmp/opura-celt-output.txt
python3 tools/celt_output_fixtures.py target/reference/opus --tables > /tmp/opura-celt-output-tables.rs
rustfmt --edition 2024 /tmp/opura-celt-output-tables.rs
diff -u src/celt/output_tables.rs /tmp/opura-celt-output-tables.rs
```

The comb postfilter and synthesis backend are described below. Recursive CELT
bands/allocation, Hybrid and complete Opus packet output remain future work. Stage equality does not establish complete
conformance or SOTA performance.


## CELT comb postfilter

`opura::celt::postfilter::PostfilterParams::new(period, gain, tapset)` validates
classic decoder periods 15..1023 and nonnegative Q15 gains 0..24576. Period zero
is accepted for zero gain; default parameters disable the filter. `Tapset`
selects standard Set0/Set1/Set2; parameter getters expose the validated values.

`PostfilterState::process(input, new_params)` accepts one internal-rate channel
of 120/240/480/960 samples and returns a `FilteredFrame` through `samples()`.
The first 120 values transition old to current parameters. Longer frames then
transition current to newly decoded parameters before constant filtering. A
120-sample frame stores new parameters for the next call. The two parameter
generations follow upstream period clamping even when gain is zero.

Filtering retains 1026 processed samples, uses exact windowed coefficients,
different transition/constant biases and signed wrapping/clipping. Disabled
segments pass input unchanged. `parameters()` and `history()` inspect state;
`reset()` clears it. Validation precedes any mutation. Use independent instances
per output channel and apply de-emphasis after filtering. This API follows normal
decoder frame scheduling; loss and packet control belong to later orchestration.

[576 original cases](tests/celt-postfilter-fixtures.txt) compare all outputs,
every history entry and all old/current parameter fields through period/gain/tap
changes, constant/disabled filters, short/long frames, clipping, extremes and
resets. Separate checks verify short-frame delay, disabled long-frame tails and
invalid-request rollback. Five mutations detect transition/constant bias,
processed history, premature new parameters and omitted constant-filter shortcut.

```sh
python3 tools/celt_postfilter_fixtures.py target/reference/opus > /tmp/opura-celt-postfilter.txt
diff -u tests/celt-postfilter-fixtures.txt /tmp/opura-celt-postfilter.txt
```

The optional oracle extracts original scalar comb routines unchanged and applies
the original normal-decoder scheduling with the pinned standard window. The complete synthesis backend follows below; recursive bands, allocation,
packet control and conformance remain incomplete; this stage does not itself decode Opus packets.


## Complete CELT synthesis backend

`opura::celt::synthesis::CeltSynthesis::new(decimation, output_channels)` selects
fixed output rate and one/two API channels. `synthesize(spectra, energies,
controls)` takes one/two decoded normalized Q24 left/right spectra, one 21-band
Q24 energy array per encoded channel, and `SynthesisControls` for frame size,
selected bands, transient/silence flags and new postfilter parameters. Spectra
must exactly match the 120/240/480/960-bin frame size. This API starts after
entropy, allocation and recursive spectral decoding; it accepts no Opus packets.

The backend combines spectral scaling, reference mono/stereo projection, long
or interleaved-short inverse MDCTs, synthesis clipping, delayed comb filtering
and final de-emphasis/decimation. Stereo to mono halves each spectrum before
addition; mono to stereo preserves each output channel's existing histories.
Clipping occurs after all MDCT blocks without clipping pending overlap. Silence
zeros new spectra while allowing overlap/filter continuation. Returned signed-16
`PcmFrame` values are final for this classic synthesis path.

All channel, overlap, postfilter and de-emphasis changes commit atomically.
`reset()` retains output geometry. Read-only `overlap(channel)`,
`postfilter(channel)` and `deemphasis_memory()` expose diagnostic state;
`channels()` and `sample_rate()` report configuration. Invalid channels, energy
counts, spectral lengths and band ranges preserve every history.

[480 original complete-backend cases](tests/celt-synthesis-fixtures.txt) compare
final PCM, both MDCT overlaps, every postfilter history entry/parameter generation
and both de-emphasis memories. They cover every rate/frame/encoded/API channel
and long/transient combination, channel changes, selected/Hybrid high bands,
silence, gain/period/tap switches, clipping and resets. The oracle composes
original unchanged celt_synthesis/comb/de-emphasis with original standard mode,
MDCT and FFT. Five mutations detect downmix rounding, transient stride/layout,
lost overlap, omitted synthesis clipping and skipped postfilter integration.

```sh
python3 tools/celt_synthesis_fixtures.py target/reference/opus > /tmp/opura-celt-synthesis.txt
diff -u tests/celt-synthesis-fixtures.txt /tmp/opura-celt-synthesis.txt
```

Encoded CELT frame parsing/allocation/recursive bands, loss recovery, Hybrid
mixing and official full Opus conformance remain incomplete. This stage evidence
is not a complete codec or a competitive-performance result.


## CELT frame controls and TF resolution

`opura::celt::control::decode_header(packet, frame_index)` accepts a CELT-only
packet frame and returns immutable `FramePrefix` plus its positioned borrowed
entropy decoder. The prefix exposes size, silence, typed postfilter parameters,
transient/short-block count and intra-energy flag. This wrapper does not decode
coarse energies or the rest of the compressed audio.

`decode_prefix(entropy, size, start)` supports a correctly positioned cursor for
CELT start0 or Hybrid start17 after SILK. Payloads must contain 2..1275 bytes;
empty/one-byte loss handling is separate. A cursor already beyond its physical
budget rejects. Original cached-tell guards decide field presence. Silence
advances bit accounting to the physical end without changing normalized range
or reading more bytes; accounting errors leave entropy unchanged.

After coarse energies, call `decode_tf(entropy, size, bands, transient)` for a
half-open interval within 0..21. Its `TfResolution::values()` exposes 21 signed
resolution values, zero outside selected bands. Differential flags, first/later
probabilities, reserved select bit and conditional select consumption follow
upstream. `decode_spread(entropy)` then reads the spreading decision or returns
Normal when fewer than four whole bits remain. Allocation follows later.
Every control operation is transactional, bounded and allocation-free.

[Reference control sequences](tests/celt-control-fixtures.txt) compare all values
and range/tell/tell_frac on 3360 prefix→original coarse-energy→TF→spread cases.
They cover all sizes/start contexts/channels, positioned range/raw cursors,
2..1275-byte budgets, silence, near-exhaustion and all postfilter octave/pitch/
gain/tap boundaries. Another 1824 standalone TF cases target exact remaining-bit
boundaries, every LM/transient model and selected/empty intervals. Tests exhaust
all 65536 two-byte values at all four sizes and both starts, verify invalid
rollback/packet errors and silence-accounting overflow, and detect six mutations
in silence accounting, pitch, TF reservation/aggregation/select and spread fallback.

```sh
python3 tools/celt_control_fixtures.py target/reference/opus > /tmp/opura-celt-control.txt
diff -u tests/celt-control-fixtures.txt /tmp/opura-celt-control.txt
python3 tools/celt_control_fixtures.py target/reference/opus --tables > /tmp/opura-celt-control-tables.rs
rustfmt --edition 2024 /tmp/opura-celt-control-tables.rs
diff -u src/celt/control_tables.rs /tmp/opura-celt-control-tables.rs
```

The oracle extracts original prefix/spread decision blocks and `tf_decode`
unchanged, with original entropy/coarse-energy routines and tables. These stages
are not full compressed CELT or Hybrid decoding; allocation, recursive bands,
loss recovery and official complete Opus conformance remain incomplete.


## CELT bit allocation

`opura::celt::allocation::decode_allocation(entropy, size, layout, transient)`
runs after spreading and before fine-energy/shape decoding. The validated
`BandLayout` must select at least one band; payloads contain 2..1275 bytes and
an already over-budget cursor rejects. Standard caps, dynamic boosts, trim,
anti-collapse reservation, six-step curve interpolation, skipped bands,
intensity/dual stereo and fine/PVQ/excess redistribution follow original rate.c.
Every operation uses fixed storage and commits entropy only after success.

The immutable `Allocation` retains frame/layout/transient context. Getters expose
all 21 caps/boost offsets, PVQ budgets in **eighth bits** (not pulse counts),
fine bits/priorities, trim, intensity boundary, dual flag, carry balance and the
exclusive coded-band endpoint. `fine_bits()` and `fine_priority()` feed existing
energy refinement APIs. `anti_collapse_reserved()` marks the one raw bit to read
after shapes. Selected-band outputs are zero outside the interval; standard caps
are defined for all bands. Fine energy is zero through eight bits per channel.

[16296 original allocation cases](tests/celt-allocation-fixtures.txt) match every
array/scalar plus range/tell/tell_frac, and the following original fine-energy
values/counters. They cover all rates' shared internal frame models, encoded
channels, transient/long contexts, 2..1275-byte budgets, fresh/positioned cursors,
boosted/skipped/capped bands, every trim and stereo mode. Full-prefix cases first
traverse original prefix/coarse/TF/spread. Additional cases exhaust all 231
nonempty band intervals at each LM/channel/transient context. Tests independently
check budget conservation and fine limits, malformed rollback and 512 bounded
adversarial intervals. Six mutations detect boost quantum, protected skip band,
dual-bit refund, two-coefficient fine offset, balance and anti-collapse reserve.

```sh
python3 tools/celt_allocation_fixtures.py target/reference/opus > /tmp/opura-celt-allocation.txt
diff -u tests/celt-allocation-fixtures.txt /tmp/opura-celt-allocation.txt
python3 tools/celt_allocation_fixtures.py target/reference/opus --tables > /tmp/opura-celt-allocation-tables.rs
rustfmt --edition 2024 /tmp/opura-celt-allocation-tables.rs
diff -u src/celt/allocation_tables.rs /tmp/opura-celt-allocation-tables.rs
```

The optional oracle uses the unchanged decoder allocation block, init_caps,
rate.c, entropy and fine-energy routines with the pinned original standard mode.
Recursive spectral partitions/folding/stereo assembly, anti-collapse and complete
compressed CELT/Hybrid decoding remain future work.


## CELT band arithmetic and transforms

`opura::celt::bands` provides `split_gains` (Q15 cosine/sine), Q11 `log2_tan`,
`haar`, `deinterleave`/`interleave`, legacy `renormalize` and `stereo_merge`.
These are fixed-storage primitives for recursive spectral decoding. Coded theta
accepts endpoints 0/16384 and interior 64..16320: classic qn<=256 never produces
smaller interior values, where upstream cosine arithmetic is undefined.
Log-ratio inputs must be positive i16 values. Vector lengths are 1..176;
permutation strides 1/2/4/8/16 must divide the length, with Hadamard stride>=2.
Haar uses a power-of-two stride and even per-stride length. Products truncate
before sums, using the original single-precision-derived coefficient.

Renormalization accepts nonnegative Q31 gain, rounded Q14 coordinates fitting
i16 and one plus squared energy fitting positive i32. Stereo merge accepts
matching Q24 vectors, each squared norm <=2 Q48 units, and nonnegative Q31 mid
gain. It preserves the original near-cancellation fallback. Validation/scalar
errors leave all input vectors unchanged; successful operations follow original
fixed-point rounding, truncation and wrapping exactly.

[Original band evidence](tests/celt-band-fixtures.txt) covers all 16259 supported
coded angles, all 1..256 sine/cosine pairs plus wide Q15 boundaries (68644 pairs),
every supported permutation/Haar geometry (1531 cases), 420 folded/noise/edge
normalizations and 400 stereo merges including cancellation. Tests also check
permutation inversion, invalid domains/rollback and seven targeted mutations.

```sh
python3 tools/celt_band_fixtures.py target/reference/opus > /tmp/opura-celt-bands.txt
diff -u tests/celt-band-fixtures.txt /tmp/opura-celt-bands.txt
python3 tools/celt_band_fixtures.py target/reference/opus --tables > /tmp/opura-celt-band-tables.rs
rustfmt --edition 2024 /tmp/opura-celt-band-tables.rs
diff -u src/celt/band_tables.rs /tmp/opura-celt-band-tables.rs
```

The optional oracle extracts the original band/vq functions unchanged and uses
pinned original scalar math. Recursive partition entropy, folding/stereo band
assembly, anti-collapse, complete CELT/Hybrid decoding and conformance follow.
