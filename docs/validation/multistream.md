# Classic multistream framing and decoder validation

The library now provides `multistream::ChannelMapping`, `MultistreamPacket`,
`MultistreamDecoder` and `MultistreamPcm`. This is the decoder portion of ongoing
multistream integration. Multistream encoding and mapped WAV/Ogg file workflows
are still missing; the existing CLI does not gain multichannel support from
these library additions. There is no new speed or perceptual quality claim.

## Contracts

A layout owns at most255 map entries. Channels and streams are1..255,
`coupled <= streams` and `streams + coupled <=255`. Coupled streams come first,
with two coded channels each, followed by mono streams. Decoder maps may repeat
or omit coded channels;255 produces an exactly silent output. The separate
encoder-map validation requires an input occurrence of every coded channel;
repeated inputs select the first occurrence. That validation is not an encoder.

`Packet::parse_self_delimited` returns borrowed frames and the consumed prefix,
including that subpacket's trailing padding. The first streams use explicit
last-frame sizes; the final stream uses ordinary externally delimited framing.
CBR repeats the explicit size for every frame. All total durations must match,
although individual TOCs and frame counts may differ. A whole multistream
packet is limited to16MiB including padding; each stream still has at most48
frames,1275 bytes/frame and120ms. Existing ordinary parsing is unchanged.

The repacketizer can write canonical self-delimited packets with optional exact
padded length. Destination sizing includes the last-frame size before any
mutation. A failed write preserves the complete destination and borrowed state.

The persistent decoder owns independent SILK/CELT histories on bounded heap
storage. PCM is interleaved in output-map order. Every stream is decoded, even
when no output uses it; public final range XORs all constituent final ranges.
Normal decode, explicit packet loss and FEC commit the whole owner together.
Any framing or later audio error preserves every history, duration and range.
Reset retains the map, rate and signed Q8 dB gain.

Gain follows the original fixed-point frame ordering, rounding and saturation.
It also applies to PLC reused during mode-transition fades. Applying gain once
to a completed packet would miss that ordering. The default zero gain preserves
the original output path. No floating-point gain approximation is used.

## Independent native evidence

The optional generators compile original Opus1.6.1 revision
`22244de5a79bd1d6d623c32e72bf1954b56235be` externally. The checkout must be clean;
no native source is linked or vendored into Cargo or ordinary tests.

- `tests/multistream-framing-fixtures.txt` records13,328 native canonical bytes
  and consumed offsets across all TOCs, mono/stereo signalling, CBR/VBR, frame
  counts through48, zero and1275-byte frames,252-byte length boundaries and
  padding boundaries. Ordinary Rust tests match every output fingerprint,
  borrowed payload, consumed offset and untouched destination suffix.
- `tests/multistream-decoder-fixtures.txt` records560 public native decoder
  operations over eight layouts and five output rates. The layouts include
  mono/coupled streams, repeated/omitted channels, entirely silent output and
  the eight-channel Vorbis order. Inputs come from the original generic
  multistream encoder; this does not establish surround-encoder equivalence.
  Normal, PLC, FEC, reset and gain changes match full PCM16LE fingerprints,
  samples and public final ranges. Native diagnostics observe72 SILK,90 Hybrid
  and54 CELT stream packets, including36 streams with LBRR on FEC steps.
- Nine additional Rust regressions cover hand-inspectable framing, malformed
  lengths/counts/padding, resource/map limits, constituent PCM mapping, gain
  retention, whole-owner late-error rollback and successful resume. They
  supplement independent expected outputs rather than replace them.
- Four isolated mutations are detected by running assertions: removing gain
  rounding, replacing range XOR, prematurely committing a stream and returning
  a wrong consumed length. The original sources are restored after each probe.

Regeneration requires Python, a C compiler and that external pinned checkout:

```sh
python3 tools/multistream_framing_fixtures.py /path/to/opus /new/path/framing.txt
python3 tools/multistream_decoder_fixtures.py /path/to/opus /new/path/decoder.txt
cargo test --offline --locked --test multistream --test multistream_native
```

Generator output paths must be new. Compare successful generated files with the
tracked fixtures; a partial file from a failed generator is not accepted evidence.
The decoder generator requires actual mode and LBRR coverage. Native active
FEC/loss keeps VoIP in SILK even at high rates, so its later CELT phase explicitly
disables FEC/loss and selects music. Enabled controls alone are not coverage.

All seven required offline checks pass, including162 existing library tests,
11 new integration tests and30 Python tool tests. Native regeneration remains
optional; ordinary tests use the tracked synthetic fixtures.
The full codec goal remains open, including encoder/file integration and broader
compatibility, quality and competitive performance acceptance.
