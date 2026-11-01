#!/usr/bin/env python3
"""Optional pinned original silk_CNG oracle; never a Cargo dependency."""
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
void probe(int reset, int rate, int count, int signal, int lost, int noise_scale,
           int previous_gain, const opus_int32 *gains, const opus_int16 *nlsf,
           const opus_int32 *excitation, opus_int16 *pcm, opus_int32 *out) {
    if (reset) memset(&state, 0, sizeof(state));
    state.fs_kHz = rate;
    state.LPC_order = rate == 16 ? 16 : 10;
    state.nb_subfr = count;
    state.subfr_length = rate * 5;
    state.frame_length = state.subfr_length * count;
    state.prevSignalType = signal;
    state.lossCnt = lost;
    state.sPLC.randScale_Q14 = noise_scale;
    state.sPLC.prevGain_Q16[1] = previous_gain;
    memcpy(state.prevNLSF_Q15, nlsf, state.LPC_order * sizeof(*nlsf));
    memcpy(state.exc_Q14, excitation, state.frame_length * sizeof(*excitation));
    silk_decoder_control control = {0};
    memcpy(control.Gains_Q16, gains, count * sizeof(*gains));
    silk_CNG(&state, &control, pcm, state.frame_length);
    for (int i = 0; i < 320; i++) *out++ = state.sCNG.CNG_exc_buf_Q14[i];
    for (int i = 0; i < 16; i++) *out++ = state.sCNG.CNG_smth_NLSF_Q15[i];
    for (int i = 0; i < 16; i++) *out++ = state.sCNG.CNG_synth_state[i];
    *out++ = state.sCNG.CNG_smth_Gain_Q16;
    *out++ = state.sCNG.rand_seed;
    *out++ = state.sCNG.fs_kHz;
}
'''
spectra = {10: [], 16: []}
for line in (Path(__file__).resolve().parent.parent / 'tests/silk-lpc-fixtures.txt').read_text().splitlines():
    if not line.startswith('#'):
        row = list(map(int, line.split()))
        spectra[row[0]].append(row[1:1 + row[0]])
with tempfile.TemporaryDirectory(prefix='opura-cng-reference-') as temp:
    path = Path(temp)
    (path / 'probe.c').write_text(wrapper)
    files = ['CNG', 'NLSF2A', 'LPC_fit', 'LPC_inv_pred_gain', 'bwexpander_32', 'table_LSF_cos']
    subprocess.run(['cc', '-shared', '-fPIC', '-O2', '-fwrapv', '-DVAR_ARRAYS',
                    *['-I' + str(source / p) for p in ['include', 'celt', 'silk']],
                    str(path / 'probe.c'), *[str(source / 'silk' / (f + '.c')) for f in files],
                    '-o', str(path / 'probe.so')], check=True)
    lib = c.CDLL(str(path / 'probe.so'))
    i16, i32 = c.POINTER(c.c_int16), c.POINTER(c.c_int32)
    lib.probe.argtypes = [c.c_int] * 7 + [i32, i16, i32, i16, i32]
    lib.probe.restype = None
    print('# Opus v1.6.1 original silk_CNG PCM and every retained state field:', REVISION)
    print('# reset rate count signal lost scale previous_gain gains[count] NLSF[order] excitation[length] input_PCM[length] output_PCM[length] exc_state[320] spectrum_state[16] synth_state[16] gain seed rate')
    random = 0x434e4721
    for case in range(192):
        reset = int(case % 48 == 0)
        rate = [8, 8, 12, 12, 16, 16, 12, 8][(case // 6) % 8]
        if case % 24 in [15, 16]: rate = 8 if rate != 8 else 16
        count = 2 if case % 3 == 0 else 4
        length, order = rate * 5 * count, 16 if rate == 16 else 10
        phase = case % 6
        lost = int(phase >= 2 and phase <= 4 or reset)
        signal = 0 if phase <= 1 else (1 + (case // 6) % 2)
        scale = [0, 16384, 8192, 2048, 0, 16384][phase]
        previous = [65536, 131072, 2097152, 8388608, 1686110208][case % 5]
        gains = [[65536, 2097152, 8388608, 1686110208][(case + k) % 4] for k in range(count)]
        if case % 5 == 0: gains = [gains[0]] * count
        spectrum = spectra[order][(case * 37) % len(spectra[order])]
        excitation, pcm = [], []
        for i in range(length):
            random ^= (random << 13) & 0xffffffff
            random ^= random >> 17
            random ^= (random << 5) & 0xffffffff
            excitation.append((random % 6400001) - 3200000)
            pcm.append(((random >> 16) % 65536) - 32768)
        initial = pcm[:]
        output, state = (c.c_int16 * length)(*pcm), (c.c_int32 * 355)()
        lib.probe(reset, rate, count, signal, lost, scale, previous,
                  (c.c_int32 * count)(*gains), (c.c_int16 * order)(*spectrum),
                  (c.c_int32 * length)(*excitation), output, state)
        print(reset, rate, count, signal, lost, scale, previous, *gains, *spectrum,
              *excitation, *initial, *output, *state)
