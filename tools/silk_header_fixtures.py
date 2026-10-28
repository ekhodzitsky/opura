#!/usr/bin/env python3
"""Extract and execute the actual pinned upstream SILK header loops.

Optional reference generator; not required by Cargo or CI. The source checkout
must be clean Opus v1.6.1. Generated C wrapper/library live only in a temporary
directory. Usage: python3 tools/silk_header_fixtures.py /path/to/opus
"""
import ctypes as c
from pathlib import Path
import subprocess
import sys
import tempfile

REVISION = '22244de5a79bd1d6d623c32e72bf1954b56235be'
source = Path(sys.argv[1]).resolve()
if subprocess.check_output(['git', '-C', str(source), 'rev-parse', 'HEAD'], text=True).strip() != REVISION:
    raise SystemExit('Expected pinned Opus v1.6.1 revision ' + REVISION)
if subprocess.check_output(['git', '-C', str(source), 'status', '--porcelain', '--untracked-files=no']):
    raise SystemExit('Reference checkout has modified tracked files')
api = (source / 'silk/dec_API.c').read_text()
header = api.split('/* Decode VAD flags and LBRR flag */', 1)[1].split('if( lostFlag == FLAG_DECODE_NORMAL )', 1)[0]
tables = (source / 'silk/tables_other.c').read_text()
tables = tables[tables.index('static const opus_uint8 silk_LBRR_flags_2_iCDF'):tables.index('/* Table for LSB coding */')]
wrapper = '''
#include <string.h>
#include "entdec.h"
#define silk_memset memset
#define silk_RSHIFT(a,b) ((a) >> (b))
''' + tables + '''
void probe(unsigned char *input, unsigned len, int channels, int frames, unsigned *out) {
    ec_dec decoder, *psRangeDec = &decoder;
    struct { int nChannelsInternal; } control = {channels}, *decControl = &control;
    struct { int nFramesPerPacket, VAD_flags[3], LBRR_flag, LBRR_flags[3]; } channel_state[2];
    int n, i, LBRR_symbol;
    memset(channel_state, 0, sizeof(channel_state));
    for (n = 0; n < channels; n++) channel_state[n].nFramesPerPacket = frames;
    ec_dec_init(psRangeDec, input, len);
''' + header + '''
    out[0] = out[1] = 0;
    for (n = 0; n < channels; n++) for (i = 0; i < frames; i++) {
        out[0] |= channel_state[n].VAD_flags[i] << (n*3+i);
        out[1] |= channel_state[n].LBRR_flags[i] << (n*3+i);
    }
    out[2] = decoder.rng;
    out[3] = ec_tell(&decoder);
    out[4] = ec_tell_frac(&decoder);
}
'''
with tempfile.TemporaryDirectory(prefix='opura-silk-reference-') as temp:
    path = Path(temp)
    (path / 'probe.c').write_text(wrapper)
    subprocess.run(['cc', '-shared', '-fPIC', '-O2', '-I' + str(source / 'include'),
                    '-I' + str(source / 'celt'), str(path / 'probe.c'),
                    str(source / 'celt/entdec.c'), str(source / 'celt/entcode.c'),
                    '-o', str(path / 'probe.so')], check=True)
    lib = c.CDLL(str(path / 'probe.so'))
    lib.probe.argtypes = [c.POINTER(c.c_ubyte), c.c_uint, c.c_int, c.c_int, c.POINTER(c.c_uint)]
    lib.probe.restype = None
    print('# Opus v1.6.1 SILK header loops and LBRR tables:', REVISION)
    print('# packet_hex status vad_mask lbrr_mask range tell tell_frac')
    print('# Masks use bits 0..2 for mid/mono and 3..5 for side, in temporal order.')
    print('# missing = empty frame; budget = reference tell exceeds payload bits.')
    payloads = [b'', b'\0', b'\xff', b'\xff\xff', bytes(4), bytes([255])*4,
                bytes.fromhex('915ac300'), bytes.fromhex('555ac300'), bytes.fromhex('aa5ac300'),
                bytes.fromhex('345ac300'), bytes.fromhex('785ac300'), bytes.fromhex('de5ac300')]
    for config in range(16):
        frames = (1, 1, 2, 3)[config % 4] if config < 12 else 1
        for channels in [1, 2]:
            cases = payloads
            if config == 3 and channels == 2:
                cases = payloads + [bytes([255, value, 0x5a, 0xc3]) for value in range(256)]
            for data in cases:
                packet = bytes([(config << 3) | ((channels-1) << 2)]) + data
                if not data:
                    print(packet.hex(), 'missing')
                    continue
                array = (c.c_ubyte * len(data))(*data)
                result = (c.c_uint * 5)()
                lib.probe(array, len(data), channels, frames, result)
                status = 'budget' if result[3] > len(data)*8 else 'ok'
                print(packet.hex(), status, *result)
