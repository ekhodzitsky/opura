# Codec readiness

The v0.2.5 library encodes and decodes classic Opus packets in safe Rust. The
tables below identify the version and workload behind each measurement. Commands
for building and running the tests are in the [development guide](../README.md#development-and-verification).

## Supported workflows

| Workflow | Coverage and evidence |
| --- | --- |
| Decode SILK, CELT and Hybrid to signed 16-bit PCM | The v0.2.5 decoder passed all 12 official RFC 8251 streams at five output rates and two channel layouts. All 120 PCM hashes matched v0.2.4, and all encoded nonzero final ranges were verified. [Conformance results](results/decoder-pvq-0.2.5-conformance.json) |
| Recover from loss and switch modes | Reference checks include 1,680 full decoder operations covering gain, long packets, concealment, in-band FEC, transitions and reset. Error tests check rollback and retry. [v0.2.5 correctness report](results/decoder-pvq-0.2.5-correctness.json) |
| Encode mono or stereo PCM | The shared encoder supports five applications and all five input rates. Checks cover cross-mode transitions, packets from 2.5 to 120 ms, bitrate and capacity controls, DTX and in-band FEC. [Applications](encoder-applications.md) · [Long packets](encoder-long-packets.md) · [Budgets](encoder-budgets.md) |
| Use multistream and surround | The library supports channel mapping, output gain and self-delimited framing. Native comparisons cover 7,405 generic encoder operations. Canonical surround checks cover 6,760 receiver operations and include coupled streams and LFE. [Multistream](multistream.md) · [Surround](surround-encoder.md) |
| Convert WAV and Ogg Opus files | File tests include 2,160 independently decoded files across channel counts, rates, durations and rate controls. They check channel order, gain, pre-skip and final duration. [Mapped file validation](mapped-audio-files.md) |

The CLI supports one Ogg Opus logical stream with 1–8 channels and mapping
family 0, 1 or 255. Chained and multiplexed Ogg streams are unsupported. The
packet library expects the caller to handle transport framing, including RTP.
The crate requires the Rust standard library.

## Performance and quality

| Measurement | Result and scope |
| --- | --- |
| Decoder vs. libopus | On the official streams, v0.2.3 took 1.5936 times the decode time of pinned scalar fixed-point libopus 1.6.1 across ten output configurations (95% interval 1.5522–1.6209). All ten favored libopus. [Protocol and samples](../benchmarks/decoder-0.2.3.md) |
| Later decoder work | In separate paired comparisons, v0.2.4 reduced aggregate decode time by 2.0% against v0.2.3, and v0.2.5 by 4.5% against v0.2.4. Each trial used its immediate predecessor as baseline. [v0.2.4](../benchmarks/decoder-staging-0.2.4.md) · [v0.2.5](../benchmarks/decoder-pvq-0.2.5.md) |
| Decoder allocations | Across 1,003,750 normal packet calls per implementation, the v0.2.5 probe observed zero calls to the watched allocator entry points for both Opura and scalar libopus. Peak stack, heap and resident memory were outside the measurement. [Allocation observations](../benchmarks/decoder-allocations-0.2.5.md) |
| Encoder vs. libopus | The v0.1.96 in-process mono/stereo baseline took 1.244 times the scalar reference's encoding time across 60 configurations. [Protocol and samples](../benchmarks/encoder-0.1.96.md) |
| Encoder quality | Reference decoding and recording checks establish interoperability. The available speech and music waveform measurements cover two recordings. A representative listening comparison at matched bitrate and latency remains open. [Recording checks](celt-quality.md) · [CELT prediction and loss controls](celt-controls.md) |

The CELT and surround validation reports identify the Q24 arithmetic correction
in the encoder reference where it applies. The reference decoder used for
interoperability checks is unmodified. [Q24 decision](../decisions/0003-surround-q24-precision.md)

## Deployment evidence still needed

CI runs the pinned Rust toolchain on Ubuntu. Broader platform coverage, resource
measurements on malformed input, representative encoder listening tests and
comparisons with optimized SIMD implementations remain open. Application-specific
latency and memory budgets need measurements on the deployment target.
[CI configuration](../../.github/workflows/ci.yml) · [Comparison criteria](../benchmarks/competitive-landscape.md)

The rollback and inverse MDCT experiments labelled 0.2.6 in benchmark filenames
did not reach a release. Their individual reports preserve the results and
reproduction steps. [Benchmark reports](../benchmarks/) · [Release history](../../CHANGELOG.md)
