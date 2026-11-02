#!/usr/bin/env python3
"""Optional full-corpus equivalence across SILK/Hybrid speech-state extraction.

Build the same Rust examples from a baseline and candidate checkout first.
All generated vectors/logs stay in a new output directory; the sanitized report
records corpus identity and coverage, never machine-specific executable paths.
An optional pinned reference runs the existing unchanged-native replay tools.
"""
import argparse
import hashlib
import json
from pathlib import Path
import struct
import subprocess
import sys


def cases():
    result = [('silk-mono', 'silk_mono_vectors', [], None),
              ('silk-stereo', 'silk_stereo_vectors', [], None)]
    for channels in [1, 2]:
        result.append((f'silk-dtx-{channels}', 'silk_dtx_vectors', [str(channels)], None))
        for family in ['fec', 'bandwidth', 'adaptive', 'automatic']:
            for mode in ['normal', 'loss']:
                result.append((f'silk-{family}-{channels}-{mode}',
                               f'silk_{family}_vectors', [str(channels), mode], None))
    result.extend([
        ('hybrid-normal', 'hybrid_encoder_vectors', [], 'normal'),
        ('hybrid-dtx', 'hybrid_dtx_vectors', [], 'dtx'),
        ('hybrid-fec', 'hybrid_fec_vectors', [], 'fec'),
        ('hybrid-vbr-fec', 'hybrid_fec_vectors', ['vbr'], 'vbr'),
    ])
    return result


def statistics(path):
    counts = dict(records=0, streams=0, lost=0, fec_requests=0, single_byte_packets=0)
    with path.open('rb') as stream:
        magic = stream.read(8)
        if len(magic) != 8 or not magic.startswith(b'OPURA'):
            raise ValueError('Invalid corpus magic')
        hybrid = magic in [b'OPURAH01', b'OPURAH02']
        layout = struct.Struct('<BBHHIQ' if hybrid else '<BHHIQ')
        while header := stream.read(layout.size):
            if len(header) != layout.size:
                raise ValueError('Truncated corpus header')
            fields = layout.unpack(header)
            flags = fields[0]
            length, pcm_count = fields[2:4] if hybrid else fields[1:3]
            if flags & ~3 or length > 1276 or not 0 < pcm_count <= 11520:
                raise ValueError('Invalid corpus geometry')
            if hybrid and fields[1] not in [1, 2]:
                raise ValueError('Invalid decoder channels')
            if len(stream.read(length)) != length:
                raise ValueError('Truncated corpus payload')
            counts['records'] += 1
            counts['streams'] += bool(flags & 1)
            counts['fec_requests'] += bool(flags & 2)
            counts['lost'] += length == 0
            counts['single_byte_packets'] += length == 1
    if not counts['records'] or not counts['streams']:
        raise ValueError('Empty corpus or missing stream start')
    return counts


def compare(baseline, candidate):
    digest = hashlib.sha256()
    length = 0
    with baseline.open('rb') as left, candidate.open('rb') as right:
        while True:
            a, b = left.read(1048576), right.read(1048576)
            if a != b:
                raise ValueError(f'Corpus differs in block starting at byte {length}')
            if not a:
                break
            length += len(a)
            digest.update(a)
    return length, digest.hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('baseline', type=Path)
    parser.add_argument('candidate', type=Path)
    parser.add_argument('output', type=Path)
    parser.add_argument('report', type=Path)
    parser.add_argument('--reference', type=Path)
    args = parser.parse_args()
    baseline, candidate = args.baseline.resolve(), args.candidate.resolve()
    args.output.mkdir(parents=True, exist_ok=False)
    report = dict(status='in_progress', corpora=[], records=0, streams=0,
                  lost=0, fec_requests=0, single_byte_packets=0,
                  native_replay=bool(args.reference))
    def save():
        args.report.write_text(json.dumps(report, indent=2)+'\n')
    save()
    for name, example, extra, profile in cases():
        a, b = args.output/(name+'-baseline.bin'), args.output/(name+'-candidate.bin')
        with (args.output/(name+'.log')).open('w') as log:
            subprocess.run([str(baseline/example), str(a), *extra], stdout=log, stderr=log, check=True)
            subprocess.run([str(candidate/example), str(b), *extra], stdout=log, stderr=log, check=True)
            length, digest = compare(a, b)
            counts = statistics(b)
            entry = dict(name=name, example=example, arguments=extra, bytes=length,
                         sha256=digest, **counts)
            if args.reference:
                tool = Path(__file__).with_name('hybrid_encoder_validation.py' if profile else 'silk_mono_validation.py')
                command = [sys.executable, str(tool), str(args.reference), str(b)]
                if profile:
                    command.extend([str(args.output/(name+'-native.json')), '--profile', profile])
                subprocess.run(command, stdout=log, stderr=log, check=True)
                entry['native_replay'] = 'pass'
        report['corpora'].append(entry)
        for key in counts:
            report[key] += counts[key]
        save()
        print(name, counts['records'], digest, flush=True)
    if len(report['corpora']) != 24:
        raise ValueError('Incomplete speech-state corpus matrix')
    report['status'] = 'pass'
    save()


if __name__ == '__main__':
    main()
