#!/usr/bin/env python3
"""Optional unchanged native DC oracle and mono/stereo packet cross-decoder validation.

Generate vectors with the silk_mono_vectors or silk_stereo_vectors Rust example. Neither a C compiler
nor native codec library is a normal build/test dependency.
"""
import argparse
import ctypes as c
from pathlib import Path
import re
import struct
import subprocess
import tempfile
from silk_nsq_fixtures import REVISION


def fingerprint(samples):
    value = 14695981039346656037
    for sample in samples:
        for byte in struct.pack('<h', sample):
            value = ((value ^ byte)*1099511628211) & ((1 << 64)-1)
    return value


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source', type=Path)
    parser.add_argument('vectors', type=Path, nargs='?')
    parser.add_argument('--dc-fixtures', type=Path)
    args = parser.parse_args()
    if args.vectors is None and args.dc_fixtures is None:
        parser.error('supply vectors or --dc-fixtures')
    source = args.source.resolve()
    if subprocess.check_output(['git', '-C', str(source), 'rev-parse', 'HEAD'], text=True).strip() != REVISION:
        raise SystemExit('Expected pinned Opus v1.6.1')
    if subprocess.check_output(['git', '-C', str(source), 'status', '--porcelain'], text=True).strip():
        raise SystemExit('Reference checkout must be unchanged')
    def manifest(file, variable):
        match = re.search(r'^'+variable+r' = (.*?)(?=\n\n|\Z)', (source/file).read_text(), re.M|re.S)
        if match is None:
            raise SystemExit('Missing source manifest')
        return match[1].replace('\\', '').split()
    files = (manifest('silk_sources.mk', 'SILK_SOURCES') + manifest('silk_sources.mk', 'SILK_SOURCES_FIXED') +
             manifest('celt_sources.mk', 'CELT_SOURCES') + manifest('opus_sources.mk', 'OPUS_SOURCES'))
    original = (source/'src/opus_encoder.c').read_text()
    start = original.index('static void dc_reject(')
    brace = original.index('{', start)
    depth = 1
    end = brace+1
    while depth:
        depth += (original[end] == '{')-(original[end] == '}')
        end += 1
    wrapper = '#include "arch.h"\n#include "mathops.h"\n' + original[start:end]
    wrapper += '\nvoid dc_probe(const opus_int16 *x,opus_int16 *out,opus_int32 *state,int n,int rate){dc_reject(x,3,out,state,n,1,rate);}\n'
    with tempfile.TemporaryDirectory(prefix='opura-silk-mono-') as temp:
        folder = Path(temp)
        (folder/'probe.c').write_text(wrapper)
        subprocess.run(['cc', '-shared', '-fPIC', '-O2', '-fwrapv', '-DVAR_ARRAYS', '-DOPUS_BUILD', '-DFIXED_POINT', '-DDISABLE_FLOAT_API',
                        *['-I'+str(source/p) for p in ['include','celt','silk','silk/fixed','src']],
                        str(folder/'probe.c'), *[str(source/f) for f in files], '-lm', '-o', str(folder/'probe.so')], check=True)
        lib = c.CDLL(str(folder/'probe.so'))
        lib.dc_probe.restype = None
        lib.dc_probe.argtypes = [c.POINTER(c.c_int16), c.POINTER(c.c_int16), c.POINTER(c.c_int32), c.c_int, c.c_int]
        if args.dc_fixtures is not None:
            rows = ['# Unchanged native dc_reject, Opus '+REVISION+'; RATE BEFORE AFTER PCM_FNV64 INPUT[n]']
            random = 0x44434649
            for rate in [8000, 12000, 16000, 24000, 48000]:
                state = c.c_int32()
                for block in range(8):
                    length = (10 if block % 2 == 0 else 20)*rate//1000
                    samples = []
                    for i in range(length):
                        random = (1664525*random+1013904223) & 0xffffffff
                        samples.append(0 if block == 0 else -32768 if block == 1 else 32767 if block == 2 else (-32768 if i % 2 else 32767) if block == 3 else (i % 64)*1024-32768 if block == 4 else (random >> 16)-32768)
                    before = state.value
                    output = (c.c_int16*length)()
                    lib.dc_probe((c.c_int16*length)(*samples), output, c.byref(state), length, rate)
                    rows.append(' '.join(map(str, [rate, before, state.value, fingerprint(output), *samples])))
            args.dc_fixtures.write_text('\n'.join(rows)+'\n')
            print('Captured 40 native DC input/output/state cases')
        if args.vectors is not None:
            lib.opus_decoder_create.argtypes = [c.c_int, c.c_int, c.POINTER(c.c_int)]
            lib.opus_decoder_create.restype = c.c_void_p
            lib.opus_decoder_destroy.argtypes = [c.c_void_p]
            lib.opus_decode.argtypes = [c.c_void_p, c.POINTER(c.c_ubyte), c.c_int, c.POINTER(c.c_int16), c.c_int, c.c_int]
            lib.opus_decoder_ctl.argtypes = [c.c_void_p, c.c_int]
            decoder = None
            packets = streams = 0
            try:
                with args.vectors.open('rb') as vectors:
                    magic = vectors.read(8)
                    if magic not in [b'OPURAM01', b'OPURAS01']:
                        raise SystemExit('Invalid Rust vector header')
                    channels = 1 if magic == b'OPURAM01' else 2
                    while header := vectors.read(17):
                        if len(header) != 17:
                            raise SystemExit('Truncated vector metadata')
                        reset, length, count, expected_range, expected_hash = struct.unpack('<BHHIQ', header)
                        packet = vectors.read(length)
                        if len(packet) != length:
                            raise SystemExit('Truncated packet')
                        if reset:
                            if decoder:
                                lib.opus_decoder_destroy(decoder)
                            error = c.c_int()
                            decoder = lib.opus_decoder_create(48000, channels, c.byref(error))
                            if not decoder or error.value:
                                raise SystemExit('Native decoder initialization failed')
                            streams += 1
                        if not decoder:
                            raise SystemExit('Missing stream start')
                        pcm = (c.c_int16*(2880*channels))()
                        actual = lib.opus_decode(decoder, (c.c_ubyte*length).from_buffer_copy(packet), length, pcm, 2880, 0)
                        final_range = c.c_uint32()
                        status = lib.opus_decoder_ctl(decoder, 4031, c.byref(final_range))
                        if actual*channels != count or status or final_range.value != expected_range or fingerprint(pcm[:actual*channels]) != expected_hash:
                            raise SystemExit(f'Native cross-decoder mismatch at packet {packets}: samples={actual}/{count}, range={final_range.value}/{expected_range}')
                        packets += 1
            finally:
                if decoder:
                    lib.opus_decoder_destroy(decoder)
            if packets != (1440 if channels == 1 else 2880) or streams != 120:
                raise SystemExit(f'Unexpected corpus size: {streams} streams/{packets} packets')
            print(f'PASS: {streams} raw-PCM {channels}-channel streams, {packets} packets match unchanged native PCM/final ranges')


if __name__ == '__main__':
    main()
