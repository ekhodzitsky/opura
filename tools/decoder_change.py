#!/usr/bin/env python3
"""Optional paired public-decoder comparison of two source-bound release binaries."""
import argparse
from datetime import datetime, timezone
import hashlib
import json
import math
import os
from pathlib import Path
import random
import re
import statistics
import subprocess

import check_conformance as conformance
from decoder_bench import CASES, PACKETS_PER_PASS, check_sample, compare_samples, conformance_cases, hardware, schedule

ROOT = Path(__file__).resolve().parents[1]


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def sources(root):
    paths = [*root.glob('src/**/*.rs'), *[root / name for name in
             ['Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml', 'examples/decoder_bench.rs']]]
    digest = hashlib.sha256()
    for path in sorted(paths):
        digest.update(str(path.relative_to(root)).encode() + b'\0' + path.read_bytes() + b'\0')
    return digest.hexdigest()


def paired_schedule(rounds):
    return [dict(round=step['round'], case=step['case'],
                 implementations=['before' if name == 'opura' else 'after'
                                  for name in step['implementations']])
            for step in schedule(rounds)]


def interval(before, after, rng):
    ratio = statistics.median(after) / statistics.median(before)
    values = []
    for _ in range(2000):
        indices = [rng.randrange(len(before)) for _ in before]
        values.append(statistics.median([after[i] for i in indices]) /
                      statistics.median([before[i] for i in indices]))
    values.sort()
    return {'ratio': ratio, 'ci95': [values[49], values[1949]]}


def summarize(report):
    if report.get('track') != 'decoder_before_after' or report.get('schema_version') != 1:
        raise ValueError('Wrong comparison schema/track')
    rounds = report['rounds']
    if type(rounds) is not int or not 15 <= rounds <= 100:
        raise ValueError('At least15 paired rounds required')
    expected = paired_schedule(rounds)
    if report['order'] != expected or set(report['cases']) != {f'{r}/{c}' for r, c in CASES}:
        raise ValueError('Changed schedule or case matrix')
    if report['conformance_pcm_hashes_verified'] != 120:
        raise ValueError('Missing full conformance equality')
    for key in ['source_sha256', 'binary_sha256']:
        if set(report[key]) != {'before', 'after'} or any(
                re.fullmatch('[0-9a-f]{64}', value) is None for value in report[key].values()):
            raise ValueError('Invalid source/binary provenance')
    for key in ['driver_sha256', 'archive_sha256']:
        if re.fullmatch('[0-9a-f]{64}', report[key]) is None:
            raise ValueError('Invalid driver/corpus provenance')
    if report['archive_sha256'] != conformance.MANIFEST['archive_sha256']:
        raise ValueError('Wrong official corpus')
    before_hardware = report.get('hardware_before')
    after_hardware = report.get('hardware_after')
    if not isinstance(before_hardware, dict) or not isinstance(after_hardware, dict):
        raise ValueError('Missing before/after hardware metadata')
    cpu = before_hardware.get('cpu')
    if (type(cpu) is not int or cpu < 0 or type(after_hardware.get('cpu')) is not int
            or after_hardware.get('cpu') != cpu):
        raise ValueError('Changed or missing CPU affinity')
    for key in ['cpu_model', 'machine', 'kernel']:
        if not isinstance(before_hardware.get(key), str) or not before_hardware[key].strip():
            raise ValueError('Missing hardware identity: ' + key)
        if after_hardware.get(key) != before_hardware[key]:
            raise ValueError('Changed hardware identity: ' + key)
    if not isinstance(report.get('rustc'), str) or not report['rustc'].startswith('rustc '):
        raise ValueError('Missing Rust compiler identity')
    rng = random.Random(62482)
    result = {}
    for name, case in sorted(report['cases'].items()):
        if set(case) != {'before', 'after'} or any(len(case[k]) != rounds for k in case):
            raise ValueError('Incomplete paired samples')
        for a, b in zip(case['before'], case['after']):
            for sample in [a, b]:
                check_sample(sample, PACKETS_PER_PASS, 1)
                if sample['elapsed_ns'] < 200000000:
                    raise ValueError('Sample below200ms')
            compare_samples(a, b)
            if a['decoder_bytes'] != b['decoder_bytes']:
                raise ValueError('Decoder object size changed')
            compare_samples(a, case['before'][0])
            rate = int(name.split('/')[0])
            if a['samples_per_pass'] != 15390480 * rate // 48000:
                raise ValueError('Wrong PCM duration')
        before = [x['elapsed_ns'] for x in case['before']]
        after = [x['elapsed_ns'] for x in case['after']]
        result[name] = interval(before, after, rng)
    # Equal case weight. Reuse the same resampled round indices across cases.
    ratio = math.exp(statistics.mean([math.log(result[name]['ratio']) for name in result]))
    bootstrap = []
    for _ in range(2000):
        indices = [rng.randrange(rounds) for _ in range(rounds)]
        per_case = []
        for case in report['cases'].values():
            a = statistics.median([case['before'][i]['elapsed_ns'] for i in indices])
            b = statistics.median([case['after'][i]['elapsed_ns'] for i in indices])
            per_case.append(math.log(b / a))
        bootstrap.append(math.exp(statistics.mean(per_case)))
    bootstrap.sort()
    return {'cases': result, 'aggregate': {'ratio': ratio, 'ci95': [bootstrap[49], bootstrap[1949]]}}


