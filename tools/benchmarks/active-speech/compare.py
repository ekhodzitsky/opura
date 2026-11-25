#!/usr/bin/env python3
"""Compare two builds of the same active-speech worker on the pinned speech corpus."""
import argparse
import json
import math
import os
from pathlib import Path
import random
import statistics
import subprocess
import tempfile

from run import EVIDENCE, FIELDS, sha


def check_output(binary, work, corpus):
    with tempfile.TemporaryDirectory() as directory:
        subprocess.run([binary, "qualify", directory], cwd=work, check=True)
        for index, record in enumerate(corpus):
            for ext, key in [("f32", "native_pcm_sha256"), ("range", "range_sha256")]:
                path = Path(directory) / f"{index:02}.{ext}"
                if sha(path.read_bytes()) != record[key]:
                    raise ValueError(f"{binary.name} differs from the reference in {path.name}")


def paired_interval(ratios):
    logs = [math.log(ratio) for ratio in ratios]
    rng = random.Random(1701)
    samples = sorted(math.exp(statistics.mean(rng.choices(logs, k=len(logs))))
                     for _ in range(20_000))
    return [samples[499], samples[19_499]]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline", required=True, type=Path)
    parser.add_argument("--candidate", required=True, type=Path)
    parser.add_argument("--packets", required=True, type=Path)
    parser.add_argument("--cpu", type=int, required=True)
    parser.add_argument("--out", type=Path, required=True, help="New JSON results file")
    args = parser.parse_args()
    if args.out.exists() or args.cpu not in os.sched_getaffinity(0):
        parser.error("choose a new output filename and an available CPU")
    # The unchanged worker opens packets/golos_NN relative to its working directory.
    packets = args.packets.resolve()
    if packets.name != "packets":
        parser.error("the worker requires the input directory to be named packets")
    corpus = json.loads(EVIDENCE.read_text())["corpus"]
    for record in corpus:
        if sha((packets / record["file"]).read_bytes()) != record["packet_sha256"]:
            raise ValueError(f'packet digest differs for {record["file"]}')
    binaries = {"baseline": args.baseline.resolve(), "candidate": args.candidate.resolve()}
    for binary in binaries.values():
        check_output(binary, packets.parent, corpus)
    tick_hz = os.sysconf("SC_CLK_TCK")
    rows = []
    ratios = []
    for pair in range(18):
        ticks = {}
        seed = (7, 71, 113)[pair % 3]
        for arm in (binaries if pair % 2 == 0 else reversed(binaries)):
            line = subprocess.check_output(
                ["taskset", "-c", str(args.cpu), binaries[arm], "768", "80", str(seed)],
                cwd=packets.parent, text=True)
            row = dict(zip(FIELDS, map(int, line.strip().split(",")), strict=True))
            if (row["streams"], row["frames"], row["seed"], row["packets"]) != (768, 80, seed, 61_440):
                raise ValueError("incomplete or unexpected worker result")
            row.update(arm=arm, pair=pair + 1, cpu_pct=100 * row["cpu_ticks"] / tick_hz * 1e9 / row["wall_ns"])
            rows.append(row)
            ticks[arm] = row["cpu_ticks"]
        ratios.append(ticks["candidate"] / ticks["baseline"])
        print(f"Completed pair {pair + 1}/18", flush=True)
    receipt = {
        "cpu": args.cpu, "ticks_per_second": tick_hz,
        "binary_sha256": {arm: sha(binary.read_bytes()) for arm, binary in binaries.items()},
        "qualified_packets_per_arm": sum(record["packets"] for record in corpus),
        "cpu_time_geometric_mean_ratio": statistics.geometric_mean(ratios),
        "paired_bootstrap_95": paired_interval(ratios), "runs": rows,
    }
    with args.out.open("x") as output:
        json.dump(receipt, output, indent=2)
        output.write("\n")


if __name__ == "__main__":
    main()
