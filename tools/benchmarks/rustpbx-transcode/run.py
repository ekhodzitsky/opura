#!/usr/bin/env python3
"""Build the pinned RustPBX media-path experiment outside the Opura checkout."""
import argparse
import gzip
import hashlib
import json
import os
from pathlib import Path
import random
import shutil
import subprocess
import time

HERE = Path(__file__).resolve().parent
RECEIPT = HERE.parents[2] / "docs/benchmarks/results/rustpbx-transcode-0.2.8.json"


def build(work, method):
    source = work / "rustpbx"
    runner = work / "runner"
    # Require a fresh directory so an interrupted build cannot silently alter the source.
    work.mkdir(parents=True)
    subprocess.run(["git", "init", "-q", source], check=True)
    subprocess.run(["git", "-C", source, "fetch", "-q", "--depth=1",
                    "https://github.com/restsend/rustpbx.git", method["rustpbx_rev"]], check=True)
    subprocess.run(["git", "-C", source, "checkout", "-q", "--detach", "FETCH_HEAD"], check=True)
    media = source / "crates/rustpbx-media/src"
    shutil.copyfile(HERE / "opura_probe.rs", media / "opura_probe.rs")
    with (media / "egress.rs").open("a") as f:
        f.write('\n#[path = "opura_probe.rs"]\npub mod opura_probe;\n')
    (runner / "src").mkdir(parents=True)
    shutil.copyfile(HERE / "main.rs", runner / "src/main.rs")
    shutil.copyfile(HERE / "Cargo.toml.in", runner / "Cargo.toml")
    lock = gzip.decompress((HERE / "Cargo.lock.gz").read_bytes())
    if hashlib.sha256(lock).hexdigest() != method["lock_sha256"]:
        raise ValueError("Cargo.lock checksum differs from the measured build")
    (runner / "Cargo.lock").write_bytes(lock)
    subprocess.run(["cargo", "+1.100.0", "build", "--locked", "--release",
                    "--target-dir", work / "target"], check=True, cwd=runner)
    return work / "target/release/opura-pbx-path"


def measure(exe, cpu, packets, work):
    rng = random.Random(9346)
    orders = {n: (["stock", "opura"] if rng.randrange(2) else ["opura", "stock"])
              for n in (100, 250, 500)}
    command = ["taskset", "-c", str(cpu), str(exe)]
    with (work / "paired.jsonl").open("x") as out:
        for trial in range(5):
            sizes = list(orders)
            rng.shuffle(sizes)
            for size in sizes:
                signatures = []
                for arm in (orders[size] if trial % 2 == 0 else reversed(orders[size])):
                    start = time.monotonic()
                    result = subprocess.run([*command, arm, str(size), packets], check=True,
                                            capture_output=True, text=True, timeout=30)
                    row = json.loads(result.stdout)
                    row.update(arm=arm, round=trial + 1, total_process_wall_s=time.monotonic() - start)
                    if row["frames"] != size * 88 or len(row["latency_us"]) != 88:
                        raise ValueError("incomplete media run")
                    signatures.append((row["fnv"], row["silent_frames"]))
                    print(json.dumps(row), file=out, flush=True)
                if signatures[0] != signatures[1]:
                    raise ValueError(f"PCMU output changed at {size} legs, trial {trial + 1}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--work", required=True, type=Path, help="new empty workspace path")
    parser.add_argument("--packets", required=True, type=Path, help="directory of qualified Golos packets")
    parser.add_argument("--cpu", default=0, type=int, help="available logical CPU for the timed runs")
    args = parser.parse_args()
    method = json.loads(RECEIPT.read_text())["method"]
    if os.sysconf("SC_CLK_TCK") != 100 or args.cpu not in os.sched_getaffinity(0):
        parser.error("requires Linux with 100 Hz process counters and an available CPU")
    packets = args.packets.resolve()
    for name, digest in method["input_hashes"].items():
        if hashlib.sha256((packets / name).read_bytes()).hexdigest() != digest:
            raise ValueError(f"speech fixture mismatch: {name}")
    work = args.work.resolve()
    exe = build(work, method)
    for arm in ("stock", "opura"):
        path = work / f"{arm}.ulaw"
        subprocess.run(["taskset", "-c", str(args.cpu), exe, arm, "15", packets, path], check=True)
        if hashlib.sha256(path.read_bytes()).hexdigest() != method["pcmu_smoke"][arm + "_sha256"]:
            raise ValueError(f"{arm} PCMU smoke fingerprint mismatch")
    measure(exe, args.cpu, packets, work)


if __name__ == "__main__":
    main()
