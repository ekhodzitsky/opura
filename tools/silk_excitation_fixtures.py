#!/usr/bin/env python3
"""Optional Opus pulse/Q14 excitation oracle; never part of Cargo builds.
Usage: python3 tools/silk_excitation_fixtures.py /path/to/opus [--tables]
"""
import argparse
import ctypes as c
from pathlib import Path
import re
import subprocess
import tempfile

REVISION = '22244de5a79bd1d6d623c32e72bf1954b56235be'
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('source', type=Path)
parser.add_argument('--tables', action='store_true')
args = parser.parse_args()
source = args.source.resolve()
if subprocess.check_output(['git', '-C', str(source), 'rev-parse', 'HEAD'], text=True).strip() != REVISION:
    raise SystemExit('Expected Opus v1.6.1 at ' + REVISION)
if subprocess.check_output(['git', '-C', str(source), 'status', '--porcelain', '--untracked-files=no']):
    raise SystemExit('Reference checkout has modified tracked files')
if args.tables:
    content = (source / 'silk/tables_pulses_per_block.c').read_text()
    specs = [('silk_pulses_per_block_iCDF', 'SUMS', 180), ('silk_rate_levels_iCDF', 'RATES', 18),
             ('silk_shell_code_table_offsets', 'OFFSETS', 17), ('silk_sign_iCDF', 'SIGNS', 42)]
    specs += [(f'silk_shell_code_table{i}', f'SHELL{i}', 152) for i in range(4)]
    print('// Generated from Opus v1.6.1 by tools/silk_excitation_fixtures.py --tables.')
    print('// Copyright (c) 2006-2011, Skype Limited. See THIRD_PARTY_NOTICES.md.')
    for original, name, size in specs:
        match = re.search(r'\b' + original + r'\s*\[[^;=]+?=\s*\{(.*?)\};', content, re.S)
        if not match: raise SystemExit('Missing table ' + original)
        values = [int(s) for s in re.findall(r'\d+', match[1])]
        if len(values) != size: raise SystemExit('Wrong table length ' + original)
        print(f'pub(super) const {name}: [u8; {size}] = {values};')
    raise SystemExit(0)

core = (source / 'silk/decode_core.c').read_text()
section = core[core.index('    rand_seed = psDec->indices.Seed;'):core.index('    /* Copy LPC state */')]
wrapper = '''
#include "main.h"
static void excitation(silk_decoder_state *psDec, const opus_int16 *pulses) {
    int i;
    opus_int32 rand_seed;
    opus_int32 offset_Q10 = silk_Quantization_Offsets_Q10[psDec->indices.signalType >> 1][psDec->indices.quantOffsetType];
''' + section + '''
}
void probe(unsigned char *data, unsigned len, int length, int signal, int offset, int seed,
           opus_int16 *pulses, opus_int32 *out, unsigned *entropy) {
    silk_decoder_state state = {0};
    ec_dec decoder;
    ec_dec_init(&decoder, data, len);
    state.frame_length = length;
    state.indices.signalType = signal;
    state.indices.quantOffsetType = offset;
    state.indices.Seed = seed;
    silk_decode_pulses(&decoder, pulses, signal, offset, length);
    excitation(&state, pulses);
    memcpy(out, state.exc_Q14, length * sizeof(*out));
    entropy[0] = decoder.rng;
    entropy[1] = ec_tell(&decoder);
    entropy[2] = ec_tell_frac(&decoder);
}
'''

def fingerprint(values):
    result = 14695981039346656037
    for value in values:
        for byte in (value & 0xffffffff).to_bytes(4, 'little'):
            result = ((result ^ byte) * 1099511628211) & 0xffffffffffffffff
    return result

with tempfile.TemporaryDirectory(prefix='opura-excitation-reference-') as temp:
    path = Path(temp)
    (path / 'probe.c').write_text(wrapper)
    files = ['celt/entdec.c', 'celt/entenc.c', 'celt/entcode.c', 'silk/tables_other.c',
             'silk/tables_pulses_per_block.c', 'silk/decode_pulses.c', 'silk/shell_coder.c', 'silk/code_signs.c']
    subprocess.run(['cc', '-shared', '-fPIC', '-O2', '-I' + str(source / 'include'),
                    '-I' + str(source / 'celt'), '-I' + str(source / 'silk'), str(path / 'probe.c')]
                   + [str(source / file) for file in files] + ['-o', str(path / 'probe.so')], check=True)
    lib = c.CDLL(str(path / 'probe.so'))
    lib.probe.argtypes = [c.POINTER(c.c_ubyte), c.c_uint, c.c_int, c.c_int, c.c_int, c.c_int,
                          c.POINTER(c.c_int16), c.POINTER(c.c_int32), c.POINTER(c.c_uint)]
    lib.probe.restype = None
    print('# Opus v1.6.1 pulse decoder and decode_core excitation:', REVISION)
    print('# input HEX; LENGTH SIGNAL OFFSET SEED PULSE_FNV64 Q14_FNV64 RANGE TELL TELL_FRAC')
    print('# FNV-1a over LE-u32 signed sample bit patterns; only LENGTH samples, excluding shell padding.')
    inputs = [bytes(1024), bytes([255])*1024, bytes(range(256))*4]
    random = 0x65786369
    for _ in range(8):
        data = bytearray()
        for _ in range(1024):
            random ^= (random << 13) & 0xffffffff
            random ^= random >> 17
            random ^= (random << 5) & 0xffffffff
            data.append(random & 255)
        inputs.append(bytes(data))
    for data in inputs:
        print('input', data.hex())
        array = (c.c_ubyte * len(data))(*data)
        for length in [80, 120, 160, 240, 320]:
            for signal in range(3):
                for offset in range(2):
                    for seed in range(4):
                        pulses = (c.c_int16 * 320)()
                        output = (c.c_int32 * 320)()
                        entropy = (c.c_uint * 3)()
                        lib.probe(array, len(data), length, signal, offset, seed, pulses, output, entropy)
                        print(length, signal, offset, seed, fingerprint(pulses[:length]), fingerprint(output[:length]), *entropy)
