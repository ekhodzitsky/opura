#!/usr/bin/env python3
"""Optional real-process SILK WAV/Ogg interoperability and duration validation.

Uses independent FFmpeg decoding; ordinary Cargo tests need no external tool.
"""
import argparse
import array
import ctypes as c
from ctypes.util import find_library
import hashlib
import json
import re
from pathlib import Path
import struct
import subprocess
import wave
from audio_baseline import packets


def run(command):
    return subprocess.run(command, check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('binary', type=Path)
    parser.add_argument('output', type=Path)
    parser.add_argument('--channels', type=int, choices=[1, 2], default=1)
    parser.add_argument('--dtx', action='store_true')
    parser.add_argument('--automatic', action='store_true')
    parser.add_argument('--fec', type=int, choices=range(101), default=0)
    args = parser.parse_args()
    channels = args.channels
    bitrate = 64101 if args.fec and channels == 2 else 32101
    probe = None
    probe_version = None
    if args.fec:
        name = find_library('opus')
        if name is None:
            raise SystemExit('Optional FEC packet checks require external libopus')
        probe = c.CDLL(name)
        if not hasattr(probe, 'opus_packet_has_lbrr'):
            raise SystemExit('External libopus lacks opus_packet_has_lbrr; use a newer optional validator library')
        probe.opus_packet_has_lbrr.argtypes = [c.POINTER(c.c_ubyte), c.c_int]
        probe.opus_packet_has_lbrr.restype = c.c_int
        probe.opus_get_version_string.restype = c.c_char_p
        probe_version = probe.opus_get_version_string().decode()
    binary = args.binary.resolve()
    args.output.mkdir(parents=True, exist_ok=False)
    rows = []
    for rate in [8000, 12000, 16000, 24000, 48000]:
        for bandwidth in (['auto'] if args.automatic else ['nb', 'mb', 'wb']):
            for ms in [10, 20, 40, 60]:
                folder = args.output/f'{rate}-{bandwidth}-{ms}'
                folder.mkdir()
                frames = (rate*2 if args.dtx or args.fec else rate//7)+17
                samples = [value for i in range(frames)
                           for value in ([((i % 80)-40)*400] if channels == 1 else
                                         [((i % 80)-40)*400, ((i % 53)-26)*600])]
                if args.dtx:
                    # Speech -> digital silence -> speech; preserve all timestamp slots.
                    for i in range(rate//10, rate*3//2):
                        samples[i*channels:(i+1)*channels] = [0]*channels
                with wave.open(str(folder/'input.wav'), 'wb') as output:
                    output.setparams((channels, 2, rate, 0, 'NONE', 'not compressed'))
                    output.writeframes(struct.pack('<'+'h'*len(samples), *samples))
                encoded = run([str(binary), 'encode', str(folder/'input.wav'), str(folder/'encoded.opus'),
                     '--mode', 'silk', '--bandwidth', bandwidth, '--frame-ms', str(ms), '--bitrate', str(bitrate),
                     *(['--dtx'] if args.dtx else []), *(['--fec', str(args.fec)] if args.fec else [])])
                plc_match = re.search(rb'(\d+) PLC fallback packets', encoded.stderr)
                if plc_match is None:
                    raise SystemExit('Missing CLI PLC diagnostics')
                plc_packets = int(plc_match[1])
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
                # FFmpeg can embed a different libopus version/configuration.
                # Retain its PCM deltas; exact fixed-point reference equality is
                # separately enforced by silk_cli_native_validation.py.
                own_samples = array.array('h', pcm)
                other_samples = array.array('h', independent)
                deltas = [abs(a-b) for a,b in zip(own_samples,other_samples) if a!=b]
                reproduction = None
                if len(builtin) != expected_bytes:
                    # Do not hide a default-decoder failure: require an independent
                    # libopus encoder to reproduce the same FFmpeg duration bug.
                    original = folder/'ffmpeg-original.opus'
                    run(['ffmpeg', '-v', 'error', '-i', str(folder/'input.wav'), '-c:a', 'libopus',
                         '-application', 'voip', '-frame_duration', str(ms), '-cutoff',
                         {'nb': '4000', 'mb': '6000', 'wb': '8000'}.get(bandwidth, str(min(rate//2, 8000))), '-b:a',
                         {'nb': '16000', 'mb': '20000', 'wb': '24000'}.get(bandwidth, str(bitrate)), '-vbr', 'off', str(original)])
                    reproduced = decoded_with('opus', original)
                    reference = decoded_with('libopus', original)
                    if len(reproduced) != len(builtin) or len(reference) != expected_bytes:
                        raise SystemExit(f'Unexplained default FFmpeg duration failure: {folder.name}')
                    reproduction = {'native_encoder': 'FFmpeg libopus/voip/CBR',
                                    'default_decoder_samples': len(reproduced)//(channels*2),
                                    'reference_decoder_samples': len(reference)//(channels*2)}
                skip, encoded_packets = packets(folder/'encoded.opus')
                if skip != 312 or len(encoded_packets) != (expected+312+ms*48-1)//(ms*48):
                    raise SystemExit(f'Packet duration/preskip accounting mismatch: {folder.name}')
                suppressed = sum(len(packet) == 1 for packet in encoded_packets)
                if args.dtx and suppressed < 1:
                    raise SystemExit(f'No DTX packets: {folder.name}')
                fec_packets = 0
                if probe:
                    for packet in encoded_packets:
                        present = probe.opus_packet_has_lbrr((c.c_ubyte*len(packet)).from_buffer_copy(packet), len(packet))
                        if present < 0:
                            raise SystemExit(f'Native FEC packet probe failed: {folder.name}')
                        fec_packets += present != 0
                    if fec_packets == 0:
                        raise SystemExit(f'No independently detected FEC packets: {folder.name}')
                for index, packet in enumerate(encoded_packets):
                    expected_budget = ((index+1)*bitrate*ms+4000)//8000 - (index*bitrate*ms+4000)//8000
                    if len(packet) != expected_budget and not (args.dtx and len(packet) == 1):
                        raise SystemExit(f'Unexpected CBR packet size: {folder.name}/{index}')
                packet_data = folder.joinpath('encoded.opus').read_bytes()
                head = packet_data.index(b'OpusHead')
                if struct.unpack_from('<H', packet_data, head+10)[0] != 312:
                    raise SystemExit('Incorrect SILK pre-skip')
                rows.append({'rate': rate, 'bandwidth': bandwidth, 'frame_ms': ms,
                             'channels': channels, 'bitrate': bitrate, 'fec_loss_percent': args.fec,
                             'fec_packets': fec_packets, 'plc_packets': plc_packets, 'dtx': args.dtx, 'dtx_packets': suppressed,
                             'packets': len(encoded_packets), 'packet_bytes': sum(map(len, encoded_packets)),
                             'source_samples': frames, 'decoded_samples_48k': expected,
                             'opura_pcm_sha256': hashlib.sha256(pcm).hexdigest(),
                             'ffmpeg_libopus_pcm_sha256': hashlib.sha256(independent).hexdigest(),
                             'ffmpeg_libopus_differing_samples': len(deltas),
                             'ffmpeg_libopus_max_abs_difference': max(deltas,default=0),
                             'ffmpeg_builtin_samples_48k': len(builtin)//(channels*2),
                             'ffmpeg_builtin_reference_reproduction': reproduction,
                             'ogg_sha256': hashlib.sha256(packet_data).hexdigest()})
    report = {'version': run([str(binary), '--version']).stdout.decode().strip(),
              'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(),
              'fec_probe_version': probe_version,
              'ffmpeg_version': run(['ffmpeg', '-version']).stdout.decode().splitlines()[0],
              'cases': rows, 'status': 'pass',
              'scope': f'{len(rows)} synthetic {channels}-channel CLI files, DTX={args.dtx}, FEC loss={args.fec}; FFmpeg/libopus exact duration, pre-skip; builtin decoder failures retained and independently reproduced; no quality or speed claim'}
    (args.output/'report.json').write_text(json.dumps(report, indent=2)+'\n')
    print(f'PASS: {len(rows)} {channels}-channel SILK CLI files have exact FFmpeg/libopus durations and312-sample pre-skip')
    print('Retained default FFmpeg decoder duration failures:', sum(row['ffmpeg_builtin_reference_reproduction'] is not None for row in rows))


if __name__ == '__main__':
    main()
