#!/usr/bin/env python3
"""Optional real-recording CELT/SILK file baseline; no perceptual/SOTA claim.

Uses only Python's standard library, the native opura CLI, and an unchanged
external fixed-point libopus checkout. Recordings and listening files stay local.
Timing is Opura process wall time, not a competitive core-codec benchmark.
"""
import argparse
from array import array
import ctypes as c
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import re
import statistics
import struct
import subprocess
import sys
import tempfile
import time
import urllib.request
import wave

REVISION = "22244de5a79bd1d6d623c32e72bf1954b56235be"
CORPUS = {
    "speech_orig": "625ae47a791214ca5cef9927ac51ea085158a5ca1e3cc77db019b15579d63aa5",
    "music_orig": "ccc1fef039e7c77a0e3d3a8719cf5ec0a775dce5849113b7646a910d11a7b869",
}
CFLAGS = ["-shared", "-fPIC", "-O2", "-fwrapv", "-DVAR_ARRAYS", "-DOPUS_BUILD",
          "-DFIXED_POINT", "-DDISABLE_FLOAT_API"]


def run(command):
    return subprocess.check_output(list(map(str, command)), stderr=subprocess.PIPE, text=True)


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def read_wave(path):
    with wave.open(str(path), "rb") as source:
        if source.getsampwidth() != 2 or source.getframerate() != 48000:
            raise ValueError("Expected PCM16/48 kHz recording")
        channels = source.getnchannels()
        if channels not in (1, 2):
            raise ValueError("Expected mono/stereo")
        values = array("h", source.readframes(source.getnframes()))
    if sys.byteorder != "little":
        values.byteswap()
    return channels, values


def write_wave(path, channels, values):
    data = array("h", values)
    if sys.byteorder != "little":
        data.byteswap()
    with wave.open(str(path), "wb") as output:
        output.setparams((channels, 2, 48000, 0, "NONE", "not compressed"))
        output.writeframes(data.tobytes())


def signal_metrics(original, decoded):
    """Aligned sample diagnostics only; SNR is not a perceptual quality score."""
    if len(original) != len(decoded) or not original:
        raise ValueError("Nonempty aligned PCM of equal length required")
    energy = sum(x*x for x in original)
    output_energy = sum(y*y for y in decoded)
    error = sum((x-y)**2 for x, y in zip(original, decoded))
    dot = sum(x*y for x, y in zip(original, decoded))
    count = len(original)
    return {
        "samples_interleaved": count,
        "rmse": math.sqrt(error/count),
        "snr_db": 10*math.log10(energy/error) if energy and error else None,
        "identical": error == 0,
        "correlation": dot/math.sqrt(energy*output_energy) if energy and output_energy else None,
        "rms": math.sqrt(output_energy/count),
        "peak": max(abs(y) for y in decoded),
        "clipped_samples": sum(y in (-32768, 32767) for y in decoded),
        "max_abs_error": max(abs(x-y) for x, y in zip(original, decoded)),
        "mean_error": sum(y-x for x, y in zip(original, decoded))/count,
    }


def packets(path):
    """Extract the known complete-page CLI output for independent native decode."""
    data = path.read_bytes()
    offset = 0
    result = []
    pending = bytearray()
    while offset < len(data):
        if len(data)-offset < 27 or data[offset:offset+4] != b"OggS":
            raise ValueError("Truncated Ogg page")
        count = data[offset+26]
        start = offset+27
        end = start+count
        if end > len(data):
            raise ValueError("Truncated Ogg laces")
        laces = data[start:end]
        offset = end
        for size in laces:
            if offset+size > len(data):
                raise ValueError("Truncated Ogg body")
            pending.extend(data[offset:offset+size])
            offset += size
            if size < 255:
                result.append(bytes(pending))
                pending.clear()
    if pending or len(result) < 3 or result[0][:8] != b"OpusHead" or result[1][:8] != b"OpusTags":
        raise ValueError("Incomplete Opus output")
    skip = struct.unpack_from("<H", result[0], 10)[0]
    return skip, result[2:]


