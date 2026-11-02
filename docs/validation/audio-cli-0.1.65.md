# Native audio command: Opura 0.1.65

The zero-dependency `opura` command now converts PCM16 WAV to adaptive CELT
Ogg Opus and decodes the supported Ogg subset through the full native decoder.
This record proves interoperability, geometry, duration and packet accounting;
it does not measure perceptual quality, speed or competitive leadership.

## Independent file checks

[Raw results](audio-cli-0.1.65.json) retain all 51 encoded cases and three
original reference files. FFmpeg 7.0.2 independently demuxes/decodes every
encoded file; FFprobe identifies Opus, rate/channels and every packet size.
The command's own decode and FFmpeg both return exactly the expected number
of 48 kHz mono/stereo samples, including empty and one-sample inputs.

The matrix covers five input rates (8/12/16/24/48 kHz), mono/stereo and all
four durations (2.5/5/10/20 ms), using a fractional 64,321 packet bits/s.
Additional cases exercise empty input, one sample, just below/on/above a
frame, exact 255/510-byte lacing boundaries, maximum 1,275/1,276-byte packets
and audio page flushing with over one second of input. The expected aggregate
packet bytes are independently calculated from the requested bitrate and
packet count, including TOC bytes and excluding Ogg overhead.

Three small synthetic reference files cover SILK mono, Hybrid stereo and CELT
stereo. The command's trimmed PCM matches unchanged fixed-point public libopus
decoder output byte for byte; FFmpeg independently verifies their geometry and
duration. Native revision, modes, pre-skip, rates and counts are recorded in
`tests/audio/manifest.json`. The original input contains 4,663 samples/channel.
These fixtures are compatibility checks, not a representative listening corpus.

## Retained checks and supported limits

Ordinary Cargo tests require no external codec, compiler or executable. They
exercise actual CLI processes across the rate/channel/duration matrix, original
PCM, malformed/truncated pages and WAV, CRC, sequence/EOS/continuation/packet
bounds, metadata bounds, unsupported header interpretation and existing-output
refusal. A shifted native stream verifies that initial granule origin does not
alter pre-skip. An impulse at sample 480 stays at sample 480 after encode/decode,
checking the encoded 120-sample delay compensation as well as sample counts.

The CLI supports one mapping-family-zero mono/stereo stream with zero header
gain. Chaining/multiplexing and nonzero gain are explicitly rejected. Packet
assembly, including metadata, is limited to 65,536 bytes; output WAV uses RIFF32.
I/O or decoding errors can leave a newly created partial output. Existing
output paths are never overwritten. The library packet API remains independent
of file containers. Normative container references are
[RFC 3533](https://www.rfc-editor.org/rfc/rfc3533.html) and
[RFC 7845](https://www.rfc-editor.org/rfc/rfc7845.html).

## Reproduction

```sh
cargo build --offline --locked --release --bin opura
python3 tools/check_audio_cli.py target/release/opura --report target/audio-cli-check.json
# Optional: unchanged clean external libopus checkout at the manifest revision.
python3 tools/audio_cli_fixtures.py target/reference/opus target/audio-fixtures-check
```

FFmpeg/FFprobe and the optional native fixture generator are validation tools,
not build or ordinary test dependencies. All seven fixture artifacts reproduce
byte for byte with the pinned reference. Real-recording quality/speed evaluation
and SILK/Hybrid encoding remain subsequent work.

All seven required offline gates pass for this version: formatting, debug and
release tests, all-target Clippy with warnings denied, rustdoc with warnings
denied, dependency-policy validation and the Python tool unit suite.
