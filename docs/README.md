# Opura documentation

## Use Opura

| Task | Where to start |
| --- | --- |
| Add the library and encode or decode a packet | [Library quick start](../README.md#use-the-library) |
| Reduce memory in servers with many simultaneous speech streams | [Compact decoder measurements](benchmarks/compact-speech-0.2.11.md) |
| Convert WAV and Ogg Opus files | [Audio command](audio-cli.md) |
| Use channel mappings, output gain or surround encoding | [Multistream API](validation/multistream.md) and [surround encoding](validation/surround-encoder.md) |
| Check supported configurations and compare performance | [Codec readiness](validation/codec-readiness.md) |
| Review a version or migration | [Changelog](../CHANGELOG.md) |

Generate the API reference from the repository root with
`cargo doc --offline --locked --no-deps --open`. Start with
`decoder::OpusDecoder`, `encoder::OpusEncoder` or `Packet` for borrowed frame
slices. For a lost packet, `conceal_samples` requests concealment. If the next
packet is available, `decode_fec` can recover the missing interval before the
normal call to `decode` for the received packet.

## Development and verification

The repository pins Rust 1.87.0. With that toolchain, rustfmt and Clippy
installed, run the same checks as CI from the repository root:

```sh
cargo fmt --all -- --check
cargo test --offline --locked
cargo test --offline --locked --release
cargo clippy --offline --locked --all-targets -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --offline --locked --no-deps
python3 tools/check_dependencies.py
python3 -m unittest discover -s tools -p 'test_*.py'
python3 tools/check_package.py
```

The package check rebuilds the CLI and API documentation
from the distribution archive and runs the README example in a separate application.
The Rust regression suite reads the fixtures from a full repository checkout. Regenerating reference data or running
the full official conformance matrix requires external inputs. The conformance
guide gives the reference revision, vector archive and commands.
[Conformance procedure](validation/decoder-0.1.40.md) · [v0.2.5 results](validation/results/decoder-pvq-0.2.5-conformance.json)

## Design and history

- [Pure Rust and zero dependencies](decisions/0001-pure-rust-zero-dependencies.md), [decoder-first development](decisions/0002-decoder-first.md) and [surround Q24 precision](decisions/0003-surround-q24-precision.md) record accepted design decisions.
- [Validation reports](validation/) describe codec behavior and reference comparisons. [Benchmarks](benchmarks/) record workloads, environments, raw measurements and limits.
- Historical implementation notes cover [file workflows and packet framing](history/files-and-packets.md), [SILK decoding](history/silk-decoder.md), [CELT decoding](history/celt-decoder.md), [Hybrid and unified decoding](history/unified-decoder.md) and [range and CELT encoding](history/celt-encoder.md). Their examples and descriptions of pending work refer to the individual milestones.
- [Annotated version tags](https://github.com/ekhodzitsky/opura/tags) match Cargo versions and changelog entries. Before 1.0, incompatible API changes increment the minor version. Compatible additions and fixes increment the patch version.
