#!/usr/bin/env python3
"""Optional unchanged-native decoding of actual CELT VBR/CVBR PCM/loss corpus."""
import argparse
import ctypes as c
import hashlib
import json
from pathlib import Path
import struct
import tempfile
from audio_baseline import compile_reference, REVISION
from silk_mono_validation import fingerprint


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source', type=Path)
    parser.add_argument('vectors', type=Path)
    parser.add_argument('report', type=Path)
    parser.add_argument('--channel-transitions', action='store_true',
                        help='Allow variable wire channels with a fixed decoder output layout')
    args = parser.parse_args()
    report = {'status': 'in_progress', 'reference_revision': REVISION,
              'vector_sha256': hashlib.sha256(args.vectors.read_bytes()).hexdigest(),
              'streams': 0, 'records': 0, 'lost': 0, 'silence_packets': 0}
    if args.channel_transitions:
        report.update(mono_packets=0, stereo_packets=0, received_wire_changes=0)
    args.report.write_text(json.dumps(report, indent=2)+'\n')
    with tempfile.TemporaryDirectory(prefix='opura-celt-vbr-decode-') as temp:
        lib = compile_reference(args.source.resolve(), Path(temp))
        lib.opus_decoder_ctl.argtypes = [c.c_void_p, c.c_int]
        lib.opus_decoder_ctl.restype = c.c_int
        decoder = None
        try:
            with args.vectors.open('rb') as vectors:
                if vectors.read(8) != b'OPURACV1':
                    raise ValueError('Wrong CELT VBR corpus format')
                channels = 0
                previous_wire = None
                while header := vectors.read(18):
                    if len(header) != 18:
                        raise ValueError('Truncated record header')
                    reset, actual_channels, length, count, expected_range, expected_hash = struct.unpack('<BBHHIQ', header)
                    if reset not in (0, 1) or actual_channels not in (1, 2) or length > 1276:
                        raise ValueError('Invalid record geometry')
                    packet = vectors.read(length)
                    if len(packet) != length:
                        raise ValueError('Truncated packet')
                    if reset:
                        if decoder:
                            lib.opus_decoder_destroy(decoder)
                            decoder = None
                        error = c.c_int()
                        decoder = lib.opus_decoder_create(48000, actual_channels, c.byref(error))
                        if not decoder or error.value:
                            raise ValueError('Native decoder creation failed')
                        channels = actual_channels
                        previous_wire = None
                        report['streams'] += 1
                    if not decoder or channels != actual_channels or count % channels or count // channels not in (120,240,480,960):
                        raise ValueError('Invalid duration or missing stream start')
                    if packet and (packet[0] >> 3 < 16 or (not args.channel_transitions and ((packet[0] >> 2) & 1)+1 != channels)):
                        raise ValueError('Wrong CELT packet geometry')
                    if packet and args.channel_transitions:
                        wire = ((packet[0] >> 2) & 1)+1
                        report['mono_packets' if wire == 1 else 'stereo_packets'] += 1
                        report['received_wire_changes'] += int(previous_wire is not None and previous_wire != wire)
                        previous_wire = wire
                    pcm = (c.c_int16*1920)()
                    data = (c.c_ubyte*length).from_buffer_copy(packet) if packet else None
                    actual = lib.opus_decode(decoder, data, length, pcm, count // channels, 0)
                    final_range = c.c_uint32()
                    status = lib.opus_decoder_ctl(decoder, 4031, c.byref(final_range))
                    if actual*channels != count or status or final_range.value != expected_range or fingerprint(pcm[:count]) != expected_hash:
                        raise ValueError(f'Native mismatch at record {report["records"]}: samples={actual*channels}/{count}, range={final_range.value}/{expected_range}')
                    report['records'] += 1
                    report['lost'] += int(not packet)
                    report['silence_packets'] += int(len(packet) == 3 and packet[1] & 128 != 0)
        finally:
            if decoder:
                lib.opus_decoder_destroy(decoder)
    if (report['streams'], report['records'], report['lost']) != (80, 10240, 440) or report['silence_packets'] == 0:
        raise ValueError('Unexpected corpus size or missing silence/loss coverage')
    if args.channel_transitions and min(report['mono_packets'], report['stereo_packets'], report['received_wire_changes']) < 100:
        raise ValueError('Missing encoded-channel transition coverage')
    report['status'] = 'pass'
    args.report.write_text(json.dumps(report, indent=2)+'\n')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