def compile_reference(source, directory):
    if run(["git", "-C", source, "rev-parse", "HEAD"]).strip() != REVISION:
        raise ValueError("Expected pinned libopus v1.6.1 revision")
    if run(["git", "-C", source, "status", "--porcelain"]).strip():
        raise ValueError("Reference checkout must be unchanged")
    def manifest(file, variable):
        match = re.search(r"^"+variable+r" = (.*?)(?=\n\n|\Z)",
                          (source/file).read_text(), re.M | re.S)
        if match is None:
            raise ValueError("Missing native source manifest")
        return match[1].replace("\\", "").split()
    files = (manifest("silk_sources.mk", "SILK_SOURCES") +
             manifest("silk_sources.mk", "SILK_SOURCES_FIXED") +
             manifest("celt_sources.mk", "CELT_SOURCES") +
             manifest("opus_sources.mk", "OPUS_SOURCES"))
    wrapper = directory/"configure.c"
    wrapper.write_text('''#include "opus.h"
#include "arch.h"
#if !OPUS_FAST_INT64
#error "Expected fixed fast64 reference"
#endif
int configure(OpusEncoder *s,int channels,int bitrate,int *skip){
 int r=0;
 r|=opus_encoder_ctl(s,OPUS_SET_BITRATE(bitrate));
 r|=opus_encoder_ctl(s,OPUS_SET_VBR(0));
 r|=opus_encoder_ctl(s,OPUS_SET_COMPLEXITY(10));
 r|=opus_encoder_ctl(s,OPUS_SET_FORCE_CHANNELS(channels));
 r|=opus_encoder_ctl(s,OPUS_SET_BANDWIDTH(OPUS_BANDWIDTH_FULLBAND));
 r|=opus_encoder_ctl(s,OPUS_GET_LOOKAHEAD(skip));return r;
}
int configure_silk(OpusEncoder *s,int channels,int bitrate,int bandwidth,int dtx,int loss,int *skip){
 int r=0;
 r|=opus_encoder_ctl(s,OPUS_SET_BITRATE(bitrate));
 r|=opus_encoder_ctl(s,OPUS_SET_VBR(0));
 r|=opus_encoder_ctl(s,OPUS_SET_COMPLEXITY(10));
 r|=opus_encoder_ctl(s,OPUS_SET_FORCE_CHANNELS(channels));
 r|=opus_encoder_ctl(s,OPUS_SET_MAX_BANDWIDTH(bandwidth==OPUS_AUTO?OPUS_BANDWIDTH_FULLBAND:bandwidth));
 r|=opus_encoder_ctl(s,OPUS_SET_BANDWIDTH(bandwidth));
 r|=opus_encoder_ctl(s,OPUS_SET_DTX(dtx));
 r|=opus_encoder_ctl(s,OPUS_SET_INBAND_FEC(loss>0));
 r|=opus_encoder_ctl(s,OPUS_SET_PACKET_LOSS_PERC(loss));
 r|=opus_encoder_ctl(s,OPUS_GET_LOOKAHEAD(skip));return r;
}
''')
    library = directory/"reference.so"
    includes = ["-I"+str(source/p) for p in ["include", "celt", "silk", "silk/fixed", "src"]]
    subprocess.run(["cc", *CFLAGS, *includes, str(wrapper),
                    *[str(source/f) for f in files], "-lm", "-o", str(library)], check=True)
    lib = c.CDLL(str(library))
    pointer = c.c_void_p
    i32, i16, u8 = c.POINTER(c.c_int), c.POINTER(c.c_int16), c.POINTER(c.c_ubyte)
    signatures = [
        ("opus_encoder_create", [c.c_int, c.c_int, c.c_int, i32], pointer),
        ("opus_decoder_create", [c.c_int, c.c_int, i32], pointer),
        ("configure", [pointer, c.c_int, c.c_int, i32], c.c_int),
        ("configure_silk", [pointer,c.c_int,c.c_int,c.c_int,c.c_int,c.c_int,i32],c.c_int),
        ("opus_packet_has_lbrr", [u8,c.c_int],c.c_int),
        ("opus_packet_get_bandwidth", [u8],c.c_int),
        ("opus_packet_parse", [u8,c.c_int,u8,c.POINTER(u8),i16,i32],c.c_int),
        ("opus_encode", [pointer, i16, c.c_int, u8, c.c_int32], c.c_int32),
        ("opus_decode", [pointer, u8, c.c_int32, i16, c.c_int, c.c_int], c.c_int),
        ("opus_encoder_destroy", [pointer], None),
        ("opus_decoder_destroy", [pointer], None),
    ]
    for name, args, result in signatures:
        function = getattr(lib, name)
        function.argtypes, function.restype = args, result
    return lib


