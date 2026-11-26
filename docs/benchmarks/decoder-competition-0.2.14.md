# Decoder comparison on Opura 0.2.14

This comparison measures the released Opura compact and fixed decoders against
seven other implementation builds on AMD Ryzen AI 9 HX 370, Linux
x86-64. It covers classic signed-16-bit Opus PCM. The results distinguish
official conformance, real-recording quality checks, single-stream CPU and
packet latency, and memory retained across active streams.

[Full measurements](results/decoder-competition-0.2.14.json) ·
[Reproduction bundle](../../tools/benchmarks/competitor/decoder-competition-0.2.14-reproduce.tar.gz)

## Implementations and correctness

All Rust workers used Rust 1.100.0, default Cargo release and crate features,
with `-C target-cpu=native`. The native arm used the unmodified system
libopus 1.6.1 and runtime CPU dispatch. All adapters produce interleaved
signed PCM16 in a caller-owned buffer. Float-only APIs convert and saturate
to PCM16 inside the timed region. The ruopus adapter uses its public owned
PCM16 return and copies into the shared caller buffer. The opurs arm is a
pinned development revision (manifest version 0.3.0, commit
`f0f8941dcc1836a4564cb12f0d3fbe4c0b0c5a6a`); it has no published crate or
version tag at the time of testing.

Each entry in the rate columns covers 12 RFC 8251 vectors at two output
channel counts. Both official reference variants were accepted. A run
qualifies if it produces the complete expected sample count, passes the PCM
metric against at least one reference, and matches the encoded nonzero final
range where available. All measured implementations expose final range.

| Decoder | 8 kHz | 12 kHz | 16 kHz | 24 kHz | 48 kHz | Official total | Real recordings |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Opura 0.2.14 compact | 24 | 24 | 24 | 24 | 24 | 120/120 | 12/12 |
| Opura 0.2.14 fixed | 24 | 24 | 24 | 24 | 24 | 120/120 | 12/12 |
| libopus 1.6.1 | 24 | 24 | 24 | 24 | 24 | 120/120 | 12/12 |
| opurs development | 24 | 24 | 24 | 24 | 24 | 120/120 | 12/12 |
| ropus 0.12.18 | 24 | 24 | 24 | 24 | 24 | 120/120 | 12/12 |
| ruopus 0.1.2 | 24 | 24 | 24 | 24 | 24 | 120/120 | 12/12 |
| opus-rs 0.1.42 | 13 | 15 | 23 | 22 | 22 | 95/120 | 12/12 |
| rusty-opus 1.2.1 | 19 | 18 | 18 | 18 | 18 | 91/120 | 12/12 |
| opus-pure 0.2.5 | 2 | 4 | 8 | 12 | 24 | 50/120 | 11/12 |

All nine arms completed the official packet/sample counts. The first eight
matched all 200,750 nonzero final-range checks. The 70 failing opus-pure
configurations included 38 PCM-only failures and 32 with both PCM and range
failures. Its total of 216 range mismatches counts repeated configurations,
with mismatches on streams 08, 09, 10 and 12 at four lower rates. The
25 opus-rs and 29 rusty-opus failures were PCM metric failures. Rusty-opus
passed all 60 stereo configurations. Independent packet-by-packet checks
located a mono output discontinuity when encoded SILK channels switch on
vectors 02 and 04. On vector02, comparison against libopus at 48 kHz mono
was exact until packet 602, where the maximum difference reached 947 LSB.
The remaining failing cases were not diagnosed individually.

The real recordings were Golos speech clip 00 (201 SILK packets), Mandarin
SIP speech and independent AMI speech (1,600 Hybrid fullband packets each),
and the Xiph music excerpt (501 CELT packets). Each ran at 16 kHz mono,
48 kHz mono and 48 kHz stereo. The real-recording metric compares against
same-output-channel native libopus PCM with both phase inversion settings,
using the standard opus_compare metric. Mono reference PCM is duplicated in
the two-channel reference file format. This was corrected before timing:
comparing direct mono music with direct stereo native PCM rejected libopus
itself. The additional phase reference avoids rejecting a valid CELT
phase option. The official references were unchanged. The real corpus
is not an official conformance suite.

