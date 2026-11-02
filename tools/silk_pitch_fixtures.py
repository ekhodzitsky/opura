#!/usr/bin/env python3
"""Optional pinned Opus oracle for pitch/LTP/seed; never part of Cargo builds.
Usage: python3 tools/silk_pitch_fixtures.py /path/to/opus [--tables]
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
    specs = [
        ('tables_pitch_lag.c', 'silk_pitch_lag_iCDF', 'LAG', 'u8', 32),
        ('tables_pitch_lag.c', 'silk_pitch_delta_iCDF', 'DELTA', 'u8', 21),
        ('tables_pitch_lag.c', 'silk_pitch_contour_iCDF', 'CONTOUR20', 'u8', 34),
        ('tables_pitch_lag.c', 'silk_pitch_contour_NB_iCDF', 'CONTOUR20_NB', 'u8', 11),
        ('tables_pitch_lag.c', 'silk_pitch_contour_10_ms_iCDF', 'CONTOUR10', 'u8', 12),
        ('tables_pitch_lag.c', 'silk_pitch_contour_10_ms_NB_iCDF', 'CONTOUR10_NB', 'u8', 3),
        ('pitch_est_tables.c', 'silk_CB_lags_stage2_10_ms', 'LAGS10_NB', 'i8', 6),
        ('pitch_est_tables.c', 'silk_CB_lags_stage3_10_ms', 'LAGS10', 'i8', 24),
        ('pitch_est_tables.c', 'silk_CB_lags_stage2', 'LAGS20_NB', 'i8', 44),
        ('pitch_est_tables.c', 'silk_CB_lags_stage3', 'LAGS20', 'i8', 136),
        ('pitch_est_tables.c', 'silk_Lag_range_stage3', 'SEARCH20', 'i8', 24),
        ('pitch_est_tables.c', 'silk_Lag_range_stage3_10_ms', 'SEARCH10', 'i8', 4),
        ('tables_LTP.c', 'silk_LTP_per_index_iCDF', 'PERIODICITY', 'u8', 3),
        ('tables_other.c', 'silk_LTPscale_iCDF', 'SCALE', 'u8', 3),
        ('tables_other.c', 'silk_LTPScales_table_Q14', 'SCALES_Q14', 'i16', 3),
    ]
    for i, count in enumerate([8, 16, 32]):
        specs += [('tables_LTP.c', f'silk_LTP_gain_iCDF_{i}', f'GAIN{i}', 'u8', count),
                  ('tables_LTP.c', f'silk_LTP_gain_BITS_Q5_{i}', f'BITS{i}', 'u8', count),
                  ('tables_LTP.c', f'silk_LTP_gain_vq_{i}_gain', f'EFFECTIVE_GAIN{i}', 'u8', count),
                  ('tables_LTP.c', f'silk_LTP_gain_vq_{i}', f'COEFFICIENTS{i}', 'i8', count * 5)]
    for count in [4, 6, 8]:
        specs.append(('tables_other.c', f'silk_uniform{count}_iCDF', f'UNIFORM{count}', 'u8', count))
    print('// Generated from Opus v1.6.1 by tools/silk_pitch_fixtures.py --tables.')
    print('// Copyright (c) 2006-2011, Skype Limited. See THIRD_PARTY_NOTICES.md.')
    for file, original, name, kind, length in specs:
        content = (source / 'silk' / file).read_text()
        match = re.search(r'\b' + original + r'\s*\[[^;=]+?=\s*\{(.*?)\};', content, re.S)
        if not match: raise SystemExit('Missing array ' + original)
        values = [int(x) for x in re.findall(r'-?\d+', re.sub(r'/\*.*?\*/', '', match[1], flags=re.S))]
        if len(values) != length: raise SystemExit('Wrong array size ' + original)
        print(f'pub(in crate::silk) const {name}: [{kind}; {length}] = {values};')
    raise SystemExit(0)

indices = (source / 'silk/decode_indices.c').read_text()
section = indices[indices.index('    if( psDec->indices.signalType == TYPE_VOICED )'):indices.rfind('}')]
parameters = (source / 'silk/decode_parameters.c').read_text()
reconstruct = parameters[parameters.index('    if( psDec->indices.signalType == TYPE_VOICED )'):parameters.rfind('}')]
wrapper = '''
#include "main.h"
static void suffix(silk_decoder_state *psDec, ec_dec *psRangeDec, int condCoding) {
    int decode_absolute_lagIndex, k;
    opus_int16 delta_lagIndex;
''' + section + '''
}
static void parameters(silk_decoder_state *psDec, silk_decoder_control *psDecCtrl) {
    int k, i, Ix;
    const opus_int8 *cbk_ptr_Q7;
''' + reconstruct + '''
}
void probe(unsigned char *data, unsigned len, int rate, int subframes, int *out) {
    silk_decoder_state state = {0};
    silk_decoder_control control = {0};
    ec_dec decoder;
    int i, k, step;
    const int signals[9] = {2,2,2,1,2,0,2,2,2};
    const int codings[9] = {0,2,1,2,2,2,2,0,2};
    state.fs_kHz = rate;
    state.nb_subfr = subframes;
    state.pitch_lag_low_bits_iCDF = rate == 8 ? silk_uniform4_iCDF : rate == 12 ? silk_uniform6_iCDF : silk_uniform8_iCDF;
    state.pitch_contour_iCDF = rate == 8
        ? (subframes == 2 ? silk_pitch_contour_10_ms_NB_iCDF : silk_pitch_contour_NB_iCDF)
        : (subframes == 2 ? silk_pitch_contour_10_ms_iCDF : silk_pitch_contour_iCDF);
    ec_dec_init(&decoder, data, len);
    for (step = 0; step < 9; step++) {
        memset(&state.indices, 0, sizeof(state.indices));
        state.indices.signalType = signals[step];
        suffix(&state, &decoder, codings[step]);
        parameters(&state, &control);
        *out++ = signals[step];
        *out++ = codings[step];
        *out++ = state.indices.lagIndex;
        *out++ = state.indices.contourIndex;
        *out++ = state.indices.PERIndex;
        *out++ = state.indices.LTP_scaleIndex;
        *out++ = control.LTP_scale_Q14;
        *out++ = state.indices.Seed;
        for (i = 0; i < subframes; i++) *out++ = state.indices.LTPIndex[i];
        for (i = 0; i < subframes; i++) *out++ = control.pitchL[i];
        for (i = 0; i < subframes; i++) for (k = 0; k < 5; k++) *out++ = control.LTPCoef_Q14[i * 5 + k];
        *out++ = decoder.rng;
        *out++ = ec_tell(&decoder);
        *out++ = ec_tell_frac(&decoder);
    }
}
'''
with tempfile.TemporaryDirectory(prefix='opura-pitch-reference-') as temp:
    path = Path(temp)
    (path / 'probe.c').write_text(wrapper)
    files = ['celt/entdec.c', 'celt/entcode.c', 'silk/tables_other.c', 'silk/tables_pitch_lag.c',
             'silk/tables_LTP.c', 'silk/pitch_est_tables.c', 'silk/decode_pitch.c']
    subprocess.run(['cc', '-shared', '-fPIC', '-O2', '-I' + str(source / 'include'),
                    '-I' + str(source / 'celt'), '-I' + str(source / 'silk'), str(path / 'probe.c')]
                   + [str(source / file) for file in files] + ['-o', str(path / 'probe.so')], check=True)
    lib = c.CDLL(str(path / 'probe.so'))
    lib.probe.argtypes = [c.POINTER(c.c_ubyte), c.c_uint, c.c_int, c.c_int, c.POINTER(c.c_int)]
    lib.probe.restype = None
    print('# Opus v1.6.1 pitch/LTP/seed:', REVISION)
    print('# frame HEX RATE_KHZ SUBFRAMES; nine steps follow:')
    print('# SIGNAL CODING LAG_INDEX CONTOUR PERIODICITY SCALE_INDEX SCALE_Q14 SEED LTP_INDICES[N] LAGS[N] COEFFICIENTS_Q14[N*5] RANGE TELL TELL_FRAC')
    inputs = [bytes(256), bytes([255]) * 256, bytes(range(256))]
    seed = 0x70697463
    for _ in range(64):
        data = bytearray()
        for _ in range(256):
            seed ^= (seed << 13) & 0xffffffff
            seed ^= seed >> 17
            seed ^= (seed << 5) & 0xffffffff
            data.append(seed & 255)
        inputs.append(bytes(data))
    for data in inputs:
        for rate in [8, 12, 16]:
            for count in [2, 4]:
                width = 11 + 7 * count
                result = (c.c_int * (9 * width))()
                array = (c.c_ubyte * len(data))(*data)
                lib.probe(array, len(data), rate, count, result)
                print('frame', data.hex(), rate, count)
                for step in range(9):
                    values = list(result[step*width:(step+1)*width])
                    values[-3] &= 0xffffffff
                    print(*values)