def verified_conformance(root, path):
    report = json.loads(path.read_text())
    original = conformance.ROOT
    try:
        conformance.ROOT = root
        if conformance.source_fingerprint() != report['source_sha256']:
            raise ValueError('Stale conformance source')
    finally:
        conformance.ROOT = original
    return conformance_cases(report)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--check', type=Path)
    for name in ['before', 'after', 'before-source', 'after-source', 'before-conformance', 'after-conformance', 'archive', 'output']:
        parser.add_argument('--' + name, type=Path)
    parser.add_argument('--rounds', type=int, default=15)
    parser.add_argument('--cpu', type=int, default=0)
    args = parser.parse_args()
    if args.check:
        report = json.loads(args.check.read_text())
        if report.get('status') != 'pass' or report['summary'] != summarize(report):
            raise ValueError('Incomplete or altered comparison')
        print(json.dumps(report['summary']['aggregate']))
        return
    if any(getattr(args, name) is None for name in ['before', 'after', 'before_source', 'after_source', 'before_conformance', 'after_conformance', 'archive', 'output']):
        parser.error('All binary/source/conformance/archive/output arguments required')
    if not 15 <= args.rounds <= 100 or args.cpu not in os.sched_getaffinity(0):
        raise ValueError('Invalid rounds/CPU')
    roots = {'before': args.before_source.resolve(), 'after': args.after_source.resolve()}
    binaries = {'before': args.before.resolve(), 'after': args.after.resolve()}
    a = verified_conformance(roots['before'], args.before_conformance)
    b = verified_conformance(roots['after'], args.after_conformance)
    if any(a[key]['pcm_sha256'] != b[key]['pcm_sha256'] for key in a):
        raise ValueError('Conformance PCM changed')
    args.output.mkdir(parents=True, exist_ok=False)
    vectors = args.output / 'vectors'
    conformance.extract_verified(args.archive, vectors)
    order = paired_schedule(args.rounds)
    report = dict(schema_version=1, track='decoder_before_after', status='in_progress', rounds=args.rounds,
                  created_utc=datetime.now(timezone.utc).isoformat(), order=order,
                  source_sha256={k: sources(v) for k, v in roots.items()}, binary_sha256={k: sha(v) for k, v in binaries.items()},
                  driver_sha256=sha(Path(__file__)), rustc=subprocess.check_output(['rustc', '--version', '--verbose'], text=True).strip(),
                  hardware_before=hardware(args.cpu), archive_sha256=sha(args.archive), conformance_pcm_hashes_verified=120,
                  cases={f'{r}/{c}': {'before': [], 'after': []} for r, c in CASES},
                  scope='Same official packets through public owning PCM APIs; no explicit FEC/PLC throughput, native competitor, encoder speed or SOTA claim.')
    def save():
        (args.output / 'report.json').write_text(json.dumps(report, indent=2, sort_keys=True) + '\n')
    save()
    for step in order:
        rate, channels = step['case'].split('/')
        for name in step['implementations']:
            command = ['taskset', '-c', str(args.cpu), str(binaries[name]), str(vectors), rate, channels, '1', '1']
            result = subprocess.run(command, capture_output=True, text=True, check=True)
            sample = json.loads(result.stdout)
            check_sample(sample, PACKETS_PER_PASS, 1)
            if sha(binaries[name]) != report['binary_sha256'][name] or sources(roots[name]) != report['source_sha256'][name]:
                raise ValueError('Binary/source changed during measurement')
            report['cases'][step['case']][name].append(sample)
            save()
            print(step['round'], step['case'], name, sample['elapsed_ns'], flush=True)
    report['hardware_after'] = hardware(args.cpu)
    report['summary'] = summarize(report)
    report['status'] = 'pass'
    save()
    print(json.dumps(report['summary']['aggregate']))


if __name__ == '__main__':
    main()
