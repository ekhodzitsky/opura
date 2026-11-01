#!/usr/bin/env python3
"""Optional pinned SILK stereo entropy/reconstruction oracle and tables."""
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
parser.add_argument('--entropy', action='store_true')
args = parser.parse_args()
source = args.source.resolve()
if subprocess.check_output(['git', '-C', str(source), 'rev-parse', 'HEAD'], text=True).strip() != REVISION:
    raise SystemExit('Expected Opus v1.6.1 at ' + REVISION)
if subprocess.check_output(['git', '-C', str(source), 'status', '--porcelain', '--untracked-files=no']):
    raise SystemExit('Reference checkout has modified tracked files')
if args.tables:
    content = (source / 'silk/tables_other.c').read_text()
    print('// Generated from Opus v1.6.1 by tools/silk_stereo_fixtures.py --tables.')
    print('// Copyright (c) 2006-2011, Skype Limited. See THIRD_PARTY_NOTICES.md.')
    for original, name, kind, size in [
        ('silk_stereo_pred_quant_Q13', 'QUANT', 'i16', 16),
        ('silk_stereo_pred_joint_iCDF', 'JOINT', 'u8', 25),
        ('silk_stereo_only_code_mid_iCDF', 'MID_ONLY', 'u8', 2),
        ('silk_uniform3_iCDF', 'UNIFORM3', 'u8', 3),
        ('silk_uniform5_iCDF', 'UNIFORM5', 'u8', 5),
    ]:
        match = re.search(r'\b' + original + r'\s*\[[^;=]+?=\s*\{(.*?)\};', content, re.S)
        if not match: raise SystemExit('Missing table ' + original)
        values = [int(v) for v in re.findall(r'-?\d+', match[1])]
        if len(values) != size: raise SystemExit('Unexpected table size ' + original)
        print(f'pub(super) const {name}: [{kind}; {size}] = {values};')
    raise SystemExit(0)
