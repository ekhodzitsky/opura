#!/usr/bin/env python3
"""Optional original-libopus cross-decode of connected Hybrid PCM/loss vectors."""
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
    parser.add_argument('--profile', choices=['normal', 'dtx', 'fec'], default='normal')
    args = parser.parse_args()
    report = {'status': 'in_progress', 'reference_revision': REVISION,
              'vector_sha256': hashlib.sha256(args.vectors.read_bytes()).hexdigest(),
              'profile': args.profile, 'streams': 0, 'records': 0, 'lost': 0, 'dtx_packets': 0,
              'fec_requests': 0, 'fec_requests_with_any_lbrr': 0, 'lbrr_primary_records': 0, 'dtx_fec_requests': 0}
    args.report.write_text(json.dumps(report, indent=2) + '\n')
    with tempfile.TemporaryDirectory(prefix='opura-hybrid-validation-') as temp:
        lib = compile_reference(args.source.resolve(), Path(temp))
        lib.opus_decoder_ctl.argtypes = [c.c_void_p, c.c_int]
        lib.opus_decoder_ctl.restype = c.c_int
        decoder = None
        try:
            with args.vectors.open('rb') as vectors:
                if vectors.read(8) != (b'OPURAH02' if args.profile == 'fec' else b'OPURAH01'):
                    raise ValueError('Wrong Hybrid vector format')
                channels = 0
                while header := vectors.read(18):
                    if len(header) != 18:
                        raise ValueError('Truncated vector metadata')
                    reset, actual_channels, length, count, expected_range, expected_hash = struct.unpack('<BBHHIQ', header)
                    if reset & ~(3 if args.profile == 'fec' else 1) or actual_channels not in (1, 2) or length > 1276:
                        raise ValueError('Invalid Hybrid vector geometry')
                    packet = vectors.read(length)
                    if len(packet) != length:
                        raise ValueError('Truncated packet')
                    fec = bool(reset & 2)
                    if reset & 1:
                        if decoder:
                            lib.opus_decoder_destroy(decoder)
                            decoder = None
                        error = c.c_int()
                        decoder = lib.opus_decoder_create(48000, actual_channels, c.byref(error))
                        if not decoder or error.value:
                            raise ValueError('Native decoder creation failed')
                        channels = actual_channels
                        report['streams'] += 1
                    if not decoder or channels != actual_channels or count // channels not in (480, 960) or count % channels:
                        raise ValueError('Missing stream start or invalid duration')
                    if packet and not 12 <= packet[0] >> 3 <= 15:
                        raise ValueError('Not a Hybrid packet')
                    pcm = (c.c_int16*1920)()
                    data = (c.c_ubyte*length).from_buffer_copy(packet) if packet else None
                    actual = lib.opus_decode(decoder, data, length, pcm, count // channels, int(fec))
                    final_range = c.c_uint32()
                    status = lib.opus_decoder_ctl(decoder, 4031, c.byref(final_range))
                    if actual * channels != count or status or final_range.value != expected_range or fingerprint(pcm[:count]) != expected_hash:
                        raise ValueError(f'Native mismatch at record {report["records"]}: samples={actual * channels}/{count}, range={final_range.value}/{expected_range}')
                    report['records'] += 1
                    report['lost'] += int(not packet)
                    report['dtx_packets'] += int(not fec and len(packet) == 1)
                    report['dtx_fec_requests'] += int(fec and len(packet) == 1)
                    report['fec_requests'] += int(fec)
                    lbrr = bool(packet) and lib.opus_packet_has_lbrr(data, length) > 0
                    report['lbrr_primary_records'] += int(not fec and lbrr)
                    report['fec_requests_with_any_lbrr'] += int(fec and lbrr)
        finally:
            if decoder:
                lib.opus_decoder_destroy(decoder)
    expected = {'normal': (240, 5760, 480), 'dtx': (144, 21600, 288), 'fec': (1152, 184320, 0)}[args.profile]
    if (report['streams'], report['records'], report['lost']) != expected:
        raise ValueError('Unexpected corpus size')
    if args.profile == 'dtx' and report['dtx_packets'] == 0:
        raise ValueError('Missing DTX coverage')
    if args.profile == 'fec' and (report['fec_requests'] != 30528 or report['lbrr_primary_records'] == 0 or report['dtx_packets'] == 0):
        raise ValueError('Missing FEC/DTX coverage')
    report['status'] = 'pass'
    args.report.write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