## CPU and packet latency on recordings

Each cell reports median CPU milliseconds per second of decoded audio and
median per-run p99 wall-clock microseconds per packet. Lower values are
better. CPU runs use a preloaded stream and include decoder creation per
pass, one untimed warmup and 2–10 measured repetitions. Separate latency
runs use one measured pass and time each packet with `Instant`. All timed
cases are independently qualified above, with fresh processes pinned to CPU
4. The four real inputs run in seven independently randomized rounds.
The official timing subset comprises all 12 vectors at 16 kHz mono and 48
kHz stereo in five rounds.

| Decoder | SILK 16k mono | SILK 48k mono | Hybrid Mandarin 48k mono | Hybrid AMI 48k mono | CELT music 48k stereo |
| --- | ---: | ---: | ---: | ---: | ---: |
| Opura compact | 0.647 / 17.1 | 0.771 / 19.7 | 1.635 / 48.4 | 1.928 / 51.8 | 2.435 / 63.9 |
| Opura fixed | 0.697 / 17.8 | 0.842 / 22.0 | 1.544 / 48.0 | 1.854 / 46.7 | 2.342 / 61.1 |
| libopus | 1.217 / 27.7 | 1.588 / 36.1 | 2.026 / 56.9 | 2.321 / 60.9 | 1.788 / 49.7 |
| opurs | 0.622 / 16.4 | 0.844 / 20.6 | 1.497 / 42.1 | 1.824 / 46.1 | 1.908 / 47.3 |
| ropus | 0.619 / 17.3 | 0.896 / 24.7 | 1.553 / 47.4 | 1.797 / 43.8 | 2.006 / 49.8 |
| ruopus | 0.473 / 11.6 | 0.718 / 17.0 | 1.165 / 33.9 | 1.432 / 38.8 | 1.643 / 42.8 |
| opus-rs | 0.647 / 17.1 | 1.020 / 25.9 | 1.609 / 45.0 | 1.906 / 50.2 | 1.976 / 50.1 |
| rusty-opus | 0.621 / 16.0 | 0.870 / 21.2 | 1.422 / 40.5 | 1.729 / 46.0 | 1.890 / 51.5 |
| opus-pure | 0.596 / 14.6 | 0.820 / 19.5 | 1.327 / 37.9 | 1.625 / 42.4 | 1.626 / 39.5 |

Ruopus used 29.04% less paired CPU than compact Opura on Mandarin Hybrid
(95% descriptive interval 28.63–29.36%) and 26.29% less on AMI Hybrid
(25.75–26.98%). It used 34.16% less on the stereo music clip (32.55–36.17%).
Opus-pure was similarly fast on the music clip but failed the lower-rate
official cases described above. On the Golos 48 kHz mono SILK clip Opura's
median CPU was 0.771 ms per second of audio; ruopus used 0.718. The system
libopus build was slower than Opura on these speech clips, but faster on
the music clip. These observations apply to this hardware and build.

## Official stream CPU

Each row compares a competitor against compact Opura over the same set of
qualifying vectors for that output geometry. Ratios use the geometric mean
of the five within-round ratios of summed CPU time per pass. Values below
one favor the competitor; the number of vectors makes the comparison scope
explicit. Descriptive percentile intervals use seeded bootstrap of rounds.

| Decoder | 16 kHz mono ratio (vectors) | 48 kHz stereo ratio (vectors) |
| --- | ---: | ---: |
| Opura compact | 1.000 (12) | 1.000 (12) |
| Opura fixed | 0.947 (12) | 0.969 (12) |
| libopus | 0.766 (12) | 0.869 (12) |
| opurs | 0.812 (12) | 0.823 (12) |
| ropus | 0.827 (12) | 0.882 (12) |
| ruopus | 0.689 (12) | 0.728 (12) |
| opus-rs | 0.790 (12) | 0.924 (11) |
| rusty-opus | 0.888 (6) | 0.835 (12) |
| opus-pure | 0.786 (4) | 0.740 (12) |