def native_encode(lib, channels, bitrate, n, values, *, mode="celt", bandwidth="auto", dtx=False, fec=0):
    error = c.c_int()
    # Public restricted modes fix CELT/SILK without changing native algorithms.
    encoder = lib.opus_encoder_create(48000, channels, 2051 if mode == "celt" else 2052, c.byref(error))
    if not encoder or error.value:
        raise ValueError("Native encoder initialization failed")
    try:
        skip = c.c_int()
        lookahead = 120 if mode == "celt" else 312
        if mode == "celt":
            configured = lib.configure(encoder, channels, bitrate, c.byref(skip))
        else:
            band = {"auto": -1000, "nb": 1101, "mb": 1102, "wb": 1103}[bandwidth]
            configured = lib.configure_silk(encoder, channels, bitrate, band, int(dtx), fec, c.byref(skip))
        if configured or skip.value != lookahead:
            raise ValueError("Native settings/delay mismatch")
        count = len(values)//channels
        frames = (count+lookahead+n-1)//n
        output = (c.c_ubyte*1276)()
        pcm = (c.c_int16*(n*channels))()
        result = []
        for frame in range(frames):
            start = frame*n*channels
            for i in range(len(pcm)):
                pcm[i] = values[start+i] if start+i < len(values) else 0
            length = lib.opus_encode(encoder, pcm, n, output, bitrate*n//(48000*8))
            if length < 1:
                raise ValueError("Native encode failed: "+str(length))
            packet = bytes(output[:length])
            if (mode == "celt" and packet[0] >> 3 < 16) or (mode == "silk" and packet[0] >> 3 >= 12):
                raise ValueError("Native packet mode differs from requested "+mode)
            result.append(packet)
        return result
    finally:
        lib.opus_encoder_destroy(encoder)


def native_decode(lib, channels, encoded, skip, count):
    error = c.c_int()
    decoder = lib.opus_decoder_create(48000, channels, c.byref(error))
    if not decoder or error.value:
        raise ValueError("Native decoder initialization failed")
    result = array("h")
    pcm = (c.c_int16*(5760*channels))()
    try:
        for packet in encoded:
            data = (c.c_ubyte*len(packet)).from_buffer_copy(packet)
            length = lib.opus_decode(decoder, data, len(packet), pcm, 5760, 0)
            if length < 0:
                raise ValueError("Native decode failed: "+str(length))
            result.extend(pcm[:length*channels])
    finally:
        lib.opus_decoder_destroy(decoder)
    result = result[skip*channels:(skip+count)*channels]
    if len(result) != count*channels:
        raise ValueError("Native decoded duration mismatch")
    return result


def packet_diagnostics(lib, encoded, frame_samples):
    if not encoded or frame_samples <= 0:
        raise ValueError("Nonempty packets and positive frame duration required")
    counts = {"packets": len(encoded), "packet_bytes": sum(map(len, encoded)),
              "dtx_packets": 0, "plc_packets": 0, "fec_packets": 0, "bandwidth_packets": {}}
    names = {1101: "nb", 1102: "mb", 1103: "wb", 1104: "swb", 1105: "fb"}
    for packet in encoded:
        if not packet:
            raise ValueError("Empty encoded packet")
        data = (c.c_ubyte*len(packet)).from_buffer_copy(packet)
        fec = lib.opus_packet_has_lbrr(data, len(packet))
        if fec < 0:
            raise ValueError("Native packet FEC inspection failed")
        counts["fec_packets"] += int(fec != 0)
        bandwidth = lib.opus_packet_get_bandwidth(data)
        if bandwidth not in names:
            raise ValueError("Invalid native bandwidth")
        band = names[bandwidth]
        counts["bandwidth_packets"][band] = counts["bandwidth_packets"].get(band, 0)+1
        counts["dtx_packets"] += int(len(packet) == 1)
        frames = (c.POINTER(c.c_ubyte)*48)()
        sizes = (c.c_int16*48)()
        toc = c.c_ubyte()
        number = lib.opus_packet_parse(data, len(packet), c.byref(toc), frames, sizes, None)
        if number < 1:
            raise ValueError("Native packet parse failed")
        counts["plc_packets"] += int(number == 1 and sizes[0] == 1 and frames[0][0] == 0)
    counts["mean_packet_bitrate_bps"] = counts["packet_bytes"]*8/(len(encoded)*frame_samples/48000)
    return counts


def validate_packet_budgets(encoded, bitrate, frame_samples, dtx):
    for i, packet in enumerate(encoded):
        expected = (((i+1)*bitrate*frame_samples+192000)//384000 -
                    (i*bitrate*frame_samples+192000)//384000)
        if len(packet) != expected and not (dtx and len(packet) == 1):
            raise ValueError("Unexpected packet budget at "+str(i))


def encode_diagnostic(command, mode):
    result = subprocess.run(list(map(str, command)), stdout=subprocess.PIPE,
                            stderr=subprocess.PIPE, text=True, check=True)
    if mode == "celt":
        return 0
    match = re.search(r"(\d+) PLC fallback packets", result.stderr)
    if match is None:
        raise ValueError("Missing Opura PLC diagnostic")
    return int(match[1])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("source", type=Path)
    parser.add_argument("corpus", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--rounds", type=int, default=5)
    parser.add_argument("--mode", choices=["celt", "silk"], default="celt")
    parser.add_argument("--bandwidth", choices=["auto", "nb", "mb", "wb"])
    parser.add_argument("--dtx", action="store_true")
    parser.add_argument("--fec", type=int, choices=range(101), default=0)
    parser.add_argument("--recording", action="append", choices=list(CORPUS))
    parser.add_argument("--bitrate", type=int, action="append")
    parser.add_argument("--download", action="store_true")
    args = parser.parse_args()
    if not 3 <= args.rounds <= 31:
        parser.error("rounds must be 3..31")
    if args.mode == "celt" and (args.bandwidth is not None or args.dtx or args.fec):
        parser.error("bandwidth, DTX and FEC require SILK")
    if args.bitrate and any(value <= 0 for value in args.bitrate):
        parser.error("bitrate must be positive")
    bandwidth = args.bandwidth or "auto"
    lookahead = 120 if args.mode == "celt" else 312
    binary, source = args.binary.resolve(), args.source.resolve()
    args.output.mkdir(parents=True, exist_ok=False)
    args.corpus.mkdir(parents=True, exist_ok=True)
    report = {"schema_version": 1, "status": "in_progress", "opura_version": run([binary, "--version"]).strip(),
              "reference_revision": REVISION, "reference_cflags": CFLAGS,
              "reference_settings": ("restricted-low-delay CELT, fullband, CBR, forced channels, complexity 10" if args.mode == "celt" else
                                     f"restricted-SILK, preferred bandwidth {bandwidth}, CBR, forced channels, complexity 10, DTX={args.dtx}, loss/FEC={args.fec}"),
              "mode": args.mode, "bandwidth": "fb" if args.mode == "celt" else bandwidth,
              "dtx": args.dtx, "fec_loss_percent": args.fec,
              "timing_scope": "Opura process wall time including startup and WAV/Ogg I/O; no native speed comparison",
              "metric_scope": "aligned PCM signal diagnostics, not perceptual quality scores",
              "platform": platform.platform(), "machine": platform.machine(),
              "python": platform.python_version(), "cc": run(["cc", "--version"]).splitlines()[0],
              "rustc": run(["rustc", "--version"]).strip(), "binary_sha256": sha(binary),
              "tool_sha256": sha(Path(__file__)),
              "build_profile": "release; actual binary hash recorded",
              "affinity_cpus": sorted(os.sched_getaffinity(0)) if hasattr(os, "sched_getaffinity") else None,
              "rounds": args.rounds, "corpus": [], "cases": []}
    root = Path(__file__).resolve().parent.parent
    fingerprint = hashlib.sha256()
    for path in sorted((root/"src").rglob("*.rs")) + [root/"Cargo.toml", root/"Cargo.lock"]:
        fingerprint.update(str(path.relative_to(root)).encode()+b"\0"+path.read_bytes())
    report["codec_source_sha256"] = fingerprint.hexdigest()
    cpuinfo = Path("/proc/cpuinfo")
    if cpuinfo.exists():
        report["cpu"] = next((s.split(":", 1)[1].strip() for s in cpuinfo.read_text().splitlines()
                              if s.startswith("model name")), "unknown")
    with tempfile.TemporaryDirectory(prefix="opura-audio-reference-") as temp:
        lib = compile_reference(source, Path(temp))
        for name in dict.fromkeys(args.recording or CORPUS):
            expected_hash = CORPUS[name]
            path = args.corpus/(name+".wav")
            url = "https://media.xiph.org/opus/samples/examples/"+name+".wav"
            if not path.exists() and args.download:
                with urllib.request.urlopen(url, timeout=30) as response:
                    data = response.read(20_000_001)
                if len(data) > 20_000_000 or hashlib.sha256(data).hexdigest() != expected_hash:
                    raise ValueError("Downloaded recording size/hash mismatch")
                path.write_bytes(data)
            if sha(path) != expected_hash:
                raise ValueError("Recording hash mismatch: "+name)
            channels, values = read_wave(path)
            count = len(values)//channels
            report["corpus"].append({"name": name, "url": url, "sha256": expected_hash,
                                     "channels": channels, "samples": count, "rate": 48000,
                                     "input_peak": max(abs(x) for x in values),
                                     "input_clipped_samples": sum(x in (-32768, 32767) for x in values)})
            if args.mode == "celt":
                rates = [24000, 48000] if channels == 1 else [64000, 128000]
                frame_sizes = [("5", 240), ("20", 960)]
            else:
                rates = [12000, 16000, 24000] if channels == 1 else [24000, 32000, 48000]
                frame_sizes = [("10", 480), ("20", 960), ("40", 1920), ("60", 2880)]
            rates = list(dict.fromkeys(args.bitrate or rates))
            for bitrate in rates:
                for frame, n in frame_sizes:
                    if bitrate*n % 384000 or not 3 <= bitrate*n//384000 <= 1276:
                        raise ValueError("Matched CBR comparison requires integer packet budgets in3..1276")
                    case = args.output/f"{name}-{bitrate}-{frame}ms"
                    case.mkdir()
                    encoded, decoded = case/"opura.opus", case/"opura.wav"
                    encode = [binary, "encode", path, encoded, "--bitrate", bitrate, "--frame-ms", frame]
                    if args.mode == "silk":
                        encode += ["--mode", "silk", "--bandwidth", bandwidth,
                                   *(["--dtx"] if args.dtx else []), *(["--fec", args.fec] if args.fec else [])]
                    decode = [binary, "decode", encoded, decoded]
                    # One unmeasured full warmup, then the same command/file pair.
                    plc_count = encode_diagnostic(encode, args.mode)
                    run(decode)
                    warmup_hashes = (sha(encoded), sha(decoded))
                    timings = {"encode": [], "decode": []}
                    for _ in range(args.rounds):
                        encoded.unlink()
                        decoded.unlink()
                        for key, command in [("encode", encode), ("decode", decode)]:
                            start = time.perf_counter_ns()
                            observed = encode_diagnostic(command, args.mode) if key == "encode" else run(command)
                            if key == "encode" and observed != plc_count:
                                raise ValueError("Nondeterministic PLC diagnostics")
                            timings[key].append(time.perf_counter_ns()-start)
                        if (sha(encoded), sha(decoded)) != warmup_hashes:
                            raise ValueError("Nondeterministic command output")
                    skip, own_packets = packets(encoded)
                    reference_packets = native_encode(lib, channels, bitrate, n, values, mode=args.mode,
                                                      bandwidth=bandwidth, dtx=args.dtx, fec=args.fec)
                    validate_packet_budgets(own_packets, bitrate, n, args.dtx)
                    validate_packet_budgets(reference_packets, bitrate, n, args.dtx)
                    if len(own_packets) != len(reference_packets) or skip != lookahead:
                        raise ValueError("Unequal packet count/delay")
                    own_packet_stats = packet_diagnostics(lib, own_packets, n)
                    native_packet_stats = packet_diagnostics(lib, reference_packets, n)
                    if own_packet_stats["plc_packets"] != plc_count:
                        raise ValueError("Opura PLC diagnostic differs from native packet inspection")
                    own = native_decode(lib, channels, own_packets, skip, count)
                    reference = native_decode(lib, channels, reference_packets, lookahead, count)
                    own_channels, own_cli_pcm = read_wave(decoded)
                    if own_channels != channels or own_cli_pcm != own:
                        raise ValueError("CLI decode differs from pinned original native decode")
                    write_wave(case/"reference.wav", channels, reference)
                    serialized_reference = b"".join(struct.pack("<H", len(packet))+packet for packet in reference_packets)
                    (case/"reference-packets.bin").write_bytes(serialized_reference)
                    seconds = count/48000
                    row = {"recording": name, "bitrate": bitrate, "frame_ms": frame,
                           "samples": count, "channels": channels, "packets": len(own_packets),
                           "packet_bytes": sum(map(len, own_packets)), "ogg_bytes": encoded.stat().st_size,
                           "opura": signal_metrics(values, own), "libopus": signal_metrics(values, reference),
                           "opura_packet_stats": own_packet_stats, "libopus_packet_stats": native_packet_stats,
                           "decoded_matches_reference": own == reference,
                           "packets_match_reference": own_packets == reference_packets,
                           "reference_packets_sha256": hashlib.sha256(serialized_reference).hexdigest(),
                           "wall_ns": timings, "median_realtime_factor": {
                               key: statistics.median(v)/1e9/seconds for key, v in timings.items()},
                           "encoded_sha256": warmup_hashes[0], "decoded_sha256": warmup_hashes[1],
                           "reference_pcm_sha256": sha(case/"reference.wav")}
                    report["cases"].append(row)
                    (args.output/"report.json").write_text(json.dumps(report, indent=2, allow_nan=False)+"\n")
                    print(name, bitrate, frame, "SNR", row["opura"]["snr_db"], row["libopus"]["snr_db"], flush=True)
    report["status"] = "pass"
    (args.output/"report.json").write_text(json.dumps(report, indent=2, allow_nan=False)+"\n")


if __name__ == "__main__":
    main()
