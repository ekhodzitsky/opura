# opura

Opura is a pure Rust, zero-dependency Opus implementation under development.
**Mono/stereo SILK packets support final PCM output, explicit loss concealment and in-band FEC.**
Packet framing, range decoding and the SILK channel primitives are also available.
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
`sqrt`, `rsqrt_normalized`, `cos_norm`, `log2`, `exp2`, `reciprocal`,
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
