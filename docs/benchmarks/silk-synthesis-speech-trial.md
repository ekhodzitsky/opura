# SILK synthesis on 768 active speech streams

Two implementations of the SILK short-term linear predictor were compared separately with v0.2.8. The transposed predictor cut CPU time on 768 speech streams by **14.2%** (paired 95% interval **11.6%–16.6%**). The direct fixed-order predictor had a CPU time ratio of 1.003. Neither candidate met the predeclared deadline limit in every run.

## Transposed LPC

The transposed implementation carries each past sample's contributions to future output samples in a short array. It keeps the original per-product fixed-point shift, wrapping sum and final saturation. This candidate was selected after an exploratory replay without real-time pacing, then tested in a new set of 18 pairs using the original workload, baseline binary and thresholds. The patched synthesis file has SHA-256 `cc8b0e46325985f5a6117655d15017d4ba5d0211c8f50fa386f352279a2e765c`. All 3,101 original packets matched the reference PCM and final ranges for both arms. [Transposed patch](silk-synthesis-transposed.patch) · [All 36 transposed comparison runs](results/silk-synthesis-transposed-speech-trial.json)

| Metric | v0.2.8 | Transposed LPC |
| --- | ---: | ---: |
| Median CPU utilization | 59.04% | 49.69% |
| Median peak RSS, MiB | 30.11 | 30.12 |
| Median run-level p99 completion latency, ms | 1.34 | 1.23 |
| Worst run-level p99 completion latency, ms | 334.16 | 263.37 |
| Packets missing 20 ms / total | 70,577 / 1,105,920 | 14,107 / 1,105,920 |
| Runs with a missed deadline | 4 / 18 | 1 / 18 |

The paired candidate-to-baseline CPU time ratio was 0.8584 (95% interval 0.8336–0.8838). Median utilization improved by 9.35 percentage points of one core. The candidate delivered every packet on time in 17 trials. In trial 16 it missed 14,107 of 61,440 packet deadlines (22.96%), above the 0.1% limit. The baseline missed 4,621 deadlines in that same pair. An unrelated process was observed on the measured CPU's SMT sibling during the session. Other host activity was uncontrolled, and the counters cannot attribute a specific late packet to an external process.

All 120 official rate, channel and vector combinations reproduced the 0.2.8 PCM hashes, and all 200,750 nonzero final ranges matched. An independent scalar regression test exercises valid 10- and 16-tap filters with extreme signed inputs and compares the complete working history, including saturation and wrapping arithmetic. [Transposed conformance results](../validation/results/silk-lpc-transposed-trial.json)

### Try the prototype in a Rust application

The pilot commit contains the transposed decoder and its scalar equivalence test. Use a pinned Git dependency to compare it with the published `0.2.8` crate on the deployment's workload. The crate API is the same. [Pilot commit](https://github.com/ekhodzitsky/opura/commit/ff19c2d090ab8d0f5bb0154e49bc7f6a111bb4bd)

```toml
[dependencies]
opura = { git = "https://github.com/ekhodzitsky/opura.git", rev = "ff19c2d090ab8d0f5bb0154e49bc7f6a111bb4bd" }
```

## Direct fixed-order LPC

The fixed-order LPC candidate required 1.0029 times the CPU time of v0.2.8 (paired 95% interval 0.9572–1.0529). Across 18 pairs, median worker utilization was 56.9% of a core for v0.2.8 and 57.5% for the candidate. Both produced bit-identical PCM and final ranges on all 15 reference recordings. The candidate missed the CPU and deadline criteria at this load. [All 36 worker runs](results/silk-synthesis-speech-trial.json)

| Metric | v0.2.8 | Fixed-order LPC candidate |
| --- | ---: | ---: |
| Median CPU utilization | 56.87% | 57.50% |
| Median peak RSS, MiB | 30.11 | 30.12 |
| Median run-level p99 completion latency, ms | 1.75 | 1.23 |
| Worst run-level p99 completion latency, ms | 48.50 | 97.84 |
| Packets missing 20 ms / total | 11,496 / 1,105,920 | 23,493 / 1,105,920 |
| Runs with a missed deadline | 3 / 18 | 3 / 18 |

The candidate picks the 10- or 16-tap LPC synthesis kernel once per subframe, reverses the coefficient order for forward reads of the sample history, and scales the output in a separate loop. Its measured synthesis file has SHA-256 `844d05a969792962c61f5ef2409c6b34ce46703f70d044ab86d1984654d4712e`. The baseline file has SHA-256 `55d61f5b8828abc1f57c18673c1dd55c5e547a13328f22e4a3f6239e65b1066a`.

### Default x86-64 build of the direct predictor

A secondary six-pair check used the same worker and packet corpus with rustc 1.87.0, default x86-64 code generation and logical CPU 6. Across the six runs, the candidate used 9.40 CPU seconds versus 10.43 for the baseline (9.9% less). Of 368,640 timed packets per arm, 48,611 missed their deadlines with the candidate and 315,547 with the baseline. The compiler version, target tuning and logical CPU all differed from the primary comparison. To repeat this build, use `+1.87.0` and unset `RUSTFLAGS` in the steps below. The receipt records the seed and arm order of every pair. [Secondary measurements and build identities](results/silk-synthesis-generic-speech-trial.json)

The fixed-order candidate also reproduced the released PCM hashes in all 120 official vector, rate and channel combinations and matched all 200,750 nonzero final-range checks. The SILK synthesis, packet-loss concealment, recovery and FEC regression tests passed. [Official vector results](../validation/results/silk-lpc-fixed-order-trial.json)

## Method

