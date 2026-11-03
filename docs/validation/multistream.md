# Classic multistream framing and decoder validation

The library provides `multistream::ChannelMapping`, `MultistreamPacket`,
`MultistreamDecoder`, `MultistreamPcm` and persistent generic encoding through
`MultistreamEncoder`/`MultistreamEncodeOptions`. Mapped WAV/Ogg file workflows
and surround energy-mask/LFE analysis remain unfinished; the existing CLI does
not gain multichannel support from these library additions. Mono/stereo file
decoding now honors signed header gain. There is no new speed or quality claim.

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


## Generic encoder validation

The encoder accepts complete mapped interleaved PCM and an inclusive whole-packet
byte cap. Total bitrate is positive and bounded to191250000bps, then normalized
to500..750000bps per mapped input channel as in the original generic API.
Allocation accounts for coupled and mono coded channels. Common application,
signal, bandwidth, loss/FEC/DTX, complexity and rate-mode controls pass to each
persistent owner. Every coded channel needs an input occurrence; repeated input
entries use the first and silent slots are ignored. A later stream/control or
framing failure preserves all histories. Successful packets XOR every stream's
final range and expose the nominal rates passed to constituent APIs.

CBR rounds the total target to whole bytes, reserves remaining streams and
self-delimited size fields, adjusts the final stream's nominal budget and pads
only the final transport subpacket to the total.100ms packets reserve the extra
frame-count bytes even for tiny-budget PLC. The cap is bounded to16MiB. The
underlying library now accepts long capacities through7662 bytes, including the
complete six-frame length header; the mono/stereo CLI retains its separate cap
until mapped file integration is complete.

`tools/multistream_rate_fixtures.py` observes the unchanged native generic
allocation kernel through temporary observation wrappers. Its3240 cases cover
nine durations, five rates, eight maps through255 channels and nine requested
rates per geometry. Rust matches normalized totals, complete allocation arrays
and sums in `tests/multistream-rate-fixtures.txt`.

`examples/multistream_encode_vectors.rs` exports229 predeclared cases and7405
operations, including1..8 channels, a repeated/silent map, five input rates, all
five applications, all packet durations and CBR/VBR/CVBR. Separate8s voice
profiles include actual quiet spans, dropped packets, PLC and FEC recovery.
`tools/multistream_encoder_interop.py` decodes all packets with unchanged native
multistream decoding, matching every sample count, final range and full PCM
fingerprint. It rejects corrupted expected ranges/PCM and requires each exact
case/control schedule, actual suppression and LBRR on FEC steps. It does not
require unsupported byte or quality identity with the pristine native encoder.
The report records the source fingerprint and dirty-worktree provenance.

```sh
python3 tools/multistream_rate_fixtures.py /path/to/opus /new/path/rates.txt
python3 tools/multistream_encoder_interop.py /path/to/opus /new/path/interop
cargo test --offline --locked --test multistream_encoder
```

Three encoder regressions also exercise all input rates/applications, ignored
and repeated input slots, exact CBR budgets, every duration, large final padding,
actual later-stream control failure and successful resume. A separate18-case
CLI integration checks signed gain for existing SILK/CELT/Hybrid files against
the frame decoder after exact pre-skip/end trimming. These cover wiring and
budget semantics; independent native fixtures cover gain arithmetic and PCM.

The encoder/file-gain integration passes all seven offline gates with 163 library
and 33 Python tests. Four additional running-assertion mutations detect wrong
rate offsets, last-occurrence input selection, partial state commits and CBR
rounding. The sources are restored; targeted tests and fresh independent native
validation pass again. Both full exports are byte-identical. The
[source-fingerprinted report](results/multistream-generic-encoder.json) records
7405 actual operations, with 49 FEC-step LBRR streams and 3108 suppressed frames.
The earlier seven-gate statement above records the decoder-only checkpoint.