wrapper = r'''
#include "main.h"
static stereo_dec_state state;
void entropy(int joint, int coarse0, int fine0, int coarse1, int fine1, int only_mid,
             unsigned char *data, opus_uint32 *out) {
    ec_enc encoder;
    ec_enc_init(&encoder, data, 16);
    ec_enc_icdf(&encoder, joint, silk_stereo_pred_joint_iCDF, 8);
    ec_enc_icdf(&encoder, coarse0, silk_uniform3_iCDF, 8);
    ec_enc_icdf(&encoder, fine0, silk_uniform5_iCDF, 8);
    ec_enc_icdf(&encoder, coarse1, silk_uniform3_iCDF, 8);
    ec_enc_icdf(&encoder, fine1, silk_uniform5_iCDF, 8);
    ec_enc_icdf(&encoder, only_mid, silk_stereo_only_code_mid_iCDF, 8);
    ec_enc_done(&encoder);
    ec_dec decoder;
    ec_dec_init(&decoder, data, 16);
    opus_int32 prediction[2];
    silk_stereo_decode_pred(&decoder, prediction);
    *out++ = prediction[0];
    *out++ = prediction[1];
    *out++ = decoder.rng; *out++ = ec_tell(&decoder); *out++ = ec_tell_frac(&decoder);
    int mid;
    silk_stereo_decode_mid_only(&decoder, &mid);
    *out++ = mid;
    *out++ = decoder.rng; *out++ = ec_tell(&decoder); *out++ = ec_tell_frac(&decoder);
}
void reconstruct(int reset, int rate, int length, int p0, int p1,
                 const opus_int16 *mid, const opus_int16 *side, opus_int16 *left,
                 opus_int16 *right, opus_int32 *out) {
    if (reset) memset(&state, 0, sizeof(state));
    opus_int16 a[322], b[322];
    memcpy(a + 2, mid, length * sizeof(*mid));
    memcpy(b + 2, side, length * sizeof(*side));
    opus_int32 prediction[2] = {p0, p1};
    silk_stereo_MS_to_LR(&state, a, b, prediction, rate, length);
    memcpy(left, a + 1, length * sizeof(*left));
    memcpy(right, b + 1, length * sizeof(*right));
    *out++ = state.pred_prev_Q13[0]; *out++ = state.pred_prev_Q13[1];
    *out++ = state.sMid[0]; *out++ = state.sMid[1];
    *out++ = state.sSide[0]; *out++ = state.sSide[1];
}
'''
with tempfile.TemporaryDirectory(prefix='opura-stereo-reference-') as temp:
    path = Path(temp)
    (path / 'probe.c').write_text(wrapper)
    files = ['celt/entdec.c', 'celt/entenc.c', 'celt/entcode.c', 'silk/tables_other.c',
             'silk/stereo_decode_pred.c', 'silk/stereo_MS_to_LR.c']
    subprocess.run(['cc', '-shared', '-fPIC', '-O2', '-fwrapv', '-DVAR_ARRAYS',
                    *['-I' + str(source / p) for p in ['include', 'celt', 'silk']],
                    str(path / 'probe.c'), *[str(source / f) for f in files],
                    '-o', str(path / 'probe.so')], check=True)
    lib = c.CDLL(str(path / 'probe.so'))
    lib.entropy.argtypes = [c.c_int] * 6 + [c.POINTER(c.c_ubyte), c.POINTER(c.c_uint32)]
    lib.entropy.restype = None
    lib.reconstruct.argtypes = [c.c_int] * 5 + [c.POINTER(c.c_int16)] * 4 + [c.POINTER(c.c_int32)]
    lib.reconstruct.restype = None
    print('# Opus v1.6.1 original SILK stereo routines:', REVISION)
    if args.entropy:
        print('# HEX PRED0 PRED1 RANGE TELL FRAC MID_ONLY RANGE TELL FRAC')
        for joint in range(25):
            for coarse0 in range(3):
                for fine0 in range(5):
                    for coarse1 in range(3):
                        for fine1 in range(5):
                            data, out = (c.c_ubyte * 16)(), (c.c_uint32 * 9)()
                            flag = (joint + coarse0 + fine0 + coarse1 + fine1) % 2
                            lib.entropy(joint, coarse0, fine0, coarse1, fine1, flag, data, out)
                            values = list(out)
                            values[0] = c.c_int32(values[0]).value
                            values[1] = c.c_int32(values[1]).value
                            print(bytes(data).hex(), *values)
        raise SystemExit(0)
    print('# reset rate length P0 P1 MID[length] SIDE[length]; next line LEFT[length] RIGHT[length] PREVIOUS_P[2] MID_STATE[2] SIDE_STATE[2]')
    # Derive independently dequantized predictor pairs through the original
    # decoder, including extreme jumps that exercise signed-16 interpolation.
    predictions = []
    for joint in [0, 4, 20, 24, 12]:
        data, out = (c.c_ubyte * 16)(), (c.c_uint32 * 9)()
        lib.entropy(joint, 0 if joint % 2 else 2, joint % 5,
                    2 if joint % 2 else 0, (joint + 4) % 5, 0, data, out)
        predictions.append([c.c_int32(out[0]).value, c.c_int32(out[1]).value])
    random = 0x53544552
    for case in range(192):
        reset = int(case % 32 == 0)
        rate = [8, 12, 16, 16, 12, 8][(case // 4) % 6]
        length = rate * (10 if case % 3 == 0 else 20)
        prediction = predictions[case % len(predictions)]
        mid, side = [], []
        for i in range(length):
            random ^= (random << 13) & 0xffffffff
            random ^= random >> 17
            random ^= (random << 5) & 0xffffffff
            kind = case % 8
            a = 0 if kind == 0 else 32767 if kind == 1 else -32768 if kind == 2 else (random & 65535) - 32768
            b = a if kind in [1, 3] else max(-32768, min(32767, -a)) if kind in [2, 4] else 0 if kind == 5 else ((random >> 16) & 65535) - 32768
            mid.append(a); side.append(b)
        left, right, state_out = (c.c_int16 * length)(), (c.c_int16 * length)(), (c.c_int32 * 6)()
        lib.reconstruct(reset, rate, length, *prediction, (c.c_int16 * length)(*mid),
                        (c.c_int16 * length)(*side), left, right, state_out)
        print(reset, rate, length, *prediction, *mid, *side)
        print(*left, *right, *state_out)
