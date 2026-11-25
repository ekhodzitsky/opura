# Audio command

Build the converter with `cargo build --release --offline --locked --bin opura`.
Run `target/release/opura --help` for the full option list.

## Encode

The input is a little-endian PCM16 WAV file at 8, 12, 16, 24 or 48 kHz.
Choose a mode based on the input and the packet duration you need.

| Mode | Input | Frame or packet duration |
| --- | --- | --- |
| `--mode auto` | 1–8 channels, automatic mode and bandwidth selection | Complete packets from 2.5 to 120 ms |
| `--mode celt` (default) | Positioned mono or stereo | 2.5, 5, 10 or 20 ms frames |
| `--mode silk` | Positioned mono or stereo | 10, 20, 40 or 60 ms frames |
| `--mode hybrid` | Positioned mono or stereo at 24 or 48 kHz | 10 or 20 ms frames |

For speech, this command requests variable-rate encoding at 32 kbit/s, silence
suppression and in-band forward error correction for an estimated 10% packet loss:

```sh
target/release/opura encode speech.wav speech.opus --mode auto --application voip --signal voice --bitrate 32000 --vbr --dtx --fec 10
```

`--bitrate` counts Opus packet bits and excludes Ogg overhead. For mapped audio
it sets the total across streams. Positioned multichannel input uses canonical
WAVE speaker masks and surround encoding. Unpositioned audio preserves channel
order using mapping family 255. [Mapped file formats](https://github.com/ekhodzitsky/opura/blob/main/docs/validation/mapped-audio-files.md)

## Decode

```sh
target/release/opura decode speech.opus decoded.wav
```

The decoder reads one Ogg Opus logical stream with 1–8 channels and mapping
family 0, 1 or 255. It applies header gain, pre-skip and the final granule to
produce a 48 kHz PCM16 WAV file with the decoded channel order and duration.
Chained or multiplexed Ogg streams are unsupported.

Both commands require a new output path. If conversion fails after writing
begins, discard the partial output. [Timing and interoperability checks](https://github.com/ekhodzitsky/opura/blob/main/docs/validation/audio-cli-0.1.65.md)
