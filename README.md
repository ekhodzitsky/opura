# opura

Opura is a pure Rust, zero-dependency Opus implementation under development.
**Packet framing, range decoding, SILK headers, gain and NLSF reconstruction are
implemented.** It does not encode or decode audio yet. The accepted direction is a complete decoder
first, then an encoder, prioritizing speed and reliability. See the
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
for project constraints. Audio-layer decoding remains future work; no audio compatibility or performance
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

These are synthetic primitive tests. LPC conversion, pitch, excitation, stereo
reconstruction and PCM output remain unimplemented; no full decoder or
performance claims follow from matching these segments.

## SILK spectral parameters

`opura::silk::nlsf::NlsfState` consumes the NLSF symbols immediately after
gains. It supports order 10 (8/12 kHz SILK) and order 16 (16 kHz, including
Hybrid), reconstructs stabilized Q15 spectral frequencies, and interpolates
the first half of 20 ms frames using per-channel history. Ten-millisecond
frames do not interpolate. Default/reset and order changes disable interpolation
for the first frame. The caller must also reset on sample-rate changes that
preserve order, such as 8↔12 kHz. This stage does not produce LPC coefficients.

The decoder uses fixed storage without allocation. Failure leaves spectral
history and entropy unchanged. Like gains, this primitive must participate in
a future frame-level transaction before its history is used for another frame.
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
