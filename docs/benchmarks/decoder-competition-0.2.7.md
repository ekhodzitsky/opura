# Opura 0.2.7, opus-rs 0.1.42 and libopus 1.6.1

On a Ryzen AI 9 HX 370, Opura passed the official PCM checks in both tested output configurations and used about 34 KiB of additional private dirty memory per warmed decoder. The other two implementations decoded faster. A separate GigaSTT experiment measured the cost of a decoder change inside a speech recognition application. [Application results](gigastt-decoder-0.2.7.md)

## Correctness and decode time

Each implementation decoded all 12 RFC 8251 streams independently, with a fresh decoder per stream. Qualification required the expected sample count, every nonzero final range and a pass against an official PCM reference using the pinned upstream `opus_compare`. Opura and system libopus passed 24/24 configurations. The published opus-rs 0.1.42 passed 23/24. For vector 07 at 48 kHz stereo, it matched all 4,186 final ranges, but the decoded PCM failed both official references. The speed comparison uses only streams qualified by both members of each pair. [Per-stream checks and 1,065 timing observations](results/decoder-competition-0.2.7.json)

| Output | Comparator | Streams | Packets per round | Opura µs/packet | Comparator µs/packet | Paired Opura/comparator time, median (95% interval) |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| 16 kHz mono | opus-rs | 12 | 20,075 | 24.55 | 17.50 | 1.410 (1.402–1.454) |
| 16 kHz mono | libopus | 12 | 20,075 | 24.55 | 17.45 | 1.436 (1.412–1.456) |
| 48 kHz stereo | opus-rs | 11 | 15,889 | 36.09 | 28.20 | 1.266 (1.244–1.315) |
| 48 kHz stereo | libopus | 12 | 20,075 | 31.42 | 24.21 | 1.282 (1.256–1.321) |

The time columns are medians of the mean packet time across 15 rounds. The last column takes paired ratios within rounds before computing their median. The 95% intervals resample those 15 rounds. Packet-level p99 was computed within each stream and round. On the 11 common stereo streams, the median p99 by stream ranged from 36 to 245 µs for Opura and from 34 to 212 µs for opus-rs. These ranges describe clean-stream replay on one shared workstation.

Each adapter kept decoder state across packets and warmed up on one replay of the same stream. Inside each timed sample, the adapter measured individual calls to the public decoder. Timing includes production of the library's usual output type (PCM16 for Opura and native libopus, f32 for opus-rs). File I/O, decoder creation, packet-duration lookup and the f32-to-PCM16 conversion for verification ran outside the timer. A seeded shuffle varied stream and decoder order in each round. Both Rust crates used their default features, rustc 1.100.0 and `-C target-cpu=native`. The native probe dynamically linked the system amd64 libopus 1.6.1 distribution build. CPU dispatch was left enabled. The executable and library hashes are in the raw results.

## Additional live decoder memory

| Output | Opura | opus-rs | System libopus |
| --- | ---: | ---: | ---: |
| 16 kHz mono | 33.69 KiB | 175.69 KiB | 187.25 KiB |
| 48 kHz stereo | 33.69 KiB | 175.75 KiB | 195.75 KiB |

Each independent decoder processed 16 packets from each of three streams covering SILK, Hybrid and CELT. The probe kept the states alive, decoded sequentially, and read Linux `Private_Dirty` at 1, 33 and 65 states. The table reports the median of five process runs using the change from 1 to 65 divided by 64. Service RSS, per-thread stacks, output queues and concurrent requests require separate measurements. [Memory observations and build identities](results/decoder-competition-memory-0.2.7.json)

## Reproduce on Linux

Prepare the official vector files, the pinned libopus source checkout at `22244de5a79bd1d6d623c32e72bf1954b56235be`, an installed shared libopus, and cached registry releases of both Rust crates and their enabled dependencies. Set `VECTORS`, `OPUS_SOURCE`, and `LIBOPUS_SO` to those locations. Choose available logical CPU numbers for the host. The tools build their adapters in temporary directories outside the Opura crate. [Verified vector inputs](../validation/decoder-0.1.40.md#provenance-and-method)

```sh
python3 tools/benchmarks/competitor/run.py --vectors "$VECTORS" --source "$OPUS_SOURCE" \
  --libopus "$LIBOPUS_SO" --cpu 2 --out target/competition-timing.json
python3 tools/benchmarks/competitor/summarize.py target/competition-timing.json
python3 tools/benchmarks/competitor/memory.py --vectors "$VECTORS" \
  --libopus "$LIBOPUS_SO" --cpu 8 --out target/competition-memory.json
```

## Where to optimize

Streams 09 and 10 account for 36% of Opura's measured excess time relative to opus-rs in mono and 50% in stereo, summing per-stream median paired time differences. Both contain predominantly CELT packets. These two inputs were replayed at 48 kHz stereo with Opura 0.2.7 compiled with line tables and native CPU tuning. Each of four independent processes replayed its stream 160 times. The last pass reproduced its official final ranges and the PCM hash recorded in the qualification run.

| Stream | CPU sampling frequency | Total samples | In CELT `theta`, including callees | At its two CDF fill lines | In `decode_cdf` beneath `theta` |
| --- | ---: | ---: | ---: | ---: | ---: |
| 09 | 197 Hz | 2,532 | 708 | 186 | 208 |
| 09 | 211 Hz | 2,797 | 797 | 223 | 239 |
| 10 | 197 Hz | 2,855 | 548 | 164 | 156 |
| 10 | 211 Hz | 3,066 | 577 | 182 | 174 |

Building and reading the cumulative distribution in CELT's angle decoder accounted for 15.6–16.5% of CPU samples on stream 09 and 11.2–11.6% on stream 10. A focused experiment can compute the two distributions' interval boundaries directly, verify symbol and entropy state equivalence, and replay the full conformance and timing suites. A service-level follow-up would measure CPU and peak memory under concurrent streams on the target hardware. [CELT partition decoder](../../src/celt/partition.rs)
