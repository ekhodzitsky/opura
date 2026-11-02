#!/usr/bin/env python3
"""Optional deterministic-loss diagnostics for retained SILK recording baselines.

Both encoders use the same pinned native decoder and one-packet FEC playout
wait. This is aligned waveform analysis on two examples, not a listening score
or representative network/quality evaluation. Artifacts remain local.
"""
import argparse
from array import array
import ctypes as c
import hashlib
import json
from pathlib import Path
import struct
import tempfile
from audio_baseline import (CORPUS, REVISION, compile_reference, packets, read_wave,
                            sha, signal_metrics, write_wave)

PATTERNS = {"isolated10": (10, (4,)), "burst10": (20, (5, 6))}


def replay_plan(count, lost):
    """One output frame per time slot; only the latest loss can use next FEC."""
    if count < 1 or any(not isinstance(i, int) or i < 0 or i >= count for i in lost):
        raise ValueError("Nonempty timeline and in-range integer losses required")
    plan = []
    for i in range(count):
        if i not in lost:
            plan.append((i, False))
        elif i+1 < count and i+1 not in lost:
            plan.append((i+1, True))
        else:
            plan.append((None, False))
    return plan


def read_reference_packets(path):
    data = path.read_bytes()
    result = []
    offset = 0
    while offset < len(data):
        if offset+2 > len(data):
            raise ValueError("Truncated packet length")
        length = struct.unpack_from("<H", data, offset)[0]
        offset += 2
        if not 1 <= length <= 1276 or offset+length > len(data):
            raise ValueError("Invalid retained reference packet")
        result.append(data[offset:offset+length])
        offset += length
    if not result:
        raise ValueError("No retained reference packets")
    return result


