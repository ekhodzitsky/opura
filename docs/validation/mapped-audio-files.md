# Mapped Ogg Opus and WAVE files

The CLI supports one classic Ogg Opus logical stream with1..8 output channels,
mapping families0/1/255 and signed Q8 output gain. `--mode auto` encodes
positioned or unpositioned PCM16 WAVE at all five standard rates. The codec
library remains safe Rust with zero Cargo dependencies. External reference
tools below are optional validation inputs, never build/test requirements.

## Mapping and timing contracts

Family0 retains implicit mono/stereo mapping and the original encoder path.
Family1 uses Opus/Vorbis order in OpusHead and WAVE speaker order in PCM files.
The implementation permutes the mapping passed to the multistream owner, as
described by the original public `opus_multistream.h`; samples are streamed
without a second file-wide reorder buffer. Decoding accepts repeated/omitted
coded channels and silent255 entries. Every coded stream still participates in
decoding and native final-range XOR inside the library.

| Channels | Positioned WAVE layout | Mask |
|---|---|---|
|1|Mono center|0x4|
|2|Stereo|0x3|
|3|L/R/C|0x7|
|4|L/R/back L/back R|0x33|
|5|L/R/C/back L/back R|0x37|
|6|L/R/C/LFE/back L/back R|0x3f|
|7|L/R/C/LFE/back center/side L/side R|0x70f|
|8|L/R/C/LFE/back L/back R/side L/side R|0x63f|

WAVE extensible input must contain PCM16,16 valid bits and either the canonical
mask above or zero. Zero means unpositioned channels, preserved with family255
and one mono stream per channel. Legacy PCM WAVE with more than two channels
also has no asserted speaker positions and selects family255. Unsupported
nonzero masks reject before output creation. Positioned mono/stereo output
keeps the original44-byte PCM header; other output uses68-byte WAVE extensible
headers and an explicit speaker mask, including zero for family255.

Encoding uses the existing generic mapped encoder, native generic stream rate
allocation and a whole-packet cap. It does **not** implement native surround
energy masking or LFE-specific analysis. Family1 layout metadata does not imply
surround encoder quality equivalence. The default target is64000bps per coded
stream; `--bitrate` sets the total target. `--channels` is forwarded to each
constituent stream; requesting two channels fails before creating a file if any
constituent is mono. Fixed CELT/SILK/Hybrid file modes require positioned
mono/stereo input; mapped/unpositioned files use `--mode auto`.

All packets retain the48kHz granule clock. The application selects common
lookahead120 or312; pre-skip and the exact final granule remove leading delay
and tail padding, including empty/partial source frames. Header gain is applied
by the frame decoder before trimming. No floating gain approximation is added.

Ogg pages remain bounded to255 laces/65025 body bytes. Writer continuation flags,
unknown granules on incomplete pages, zero-length terminating laces, page CRC,
monotonic granules and EOS are checked through continuation boundaries. A packet
including metadata/padding is bounded to16MiB. Output WAVE remains RIFF32 with
checked header/data sizes; input buffers hold at most5760 samples per channel.
Existing destinations are refused. I/O/audio errors may leave a new partial
output; invalid headers, speaker masks and preflight controls reject earlier.
Chained/multiplexed Ogg streams and unknown mapping families remain unsupported.

## Independent evidence

`tools/mapped_audio_fixtures.py` compiles unchanged pinned libopus1.6.1 revision
`22244de5a79bd1d6d623c32e72bf1954b56235be`. The public native surround factory
provides canonical maps and synthetic packets. FFmpeg7.0.2 independently decodes
those files; correlation against native fixed-point PCM identifies every
Vorbis-to-WAVE permutation with a required score above0.99 and margin0.1.
The script does not import/copy the Rust permutation table.

The60 captured files in `tests/audio/mapped` include canonical1..8channel maps,
family255 identity maps, repeated channels and entirely silent output under
both families1/255. Native PCM hashes cover gains0/+6/-6dB, pre-skip0/120/312,
and final trimming0/1/959 samples. Ordinary Rust integration tests decode every
file and compare exact PCM16LE hashes, lengths, speaker masks and header sizes.
The fixtures contain only deterministic synthetic encoded audio, no recordings
or foreign source. They do not establish encoder perceptual quality.

Five mapped CLI integration tests additionally exercise240 empty/partial/tail
cases across five rates,1..8 channels and positioned/unpositioned input, all
applications, long packets and rate modes. Malformed mapping headers and invalid
masks/controls/caps reject without creating output. A direct-library packet
comparison verifies WAVE input routing and complete CBR controls against maps
from the independently generated native fixtures. Binary unit tests exercise
extensible format boundaries, RIFF32 overflow,65025-byte continuation boundaries
and exact/excess16MiB packet limits. Python evidence-reader tests reject corrupt
CRC, sequence, continuation/EOS and WAVE metadata.

The optional `tools/mapped_audio_interop.py` checks actual Rust CLI output using
the original native decoder. Its predeclared2160 cases span five rates,
1..8 channels, positioned/unpositioned layouts, nine durations2.5..120ms and
CBR/VBR/CVBR, rotating supported applications. It independently reconstructs Ogg
packets, verifies CRC/granules, whole-packet CBR budgets and exact native PCM after
trim in WAVE order. Longer48kHz AUDIO/restricted-CELT cases also correlate each
decoded channel against distinct source tones; two mutually consistent decode
paths alone could miss swapped encoder input channels. This is not a quality
score. The report includes source/binary hashes and Git/worktree provenance.
Five isolated source mutations break output mapping, input mapping, gain,
pre-skip and continuation flags; tests detect each running failure, then pass
again after the original sources are restored.

```sh
python3 tools/mapped_audio_fixtures.py /path/to/opus /new/path/fixtures --ffmpeg /path/to/ffmpeg
cargo test --offline --locked --test mapped_audio_cli
cargo build --offline --locked --release --bin opura
python3 tools/mapped_audio_interop.py /path/to/opus target/release/opura /new/path/interop.json
```

Outputs must be new; failed/partial runs are not retained as successful evidence.
The0.1.98 file milestone passes all seven required offline gates, including
163 library tests,15 binary unit tests, five mapped CLI integration tests and
36 Python tool tests. The
[source-fingerprinted native file report](results/mapped-audio-files-0.1.98.json)
retains all2160 cases; dirty-worktree provenance is explicit, and the source hash
is checked again against the committed milestone.

The library milestone0.1.97's full RFC8251 report remains historical evidence for
that exact source. This file integration does not change synthesis algorithms.
The full codec goal, native surround analysis, representative quality comparisons
and competitive performance acceptance remain open.
