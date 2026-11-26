"""Reproduce the compact CPU comparison with the existing Linux x86-64 worker."""
import argparse
import hashlib
import itertools
import json
import math
from pathlib import Path
import random
import statistics
import subprocess

import run as replay


ARMS = ("fixed", "compact", "candidate")
CLIPS = ("ES2002a", "IS1000a", "TS3003a")
RESULTS = replay.RECEIPT.with_name("compact-speech-0.2.12.json")


def schedule():
    permutations = list(itertools.permutations(ARMS))
    random.Random(117).shuffle(permutations)
    counts = replay.COUNTS
    for cycle in range(18):
        start = cycle % len(counts)
        for count in counts[start:] + counts[:start]:
            order = permutations[(cycle % 6 + counts.index(count)) % 6]
            shift = (cycle % 6 + cycle // 6) % 3 if count == 1 else 0
            yield cycle, count, shift, order


def paired_interval(ratios, trials, count):
    groups = {}
    for ratio, row in zip(ratios, trials):
        groups.setdefault(row["first_clip"], []).append(ratio)
    rng = random.Random(117 + count)
    medians = []
    for _ in range(20_000):
        sample = []
        for clip in sorted(groups):
            group = groups[clip]
            sample.extend(rng.choices(group, k=len(group)))
        medians.append(statistics.median(sample))
    medians.sort()
    # Linear interpolation between adjacent sample quantiles.
    limits = []
    for probability in (0.025, 0.975):
        index = (len(medians) - 1) * probability
        lo = math.floor(index)
        hi = math.ceil(index)
        limits.append(medians[lo] + (index - lo) * (medians[hi] - medians[lo]))
    return limits


def run(args):
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=False)
    source = json.loads(replay.RECEIPT.read_text())
    replay.prepare_inputs(args, source, out / "inputs")
    inputs = [out]
    for shift in (1, 2):
        cwd = out / f"rotation-{shift}"
        inputs.append(cwd)
        alias = cwd / "inputs"
        alias.mkdir(parents=True)
        for i, name in enumerate(CLIPS):
            original = CLIPS[(i + shift) % len(CLIPS)]
            (alias / f"{name}.packets").symlink_to(out / "inputs" / f"{original}.packets")
        (alias / "switch.packet").symlink_to(out / "inputs" / "switch.packet")

    workers = {}
    expected = json.loads(RESULTS.read_text())["qualification_sha256"]
    for arm in ARMS:
        directory = out / arm
        directory.mkdir()
        checkout = args.candidate if arm == "candidate" else args.baseline
        api = "base" if arm == "fixed" else "candidate"
        workers[arm] = replay.build(api, checkout, directory)
        for count, shift in ((1, 0), (1, 1), (1, 2), (128, 0), (768, 0)):
            cmd = ["taskset", "-c", str(args.cpu), workers[arm], str(count), "verify"]
            with subprocess.Popen(cmd, stdout=subprocess.PIPE, cwd=inputs[shift]) as process:
                digest = hashlib.file_digest(process.stdout, "sha256").hexdigest()
                if process.wait() or digest != expected[f"{count}/{shift}"]:
                    raise ValueError(f"PCM/range qualification failed for {arm}, {count}, {shift}")
    if args.qualify_only:
        return

    observations = []
    with (out / "observations.jsonl").open("x") as output:
        for cycle, count, shift, arms in schedule():
            for arm in arms:
                command = ["taskset", "-c", str(args.cpu), workers[arm], str(count), "time"]
                row = json.loads(subprocess.check_output(command, cwd=inputs[shift]))
                row.update(cycle=cycle, streams=count, first_clip=shift, arm=arm)
                output.write(json.dumps(row) + "\n")
                output.flush()
                observations.append(row)
    for count in replay.COUNTS:
        data = {arm: [r for r in observations if r["arm"] == arm and r["streams"] == count] for arm in ARMS}
        ratios = [c["cpu_ns"] / b["cpu_ns"] for c, b in zip(data["candidate"], data["compact"])]
        lo, hi = paired_interval(ratios, data["candidate"], count)
        print(f"{count} streams, median paired CPU ratio vs 0.2.11 compact "
              f"{statistics.median(ratios):.4f} (95% interval {lo:.4f} to {hi:.4f})")
        for arm, rows in data.items():
            rss = statistics.median(r["warm"][0] for r in rows)
            p99 = statistics.median(sorted(r["transition_ns"])[math.ceil(count * .99) - 1] for r in rows)
            print(f"  {arm}: warm RSS {rss:g} KiB, first CELT p99 {p99 / 1000:.2f} us")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline", type=Path, required=True, help="released 0.2.11 checkout")
    parser.add_argument("--candidate", type=Path, required=True, help="checkout with the compact CPU change")
    parser.add_argument("--out", type=Path, required=True, help="new output directory")
    parser.add_argument("--cpu", type=int, required=True, help="Linux logical CPU")
    parser.add_argument("--qualify-only", action="store_true")
    source = parser.add_mutually_exclusive_group(required=True)
    source.add_argument("--packets", type=Path, help="previously prepared AMI packets")
    source.add_argument("--wav-dir", type=Path, help="original AMI WAVs")
    run(parser.parse_args())
