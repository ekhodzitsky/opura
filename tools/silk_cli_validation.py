#!/usr/bin/env python3
"""Optional real-process SILK WAV/Ogg interoperability and duration validation.

Uses independent FFmpeg decoding; ordinary Cargo tests need no external tool.
"""
import argparse
import hashlib
import json
from pathlib import Path
import struct
import subprocess
import wave


def run(command):
    return subprocess.run(command, check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('binary', type=Path)
    parser.add_argument('output', type=Path)
    parser.add_argument('--channels', type=int, choices=[1, 2], default=1)
    args = parser.parse_args()
    channels = args.channels
    binary = args.binary.resolve()
    args.output.mkdir(parents=True, exist_ok=False)
    rows = []
    for rate in [8000, 12000, 16000, 24000, 48000]:
        for bandwidth in ['nb', 'mb', 'wb']:
            for ms in [10, 20, 40, 60]:
                folder = args.output/f'{rate}-{bandwidth}-{ms}'
                folder.mkdir()
                frames = rate//7+17
                samples = [value for i in range(frames)
                           for value in ([((i % 80)-40)*400] if channels == 1 else
                                         [((i % 80)-40)*400, ((i % 53)-26)*600])]
                with wave.open(str(folder/'input.wav'), 'wb') as output:
                    output.setparams((channels, 2, rate, 0, 'NONE', 'not compressed'))
                    output.writeframes(struct.pack('<'+'h'*len(samples), *samples))
                run([str(binary), 'encode', str(folder/'input.wav'), str(folder/'encoded.opus'),
                     '--mode', 'silk', '--bandwidth', bandwidth, '--frame-ms', str(ms), '--bitrate', '32101'])
                run([str(binary), 'decode', str(folder/'encoded.opus'), str(folder/'opura.wav')])
                def decoded_with(decoder, path):
                    return run(['ffmpeg', '-v', 'error', '-c:a', decoder, '-i', str(path),
                                '-f', 's16le', '-acodec', 'pcm_s16le', '-ar', '48000', '-ac', str(channels), '-']).stdout
                independent = decoded_with('libopus', folder/'encoded.opus')
                builtin = decoded_with('opus', folder/'encoded.opus')
                expected = frames*(48000//rate)
                expected_bytes = expected*channels*2
                with wave.open(str(folder/'opura.wav'), 'rb') as decoded:
                    if (decoded.getframerate(), decoded.getnchannels(), decoded.getsampwidth(), decoded.getnframes()) != (48000, channels, 2, expected):
                        raise SystemExit(f'Own WAV geometry/duration mismatch: {folder.name}')
                    pcm = decoded.readframes(expected)
                if len(independent) != expected_bytes:
                    raise SystemExit(f'Independent duration mismatch: {folder.name}: {len(independent)//(channels*2)}/{expected}')
                reproduction = None
                if len(builtin) != expected_bytes:
                    # Do not hide a default-decoder failure: require an independent
                    # libopus encoder to reproduce the same FFmpeg duration bug.
                    original = folder/'ffmpeg-original.opus'
                    run(['ffmpeg', '-v', 'error', '-i', str(folder/'input.wav'), '-c:a', 'libopus',
                         '-application', 'voip', '-frame_duration', str(ms), '-cutoff',
                         {'nb': '4000', 'mb': '6000', 'wb': '8000'}[bandwidth], '-b:a',
                         {'nb': '16000', 'mb': '20000', 'wb': '24000'}[bandwidth], '-vbr', 'off', str(original)])
                    reproduced = decoded_with('opus', original)
                    reference = decoded_with('libopus', original)
                    if len(reproduced) != len(builtin) or len(reference) != expected_bytes:
                        raise SystemExit(f'Unexplained default FFmpeg duration failure: {folder.name}')
                    reproduction = {'native_encoder': 'FFmpeg libopus/voip/CBR',
                                    'default_decoder_samples': len(reproduced)//(channels*2),
                                    'reference_decoder_samples': len(reference)//(channels*2)}
                packet_data = folder.joinpath('encoded.opus').read_bytes()
                head = packet_data.index(b'OpusHead')
                if struct.unpack_from('<H', packet_data, head+10)[0] != 312:
                    raise SystemExit('Incorrect SILK pre-skip')
                rows.append({'rate': rate, 'bandwidth': bandwidth, 'frame_ms': ms,
                             'channels': channels, 'source_samples': frames, 'decoded_samples_48k': expected,
                             'opura_pcm_sha256': hashlib.sha256(pcm).hexdigest(),
                             'ffmpeg_libopus_pcm_sha256': hashlib.sha256(independent).hexdigest(),
                             'ffmpeg_builtin_samples_48k': len(builtin)//(channels*2),
                             'ffmpeg_builtin_reference_reproduction': reproduction,
                             'ogg_sha256': hashlib.sha256(packet_data).hexdigest()})
    report = {'version': run([str(binary), '--version']).stdout.decode().strip(),
              'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(),
              'ffmpeg_version': run(['ffmpeg', '-version']).stdout.decode().splitlines()[0],
              'cases': rows, 'status': 'pass',
              'scope': f'60 synthetic {channels}-channel CLI files; FFmpeg/libopus exact duration, pre-skip; builtin decoder failures retained and independently reproduced; no quality or speed claim'}
    (args.output/'report.json').write_text(json.dumps(report, indent=2)+'\n')
    print(f'PASS: 60 {channels}-channel SILK CLI files have exact FFmpeg/libopus durations and312-sample pre-skip')
    print('Retained default FFmpeg decoder duration failures:', sum(row['ffmpeg_builtin_reference_reproduction'] is not None for row in rows))


if __name__ == '__main__':
    main()