def replay(lib, channels, encoded, frame_samples, skip, count, lost):
    plan = replay_plan(len(encoded), lost)
    error = c.c_int()
    decoder = lib.opus_decoder_create(48000, channels, c.byref(error))
    if not decoder or error.value:
        raise ValueError("Native decoder initialization failed")
    pcm = (c.c_int16*(2880*channels))()
    result = array("h")
    stats = {"lost_packets": len(lost), "received_packets": len(encoded)-len(lost),
             "received_bytes": sum(len(p) for i, p in enumerate(encoded) if i not in lost),
             "direct_plc_calls": 0, "fec_requests": 0, "fec_requests_with_any_lbrr": 0}
    try:
        for source, fec in plan:
            data = None
            length = 0
            if source is not None:
                packet = encoded[source]
                if not packet:
                    raise ValueError("Empty retained packet")
                data = (c.c_ubyte*len(packet)).from_buffer_copy(packet)
                length = len(packet)
                if fec:
                    available = lib.opus_packet_has_lbrr(data, length)
                    if available < 0:
                        raise ValueError("Native LBRR inspection failed")
                    stats["fec_requests"] += 1
                    stats["fec_requests_with_any_lbrr"] += int(available != 0)
            else:
                stats["direct_plc_calls"] += 1
            samples = lib.opus_decode(decoder, data, length, pcm, frame_samples, int(fec))
            if samples != frame_samples:
                raise ValueError("Native loss replay duration mismatch")
            result.extend(pcm[:samples*channels])
    finally:
        lib.opus_decoder_destroy(decoder)
    result = result[skip*channels:(skip+count)*channels]
    if len(result) != count*channels:
        raise ValueError("Loss replay output duration mismatch")
    return result, stats


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path)
    parser.add_argument("corpus", type=Path)
    parser.add_argument("baseline", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    baseline_path = args.baseline/"report.json"
    baseline = json.loads(baseline_path.read_text())
    if baseline.get("status") != "pass" or baseline.get("mode") != "silk":
        parser.error("a completed SILK recording baseline is required")
    args.output.mkdir(parents=True, exist_ok=False)
    report = {"status": "in_progress", "reference_revision": REVISION,
              "baseline_report_sha256": sha(baseline_path), "tool_sha256": sha(Path(__file__)),
              "opura_version": baseline["opura_version"], "binary_sha256": baseline["binary_sha256"],
              "codec_source_sha256": baseline["codec_source_sha256"],
              "baseline_fec_loss_percent": baseline["fec_loss_percent"], "baseline_dtx": baseline["dtx"],
              "patterns": PATTERNS, "playout_wait": "one packet for both encoders, including absent-LBRR fallback",
              "metric_scope": "aligned waveform diagnostics, no listening or representative network/quality claim",
              "cases": []}
    corpus = {}
    for info in baseline["corpus"]:
        name = info["name"]
        path = args.corpus/(name+".wav")
        if name not in CORPUS or sha(path) != CORPUS[name]:
            raise ValueError("Pinned recording hash mismatch")
        corpus[name] = read_wave(path)
    with tempfile.TemporaryDirectory(prefix="opura-audio-loss-") as temp:
        lib = compile_reference(args.source.resolve(), Path(temp))
        for case in baseline["cases"]:
            channels, original = corpus[case["recording"]]
            if channels != case["channels"] or len(original) != case["samples"]*channels:
                raise ValueError("Baseline recording geometry mismatch")
            ms = int(case["frame_ms"])
            if ms not in (10, 20, 40, 60):
                raise ValueError("Unsupported SILK replay geometry")
            n = ms*48
            stem = f"{case['recording']}-{case['bitrate']}-{ms}ms"
            folder = args.baseline/stem
            if sha(folder/"opura.opus") != case["encoded_sha256"]:
                raise ValueError("Retained Opura file hash mismatch")
            if sha(folder/"reference-packets.bin") != case["reference_packets_sha256"]:
                raise ValueError("Retained reference packet hash mismatch")
            skip, own = packets(folder/"opura.opus")
            reference = read_reference_packets(folder/"reference-packets.bin")
            if skip != 312 or len(own) != len(reference) or len(own) != case["packets"]:
                raise ValueError("Unequal replay packet count/delay")
            for name, encoded, filename, expected in [
                ("opura", own, "opura.wav", case["decoded_sha256"]),
                ("libopus", reference, "reference.wav", case["reference_pcm_sha256"])]:
                if sha(folder/filename) != expected:
                    raise ValueError("Retained baseline PCM hash mismatch")
                _, expected_pcm = read_wave(folder/filename)
                decoded, _ = replay(lib, channels, encoded, n, skip, case["samples"], set())
                if decoded != expected_pcm:
                    raise ValueError("Zero-loss replay differs from baseline "+name)
            out = args.output/stem
            out.mkdir()
            for pattern, (period, offsets) in PATTERNS.items():
                lost = {i for i in range(len(own)) if i % period in offsets}
                row = {"recording": case["recording"], "bitrate": case["bitrate"], "frame_ms": ms,
                       "channels": channels, "samples": case["samples"], "pattern": pattern,
                       "actual_loss_percent": len(lost)*100/len(own), "fec_wait_ms": ms,
                       "loss_mask_sha256": hashlib.sha256(bytes(int(i in lost) for i in range(len(own)))).hexdigest()}
                for name, encoded in [("opura", own), ("libopus", reference)]:
                    decoded, stats = replay(lib, channels, encoded, n, skip, case["samples"], lost)
                    destination = out/(pattern+"-"+name+".wav")
                    write_wave(destination, channels, decoded)
                    row[name] = {"signal": signal_metrics(original, decoded), "replay": stats,
                                 "decoded_wav_sha256": sha(destination)}
                report["cases"].append(row)
                (args.output/"report.json").write_text(json.dumps(report, indent=2, allow_nan=False)+"\n")
                print(stem, pattern, "SNR", row["opura"]["signal"]["snr_db"], row["libopus"]["signal"]["snr_db"], flush=True)
    report["status"] = "pass"
    (args.output/"report.json").write_text(json.dumps(report, indent=2, allow_nan=False)+"\n")


if __name__ == "__main__":
    main()
