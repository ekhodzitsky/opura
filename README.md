# Opura

Opura is an Opus encoder and decoder written in safe Rust with zero Cargo
dependencies. Use the library to work with Opus packets and signed 16-bit PCM,
or the `opura` command to convert between WAV and Ogg Opus files.

## Use the library

Opura requires Rust 1.87 or later and the standard library. With access to
the repository, add the tagged version to your application's `Cargo.toml`:

```toml
[dependencies]
opura = { git = "https://github.com/ekhodzitsky/opura.git", tag = "v0.2.5" }
```

This example encodes one 20 ms stereo frame and decodes it at 48 kHz. Keep the
encoder and decoder alive across packets so prediction and recovery work across
frame boundaries.

```rust
use opura::{
    celt::{input::InputRate, FrameSize},
    decoder::OpusDecoder,
    encoder::{EncodeOptions, OpusEncoder},
    silk::{decoder::PcmChannels, resampler::OutputRate},
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut encoder = OpusEncoder::new(InputRate::Khz48, 2)?;
    let mut decoder = OpusDecoder::new(OutputRate::Khz48, PcmChannels::Stereo)?;

    let pcm = [0_i16; 960 * 2]; // Interleaved left and right samples.
    let options = EncodeOptions { bitrate: 64_000, ..Default::default() };
    let packet = encoder.encode(FrameSize::Ms20, &pcm, 320, options)?;
    let decoded = decoder.decode(packet.as_bytes())?;
    assert_eq!(decoded.samples_per_channel(), 960);
    Ok(())
}
```

Pass one complete Opus packet to `decode`. If your input comes from RTP or an
Ogg container, extract the packet first. The decoder supports SILK, CELT and
Hybrid at 8, 12, 16, 24 and 48 kHz, including packet loss concealment and
in-band forward error correction. The `multistream` module handles mapped
channels and surround layouts.

## Convert audio files

From the repository root with Rust 1.87.0 installed, run:

```sh
cargo build --release --offline --locked --bin opura
target/release/opura encode input.wav output.opus --mode auto --bitrate 64000
target/release/opura decode output.opus decoded.wav
```

Encoding accepts PCM16 WAV at the five standard rates. Use `--mode auto` for
automatic mode selection and 1–8 channels. The default encoding mode is CELT.
Decoding accepts one Ogg Opus logical stream and writes 48 kHz PCM16 WAV.
Choose new output paths for both commands. [File formats and options](docs/audio-cli.md)

## Validation and releases

The v0.2.5 decoder passed all 120 RFC 8251 vector, rate and channel combinations.
Encoder interoperability has been checked against libopus. Representative
listening tests and broader platform benchmarks remain open.
[Validation results and limitations](docs/validation/codec-readiness.md)

Releases are identified by annotated Git tags. Opura is available from this
repository, and crates.io publication is disabled.
[Changelog](CHANGELOG.md) · [Documentation and development](docs/README.md) · [Third-party notices](THIRD_PARTY_NOTICES.md)
