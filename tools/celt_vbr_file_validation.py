#!/usr/bin/env python3
"""Optional actual CELT VBR/CVBR recording interoperability diagnostics."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile
from audio_baseline import CORPUS, REVISION, compile_reference, native_decode, packets, read_wave, sha
from hybrid_file_validation import pcm_hash


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source', type=Path)
    parser.add_argument('binary', type=Path)
    parser.add_argument('corpus', type=Path)
    parser.add_argument('output', type=Path)
    parser.add_argument('--constrained', action='store_true')
    args = parser.parse_args()
    binary = args.binary.resolve()
    args.output.mkdir(parents=True, exist_ok=False)
    report = {'status': 'in_progress', 'reference_revision': REVISION,
              'binary_sha256': sha(binary), 'tool_sha256': sha(Path(__file__)),
              'version': subprocess.check_output([str(binary), '--version'], text=True).strip(),
              'scope': 'actual CELT CLI/native decode equality and bitrate diagnostics only; no native encoder, perceptual or speed comparison',
              'constrained': args.constrained, 'max_packet_bytes': 1276, 'cases': []}
    fingerprint = hashlib.sha256()
    root = Path(__file__).resolve().parent.parent
    for path in sorted((root/'src').rglob('*.rs')):
        fingerprint.update(path.relative_to(root).as_posix().encode())
        fingerprint.update(b'\0')
        fingerprint.update(path.read_bytes())
    report['codec_source_sha256'] = fingerprint.hexdigest()
    def save():
        (args.output/'report.json').write_text(json.dumps(report, indent=2)+'\n')
    save()
    with tempfile.TemporaryDirectory(prefix='opura-celt-vbr-files-') as temp:
        lib = compile_reference(args.source.resolve(), Path(temp))
        for name, expected in CORPUS.items():
            recording = args.corpus/(name+'.wav')
            if sha(recording) != expected:
                raise ValueError('Wrong hash-pinned recording: '+name)
            channels, original = read_wave(recording)
            count = len(original)//channels
            for ms, size in [('2.5',120),('5',240),('10',480),('20',960)]:
                for bitrate in ([32001,64001] if channels == 1 else [64001,128001]):
                    stem = f'{name}-{ms}-{bitrate}'
                    opus = args.output/(stem+'.opus')
                    wav = args.output/(stem+'.wav')
                    subprocess.run([str(binary), 'encode', str(recording), str(opus), '--mode', 'celt',
                                    '--frame-ms', ms, '--bitrate', str(bitrate),
                                    '--cvbr' if args.constrained else '--vbr'], check=True)
                    subprocess.run([str(binary), 'decode', str(opus), str(wav)], check=True)
                    skip, encoded = packets(opus)
                    if skip != 120:
                        raise ValueError('Wrong CELT lookahead')
                    limit = 1 + (1275 >> (3-[120,240,480,960].index(size)))
                    if any(not 3 <= len(p) <= limit or p[0] >> 3 != 28+[120,240,480,960].index(size) or ((p[0] >> 2) & 1)+1 != channels for p in encoded):
                        raise ValueError('Wrong packet geometry or cap')
                    decoded_channels, decoded = read_wave(wav)
                    native = native_decode(lib, channels, encoded, skip, count)
                    if decoded_channels != channels or len(decoded) != count*channels or decoded != native:
                        raise ValueError('Native CELT PCM/duration mismatch')
                    packet_bytes = sum(map(len,encoded))
                    report['cases'].append({'recording': name, 'corpus_sha256': expected,
                                            'channels': channels, 'frame_ms': float(ms), 'bitrate': bitrate,
                                            'samples_per_channel': count, 'packets': len(encoded), 'skip': skip,
                                            'packet_bytes': packet_bytes,
                                            'packet_bitrate': packet_bytes*8*48000/(len(encoded)*size),
                                            'min_packet_bytes': min(map(len,encoded)), 'max_packet_bytes': max(map(len,encoded)),
                                            'packet_lengths': len(set(map(len,encoded))),
                                            'silence_packets': sum(len(p)==3 and bool(p[1]&128) for p in encoded),
                                            'opus_sha256': sha(opus), 'pcm_sha256': pcm_hash(native), 'native_pcm_match': True})
                    save()
    if len(report['cases']) != 16 or not all(case['packet_lengths']>1 for case in report['cases']):
        raise ValueError('Missing variable-rate coverage')
    report['status'] = 'pass'
    save()
    print(json.dumps({'status': 'pass', 'cases': len(report['cases'])}))


if __name__ == '__main__':
    main()
