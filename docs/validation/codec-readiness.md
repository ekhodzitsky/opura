# Codec readiness

The v0.2.8 library encodes and decodes classic Opus packets in safe Rust. The
tables below identify the version and workload behind each measurement. Commands
for building and running the tests are in the [development guide](../README.md#development-and-verification).

## Supported workflows

| Workflow | Coverage and evidence |
| --- | --- |
| Decode SILK, CELT and Hybrid to signed 16-bit PCM | The v0.2.8 decoder passed all 12 official RFC 8251 streams at five output rates and two channel layouts. All 120 PCM hashes matched v0.2.7 and all 200,750 nonzero final-range checks passed. [Conformance results](results/decoder-theta-0.2.8-conformance.json) |
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
| Decoder vs. opus-rs and system libopus | At 16 kHz mono and 48 kHz stereo, v0.2.7 passed 24/24 official stream checks and opus-rs 0.1.42 passed 23/24. On the common passing streams Opura took 1.41× and 1.27× the opus-rs decode time respectively on one x86-64 machine. Additional warmed decoder states used about 34 KiB of private dirty memory for Opura and 176 KiB for opus-rs. [Timings and memory](../benchmarks/decoder-competition-0.2.7.md) |
| Speech recognition in GigaSTT | At four inference threads, opus-rs 0.1.41 and the transposed Opura pilot used 5.11 and 5.12 CPU-seconds per minute of speech, respectively. Two threads with opus-rs used 3.94, with p95 latency rising from 123 to 188 ms. All 450 HTTP transcripts matched across configurations. [Cost per minute](../benchmarks/gigastt-silk-pilot.md) · [Earlier CLI and HTTP comparison](../benchmarks/gigastt-decoder-0.2.7.md) |
| Voice conferencing | With 64 participants in VoiceMCU, v0.2.8 cut median server RSS from 26.9 to 17.8 MiB against native libopus, using 14% more CPU. With a 16 MiB memory limit it kept all 64 connected through the test in three of three runs. [Server measurements](../benchmarks/voicemcu-0.2.8.md) |
| Phone call transcoding | At 500 one-way RustPBX Opus-to-PCMU legs, worker RSS fell from 121.4 to 46.5 MiB against opus-rs. Opura missed four of 440 batch deadlines. Both decoders met every deadline at 250 legs. [RustPBX media path](../benchmarks/rustpbx-transcode-0.2.8.md) · [Eight bidirectional SIP calls](../benchmarks/rustpbx-call-0.2.8.md) |
| SILK speech optimization trial | At 768 speech streams the transposed LPC prototype used 14.2% less CPU time than 0.2.8 (paired 95% interval 11.6–16.6%) with similar RSS. One of 18 candidate runs missed the 20 ms deadline. The direct fixed-order candidate had a CPU-time ratio of 1.003. [Measurements and deadlines](../benchmarks/silk-synthesis-speech-trial.md) |
| Active speech decoder instances | At 768 staggered 20 ms streams, v0.2.8 saved 106.3 MiB of peak worker RSS against opus-rs and used an additional 15.6 percentage points of one core. Both met every packet deadline in the three runs. [Capacity and limits](../benchmarks/active-speech-capacity-0.2.8.md) |
| Decoder vs. libopus | On 12 official streams, v0.2.5 used 1.4619 times the decode time of scalar fixed-point libopus 1.6.1 across ten configurations (paired 95% interval 1.4563–1.4757). The interval favored libopus in all ten cases. [Protocol and all 300 samples](../benchmarks/decoder-native-0.2.5.md) |
| Decoder changes between releases | In separate paired comparisons, v0.2.4 reduced aggregate decode time by 2.0% against v0.2.3, and v0.2.5 by 4.5% against v0.2.4. Each trial used its immediate predecessor as baseline. [v0.2.4](../benchmarks/decoder-staging-0.2.4.md) · [v0.2.5](../benchmarks/decoder-pvq-0.2.5.md) |
| Current decoder vs. opus-rs | At 16 kHz mono and 48 kHz stereo, v0.2.8 took 1.306× and 1.181× the opus-rs 0.1.42 time on the shared qualifying streams. Opura passed all 24 stream/output checks. [Direct comparison](../benchmarks/decoder-theta-0.2.8.md#direct-comparison-with-opus-rs-and-system-libopus) |
| CELT band preparation | Full decoder time ratio against v0.2.8 was 0.9879 (paired 95% interval 0.9752–0.9981). The candidate missed the performance gate. The released implementation remains v0.2.8. [Results](../benchmarks/decoder-band-return-trial.md) |
| Direct CELT angle decoding | The candidate took 0.9345 times the v0.2.7 decoder time across ten output configurations (paired 95% interval 0.9263–0.9536). The candidate met both acceptance criteria. [Results](../benchmarks/decoder-theta-0.2.8.md) |
| CELT pulse-storage experiment | The candidate used 0.9600 times the v0.2.6 decoder time on the official streams (paired 95% interval 0.9535–0.9688). The trial met both acceptance criteria. [Results](../benchmarks/decoder-pulse-storage-0.2.7.md) |
| Decoder CPU profile | On 8 kHz mono and 48 kHz stereo, memory copies accounted for 19.6% and 17.4% of Opura CPU samples. The full decoder clone and CELT pulse reconstruction were frequent callers. [Sample counts and scope](../benchmarks/decoder-cpu-0.2.5.md) |
| Decoder allocations | Across 1,003,750 normal packet calls per implementation, the v0.2.5 probe observed zero calls to the watched allocator entry points for both Opura and scalar libopus. Peak stack, heap and resident memory were outside the measurement. [Allocation observations](../benchmarks/decoder-allocations-0.2.5.md) |
| Encoder vs. libopus | The v0.1.96 in-process mono/stereo baseline took 1.244 times the scalar reference's encoding time across 60 configurations. [Protocol and samples](../benchmarks/encoder-0.1.96.md) |
| Encoder quality | Reference decoding and recording checks establish interoperability. The available speech and music waveform measurements cover two recordings. A representative listening comparison at matched bitrate and latency remains open. [Recording checks](celt-quality.md) · [CELT prediction and loss controls](celt-controls.md) |

The CELT and surround validation reports identify the Q24 arithmetic correction
in the encoder reference where it applies. The reference decoder used for
interoperability checks is unmodified. [Q24 decision](../decisions/0003-surround-q24-precision.md)

## Deployment evidence still needed

CI runs the pinned Rust toolchain on Ubuntu x86-64 and the release tests on
native Linux ARM64. The ARM64 runner passed all 446 release tests. ARM64
performance, resource measurements on malformed input and representative
encoder listening scores remain open. Application-specific
latency and memory budgets need measurements on the deployment target.
[Native ARM64 test run](https://github.com/ekhodzitsky/opura/actions/runs/45643287573/job/139307379775) · [CI configuration](../../.github/workflows/ci.yml) · [Comparison criteria](../benchmarks/competitive-landscape.md)

The rollback and inverse MDCT experiments labelled 0.2.6 in benchmark filenames
did not reach a release. Their individual reports preserve the results and
reproduction steps. [Benchmark reports](../benchmarks/) · [Release history](../../CHANGELOG.md)
