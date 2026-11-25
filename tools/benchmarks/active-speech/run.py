#!/usr/bin/env python3
"""Build and replay the optional Linux active-stream experiment outside the Opura workspace."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import random
import shutil
import struct
import subprocess
import sys

HERE = Path(__file__).resolve().parent
EVIDENCE = HERE.parents[2] / "docs/benchmarks/results/active-speech-capacity-0.2.8.json"
ARMS = ("opura", "opus-rs")
FIELDS = ("streams frames seed packets wall_ns cpu_ticks initial_rss_kib final_rss_kib "
          "peak_rss_kib p50_ns missed p99_ns p999_ns").split()


def sha(data):
    return hashlib.sha256(data).hexdigest()


def prepare_packets(work, wav_dir, ogg_dir, corpus):
    packets_dir = work / "packets"
    packets_dir.mkdir()
    for clip in corpus:
        stem = clip["file"]
        if wav_dir:
            media = work / (stem + ".opus")
            subprocess.run(["ffmpeg", "-v", "error", "-i", wav_dir / (stem + ".wav"),
                            "-c:a", "libopus", "-application", "voip", "-b:a", "24k",
                            "-vbr", "off", "-frame_duration", "20", media], check=True)
        else:
            media = ogg_dir / (stem + ".opus")
        listing = json.loads(subprocess.check_output(
            ["ffprobe", "-loglevel", "error", "-select_streams", "a:0", "-show_packets",
             "-show_streams", "-show_data", "-show_entries", "packet=size,data:stream=channels",
             "-of", "json", media]))
        if len(listing["streams"]) != 1 or listing["streams"][0]["channels"] != 1:
            raise ValueError(f"{stem} must contain one mono stream")
        blob = bytearray()
        for packet in listing["packets"]:
            # ffprobe prints a zero-based byte offset, 39 hex columns, then an ASCII preview.
            data = b"".join(bytes.fromhex(line[10:49]) for line in packet["data"].splitlines() if line)
            if len(data) != int(packet["size"]):
                raise ValueError(f"unexpected ffprobe data in {stem}")
            blob.extend(struct.pack(">I", len(data)) + data)
        if len(listing["packets"]) != clip["packets"] or sha(blob) != clip["packet_sha256"]:
            raise ValueError(f"{stem} differs from the measured packet corpus")
        (packets_dir / stem).write_bytes(blob)


def build_and_qualify(work, receipt):
    (work / "src").mkdir()
    shutil.copyfile(HERE / "worker.rs", work / "src/main.rs")
    (work / "Cargo.toml").write_text(receipt["build"]["manifest"])
    (work / "Cargo.lock").write_text(receipt["build"]["lockfile"])
    env = dict(os.environ, RUSTFLAGS="-C target-cpu=native", CARGO_TARGET_DIR=str(work / "target"))
    hashes = {}
    for arm in ARMS:
        subprocess.run(["cargo", "+1.100.0", "build", "--offline", "--locked", "--release",
                        "--features", arm + "-build"], cwd=work, env=env, check=True)
        binary = work / arm
        shutil.copy2(work / "target/release/active-speech-capacity", binary)
        hashes[arm] = sha(binary.read_bytes())
        outputs = work / ("qualification-" + arm)
        outputs.mkdir()
        subprocess.run([binary, "qualify", outputs], cwd=work, check=True)
        for index, record in enumerate(receipt["corpus"]):
            for extension, expected in [("f32", "native_pcm_sha256"), ("range", "range_sha256")]:
                result = outputs / f"{index:02}.{extension}"
                if sha(result.read_bytes()) != record[expected]:
                    raise ValueError(f'{arm} failed {extension} verification on {record["file"]}')
    return hashes


def replay_grid(work, grid, cpu):
    rng = random.Random(grid["order_seed"])
    tick_hz = os.sysconf("SC_CLK_TCK")
    order = 0
    with (work / (grid["name"] + ".jsonl")).open("x") as log:
        for repeat, seed in enumerate([7, 71, 113], 1):
            loads = grid["loads"].copy()
            rng.shuffle(loads)
            for streams in loads:
                arms = list(ARMS)
                rng.shuffle(arms)
                for arm in arms:
                    order += 1
                    stdout = subprocess.check_output(["taskset", "-c", str(cpu), work / arm,
                                                      str(streams), "80", str(seed)], cwd=work, text=True)
                    row = dict(zip(FIELDS, map(int, stdout.strip().split(",")), strict=True))
                    if row["packets"] != streams * 80:
                        raise ValueError("incomplete trial")
                    row.update(arm=arm, repeat=repeat, order=order, grid=grid["name"],
                               cpu_pct=row["cpu_ticks"] / tick_hz * 1e11 / row["wall_ns"],
                               miss_pct=100 * row["missed"] / row["packets"])
                    log.write(json.dumps(row) + "\n")
                    log.flush()
                    print(grid["name"], repeat, arm, streams, flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sources = parser.add_mutually_exclusive_group(required=True)
    sources.add_argument("--wav-dir", type=Path, help="directory with the 15 pinned Golos WAV fixtures")
    sources.add_argument("--ogg-dir", type=Path, help="already encoded matching golos_00.opus through golos_14.opus")
    parser.add_argument("--out", type=Path, required=True, help="new standalone working directory")
    parser.add_argument("--cpu", type=int, default=2)
    parser.add_argument("--qualify-only", action="store_true")
    args = parser.parse_args()
    if sys.platform != "linux" or sys.byteorder != "little":
        parser.error("this harness uses Linux /proc and little-endian PCM")
    if args.cpu not in os.sched_getaffinity(0):
        parser.error("selected CPU outside the worker's affinity mask")
    work = args.out.resolve()
    work.mkdir(parents=True, exist_ok=False)
    receipt = json.loads(EVIDENCE.read_text())
    if sha((HERE / "worker.rs").read_bytes()) != receipt["build"]["worker_sha256"]:
        raise ValueError("worker source differs from the measured source")
    prepare_packets(work, args.wav_dir, args.ogg_dir, receipt["corpus"])
    hashes = build_and_qualify(work, receipt)
    (work / "qualification.json").write_text(json.dumps({"verified_arms": hashes,
                                                         "packets_per_arm": 3101}, indent=2) + "\n")
    if not args.qualify_only:
        for grid in receipt["grids"]:
            replay_grid(work, grid, args.cpu)


if __name__ == "__main__":
    main()