The worker decoded 16 kHz mono Golos speech to float PCM, with one independent decoder and output buffer per stream. Each of the 768 streams received 80 consecutive 20 ms packets after warmup. Arrivals were staggered uniformly. The 18 pairs reused audio-offset seeds 7, 71 and 113 in order, six times each. Baseline ran first in nine pairs and candidate first in nine. Each process ran on logical CPU 2 of a shared AMD Ryzen AI 9 HX 370 with the performance governor, on x86-64 Linux 7.0.0-42-generic. Both builds used rustc 1.100.0, release settings, the same Cargo project path and `-C target-cpu=native`. Each pair offered identical work, 61,440 decoded packets per arm. Measurements exclude setup and warmup. [Workload and reference corpus](active-speech-capacity-0.2.8.md#workload-and-measurements)

Before timing, the driver compared all packet-stream checksums and both workers' PCM and final-range digests with the pinned libopus 1.6.1 reference on all 3,101 original packets from 15 recordings. For the CPU comparison, the driver took user plus system CPU ticks during the timed interval. Each pair contributed one candidate-to-baseline ratio. The reported interval uses the 2.5% and 97.5% percentiles of 20,000 bootstrap samples of the paired log ratios, exponentiated. CPU utilization divides worker CPU time by measured wall time. A packet completed more than 20 ms after its scheduled arrival counts as late.

The criteria set before any timings required the upper 95% bound for CPU time to be below 1.00, with identical reference output. Each candidate run had to meet the existing 0.1% missed-deadline and 20 ms p99 limits. Median candidate p99 could exceed baseline by at most 1 ms and median peak RSS by at most 1 MiB. The run used a shared workstation, short 1.6-second windows and 15 recordings. CPU, latency and memory observations apply to this worker and host.

## Reproduce

Prepare the packets using the original active-speech runner with `--qualify-only`. The commands below expect its output in `/tmp/opura-active-speech`. Build the same driver twice, keeping its package version, absolute source path, Rust flags and lockfile fixed. Apply the archived patch to a copy of v0.2.8 before building the candidate. Run the commands from an Opura checkout containing this report. [Corpus preparation](active-speech-capacity-0.2.8.md#reproduce) · [Paired driver](../../tools/benchmarks/active-speech/compare.py) · [Measured candidate patch](silk-synthesis-fixed-order.patch)

```sh
repo="$PWD"
bench="$(mktemp -d)"
mkdir -p "$bench/codec" "$bench/driver/src"
git archive v0.2.8 src Cargo.toml README.md | tar -x -C "$bench/codec"
cp /tmp/opura-active-speech/Cargo.toml "$bench/driver/"
cp tools/benchmarks/active-speech/worker.rs "$bench/driver/src/main.rs"
cat >> "$bench/driver/Cargo.toml" <<'TOML'

[patch.crates-io]
opura = { path = "../codec" }
TOML
cd "$bench/driver"
export RUSTFLAGS="-C target-cpu=native"
cargo +1.100.0 generate-lockfile --offline
cargo +1.100.0 build --offline --locked --release --features opura-build
cp target/release/active-speech-capacity "$bench/baseline"
# Choose the transposed predictor or silk-synthesis-fixed-order.patch.
git -C "$bench/codec" apply "$repo/docs/benchmarks/silk-synthesis-transposed.patch"
cargo +1.100.0 build --offline --locked --release --features opura-build
cp target/release/active-speech-capacity "$bench/candidate"
python3 "$repo/tools/benchmarks/active-speech/compare.py" \
    --baseline "$bench/baseline" --candidate "$bench/candidate" \
    --packets /tmp/opura-active-speech/packets --cpu 2 --out "$bench/results.json"
```

## RustPBX media worker

The direct fixed-order candidate also ran on 500 independent RustPBX Opus-to-PCMU egress legs on logical CPU 0. Across 12 balanced pairs the geometric mean CPU time ratio was **0.9015** (paired 95% bootstrap interval **0.8598–0.9365**). Median CPU use fell from **0.403 to 0.366 of one core**, with median peak RSS of **46.51 MiB** for both. The builds produced identical PCMU checksums for all 44,000 measured output frames per process. Thirteen of 1,056 baseline batches and 15 of 1,056 candidate batches exceeded 20 ms. Both processes ran on a shared host. The test covers one-way media processing through the PCMU encoder. [Measurements](results/silk-lpc-rustpbx-trial.json) · [Media-path workload](rustpbx-transcode-0.2.8.md)

The transposed filter ran in a separate set of 12 pairs against the same baseline executable. It used **5.3% less CPU time** (ratio **0.9469**, paired 95% interval **0.9062–0.9762**). Median peak RSS was 46.42 MiB for baseline and 46.22 MiB for the candidate. Over 1,056 batches per arm, 21 baseline and nine candidate batches exceeded 20 ms. All output checksums matched. Each trial used its own contemporary baseline, so the two candidate percentages measure separate comparisons. [Transposed media worker results](results/silk-transposed-rustpbx-trial.json)

To reproduce, prepare the same qualified packets and the pinned RustPBX worker using the linked media-path guide. With rustc 1.100.0 and the default release profile, build that worker twice from the same directories. Give the runner a local Opura path dependency, build the 0.2.8 source, then apply the candidate patch to that source and rebuild. Save the resulting executables as `baseline` and `candidate` in one working directory. The command below writes all 24 runs to a new `samples.jsonl` file. The 95% interval resamples the 12 paired CPU-time ratios with replacement 10,000 times (seed 104) and takes the 2.5th and 97.5th percentiles of their geometric means. [Paired runner](../../tools/benchmarks/rustpbx-transcode/compare.py)

```sh
python3 tools/benchmarks/rustpbx-transcode/compare.py "$WORK" "$PACKETS" 0
```
