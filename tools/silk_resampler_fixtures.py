#!/usr/bin/env python3
"""Optional pinned SILK decoder resampler PCM/state oracle and table extraction."""
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
    print('// Generated from Opus v1.6.1 by tools/silk_resampler_fixtures.py --tables.')
    print('// Copyright (c) 2006-2011, Skype Limited. See THIRD_PARTY_NOTICES.md.')
    content = (source / 'silk/resampler_rom.c').read_text()
    for original, name, count in [('silk_Resampler_3_4_COEFS', 'DOWN_3_4', 29),
                                  ('silk_Resampler_2_3_COEFS', 'DOWN_2_3', 20),
                                  ('silk_Resampler_1_2_COEFS', 'DOWN_1_2', 14),
                                  ('silk_resampler_frac_FIR_12', 'FRACTIONAL', 48)]:
        match = re.search(r'\b' + original + r'\s*\[[^;=]+?=\s*\{(.*?)\};', content, re.S)
        if not match: raise SystemExit('Missing reference table ' + original)
        values = [int(v) for v in re.findall(r'-?\d+', match[1])]
        if len(values) != count: raise SystemExit('Wrong table size ' + original)
        print(f'pub(super) const {name}: [i16; {count}] = {values};')
    header = (source / 'silk/resampler_rom.h').read_text()
    for suffix in [0, 1]:
        match = re.search(r'silk_resampler_up2_hq_' + str(suffix) + r'\s*\[\s*3\s*\]\s*=\s*\{(.*?)\}', header)
        if not match: raise SystemExit('Missing upsampler coefficients')
        values = []
        for value in match[1].split(','):
            terms = value.split('-')
            values.append(int(terms[0]) - (int(terms[1]) if len(terms) == 2 else 0))
        print(f'pub(super) const UP_{suffix}: [i16; 3] = {values};')
    raise SystemExit(0)
wrapper = r'''
#include "resampler_private.h"
#include <stdlib.h>
static silk_resampler_state_struct state;
static int previous_input, previous_output;
void probe(int reset, int input_rate, int output_rate, const opus_int16 *input, int length,
           opus_int16 *output, opus_int32 *out) {
    if (reset || previous_input != input_rate || previous_output != output_rate) {
        if (silk_resampler_init(&state, input_rate * 1000, output_rate * 1000, 0)) abort();
    }
    previous_input = input_rate;
    previous_output = output_rate;
    if (silk_resampler(&state, output, input, length)) abort();
    for (int i=0; i<6; i++) *out++ = state.sIIR[i];
    for (int i=0; i<8; i++) *out++ = state.resampler_function == 2 ? state.sFIR.i16[i] : 0;
    for (int i=0; i<24; i++) *out++ = state.resampler_function == 3 ? state.sFIR.i32[i] : 0;
    for (int i=0; i<16; i++) *out++ = state.delayBuf[i];
    for (int i=16; i<96; i++) if (state.delayBuf[i] != 0) abort();
    *out++ = state.resampler_function;
    *out++ = state.invRatio_Q16;
    *out++ = state.inputDelay;
    *out++ = state.FIR_Order;
    *out++ = state.FIR_Fracs;
}
'''
files = ['resampler', 'resampler_private_up2_HQ', 'resampler_private_IIR_FIR',
         'resampler_private_down_FIR', 'resampler_private_AR2', 'resampler_rom']
with tempfile.TemporaryDirectory(prefix='opura-resample-reference-') as temp:
    path = Path(temp)
    (path / 'probe.c').write_text(wrapper)
    subprocess.run(['cc', '-shared', '-fPIC', '-O2', '-fwrapv', '-DVAR_ARRAYS',
                    *['-I' + str(source / p) for p in ['include', 'celt', 'silk']],
                    str(path / 'probe.c'), *[str(source / 'silk' / (f + '.c')) for f in files],
                    '-o', str(path / 'probe.so')], check=True)
    lib = c.CDLL(str(path / 'probe.so'))
    lib.probe.argtypes = [c.c_int] * 3 + [c.POINTER(c.c_int16), c.c_int, c.POINTER(c.c_int16), c.POINTER(c.c_int32)]
    lib.probe.restype = None
    print('# Opus v1.6.1 original decoder silk_resampler PCM/state:', REVISION)
    print('# reset input_khz output_khz milliseconds input[length]; next line output[ms*output_khz] IIR[6] FIR16[8] FIR32[24] delay[16] method ratio delay order fractions')
    random = 0x72617465
    for output_rate in [8, 12, 16, 24, 48]:
        for case in range(72):
            input_rate = [8, 8, 12, 12, 16, 16, 12, 8][(case // 3) % 8]
            reset = int(case % 24 == 0)
            ms = [1, 2, 5, 10, 20, 40, 60, 120][case % 8]
            data = []
            for i in range(input_rate * ms):
                random ^= (random << 13) & 0xffffffff
                random ^= random >> 17
                random ^= (random << 5) & 0xffffffff
                kind = case % 6
                data.append(0 if kind == 0 else (32767 if i == 0 else 0) if kind == 1
                            else (32767 if i % 2 else -32768) if kind == 2 else
                            32767 if kind == 3 else -32768 if kind == 4 else (random & 65535) - 32768)
            samples = (c.c_int16 * len(data))(*data)
            pcm, state = (c.c_int16 * (output_rate * ms))(), (c.c_int32 * 59)()
            lib.probe(reset, input_rate, output_rate, samples, len(data), pcm, state)
            print(reset, input_rate, output_rate, ms, *data)
            print(*pcm, *state)