Ruopus used 31.10% less CPU than compact Opura across all 12 official
streams at 16 kHz mono (95% interval 30.93–31.30%) and 27.19% less at 48
kHz stereo (26.78–27.57%). Matching those results would require substantial
decoder work across both SILK and CELT. Opura's fixed decoder is also
slightly faster than its compact decoder on these full-matrix aggregates.

## Memory per active stream

Every fresh worker decodes the first 24 consecutive packets of the nominated
recording for every retained decoder state. It verifies sample and final-range
equality across independent streams, then reads `/proc/self/smaps_rollup`
while all states are live. The workers use shared caller-owned output buffers.
The marginal private dirty memory is the difference in median warm-minus-before
private dirty memory from 128 to 768 streams divided by 640. Five fresh
processes per load (1, 128 and 768) and arm were measured. Values are KiB per
stream, including allocator effects. The second value is full warm process
RSS in MiB at 768 streams.

| Decoder | SILK 16k mono | Hybrid 48k mono | CELT music 48k stereo |
| --- | ---: | ---: | ---: |
| Opura compact | 7.5 / 8.5 | 21.0 / 18.6 | 33.8 / 28.2 |
| Opura fixed | 33.7 / 28.2 | 33.7 / 28.2 | 33.7 / 28.2 |
| libopus | 188.0 / 144.7 | 188.0 / 144.7 | 196.0 / 150.6 |
| opurs | 43.5 / 36.2 | 43.5 / 36.5 | 43.5 / 36.5 |
| ropus | 69.6 / 55.5 | 69.6 / 55.5 | 78.2 / 62.0 |
| ruopus | 26.6 / 23.8 | 27.1 / 24.8 | 35.6 / 31.2 |
| opus-rs | 174.8 / 134.7 | 174.8 / 134.8 | 174.8 / 134.8 |
| rusty-opus | 166.0 / 128.1 | 166.0 / 128.2 | 281.1 / 214.5 |
| opus-pure | 147.5 / 114.1 | 150.0 / 116.1 | 221.8 / 170.0 |

Opura had the lowest retained memory among the tested implementations.
Compared with ruopus, the compact decoder's marginal private dirty memory
was 71.9% lower for SILK, 22.6% lower for Hybrid and 5.2% lower for stereo
CELT. The fixed decoder retained slightly less private dirty memory per
stereo stream than its compact counterpart. These are standalone process
measurements, with no service/network buffers or scheduling.

## Next decoder targets

The source audit of ruopus 0.1.2 identifies several differences worth
profiling. Its default feature set reaches a cached real FFT plan backed
by rustfft through spectrograms, enabling architecture-specific SIMD.
Its CELT synthesis uses floating-point arithmetic and a single per-channel
decode history used for synthesis, postfiltering and loss concealment.
Opura uses fixed-point arithmetic with separate postfilter and concealment
histories, transactional staging and no external dependencies. On the SILK
path, ruopus also updates decoded state directly. These structural differences
are hypotheses for the CPU gap; they do not quantify causality. [Pinned ruopus source](https://github.com/jmg049/ruopus/tree/b7aab670bce04eaa10731b152e9834b89a21cde3)

A targeted next experiment should profile both implementations on the
qualified SILK and Hybrid recordings, then evaluate a small safe change
to remove redundant history or entropy-state copying while preserving
transactional behavior and PCM. CELT FFT and inverse MDCT remain priority
hotspots, but previous tested variants missed the performance acceptance
gate. The benchmark does not establish the fastest implementation across
all hardware, rates, codecs, or deployment conditions.

## Reproduction

The linked archive contains all nine adapters, their locked dependencies,
the shared worker, exact input fingerprints, scripts and the preregistered
CPU, latency and memory schedule. Follow `REPRODUCE.md` in the extracted
archive to obtain the pinned public recording sources, the official RFC
vectors and the reference libopus source. It includes no audio or external
codec source or executable. The complete receipt retains all 1,754 CPU,
1,754 latency and 405 memory process rows, with no measured-worker errors,
and the 1,188 qualification rows. It also records the pre-timing
real-reference corrections and both PCM and final-range outcomes.
