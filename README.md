# Opura

Opura is an Opus encoder and decoder written in safe Rust with zero Cargo
dependencies. Use the library to work with Opus packets and signed 16-bit PCM,
or the `opura` command to convert between WAV and Ogg Opus files.

## Use the library

Opura requires Rust 1.87 or later and the standard library. For the crates.io
release, add the following dependency to your application's `Cargo.toml`:

```toml
[dependencies]
opura = "0.2.7"
```

For a Git dependency, use the repository URL and an available release tag,
for example `opura = { git = "https://github.com/ekhodzitsky/opura.git", tag = "v0.2.7" }`.

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

Install the CLI from the registry with `cargo install opura --locked`, or build
and run it from a source checkout:

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

The v0.2.7 decoder passed all 120 RFC 8251 vector, rate and channel combinations.
On one x86-64 machine, each additional warmed Opura decoder used about 34 KiB of
private dirty memory, compared with 176 KiB for opus-rs 0.1.42. Opus-rs decoded
the shared qualifying streams faster at both tested output settings. Encoder
interoperability has been checked against libopus.
[Decoder comparison and raw measurements](https://github.com/ekhodzitsky/opura/blob/main/docs/benchmarks/decoder-competition-0.2.7.md) ·
[Validation results and limitations in the repository](https://github.com/ekhodzitsky/opura/blob/main/docs/validation/codec-readiness.md)

The source repository contains the full fixture suite, examples and reference
tools. Run the full test suite from a checkout of the corresponding release.
[Changelog](CHANGELOG.md) · [Repository documentation](https://github.com/ekhodzitsky/opura/blob/main/docs/README.md)

## License

Original Opura code is available under the MIT License. Adapted Opus
components retain their BSD terms. Binary redistributions must carry the
applicable notices. [MIT License](LICENSE) · [Third-party notices](THIRD_PARTY_NOTICES.md)
