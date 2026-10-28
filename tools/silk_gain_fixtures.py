#!/usr/bin/env python3
"""Optional upstream SILK gain reference; never part of the Cargo build.

Usage: python3 tools/silk_gain_fixtures.py /path/to/opus [--dequant]
Uses an extracted decode_indices prefix plus original gain reconstruction.
"""
import argparse
import ctypes as c
from pathlib import Path
import subprocess
import tempfile

REVISION = '22244de5a79bd1d6d623c32e72bf1954b56235be'
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('source', type=Path)
parser.add_argument('--dequant', action='store_true')
args = parser.parse_args()
source = args.source.resolve()
if subprocess.check_output(['git', '-C', str(source), 'rev-parse', 'HEAD'], text=True).strip() != REVISION:
    raise SystemExit('Expected clean Opus v1.6.1 at ' + REVISION)
if subprocess.check_output(['git', '-C', str(source), 'status', '--porcelain', '--untracked-files=no']):
    raise SystemExit('Reference checkout has modified tracked files')
indices = (source / 'silk/decode_indices.c').read_text()
prefix = indices[indices.index('    if( decode_LBRR ||'):indices.index('    /* Decode LSF Indices */')]
# Remove only the final decorative comment before the next section.
prefix = prefix[:prefix.rfind('    /**********************/')]
wrapper = '''
#include "main.h"
static void prefix(silk_decoder_state *psDec, ec_dec *psRangeDec, int decode_LBRR, int condCoding) {
    int i, Ix, FrameIndex = 0;
''' + prefix + '''
}
void probe(unsigned char *input, unsigned len, int subframes, int activity, unsigned *out) {
    silk_decoder_state state = {0};
    ec_dec decoder;
    opus_int8 previous = 10;
    opus_int32 gains[4] = {0};
    int step, i, coding;
    ec_dec_init(&decoder, input, len);
    state.nb_subfr = subframes;
    state.VAD_flags[0] = activity == 1;
    for (step = 0; step < 4; step++) {
        coding = step == 0 || step == 3 ? CODE_INDEPENDENTLY : CODE_CONDITIONALLY;
        prefix(&state, &decoder, activity == 2, coding);
        silk_gains_dequant(gains, state.indices.GainsIndices, &previous, coding == CODE_CONDITIONALLY, subframes);
        out[0] = state.indices.signalType;
        out[1] = state.indices.quantOffsetType;
        for (i = 0; i < 4; i++) { out[2+i] = state.indices.GainsIndices[i]; out[6+i] = gains[i]; }
        out[10] = previous;
        out[11] = decoder.rng;
        out[12] = ec_tell(&decoder);
        out[13] = ec_tell_frac(&decoder);
        out += 14;
    }
}
void transition(int previous, int index, int conditional, unsigned *out) {
    opus_int8 prev = previous;
    opus_int8 indices[4] = {index, 0, 0, 0};
    opus_int32 gains[4] = {0};
    silk_gains_dequant(gains, indices, &prev, conditional, 1);
    out[0] = prev;
    out[1] = gains[0];
}
'''


def fingerprint(values):
    value = 14695981039346656037
    for number in values:
        for byte in number.to_bytes(4, 'little'):
            value = ((value ^ byte) * 1099511628211) & 0xffffffffffffffff
    return value


with tempfile.TemporaryDirectory(prefix='opura-gain-reference-') as temp:
    path = Path(temp)
    (path / 'probe.c').write_text(wrapper)
    files = ['celt/entdec.c', 'celt/entcode.c', 'silk/tables_gain.c', 'silk/tables_other.c',
             'silk/gain_quant.c', 'silk/log2lin.c', 'silk/lin2log.c']
    subprocess.run(['cc', '-shared', '-fPIC', '-O2', '-I' + str(source / 'include'),
                    '-I' + str(source / 'celt'), '-I' + str(source / 'silk'), str(path / 'probe.c')]
                   + [str(source / file) for file in files] + ['-o', str(path / 'probe.so')], check=True)
    lib = c.CDLL(str(path / 'probe.so'))
    lib.probe.argtypes = [c.POINTER(c.c_ubyte), c.c_uint, c.c_int, c.c_int, c.POINTER(c.c_uint)]
    lib.probe.restype = None
    lib.transition.argtypes = [c.c_int, c.c_int, c.c_int, c.POINTER(c.c_uint)]
    lib.transition.restype = None
    print('# Opus v1.6.1 decode_indices/gain_quant/log2lin:', REVISION)
    if args.dequant:
        print('# linear INDEX Q16; transition PREVIOUS FNV64_INDEPENDENT FNV64_CONDITIONAL')
        print('# FNV-1a over LE-u32 (result index, result Q16) pairs in encoded-index order.')
        result = (c.c_uint * 2)()
        for index in range(64):
            lib.transition(0, index, 0, result)
            print('linear', index, result[1])
        for previous in range(64):
            hashes = []
            for conditional, count in [(0, 64), (1, 41)]:
                values = []
                for index in range(count):
                    lib.transition(previous, index, conditional, result)
                    values.extend(result)
                hashes.append(fingerprint(values))
            print('transition', previous, *hashes)
    else:
        print('# frame HEX SUBFRAMES ACTIVITY(0=inactive,1=active,2=redundant)')
        print('# CODING(0=independent,1=conditional) TYPE OFFSET INDICES[4] Q16[4] PREVIOUS RANGE TELL TELL_FRAC')
        seed = 0x6761696e
        inputs = [bytes(64), bytes([255])*64, bytes(range(64))]
        for _ in range(8):
            data = bytearray()
            for _ in range(64):
                seed ^= (seed << 13) & 0xffffffff
                seed ^= seed >> 17
                seed ^= (seed << 5) & 0xffffffff
                data.append(seed & 255)
            inputs.append(bytes(data))
        for data in inputs:
            for subframes in [2, 4]:
                for activity in [0, 1, 2]:
                    array = (c.c_ubyte * len(data))(*data)
                    out = (c.c_uint * 56)()
                    lib.probe(array, len(data), subframes, activity, out)
                    print('frame', data.hex(), subframes, activity)
                    for step in range(4):
                        print(int(step in (1, 2)), *out[step*14:(step+1)*14])
