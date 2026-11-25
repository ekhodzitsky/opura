"""Compare two builds of the RustPBX media probe on the same 500 transcoding legs."""
import json
from pathlib import Path
import random
import subprocess
import sys

work = Path(sys.argv[1]).resolve()
packets = Path(sys.argv[2]).resolve()
cpu = sys.argv[3]
orders = [("baseline", "candidate"), ("candidate", "baseline")] * 6
random.Random(104).shuffle(orders)
with (work / "samples.jsonl").open("x") as out:
    for pair, order in enumerate(orders, start=1):
        for position, arm in enumerate(order):
            values = subprocess.check_output(
                ["taskset", "-c", cpu, work / arm, "opura", "500", packets], text=True)
            row = json.loads(values)
            if row["frames"] != 44000 or len(row["latency_us"]) != 88:
                raise ValueError("incomplete worker output")
            row.update(arm=arm, pair=pair, position=position)
            print(json.dumps(row, separators=(",", ":")), file=out, flush=True)
rows = [json.loads(line) for line in (work / "samples.jsonl").read_text().splitlines()]
if len({(r["fnv"], r["silent_frames"]) for r in rows}) != 1:
    raise ValueError("outgoing PCMU audio differs")
