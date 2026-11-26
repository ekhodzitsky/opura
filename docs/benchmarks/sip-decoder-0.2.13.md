# Decoder CPU on Hybrid SIP speech

With the RustPBX compiler and release settings (Rust 1.98.1, optimization level 2), Opura 0.2.13 used **3.77% less decoder CPU** than 0.2.12 on a 32-second Mandarin SIP stream and **4.49% less** on an independent English meeting excerpt. Both tests decoded Hybrid fullband packets to mono 48 kHz PCM with `CompactOpusDecoder::decode_into`. On the Mandarin stream, the descriptive 95% interval for the saving was 2.12–5.11%. The processor was a Ryzen AI 9 HX 370. [Per-process timings, input fingerprints and profiles](results/sip-decoder-0.2.13.json)

| Stream and measured API | 0.2.12 CPU ms/pass | 0.2.13 CPU ms/pass | Paired CPU change (95% interval) |
| --- | ---: | ---: | ---: |
| Mandarin, decoder | 65.88 | 62.40 | −3.77% (−5.11% to −2.12%) |
| AMI, decoder | 74.93 | 72.00 | −4.49% (−6.48% to −2.81%) |
| Golos clip 00, decoder | 3.84 | 3.59 | −7.29% (−10.15% to −4.49%) |
| Mandarin, RustPBX audio-codec adapter | 66.17 | 65.49 | −1.27% (−4.44% to +1.85%) |

Times are medians over fresh processes. The percentage compares the two arms within each paired round. The adapter measurement includes its buffer handling and sample conversion. Its interval crosses parity. These measurements do not establish a speedup for the complete SIP server.

A second experiment used the minimum supported Rust version (1.87) and Cargo’s standard release profile (optimization level 3). In 12 paired rounds per recording, decoder CPU changed by +1.05% on Mandarin (95% interval −1.22% to +3.41%) and −4.14% on AMI (−7.90% to −0.68%). Only the AMI saving reproduced under both configurations.

## Where the cycles went

The primary stream contained 1,600 mono 20 ms Hybrid fullband packets. In the sampled 0.2.12 decoder, 141 of 1,142 executable samples landed in the SILK resampler filter and another 65 in its initial upsampling stage. Other major costs included the CELT FFT, SILK synthesis and CELT inverse MDCT. An initial comparison on identical packets gave 66.66 ms median CPU per call for the 0.2.12 decoder and 48.66 ms for RustPBX's pinned opus-rs 0.1.32. Rounded PCM output differs between the libraries on this input. [Full-server baseline and pinned versions](rustpbx-call-0.2.12.md)

The low-frequency part of this Hybrid stream is reconstructed at 16 kHz. When the requested output is 48 kHz, each input sample generates three outputs. After the first upsampling stage, the interpolation coefficients follow three phases in order, 0, 8 and 4. Version 0.2.13 uses that sequence directly during each filter batch. The same sequence applies to 8 → 24 kHz. This preserves the fixed-point products, rounding, clipping and persistent state without allocating a new buffer.

## Inputs and verification

The historical call measurements stored RTP headers. To obtain the payloads, the same pinned sipbot repeated the public Mandarin speech fixture with the same negotiated Opus settings on loopback. Two fresh transmissions produced identical payload files; the byte count also matched the historical calls. The independent AMI input came from seconds 345–377 of meeting ES2002a. Each SIP stream contained 1,600 packets. A SILK-only control used the first Golos file in the existing replay corpus. [AMI source and licence](compact-speech-0.2.11.md#reproduce) · [Golos fixture preparation](active-speech-capacity-0.2.8.md#reproduce)

Both public APIs passed the same 120 RFC 8251 vector, rate and channel combinations as v0.2.12. All PCM hashes matched the previous release, with 200,750 final-range checks per API.

On both SIP streams and all 15 Golos recordings, both Opura decoder APIs reproduced all 6,048,960 release PCM samples, and the final entropy range matched libopus 1.6.1 after each of 6,301 packets. Exact old/new PCM equivalence and comparison with native libopus are separate checks. [Input and output verification](results/sip-decoder-0.2.13.json)

## Reproduction

Extract the [replay and input preparation bundle](../../tools/benchmarks/rustpbx-call/decoder-0.2.13-reproduce.tar.gz). Follow `input/REPRODUCE.md` for the two SIP captures. The processing benchmark uses preloaded, length-prefixed packets and excludes file I/O and network traffic from each timing. Each pass creates a fresh decoder for the call and keeps its state across packets.

In the bundle's `bench` directory, build the three workers with Rust 1.98.1 and the official audio-codec 0.4.6 crate archive. Use `python3 build.py ARM ARCHIVE NEW_DIRECTORY` for `stock` and `base`. For `candidate`, also pass `--candidate-source` with the Opura v0.2.13 checkout. Run `python3 run.py PHASE PACKET_FILE` for the five phases in `protocol.json`. The primary and adapter phases use Mandarin, with separate AMI Hybrid and Golos SILK controls. The compiler and optimization level match the RustPBX study. The runner pins the worker to one logical CPU, warms it once, measures thread CPU and bootstraps whole mirrored blocks to retain the pairing.

For the Rust 1.87 check, run `python3 build.py base` and `python3 build.py candidate /path/to/v0.2.13` from the bundle’s `msrv` directory with the published 0.2.12 crate cached. Run `python3 run.py msrv_mandarin PACKET_FILE` and `python3 run.py msrv_ami PACKET_FILE` with their respective captures. Both runners use Linux logical CPU 4.
