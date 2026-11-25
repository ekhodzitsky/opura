#!/usr/bin/env python3
"""Measure the growth in resident Private_Dirty pages for 64 more active decoder states."""
import argparse
import ctypes as ct
import json
import os
from pathlib import Path
import random
import shutil
import struct
import subprocess
import sys
import tempfile

sys.dont_write_bytecode = True
from run import CASES, HERE, PACKAGES, SEED, digest_file, streams, write_report


def read_packets(data):
    pos = 0
    while pos < len(data):
        length, samples, _ = struct.unpack_from('>III', data, pos)
        yield samples, data[pos + 12:pos + 12 + length]
        pos += 12 + length


def native_probe(rate, channels, path, library):
    native = ct.CDLL(library)
    native.opus_decoder_create.argtypes = [ct.c_int, ct.c_int, ct.POINTER(ct.c_int)]
    native.opus_decoder_create.restype = ct.c_void_p
    native.opus_decode.argtypes = [ct.c_void_p, ct.c_char_p, ct.c_int32, ct.POINTER(ct.c_int16), ct.c_int, ct.c_int]
    native.opus_decode.restype = ct.c_int
    native.opus_decoder_destroy.argtypes = [ct.c_void_p]
    native.opus_decoder_destroy.restype = None
    output = (ct.c_int16 * (rate * 120 // 1000 * channels))()
    packets = list(read_packets(path.read_bytes()))
    states = []
    try:
        for target in [1, 33, 65]:
            while len(states) < target:
                error = ct.c_int()
                state = native.opus_decoder_create(rate, channels, ct.byref(error))
                if not state or error.value:
                    raise RuntimeError('decoder creation')
                states.append(state)
                for samples, packet in packets:
                    if native.opus_decode(state, packet, len(packet), output, samples, 0) != samples:
                        raise RuntimeError('sample count')
            stats = Path('/proc/self/smaps_rollup').read_text().splitlines()
            print(target, next(int(s.split()[1]) for s in stats if s.startswith('Private_Dirty:')))
    finally:
        for state in states:
            native.opus_decoder_destroy(state)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--vectors', type=Path, required=True)
    parser.add_argument('--libopus', type=Path, required=True)
    parser.add_argument('--cpu', type=int, required=True)
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--toolchain', default='1.100.0')
    args = parser.parse_args()
    if args.out.exists():
        parser.error('new output path required')
    lib = args.libopus.resolve()
    with tempfile.TemporaryDirectory() as directory:
        work = Path(directory)
        dependencies = ''.join(f'{name} = "={v}"\n' for name, v in PACKAGES.items())
        manifest = ('[package]\nname = "memory-probe"\nversion = "0.0.0"\nedition = "2021"\n'
                    '[[bin]]\nname = "memory-probe"\npath = "memory.rs"\n[dependencies]\n')
        (work / 'Cargo.toml').write_text(manifest + dependencies)
        shutil.copyfile(HERE / 'memory.rs', work / 'memory.rs')
        env = dict(os.environ, RUSTFLAGS='-C target-cpu=native', CARGO_TARGET_DIR=str(work / 'target'))
        subprocess.run(['cargo', '+' + args.toolchain, 'build', '--offline', '--release'], cwd=work, env=env, check=True)
        binary = work / 'target/release/memory-probe'
        inputs = {(rec['rate'], rec['vector']): path for path, rec in streams(work, args.vectors, ct.CDLL(str(lib)))}
        corpus = {}
        for rate, _ in CASES:
            sequence = b''
            for vector in (2, 5, 1):
                packets = list(read_packets(inputs[rate, vector].read_bytes()))[:16]
                sequence += b''.join(struct.pack('>III', len(data), samples, 0) + data for samples, data in packets)
            corpus[rate] = work / f'memory-{rate}.packets'
            corpus[rate].write_bytes(sequence)
        os.sched_setaffinity(0, {args.cpu})
        report = dict(schema=1, cpu=args.cpu, seed=SEED, page_bytes=os.sysconf('SC_PAGE_SIZE'),
                      toolchain=args.toolchain, cargo_lock=(work / 'Cargo.lock').read_text(),
                      libopus_sha256=digest_file(lib), probe_sha256=digest_file(binary),
                      source_sha256={n: digest_file(HERE / n) for n in ('memory.rs', 'memory.py', 'run.py')},
                      corpus_sha256={r: digest_file(p) for r, p in corpus.items()}, samples=[])
        rng = random.Random(SEED)
        for round_number in range(5):
            cases = [(name, rate, ch) for rate, ch in CASES for name in (*PACKAGES, 'libopus')]
            rng.shuffle(cases)
            for name, rate, channels in cases:
                if name == 'libopus':
                    command = [sys.executable, __file__, 'native', str(rate), str(channels), corpus[rate], lib]
                else:
                    command = [binary, name, str(rate), str(channels), corpus[rate]]
                text = subprocess.check_output(command, text=True, timeout=120)
                values = {int(n): int(kib) for n, kib in (s.split() for s in text.splitlines())}
                if set(values) != {1, 33, 65}:
                    raise RuntimeError('missing live-decoder counts')
                report['samples'].append(dict(round=round_number + 1, name=name, rate=rate, channels=channels,
                                              private_dirty_kib=values))
        write_report(args.out, report)


if __name__ == '__main__':
    if sys.argv[1:2] == ['native']:
        native_probe(int(sys.argv[2]), int(sys.argv[3]), Path(sys.argv[4]), sys.argv[5])
    else:
        main()
