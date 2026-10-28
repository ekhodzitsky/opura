#!/usr/bin/env python3
"""Offline, optional Linux packet-framing benchmark. No Cargo dependencies."""
import argparse
import ctypes
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import random
import statistics
import struct
import subprocess

ROOT = Path(__file__).resolve().parent.parent
GROUPS = ('single', 'two_cbr', 'two_vbr', 'multi_cbr', 'multi_vbr', 'padding', 'invalid')


def run(args):
    return subprocess.check_output(list(map(str, args)), cwd=ROOT, text=True).strip()


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def source_digest():
    digest = hashlib.sha256()
    for path in sorted((ROOT / 'src').rglob('*.rs')):
        digest.update(str(path.relative_to(ROOT)).encode() + b'\0' + path.read_bytes())
    return digest.hexdigest()


def corpus():
    groups = {key: [] for key in GROUPS}
    for line in (ROOT / 'tests/packet-fixtures.txt').read_text().splitlines():
        if not line or line.startswith('#'):
            continue
        encoded, expected = line.split()
        data = b''
        if encoded != '-':
            for chunk in encoded.split(','):
                if '*' in chunk:
                    byte, count = chunk.split('*')
                    data += bytes.fromhex(byte) * int(count)
                else:
                    data += bytes.fromhex(chunk)
        if expected == 'error':
            group, fingerprint = 'invalid', 0
        else:
            spans = [tuple(map(int, span.split(':'))) for span in expected.split(',')[3:]]
            fingerprint = len(spans)
            for span in spans:
                for value in span:
                    fingerprint = (fingerprint * 1_000_003 + value) & ((1 << 64) - 1)
            code = data[0] & 3
            group = ('single', 'two_cbr', 'two_vbr', 'multi_cbr')[code]
            if code == 3:
                group = 'padding' if data[1] & 64 else 'multi_vbr' if data[1] & 128 else 'multi_cbr'
        groups[group].append((data, fingerprint))
    return groups


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--libopus', type=Path, required=True, help='explicit local shared library')
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--rounds', type=int, default=21)
    parser.add_argument('--iterations', type=int, default=20000)
    parser.add_argument('--cpu', type=int)
    args = parser.parse_args()
    if args.rounds < 15 or args.iterations < 1000:
        parser.error('at least 15 rounds and 1000 iterations required')
    if args.out.exists():
        parser.error('output exists; preserve previous measurements')
    libpath = args.libopus.resolve(strict=True)
    lib = ctypes.CDLL(str(libpath))
    lib.opus_get_version_string.restype = ctypes.c_char_p
    version = lib.opus_get_version_string().decode()
    cpu = args.cpu if args.cpu is not None else min(os.sched_getaffinity(0))
    os.sched_setaffinity(0, {cpu})
    build = ROOT / 'target/benchmarks'
    build.mkdir(parents=True, exist_ok=True)
    commands = [
        ['rustc', '--edition=2024', '--crate-type=rlib', '--crate-name=opura',
         '-C', 'opt-level=3', 'src/lib.rs', '-o', build / 'libopura.rlib'],
        ['rustc', '--edition=2024', '-C', 'opt-level=3', 'tools/benchmarks/packet.rs',
         '--extern', f'opura={build}/libopura.rlib', '-o', build / 'opura'],
        ['cc', '-O3', '-Wall', '-Wextra', '-Werror', 'tools/benchmarks/packet.c',
         '-ldl', '-o', build / 'libopus'],
    ]
    for command in commands:
        run(command)
    cases = {}
    for name, packets in corpus().items():
        path = build / f'{name}.bin'
        path.write_bytes(b''.join(struct.pack('<I', len(data)) + data for data, _ in packets))
        cases[name] = dict(packets=len(packets), corpus_sha256=sha(path),
                           expected=[value for _, value in packets], samples_ns={'opura': [], 'libopus': []})
    rng = random.Random(6716)
    order = []
    for round_index in range(args.rounds):
        names = list(cases)
        rng.shuffle(names)
        for name in names:
            implementations = ['opura', 'libopus']
            rng.shuffle(implementations)
            order.append([round_index, name, implementations])
            for implementation in implementations:
                cmd = [build / implementation, build / f'{name}.bin', args.iterations]
                if implementation == 'libopus':
                    cmd.append(libpath)
                lines = run(cmd).splitlines()
                if list(map(int, lines[:-1])) != cases[name]['expected']:
                    raise RuntimeError(f'{implementation}/{name}: incorrect frame spans or acceptance')
                if not lines[-1].startswith('ns '):
                    raise RuntimeError('invalid benchmark output')
                cases[name]['samples_ns'][implementation].append(float(lines[-1][3:]))
        print(f'paired round {round_index + 1}/{args.rounds}', flush=True)
    for case in cases.values():
        del case['expected']
        case['correct'] = True
    cpu_model = next((line.split(':', 1)[1].strip() for line in Path('/proc/cpuinfo').read_text().splitlines()
                      if line.startswith('model name')), 'unknown')
    governor = Path(f'/sys/devices/system/cpu/cpu{cpu}/cpufreq/scaling_governor')
    result = dict(schema=1, track='packet', revision=run(['git', 'rev-parse', 'HEAD']),
                  source_sha256=source_digest(), measured_at=datetime.now(timezone.utc).isoformat(), dirty=bool(run(['git', 'status', '--porcelain'])),
                  environment=dict(cpu=cpu_model, affinity=[cpu], kernel=os.uname().release,
                                   governor=governor.read_text().strip() if governor.exists() else 'unknown',
                                   thermal_state='uncontrolled; inspect raw variance',
                                   rustc=run(['rustc', '-Vv']), cc=run(['cc', '--version']).splitlines()[0]),
                  libopus=dict(version=version, sha256=sha(libpath), build_flags='system package; unknown'),
                  harness_sha256={str(p.relative_to(ROOT)): sha(p) for p in [ROOT / 'tools/packet_bench.py', ROOT / 'tools/benchmarks/packet.rs', ROOT / 'tools/benchmarks/packet.c']},
                  build_commands=[[str(x).replace(str(build), 'target/benchmarks') for x in c] for c in commands],
                  rounds=args.rounds, iterations=args.iterations, warmup_iterations=1000,
                  order=order, cases=cases,
                  missing_capabilities=['PCM decode', 'encoder', 'Ogg/CLI', 'full-codec quality, memory and usability comparisons'])
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(result, indent=2) + '\n')
    print('case            opura ns   libopus ns    ratio')
    for name, case in cases.items():
        a, b = (statistics.median(case['samples_ns'][impl]) for impl in ('opura', 'libopus'))
        print(f'{name:15} {a:9.2f} {b:12.2f} {a/b:8.3f}')


if __name__ == '__main__':
    main()
