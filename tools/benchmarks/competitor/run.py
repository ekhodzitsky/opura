#!/usr/bin/env python3
"""Optional Linux head-to-head on all 12 official RFC 8251 streams."""
import argparse
import ctypes
import json
import os
from pathlib import Path
import platform
import random
import shutil
import struct
import subprocess
import sys
import tempfile

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parents[1]))
from check_conformance import MANIFEST, REVISION, digest_file, metric_result, write_report

CASES = ((16000, 1), (48000, 2))
PACKAGES = {"opura": "0.2.7", "opus-rs": "0.1.42"}
SEED = 8251


def build(work, args):
    bins = {}
    provenance = {}
    env = dict(os.environ, RUSTFLAGS="-C target-cpu=native")
    for name, version in PACKAGES.items():
        project = work / name
        project.mkdir()
        manifest = (f'[package]\nname = "probe"\nversion = "0.0.0"\nedition = "2021"\n'
                    f'[dependencies]\n{name} = "={version}"\n[[bin]]\nname = "probe"\npath = "replay.rs"\n'
                    '[features]\nopura = []\n')
        (project / 'Cargo.toml').write_text(manifest)
        shutil.copyfile(HERE / 'replay.rs', project / 'replay.rs')
        flags = ['--features', 'opura'] if name == 'opura' else []
        subprocess.run(['cargo', '+' + args.toolchain, 'build', '--offline', '--release', *flags],
                       cwd=project, env=dict(env, CARGO_TARGET_DIR=str(project / 'target')), check=True)
        bins[name] = project / 'target/release/probe'
        provenance[name] = (project / 'Cargo.lock').read_text()
    bins['libopus'] = work / 'libopus-probe'
    subprocess.run(['cc', '-O3', '-march=native', '-Wall', '-Wextra', '-Werror',
                    '-I', args.source / 'include', HERE / 'native.c', args.libopus, '-o', bins['libopus']], check=True)
    # The linker records the SONAME. Verify which binary the dynamic loader chose.
    loaded = subprocess.check_output(['ldd', bins['libopus']], text=True)
    soname = next((line.split('=>')[1].split()[0] for line in loaded.splitlines()
                   if 'libopus.so' in line and '=>' in line), None)
    if soname is None or digest_file(soname) != digest_file(args.libopus):
        raise RuntimeError('linked libopus does not match the selected file')
    compare = work / 'opus_compare'
    subprocess.run(['cc', '-O2', '-std=c99', args.source / 'src/opus_compare.c', '-lm', '-o', compare], check=True)
    return bins, provenance, compare


def streams(work, vectors, native):
    for name, expected in MANIFEST['sha1'].items():
        if digest_file(vectors / name, 'sha1') != expected:
            raise ValueError('RFC input hash mismatch: ' + name)
    samples_in = native.opus_packet_get_nb_samples
    samples_in.argtypes = [ctypes.c_char_p, ctypes.c_int32, ctypes.c_int32]
    samples_in.restype = ctypes.c_int
    streams = []
    for rate, channels in CASES:
        for num in range(1, 13):
            stem = f'testvector{num:02}'
            data = (vectors / (stem + '.bit')).read_bytes()
            pos = count = total = checks = 0
            path = work / f'{stem}-{rate}.packets'
            with path.open('wb') as output:
                while pos < len(data):
                    n, expected = struct.unpack_from('>II', data, pos)
                    packet = data[pos + 8:pos + 8 + n]
                    if not n or len(packet) != n:
                        raise ValueError('bad packet geometry')
                    samples = samples_in(packet, n, rate)
                    if not 0 < samples <= rate * 120 // 1000:
                        raise ValueError('invalid packet duration')
                    output.write(struct.pack('>III', n, samples, expected))
                    output.write(packet)
                    pos += 8 + n
                    count += 1
                    total += samples
                    checks += bool(expected)
            streams.append((path, dict(vector=num, rate=rate, channels=channels, packets=count,
                                        samples_per_channel=total, range_checks=checks)))
    return streams


def run_probe(binary, record, path, output):
    return subprocess.run([binary, str(record['rate']), str(record['channels']), path, output],
                          capture_output=True, text=True, timeout=120)


