#!/usr/bin/env python3
"""Summarize paired times on the same verified RFC streams at each output geometry."""
from collections import defaultdict
import json
import random
import statistics
import sys


def summary(report, rate, names):
    cases = [c for c in report['cases'] if c['rate'] == rate
             and all(c['validation'][name]['passed'] for name in names)]
    vectors = {c['vector'] for c in cases}
    packets = sum(c['packets'] for c in cases)
    if not packets:
        return {'vectors': [], 'packets': 0}
    times = defaultdict(dict)
    tails = defaultdict(list)
    for rnd, hz, vector, name, elapsed, p99 in report['runs']:
        if hz == rate and vector in vectors and name in names:
            if vector in times[rnd, name]:
                raise ValueError('duplicate timing observation')
            times[rnd, name][vector] = elapsed
            tails[vector, name].append(p99)
    rounds = range(1, report['rounds'] + 1)
    for r in rounds:
        for name in names:
            if set(times[r, name]) != vectors:
                raise ValueError('incomplete observation')
    total = {name: [sum(times[r, name].values()) for r in rounds] for name in names}
    ratios = [a / b for a, b in zip(total[names[0]], total[names[1]])]
    rng = random.Random(report['seed'])
    medians = sorted(statistics.median(rng.choices(ratios, k=len(ratios))) for _ in range(4000))
    return {
        'vectors': sorted(vectors), 'packets': packets, 'decoder_order': names,
        'median_us_per_packet': [statistics.median(total[name]) / packets / 1000 for name in names],
        'opura_to_other_paired_median': statistics.median(ratios),
        'ratio_bootstrap_95_interval': [medians[99], medians[3899]],
        'median_per_stream_p99_us': {
            str(v): [statistics.median(tails[v, name]) / 1000 for name in names] for v in sorted(vectors)},
    }


if __name__ == '__main__':
    with open(sys.argv[1]) as stream:
        report = json.load(stream)
    results = {rate: {other: summary(report, rate, ('opura', other))
                      for other in ('opus-rs', 'libopus')} for rate in (16000, 48000)}
    print(json.dumps(results, indent=2))
