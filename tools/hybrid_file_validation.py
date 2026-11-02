#!/usr/bin/env python3
"""Optional Hybrid CLI recording interoperability; no quality/speed comparison."""
import argparse
from array import array
import ctypes as c
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile
from audio_baseline import (CORPUS, REVISION, compile_reference, native_decode,
                            packets, read_wave, sha, validate_packet_budgets)


def pcm_hash(values):
    data = array('h', values)
    if sys.byteorder != 'little':
        data.byteswap()
    return hashlib.sha256(data.tobytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source', type=Path)
    parser.add_argument('binary', type=Path)
    parser.add_argument('corpus', type=Path)
    parser.add_argument('output', type=Path)
    parser.add_argument('--dtx', action='store_true')
    parser.add_argument('--fec', type=int, choices=range(101), default=0, metavar='0..100')
    args = parser.parse_args()
    binary = args.binary.resolve()
    args.output.mkdir(parents=True, exist_ok=False)
    report = {'status': 'in_progress', 'reference_revision': REVISION,
              'binary_sha256': sha(binary), 'tool_sha256': sha(Path(__file__)),
              'version': subprocess.check_output([str(binary), '--version'], text=True).strip(),
              'scope': 'actual Hybrid CLI/native decode equality only; no native encoder, perceptual or speed comparison',
              'dtx': args.dtx, 'fec_loss_percent': args.fec, 'cases': []}
    fingerprint = hashlib.sha256()
    root = Path(__file__).resolve().parent.parent
    for path in sorted((root/'src').rglob('*.rs')):
        fingerprint.update(path.relative_to(root).as_posix().encode())
        fingerprint.update(b'\0')
        fingerprint.update(path.read_bytes())
    report['codec_source_sha256'] = fingerprint.hexdigest()
    def save():
        (args.output/'report.json').write_text(json.dumps(report, indent=2) + '\n')
    save()
    with tempfile.TemporaryDirectory(prefix='opura-hybrid-files-') as temp:
        lib = compile_reference(args.source.resolve(), Path(temp))
        for name, expected in CORPUS.items():
            recording = args.corpus/(name+'.wav')
            if sha(recording) != expected:
                raise ValueError('Wrong hash-pinned recording: '+name)
            channels, original = read_wave(recording)
            count = len(original)//channels
            for band in ['swb', 'fb']:
                for ms in [10, 20]:
                    for bitrate in ([32001, 48001] if channels == 1 else [48001, 64001]):
                        stem = f'{name}-{band}-{ms}-{bitrate}'
                        opus = args.output/(stem+'.opus')
                        wav = args.output/(stem+'.wav')
                        subprocess.run([str(binary), 'encode', str(recording), str(opus), '--mode', 'hybrid', '--bandwidth', band,
                                        '--frame-ms', str(ms), '--bitrate', str(bitrate)] + (['--dtx'] if args.dtx else []) + (['--fec', str(args.fec)] if args.fec else []), check=True)
                        subprocess.run([str(binary), 'decode', str(opus), str(wav)], check=True)
                        skip, encoded = packets(opus)
                        if skip != 312:
                            raise ValueError('Wrong Hybrid lookahead')
                        validate_packet_budgets(encoded, bitrate, ms*48, args.dtx)
                        if any(p[0] >> 3 != (12 if band == 'swb' else 14)+(ms == 20) or ((p[0] >> 2) & 1)+1 != channels for p in encoded):
                            raise ValueError('Wrong Hybrid mode/band/duration/channels')
                        lbrr = [lib.opus_packet_has_lbrr((c.c_ubyte*len(p)).from_buffer_copy(p), len(p)) for p in encoded]
                        if any(v < 0 for v in lbrr) or (not args.fec and any(lbrr)):
                            raise ValueError('Invalid or unexpected LBRR')
                        decoded_channels, decoded = read_wave(wav)
                        native = native_decode(lib, channels, encoded, skip, count)
                        if decoded_channels != channels or len(decoded) != count*channels or decoded != native:
                            raise ValueError('Native Hybrid CLI PCM/duration mismatch')
                        case = {'recording': name, 'corpus_sha256': expected, 'channels': channels, 'bandwidth': band,
                                'frame_ms': ms, 'bitrate': bitrate, 'samples_per_channel': count,
                                'packets': len(encoded), 'skip': skip, 'packet_bytes': sum(map(len, encoded)),
                                'opus_sha256': sha(opus), 'pcm_sha256': pcm_hash(native), 'native_pcm_match': True,
                                'lbrr_packets': sum(v > 0 for v in lbrr), 'dtx_packets': sum(len(p) == 1 for p in encoded)}
                        report['cases'].append(case)
                        save()
    if len(report['cases']) != 16:
        raise ValueError('Unexpected recording matrix size')
    report['status'] = 'pass'
    save()
    print('PASS:16 actual Hybrid speech/music files match original native PCM and duration')


if __name__ == '__main__':
    main()
