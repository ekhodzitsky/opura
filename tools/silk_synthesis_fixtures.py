#!/usr/bin/env python3
"""Optional local Opus 1.6.1 decode_core oracle; not a Cargo dependency."""
import argparse
import ctypes as c
from pathlib import Path
import subprocess
import tempfile

REVISION = '22244de5a79bd1d6d623c32e72bf1954b56235be'
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('source', type=Path)
args = parser.parse_args()
source = args.source.resolve()
if subprocess.check_output(['git', '-C', str(source), 'rev-parse', 'HEAD'], text=True).strip() != REVISION:
    raise SystemExit('Expected Opus v1.6.1 at ' + REVISION)
if subprocess.check_output(['git', '-C', str(source), 'status', '--porcelain', '--untracked-files=no']):
    raise SystemExit('Reference checkout has modified tracked files')
wrapper = r'''
#include "main.h"
static silk_decoder_state state;
void probe(int reset, int rate, int count, int signal, int interpolate,
           const opus_int32 *gains, const opus_int16 *a, const opus_int16 *b,
           const int *lags, int scale, const opus_int16 *pulses, int seed, int offset,
           opus_int16 *pcm, opus_int32 *exc, opus_int32 *lpc, opus_int32 *previous,
           opus_int16 *history) {
    if (reset) { memset(&state, 0, sizeof(state)); state.prev_gain_Q16 = 65536; }
    if (state.fs_kHz != rate) {
        memset(state.sLPC_Q14_buf, 0, sizeof(state.sLPC_Q14_buf));
        memset(state.outBuf, 0, sizeof(state.outBuf));
    }
    state.fs_kHz = rate;
    state.LPC_order = rate == 16 ? 16 : 10;
    state.subfr_length = rate * 5;
    state.nb_subfr = count;
    state.frame_length = state.subfr_length * count;
    state.ltp_mem_length = rate * 20;
    state.indices.signalType = signal;
    state.indices.NLSFInterpCoef_Q2 = interpolate ? 2 : 4;
    state.indices.Seed = seed;
    state.indices.quantOffsetType = offset;
    silk_decoder_control control = {0};
    memcpy(control.Gains_Q16, gains, count * sizeof(*gains));
    memcpy(control.PredCoef_Q12[0], a, state.LPC_order * sizeof(*a));
    memcpy(control.PredCoef_Q12[1], a + state.LPC_order, state.LPC_order * sizeof(*a));
    memcpy(control.LTPCoef_Q14, b, count * 5 * sizeof(*b));
    memcpy(control.pitchL, lags, count * sizeof(*lags));
    control.LTP_scale_Q14 = scale;
    silk_decode_core(&state, &control, pcm, pulses, 0);
    int remaining = state.ltp_mem_length - state.frame_length;
    memmove(state.outBuf, state.outBuf + state.frame_length, remaining * sizeof(*pcm));
    memcpy(state.outBuf + remaining, pcm, state.frame_length * sizeof(*pcm));
    memcpy(exc, state.exc_Q14, state.frame_length * sizeof(*exc));
    memcpy(lpc, state.sLPC_Q14_buf, 16 * sizeof(*lpc));
    *previous = state.prev_gain_Q16;
    memcpy(history, state.outBuf, state.ltp_mem_length * sizeof(*history));
}
'''
filters = {10: [], 16: []}
fixture = Path(__file__).resolve().parent.parent / 'tests/silk-lpc-fixtures.txt'
for line in fixture.read_text().splitlines():
    if not line.startswith('#'):
        row = list(map(int, line.split()))
        order = row[0]
        filters[order].append(row[1 + order:1 + order * 2])
with tempfile.TemporaryDirectory(prefix='opura-synthesis-reference-') as temp:
    path = Path(temp)
    (path / 'probe.c').write_text(wrapper)
    subprocess.run(['cc', '-shared', '-fPIC', '-O2', '-fwrapv', '-DVAR_ARRAYS',
                    *['-I' + str(source / p) for p in ['include', 'celt', 'silk']],
                    str(path / 'probe.c'),
                    *[str(source / p) for p in ['silk/decode_core.c', 'silk/LPC_analysis_filter.c', 'silk/tables_other.c']],
                    '-o', str(path / 'probe.so')], check=True)
    lib = c.CDLL(str(path / 'probe.so'))
    i16, i32 = c.POINTER(c.c_int16), c.POINTER(c.c_int32)
    lib.probe.argtypes = [c.c_int] * 5 + [i32, i16, i16, c.POINTER(c.c_int), c.c_int, i16,
                                       c.c_int, c.c_int, i16, i32, i32, i32, i16]
    lib.probe.restype = None
    print('# Opus 1.6.1 loss-free decode_core + decode_frame history update:', REVISION)
    print('# reset rate count signal interpolate gains[count] A[2*order] lags[count] B[count*5] scale excitation[length] PCM[length] LPCstate[16] previous_gain history[rate*20]')
    random = 0x51a7c0de
    for case in range(192):
        reset = int(case % 32 == 0)
        rate = [8, 12, 16, 16, 12, 8][(case // 8) % 6]
        count = 2 if case % 3 == 0 else 4
        signal = [0, 2, 2, 1, 2, 1, 2, 2][case % 8]
        interp = int(count == 4 and case % 2 == 0)
        order, length = (16 if rate == 16 else 10), rate * 5 * count
        gains = [[131072, 655360, 6553600, 1686110208][(case + k) % 4] for k in range(count)]
        if case % 5 == 0:
            gains = [gains[0]] * count
        elif case % 5 == 1:
            gains = [gains[k // 2] for k in range(count)]
        current = filters[order][(case * 37) % len(filters[order])]
        first = filters[order][(case * 19) % len(filters[order])] if interp else current
        coefficients = first + current
        lags = [rate * (2 + (case * 7 + k * 3) % 17) if signal == 2 else 0 for k in range(count)]
        vectors = [[0, 0, 0, 0, 0], [0, 0, 8192, 0, 0], [-256, 1024, 12000, 2048, -512]]
        b = sum([vectors[(case + k) % 3] if signal == 2 else vectors[0] for k in range(count)], [])
        scale = [15565, 12288, 8192][case % 3] if signal == 2 else 0
        pulses = []
        for _ in range(length):
            random ^= (random << 13) & 0xffffffff
            random ^= random >> 17
            random ^= (random << 5) & 0xffffffff
            amplitude = 2 if case % 2 else 100
            pulses.append((random % (2 * amplitude + 1)) - amplitude if case % 7 else 0)
        pcm, exc, lpc, prev, history = (c.c_int16 * 320)(), (c.c_int32 * 320)(), (c.c_int32 * 16)(), c.c_int32(), (c.c_int16 * 320)()
        lib.probe(reset, rate, count, signal, interp, (c.c_int32 * count)(*gains),
                  (c.c_int16 * (order * 2))(*coefficients), (c.c_int16 * (count * 5))(*b),
                  (c.c_int * count)(*lags), scale, (c.c_int16 * length)(*pulses), case % 4, case % 2,
                  pcm, exc, lpc, c.byref(prev), history)
        print(reset, rate, count, signal, interp, *gains, *coefficients, *lags, *b, scale,
              *exc[:length], *pcm[:length], *lpc, prev.value, *history[:rate * 20])
