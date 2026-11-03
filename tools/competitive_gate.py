#!/usr/bin/env python3
"""Fail-closed competitive gate; exit 1 is an unmet target, never a skipped win."""
import argparse
import hashlib
import json
import math
from pathlib import Path
import random
import re
import statistics
import struct

from packet_bench import GROUPS, ROOT, corpus, sha, source_digest


def paired_interval(a, b, rng):
    """Equal-tail95% paired percentile interval; preserve the existing97.5% upper gate."""
    # Paired bootstrap: a and b at each index belong to the same round.
    ratios = []
    for _ in range(2000):
        indices = rng.choices(range(len(a)), k=len(a))
        ratios.append(statistics.median(a[i] for i in indices) /
                      statistics.median(b[i] for i in indices))
    ordered = sorted(ratios)
    return ordered[49], ordered[1949]


def upper_bound(a, b, rng):
    return paired_interval(a, b, rng)[1]


def evaluate(report, scope='packet'):
    if scope != 'packet':
        return ['Full-codec superiority is unestablished by this framing track: '
                'all required competitors, quality, memory and usability measurements are missing.']
    if not isinstance(report, dict):
        return ['Report must be an object.']
    if report.get('schema') != 1 or report.get('track') != 'packet':
        return ['Missing packet benchmark schema/track.']
    for key, size in [('revision', 40), ('source_sha256', 64)]:
        if not re.fullmatch(f'[0-9a-f]{{{size}}}', str(report.get(key, ''))):
            return [f'Missing {key} provenance.']
    library = report.get('libopus', {})
    if not isinstance(library, dict) or not library.get('version') or not re.fullmatch('[0-9a-f]{64}', str(library.get('sha256', ''))):
        return ['Missing libopus version/binary provenance.']
    cases = report.get('cases', {})
    if not isinstance(cases, dict) or set(cases) != set(GROUPS):
        return ['Missing or unexpected workload groups.']
    rounds = report.get('rounds', 0)
    if not isinstance(rounds, int) or rounds < 15:
        return ['At least 15 paired rounds are required.']
    errors = []
    for name, packets in corpus().items():
        case = cases[name]
        if not isinstance(case, dict):
            return [f'{name}: workload must be an object.']
        digest = hashlib.sha256(b''.join(struct.pack('<I', len(p)) + p for p, _ in packets)).hexdigest()
        if (case.get('correct') is not True or case.get('packets') != len(packets)
                or case.get('corpus_sha256') != digest):
            errors.append(f'{name}: missing correctness evidence or changed corpus.')
        samples = case.get('samples_ns', {})
        if not isinstance(samples, dict) or set(samples) != {'opura', 'libopus'}:
            errors.append(f'{name}: missing required implementation.')
            continue
        for impl, values in samples.items():
            if (not isinstance(values, list) or len(values) != rounds or
                    any(type(v) not in (int, float) or not math.isfinite(v) or v <= 0 for v in values)):
                errors.append(f'{name}/{impl}: invalid timing distribution.')
    if errors:
        return errors
    rng = random.Random(6716)
    for name, case in cases.items():
        samples = case['samples_ns']
        upper = upper_bound(samples['opura'], samples['libopus'], rng)
        if upper > 1.05:
            errors.append(f'{name}: ratio upper bound {upper:.3f} exceeds 1.050 regression limit.')
    # Equal weight per workload; preserve round pairing across all workloads.
    aggregates = []
    for _ in range(2000):
        indices = rng.choices(range(rounds), k=rounds)
        ratios = []
        for case in cases.values():
            samples = case['samples_ns']
            ratios.append(statistics.median(samples['opura'][i] for i in indices) /
                          statistics.median(samples['libopus'][i] for i in indices))
        aggregates.append(math.exp(statistics.mean(map(math.log, ratios))))
    upper = sorted(aggregates)[1949]
    if upper > 0.90:
        errors.append(f'Aggregate ratio upper bound {upper:.3f} exceeds 0.900 superiority target.')
    return errors


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('report', type=Path)
    parser.add_argument('--scope', choices=['packet', 'decoder'], default='decoder')
    args = parser.parse_args()
    try:
        report = json.loads(args.report.read_text())
        errors = evaluate(report, args.scope)
        if args.scope == 'packet' and isinstance(report, dict):
            if report.get('source_sha256') != source_digest():
                errors.append('Stale evidence: current Rust source differs from measured source.')
            harness = report.get('harness_sha256', {})
            for name in ('tools/packet_bench.py', 'tools/benchmarks/packet.rs', 'tools/benchmarks/packet.c'):
                if not isinstance(harness, dict) or harness.get(name) != sha(ROOT / name):
                    errors.append(f'Stale or missing harness evidence: {name}.')
    except (OSError, ValueError, TypeError, KeyError) as error:
        errors = [f'Invalid or missing evidence: {error}']
    for error in errors:
        print(f'FAIL: {error}')
    if not errors:
        print('PASS: packet framing vs measured libopus only; no full-codec superiority claim.')
    return bool(errors)


if __name__ == '__main__':
    raise SystemExit(main())
