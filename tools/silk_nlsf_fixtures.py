#!/usr/bin/env python3
"""Optional pinned Opus NLSF oracle; not a Cargo dependency.

Usage: python3 tools/silk_nlsf_fixtures.py /path/to/opus [--tables | --stabilize]
--tables emits Rust codebooks (format with rustfmt); default emits decode sequences.
--stabilize emits adversarial vectors passed through the upstream stabilizer.
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
mode = parser.add_mutually_exclusive_group()
mode.add_argument('--tables', action='store_true')
mode.add_argument('--stabilize', action='store_true')
args = parser.parse_args()
source = args.source.resolve()
if subprocess.check_output(['git', '-C', str(source), 'rev-parse', 'HEAD'], text=True).strip() != REVISION:
    raise SystemExit('Expected Opus v1.6.1 at ' + REVISION)
if subprocess.check_output(['git', '-C', str(source), 'status', '--porcelain', '--untracked-files=no']):
    raise SystemExit('Reference checkout has modified tracked files')

if args.tables:
    print('// Generated from Opus v1.6.1 by tools/silk_nlsf_fixtures.py --tables.')
    print('// Copyright (c) 2006-2011, Skype Limited. See THIRD_PARTY_NOTICES.md.')
    for band, order in [('NB_MB', 10), ('WB', 16)]:
        content = (source / f'silk/tables_NLSF_CB_{band}.c').read_text()
        names = [
            ('CB1', f'silk_NLSF_CB1_{band}_Q8', 'u8', 32 * order),
            ('WEIGHTS', 'silk_NLSF_CB1_' + ('WB_' if band == 'WB' else '') + 'Wght_Q9', 'i16', 32 * order),
            ('ICDF1', f'silk_NLSF_CB1_iCDF_{band}', 'u8', 64),
            ('SELECT', f'silk_NLSF_CB2_SELECT_{band}', 'u8', 16 * order),
            ('ICDF2', f'silk_NLSF_CB2_iCDF_{band}', 'u8', 72),
            ('RATES', f'silk_NLSF_CB2_BITS_{band}_Q5', 'u8', 72),
            ('PRED', f'silk_NLSF_PRED_{band}_Q8', 'u8', 2 * (order - 1)),
            ('DELTA', f'silk_NLSF_DELTA_MIN_{band}_Q15', 'i16', order + 1),
        ]
        for name, original, kind, size in names:
            match = re.search(r'\b' + original + r'\[\s*' + str(size) + r'\s*\]\s*=\s*\{([^}]+)\}', content)
            if not match:
                raise SystemExit('Missing array: ' + original)
            values = [int(x.strip()) for x in match[1].split(',') if x.strip()]
            if len(values) != size:
                raise SystemExit('Wrong array length: ' + original)
            print(f'pub(super) const {band}_{name}: [{kind}; {size}] = {values};')
    raise SystemExit(0)

indices = (source / 'silk/decode_indices.c').read_text()
section = indices[indices.index('    psDec->indices.NLSFIndices[ 0 ]'):indices.index('    if( psDec->indices.signalType == TYPE_VOICED )')]
wrapper = '''
#include "main.h"
#include "entenc.h"
static void spectral(silk_decoder_state *psDec, ec_dec *psRangeDec) {
    int i, Ix;
    opus_int16 ec_ix[16];
    opus_uint8 pred_Q8[16];
''' + section + '''
}
void probe(unsigned char *data, unsigned len, int order, int subframes, int signal, int *out) {
    silk_decoder_state state = {0};
    ec_dec decoder;
    opus_int16 current[16] = {0}, previous[16] = {0};
    int step, i, factor;
    state.psNLSF_CB = order == 10 ? &silk_NLSF_CB_NB_MB : &silk_NLSF_CB_WB;
    state.LPC_order = order;
    state.nb_subfr = subframes;
    state.indices.signalType = signal;
    ec_dec_init(&decoder, data, len);
    for (step = 0; step < 4; step++) {
        spectral(&state, &decoder);
        silk_NLSF_decode(current, state.indices.NLSFIndices, state.psNLSF_CB);
        factor = step == 0 ? 4 : state.indices.NLSFInterpCoef_Q2;
        *out++ = state.indices.NLSFInterpCoef_Q2;
        *out++ = factor;
        for (i = 0; i <= order; i++) *out++ = state.indices.NLSFIndices[i];
        for (i = 0; i < order; i++) *out++ = current[i];
        for (i = 0; i < order; i++) {
            *out++ = previous[i] + ((factor * (current[i] - previous[i])) >> 2);
            previous[i] = current[i];
        }
        *out++ = decoder.rng;
        *out++ = ec_tell(&decoder);
        *out++ = ec_tell_frac(&decoder);
    }
}
void encode_extreme(unsigned char *data, int order, int residual) {
    const silk_NLSF_CB_struct *cb = order == 10 ? &silk_NLSF_CB_NB_MB : &silk_NLSF_CB_WB;
    opus_int16 offsets[16];
    opus_uint8 prediction[16];
    ec_enc encoder;
    int step, i, symbol = residual + 4;
    ec_enc_init(&encoder, data, 256);
    silk_NLSF_unpack(offsets, prediction, cb, 0);
    for (step = 0; step < 4; step++) {
        ec_enc_icdf(&encoder, 0, cb->CB1_iCDF + 32, 8);
        for (i = 0; i < order; i++) {
            ec_enc_icdf(&encoder, symbol < 0 ? 0 : 8, cb->ec_iCDF + offsets[i], 8);
            ec_enc_icdf(&encoder, symbol < 0 ? -symbol : symbol - 8, silk_NLSF_EXT_iCDF, 8);
        }
        ec_enc_icdf(&encoder, 2, silk_NLSF_interpolation_factor_iCDF, 8);
    }
    ec_enc_done(&encoder);
}
void stabilize(opus_int16 *values, int order) {
    const silk_NLSF_CB_struct *cb = order == 10 ? &silk_NLSF_CB_NB_MB : &silk_NLSF_CB_WB;
    silk_NLSF_stabilize(values, cb->deltaMin_Q15, order);
}
'''
seed = 0x6e6c7366

def random_byte():
    global seed
    seed ^= (seed << 13) & 0xffffffff
    seed ^= seed >> 17
    seed ^= (seed << 5) & 0xffffffff
    return seed & 255

with tempfile.TemporaryDirectory(prefix='opura-nlsf-reference-') as temp:
    path = Path(temp)
    (path / 'probe.c').write_text(wrapper)
    files = ['celt/entenc.c', 'celt/entdec.c', 'celt/entcode.c', 'silk/tables_other.c',
             'silk/tables_NLSF_CB_NB_MB.c', 'silk/tables_NLSF_CB_WB.c',
             'silk/NLSF_unpack.c', 'silk/NLSF_decode.c', 'silk/NLSF_stabilize.c', 'silk/sort.c']
    subprocess.run(['cc', '-shared', '-fPIC', '-O2', '-I' + str(source / 'include'),
                    '-I' + str(source / 'celt'), '-I' + str(source / 'silk'), str(path / 'probe.c')]
                   + [str(source / file) for file in files] + ['-o', str(path / 'probe.so')], check=True)
    lib = c.CDLL(str(path / 'probe.so'))
    lib.probe.argtypes = [c.POINTER(c.c_ubyte), c.c_uint, c.c_int, c.c_int, c.c_int, c.POINTER(c.c_int)]
    lib.probe.restype = None
    lib.stabilize.argtypes = [c.POINTER(c.c_int16), c.c_int]
    lib.stabilize.restype = None
    lib.encode_extreme.argtypes = [c.POINTER(c.c_ubyte), c.c_int, c.c_int]
    lib.encode_extreme.restype = None
    print('# Opus v1.6.1 NLSF oracle:', REVISION)
    if args.stabilize:
        print('# ORDER INPUT[ORDER] STABILIZED[ORDER]')
        for order in [10, 16]:
            inputs = [[0]*order, [32767]*order, [-32768]*order,
                      [32767 if i % 2 else -32768 for i in range(order)],
                      [32767 - i * 2000 for i in range(order)]]
            inputs += [[(random_byte() * 256 + random_byte()) - 32768 for _ in range(order)] for _ in range(128)]
            for values in inputs:
                array = (c.c_int16 * order)(*values)
                lib.stabilize(array, order)
                print(order, *values, *array)
    else:
        print('# frame HEX ORDER SUBFRAMES SIGNAL; then four steps:')
        print('# CODED_FACTOR EFFECTIVE_FACTOR INDICES[ORDER+1] Q15[ORDER] FIRST_HALF_Q15[ORDER] RANGE TELL TELL_FRAC')
        inputs = [bytes(128), bytes([255])*128, bytes(range(128))]
        inputs += [bytes(random_byte() for _ in range(128)) for _ in range(64)]
        # Deliberately encode rare residual escapes absent from the random corpus.
        for order in [10, 16]:
            for residual in [-10, 7]:
                encoded = (c.c_ubyte * 256)()
                lib.encode_extreme(encoded, order, residual)
                inputs.append(bytes(encoded))
        for data in inputs:
            for order in [10, 16]:
                for subframes in [2, 4]:
                    for signal in [0, 1, 2]:
                        array = (c.c_ubyte * len(data))(*data)
                        width = 3 * order + 6
                        result = (c.c_int * (4 * width))()
                        lib.probe(array, len(data), order, subframes, signal, result)
                        print('frame', data.hex(), order, subframes, signal)
                        for step in range(4):
                            values = list(result[step*width:(step+1)*width])
                            values[-3] &= 0xffffffff
                            print(*values)