def qualify(binary, record, path, vectors, compare, work):
    pcm = work / 'decoded.pcm'
    result = run_probe(binary, record, path, pcm)
    if result.returncode:
        # Dependency panics can print machine-specific source paths. Keep only the message.
        lines = [s for s in result.stderr.splitlines() if '/' not in s and s.strip()]
        return dict(passed=False, exit=result.returncode, error=' '.join(lines)[:300])
    count, mismatches, _, _ = map(int, result.stdout.split())
    if count != record['packets'] or pcm.stat().st_size != record['samples_per_channel'] * record['channels'] * 2:
        raise RuntimeError('incomplete decoded audio')
    args = [compare] + (['-s'] if record['channels'] == 2 else []) + ['-r', str(record['rate'])]
    variants = {}
    for suffix in ('', 'm'):
        reference = vectors / f'testvector{record["vector"]:02}{suffix}.dec'
        metric = subprocess.run([*args, reference, pcm], capture_output=True, text=True, timeout=120)
        variants['no_phase_inversion' if suffix else 'normal'] = metric_result(metric)
    return dict(passed=mismatches == 0 and any(v['passed'] for v in variants.values()),
                range_mismatches=mismatches, pcm_sha256=digest_file(pcm), metrics=variants)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--vectors', type=Path, required=True, help='verified extracted official .bit/.dec files')
    parser.add_argument('--source', type=Path, required=True, help='pinned libopus git checkout for headers and opus_compare')
    parser.add_argument('--libopus', type=Path, required=True, help='installed shared libopus library')
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--cpu', type=int, required=True)
    parser.add_argument('--toolchain', default='1.100.0')
    parser.add_argument('--rounds', type=int, default=15)
    args = parser.parse_args()
    if args.out.exists() or not 1 <= args.rounds <= 100:
        parser.error('new output path and 1..100 rounds required')
    args.source = args.source.resolve()
    args.libopus = args.libopus.resolve()
    revision = subprocess.check_output(['git', '-C', args.source, 'rev-parse', 'HEAD'], text=True).strip()
    if revision != REVISION or subprocess.run(['git', '-C', args.source, 'diff', '--quiet', 'HEAD', '--', 'src/opus_compare.c', 'include']).returncode:
        parser.error('expected clean reference headers and compare at ' + REVISION)
    with tempfile.TemporaryDirectory() as directory:
        work = Path(directory)
        bins, locks, compare = build(work, args)
        native = ctypes.CDLL(str(args.libopus))
        native.opus_get_version_string.restype = ctypes.c_char_p
        inputs = streams(work, args.vectors, native)
        os.sched_setaffinity(0, {args.cpu})
        report = dict(
            schema=1, cpu=platform.processor() or platform.machine(), cpu_index=args.cpu,
            cpu_model=next((s.split(':', 1)[1].strip() for s in Path('/proc/cpuinfo').read_text().splitlines()
                            if s.startswith('model name')), None),
            os=platform.system() + ' ' + platform.release(),
            rustc=subprocess.check_output(['rustc', '+' + args.toolchain, '-Vv'], text=True).strip(),
            cc=subprocess.check_output(['cc', '--version'], text=True).splitlines()[0],
            rustflags='-C target-cpu=native', cargo_locks=locks,
            libopus_version=native.opus_get_version_string().decode(), libopus_sha256=digest_file(args.libopus),
            reference_revision=REVISION, reference_archive_sha256=MANIFEST['archive_sha256'],
            probe_sha256={k: digest_file(p) for k, p in bins.items()},
            harness_sha256={k: digest_file(HERE / k) for k in ('run.py', 'replay.rs', 'native.c')},
            seed=SEED, rounds=args.rounds, cases=[], runs=[])
        for path, record in inputs:
            record['validation'] = {name: qualify(binary, record, path, args.vectors, compare, work)
                                    for name, binary in bins.items()}
            print(record['rate'], record['vector'], {k: v['passed'] for k, v in record['validation'].items()}, flush=True)
            report['cases'].append(record)
        rng = random.Random(SEED)
        for i in range(args.rounds):
            groups = inputs.copy()
            rng.shuffle(groups)
            for path, record in groups:
                eligible = [name for name, v in record['validation'].items() if v['passed']]
                if len(eligible) < 2:
                    continue
                rng.shuffle(eligible)
                for name in eligible:
                    result = run_probe(bins[name], record, path, '-')
                    result.check_returncode()
                    count, _, elapsed, p99 = map(int, result.stdout.split())
                    if count != record['packets'] or min(elapsed, p99) <= 0:
                        raise RuntimeError('invalid timing sample')
                    report['runs'].append([i + 1, record['rate'], record['vector'], name, elapsed, p99])
            print('round', i + 1, flush=True)
        write_report(args.out, report)


if __name__ == '__main__':
    main()
