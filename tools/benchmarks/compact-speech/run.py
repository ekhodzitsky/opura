"""Replay Opura's fixed/compact decoder comparison on Linux x86-64."""
import argparse
import hashlib
import json
import math
from pathlib import Path
import shutil
import statistics
import subprocess
import wave


HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
RECEIPT = ROOT / "docs/benchmarks/results/compact-speech-0.2.11.json"
COUNTS = (1, 128, 768)


def digest(path):
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def require_digest(path, expected):
    if digest(path) != expected:
        raise ValueError(f"SHA-256 mismatch for {path}")


def encode(wav_path, destination, clip):
    require_digest(wav_path, clip["source_sha256"])
    with wave.open(str(wav_path)) as source:
        if (source.getframerate(), source.getnchannels(), source.getsampwidth()) != (16000, 1, 2):
            raise ValueError(f"Expected 16 kHz mono PCM16 in {wav_path}")
        source.setpos(960000)
        pcm = source.readframes(1920000)
    if hashlib.sha256(pcm).hexdigest() != clip["pcm_input_sha256"]:
        raise ValueError(f"Wrong excerpt from {wav_path}")
    ffmpeg = ["ffmpeg", "-v", "error", "-nostdin"]
    ogg = subprocess.check_output(ffmpeg + [
        "-f", "s16le", "-ar", "16000", "-ac", "1", "-i", "pipe:0", "-map_metadata", "-1",
        "-c:a", "libopus", "-application", "voip", "-frame_duration", "20", "-b:a", "24000",
        "-vbr", "off", "-compression_level", "10", "-fflags", "+bitexact", "-flags", "+bitexact",
        "-f", "ogg", "pipe:1",
    ], input=pcm)
    payload = subprocess.check_output(ffmpeg + [
        "-i", "pipe:0", "-map", "0:a:0", "-c", "copy", "-f", "data", "pipe:1",
    ], input=ogg)
    if len(payload) != 6001 * 60:
        raise ValueError(f"Expected 6001 constant-size 60-byte packets for {wav_path}")
    with destination.open("wb") as target:
        for i in range(0, len(payload), 60):
            target.write((60).to_bytes(4, "big"))
            target.write(payload[i:i + 60])


def prepare_inputs(args, receipt, out):
    out.mkdir()
    for name, clip in receipt["corpus"].items():
        target = out / f"{name}.packets"
        if args.packets:
            shutil.copyfile(args.packets / target.name, target)
        else:
            encode(args.wav_dir / f"{name}.Mix-Headset.wav", target, clip)
        require_digest(target, clip["packets_sha256"])
    fixture = args.baseline / "tests/opus-packet-fixtures.txt"
    with fixture.open() as source:
        for line in source:
            fields = line.split(maxsplit=9)
            if fields[:2] != ["frame", "normal"] or fields[3:6] != ["16000", "1", "960"]:
                continue
            if fields[6] == "-":
                continue
            payload = bytes.fromhex(fields[6])
            if payload and payload[0] >> 3 >= 16:
                target = out / "switch.packet"
                target.write_bytes(payload)
                require_digest(target, receipt["switch_packet_sha256"])
                return
    raise ValueError(f"No 20 ms CELT switch packet in {fixture}")


def build(arm, checkout, out):
    project = out / arm
    project.mkdir()
    shutil.copyfile(HERE / "worker.rs", project / "worker.rs")
    dependency = json.dumps(str(checkout.resolve()))
    (project / "Cargo.toml").write_text(f"""[package]
name = "compact-speech-worker"
version = "0.1.0"
edition = "2021"
[features]
compact = []
[dependencies]
opura = {{ path = {dependency} }}
[[bin]]
name = "worker"
path = "worker.rs"
""")
    flags = ["--features", "compact"] if arm == "candidate" else []
    target = project / "target"
    subprocess.run(["cargo", "+1.87.0", "build", "--offline", "--release",
                    "--target-dir", target, *flags], cwd=project, check=True)
    return target / "release/worker"


def run(args):
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=False)
    receipt = json.loads(RECEIPT.read_text())
    prepare_inputs(args, receipt, out / "inputs")
    workers = {arm: build(arm, checkout, out) for arm, checkout in
               (("base", args.baseline), ("candidate", args.candidate))}
    hashes = {}
    for arm, worker in workers.items():
        hashes[arm] = {}
        for count in COUNTS:
            cmd = ["taskset", "-c", str(args.cpu), worker, str(count), "verify"]
            with subprocess.Popen(cmd, stdout=subprocess.PIPE, cwd=out) as process:
                value = hashlib.file_digest(process.stdout, "sha256").hexdigest()
                if process.wait() != 0 or value != receipt["qualification_sha256_by_stream_count"][str(count)]:
                    raise ValueError(f"PCM/range qualification failed for {arm}, {count} streams")
            hashes[arm][count] = value
    (out / "qualification.json").write_text(json.dumps(hashes, indent=2) + "\n")
    if args.qualify_only:
        return
    rows = []
    with (out / "observations.jsonl").open("w") as output:
        for index in range(8):
            for position in range(len(COUNTS)):
                load = (position + index) % len(COUNTS)
                count = COUNTS[load]
                arms = ("base", "candidate") if (index + load) % 2 == 0 else ("candidate", "base")
                for arm in arms:
                    cmd = ["taskset", "-c", str(args.cpu), workers[arm], str(count), "time"]
                    row = json.loads(subprocess.check_output(cmd, cwd=out))
                    row.update(round=index, streams=count, arm=arm)
                    rows.append(row)
                    output.write(json.dumps(row) + "\n")
                    output.flush()
    for count in COUNTS:
        arms = {a: [r for r in rows if r["streams"] == count and r["arm"] == a] for a in workers}
        ratios = [c["cpu_ns"] / b["cpu_ns"] for b, c in zip(arms["base"], arms["candidate"])]
        print(f"{count} streams: median paired compact/fixed CPU ratio {statistics.median(ratios):.4f}")
        for arm, trials in arms.items():
            rss = statistics.median(r["warm"][0] for r in trials)
            dirty = statistics.median(r["warm"][2] for r in trials)
            p99 = statistics.median(sorted(r["transition_ns"])[math.ceil(count * .99) - 1]
                                    for r in trials) / 1000
            print(f"  {arm}: RSS {rss:g} KiB; Private_Dirty {dirty:g} KiB; first CELT p99 {p99:.2f} us")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline", required=True, type=Path, help="v0.2.10 source tree")
    parser.add_argument("--candidate", required=True, type=Path, help="source tree with CompactOpusDecoder")
    parser.add_argument("--out", required=True, type=Path, help="new output directory")
    parser.add_argument("--cpu", required=True, type=int, help="Linux logical CPU to pin")
    parser.add_argument("--qualify-only", action="store_true", help="validate output without collecting timings")
    sources = parser.add_mutually_exclusive_group(required=True)
    sources.add_argument("--packets", type=Path, help="directory of prepared .packets streams")
    sources.add_argument("--wav-dir", type=Path, help="directory containing the three original AMI WAVs")
    run(parser.parse_args())
