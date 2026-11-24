# Hybrid and unified decoder implementation

Historical notes from the milestones leading to v0.2.5. Commands assume the
repository root. For the integrated codec, see the [current support and evidence](../validation/codec-readiness.md).

## Normal Hybrid packets

`hybrid::HybridDecoder` decodes normal Hybrid packets with10/20ms frames,
encoded mono/stereo, independent mono/stereo PCM output and all five standard
output rates. The SILK low band runs at16kHz and shares one range decoder with
CELT bands17..19 (SWB) or17..21 (FB). Normal in-band LBRR is consumed before the
primary audio. Multi-frame packets support up to120ms; errors preserve both
layers and all synthesis/resampler histories.

```rust
use opura::{hybrid::HybridDecoder, silk::{decoder::PcmChannels, resampler::OutputRate}};
let mut decoder = HybridDecoder::new(OutputRate::Khz48, PcmChannels::Stereo)?;
// let pcm = decoder.decode(transport_packet)?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

Signed16 output follows the reference's canonical fixed-point `SIG2RES` then
`ADD_RES`: CELT rounds/saturates to signed16 before the layer sum saturates.
Read-only `celt()` exposes energies, overlap, postfilter and de-emphasis histories.
`reset()` retains output geometry and native CELT start/end configuration.
Empty/one-byte loss/DTX frames, explicit loss and FEC are also supported below.
Redundant CELT transition tails still require unified transition reconstruction.

`tests/hybrid-packet-fixtures.txt` contains960 public fixed-point libopus packet
results and independent complete CELT snapshots. It covers all output rates,
wire/API channel combinations, SWB/FB and10/20ms changes, silence, tones, noise,
impulses, clipping, normal LBRR, padding and up to12 frames/120ms. Packet output
and final state also match decoding the same frames separately. Retained deep
entropy failures verify rollback after SILK and after an earlier valid frame;
512 deterministic corrupt packets check bounds and atomicity.

Regenerate the optional oracle with the pinned clean external reference:

```sh
python3 tools/celt_packet_fixtures.py /path/to/opus-v1.6.1 --hybrid > tests/hybrid-packet-fixtures.txt
```

No C implementation is linked by Cargo. The generator retries boundedly when a
rapid encoder20→10ms change emits its one-byte budget-fallback PLC frame; this
corpus records normal packets only. PCM/range and state come from unchanged
original decoding routines. These subset results do not establish full Opus
conformance or competitive end-to-end performance.


## Hybrid loss, DTX and FEC

`HybridDecoder::conceal_samples(duration)` accepts120..5760 samples in120-sample
steps on a48kHz clock. Before the first audio frame, output is silent and layer
histories remain untouched. Empty/one-byte frames use the current TOC clock and
previous bandwidth/channel configuration. Explicit loss follows the last packet
clock, splitting irregular intervals into20/10/5/2.5ms pieces. SILK always advances
at least10ms per piece; its output is truncated for shorter intervals while CELT
conceals the exact high-band duration using background noise. Normal recovery
retains both layers' concealment/energy/resampler histories.

`decode_fec(packet)` reconstructs the preceding interval of the first compressed
frame only. SILK decodes available LBRR or conceals gaps; CELT conceals high bands
at the received bandwidth. The received primary packet can then be decoded with
`decode(packet)`. FEC final range reports the persistent CELT noise RNG, whereas
explicit/implicit loss reports zero. Whole packet, FEC and explicit loss calls
preserve all state on errors. Cross-mode FEC/transition tails remain pending.

`tests/hybrid-loss-fixtures.txt` matches2130 original public PCM/range results
and full CELT synthesis/PLC snapshots across all rates/channels, short/long
bursts, initial FEC, LBRR gaps, irregular duration, reset and normal recovery.
It retains a stereo PLC regression: original SILK `SMULBB` truncates tiny-gain
LTP normalization scales to signed16. Negative central coefficients and the
resulting noise balance are intentional reference arithmetic. CNG now accepts
its full nonnegative signed16 PLC noise-scale domain (0..32767);192 additional
original CNG PCM/full-state cases cover above-unity scales, leaving the previous
192-case corpus unchanged.

```sh
python3 tools/celt_packet_fixtures.py /path/to/opus-v1.6.1 --hybrid --loss > tests/hybrid-loss-fixtures.txt
python3 tools/silk_cng_fixtures.py /path/to/opus-v1.6.1 --wide-scale > tests/silk-cng-wide-fixtures.txt
```

Thirteen targeted mutations are detected. Additional tests cover128 corrupt
normal/FEC/loss/recovery sequences, invalid
loss durations, reset/initial-silence state and first-frame-only FEC. Complete
Opus mode orchestration, official conformance and encoding still follow.


## Unified Opus decoder

`decoder::OpusDecoder` owns one shared SILK/CELT stream state across all standard
coding modes. Output rate and channel count stay fixed; encoded bandwidth,
channels, mode and frame duration can change on each packet. The implementation
uses safe Rust and fixed storage, with no heap allocation or Cargo dependencies.

```rust
use opura::{decoder::OpusDecoder, silk::{decoder::PcmChannels, resampler::OutputRate}};
let mut decoder = OpusDecoder::new(OutputRate::Khz48, PcmChannels::Stereo)?;
// let pcm = decoder.decode(transport_packet)?;
let missing = decoder.conceal_samples(960)?; //20ms on the48kHz clock
assert_eq!(missing.samples_per_channel(), 960);
# Ok::<(), Box<dyn std::error::Error>>(())
```

`decode(packet)` accepts externally delimited packets up to120ms, including
empty/one-byte compressed frames (PLC/DTX). `conceal_samples(duration)` accepts
120..5760 samples in120-sample steps on a48kHz clock, independent of output rate.
`decode_fec(packet, duration)` recovers the requested missing interval: earlier
samples use PLC, then the first compressed frame supplies available LBRR; CELT
or unavailable/too-short FEC falls back to PLC. Decode the received packet
normally afterwards for its primary audio. Every complete operation rolls back
both layer histories, packet configuration and diagnostics on errors.

Transition handling includes both5ms CELT redundancy directions, physical
primary entropy-storage shrinking, native mode-dependent reset ordering,
previous-mode PLC,2.5ms squared-window fades, Hybrid-to-SILK high-band fade-out
and signed16 saturating layer accumulation. PLC follows the last packet clock;
SILK sub-10ms requests advance10ms of state and return only the needed prefix.
Final range uses the last frame plus redundant-tail XOR, reports persistent CELT
RNG during Hybrid FEC and zero for data loss. Read-only methods expose CELT
histories, previous mode/redundancy and last duration; native reset retains CELT
band limits and output geometry.

`tests/opus-packet-fixtures.txt` records2440 public pinned libopus PCM/range
results with complete independent CELT synthesis/PLC state and outer context.
It covers all three modes, standard frame/bandwidth and wire/API channel
combinations at8/12/16/24/48kHz, both redundancy directions (including stale tails),
transitions without redundancy, mixed empty/one-byte frames, irregular losses,
FEC prefixes/fallback, reset and packets up to48 frames/120ms. The same unified
API also matches the prior3002 CELT/Hybrid PLC/FEC cases unchanged. Retained deep
and late corrupt-frame failures verify rollback after a mode transition and
FEC prefix PLC;512 deterministic corrupt sequences exercise bounds and atomicity.
Twenty-two targeted mutations are detected.

```sh
python3 tools/celt_packet_fixtures.py /path/to/opus-v1.6.1 --mixed > tests/opus-packet-fixtures.txt
```

The optional oracle builds external unchanged scalar fixed-point DSP routines
in a temporary directory. Read-only observers capture state and transition
branches; no reference implementation is linked by Cargo. These sampled exact
results are complemented by the official conformance result below; full decoder
[benchmark results are recorded separately](../benchmarks/decoder-0.1.41.md). Containers,
multistream/self-delimiting framing, QEXT, neural/deep PLC and encoding are not
implemented by this API.


## Official decoder conformance

Opura0.1.40 passes all120 official RFC8251 combinations:12 streams at all five
standard rates with mono/stereo output,200750 packet decodes and200750 exact
encoded final-range checks. The unchanged original `opus_compare` accepts an
official PCM reference for every case. Two complete runs reproduced identical
PCM hashes and metric results; corrupted-range and zero-PCM probes are rejected.

See the [validation report](../validation/decoder-0.1.40.md), its checked JSON
results and reproduction command. This conformance result does not establish
performance or encoder-quality leadership. Containers, multistream and encoding
remain separate work. The optional runner uses only Python/Rust standard
libraries and an external original comparator; Cargo still has zero dependencies.

## Complete decoder performance baseline

The [0.1.41 benchmark](../benchmarks/decoder-0.1.41.md) compares public packet-to-PCM
decoding with pinned scalar fixed-point libopus over all official streams and
all ten rate/channel configurations. All PCM/range checks agree. Opura takes
about 1.54–1.80 times the reference time and uses more decoder state; the speed
target fails. The report preserves every result and reproduction command.
SIMD builds, other competitors and encoder quality remain unmeasured.
