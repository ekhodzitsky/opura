# Decoder conformance: Opura0.1.40

Validated2026-11-02 on x86-64 Linux with Rust1.87.0 and GCC15.2.0.
The [machine-readable report](decoder-0.1.40.json) identifies the exact Rust/tool
source fingerprint, compiler, original comparator source and reference revision.

## Result

All120 cases passed:12 official streams, each decoded at8/12/16/24/48kHz with
both mono and stereo output. There were200750 packet decodes and200750 nonzero
encoded final-range checks; every range matched. No decoder DSP correction was
needed after the unified decoder milestone.

The original comparator accepted at least one official PCM reference for every
case. The minimum accepted conformance score was43.4%, for vector12 at8kHz mono;
the original comparator requires a nonnegative score. This error-tolerance score
is not a perceptual listening score or an encoder-quality comparison.

Two complete runs produced identical PCM SHA256 fingerprints, packet/range
counts and comparator results. Final live fault probes also confirmed that a
one-bit change to an encoded range fails decoding, and zeroing decoded PCM makes
both original reference comparisons fail. Three Rust example tests and seven
Python fault tests additionally cover framing/bounds, endian handling, range
checks, output/flush failures, archive hashes/paths/links/expansion limits,
metric parsing and atomic report replacement (including sync failure).

## Provenance and method

The [official Opus documentation](https://opus-codec.org/docs/#test-vectors)
links the updated archive. [RFC8251 section11](https://www.rfc-editor.org/rfc/rfc8251.html#section-11)
publishes SHA1 hashes for the12 bitstreams and24 decoded outputs, and permits
either decoded reference variant. The checked manifest is
[conformance_manifest.json](../../tools/conformance_manifest.json).

- Archive SHA256: `6b26a22f9ba87b2b836906a9bb7afec5f8e54d49553b1200382520ee6fedfa55`.
- Compressed archive:74624664 bytes; validated expanded data:126653482 bytes.
- All36 file SHA1 hashes match RFC8251. Extraction accepts only known regular
  files under the expected root, with bounded member/count/total sizes.
- Comparator: unchanged `src/opus_compare.c` from Opus1.6.1 revision
  `22244de5a79bd1d6d623c32e72bf1954b56235be`, compiled with `-O2 -std=c99 -lm`.
- Rust: pinned toolchain, ordinary release profile, host target; inherited
  `RUSTFLAGS`/`CARGO_ENCODED_RUSTFLAGS` are cleared for the optional runner build.
- Input records use big-endian packet length/range fields. PCM is streamed as
  interleaved signed16 little-endian. Each nonzero expected range is checked
  before its packet's PCM is written. Packet/range accounting is checked again
  independently in Python; the original comparator also checks sample counts.

Neither the vectors nor any C implementation is bundled as a Cargo dependency.
Reference compilation and comparison are optional external validation only.
Ordinary builds/tests remain offline, safe Rust and dependency-free.

## Reproduce

Use a clean external checkout of the pinned Opus revision and the official
archive. The archive may be downloaded separately with a size/time bound:

```sh
mkdir -p target/reference
curl --fail --location --connect-timeout 15 --max-time 300 --max-filesize 134217728 \
  https://opus-codec.org/docs/opus_testvectors-rfc8251.tar.gz \
  -o target/reference/opus_testvectors-rfc8251.tar.gz
python3 tools/check_conformance.py /path/to/opus-v1.6.1 \
  target/reference/opus_testvectors-rfc8251.tar.gz --jobs 4 \
  --report target/conformance-report.json
```

The runner tests and builds the Rust example with `--offline --locked`, verifies
the archive/files, builds the original comparator and executes the complete
matrix and live fault probes. The report is replaced atomically. Rate/channel/
vector filters are available for diagnosis; filtered reports explicitly remain
incomplete and cannot claim full conformance. The example refuses to overwrite
an existing PCM file; failed runs may leave partial output only in the temporary
validation directory.

## Limits

This establishes the official classic Opus decoder-vector result on the tested
host. It does not prove correctness for every possible packet, every target or
resource failure, nor establish performance leadership. Separate generated
streams cover PLC/DTX/FEC, transitions, malformed rollback and complete internal
histories. QEXT, neural/deep PLC/OSCE, containers, multistream/self-delimiting
framing and encoding are outside this validation. End-to-end decoder benchmarks
and encoder implementation follow separately.
