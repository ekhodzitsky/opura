#!/usr/bin/env python3
"""Optional original pinned SILK decode_frame loss/CNG/recovery oracle."""
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
#include <stdlib.h>
static silk_decoder_state state;
void probe(unsigned char *data, unsigned size, int reset, int rate, int count,
           int activity, int coding, int lost, opus_int16 *pcm, opus_int32 *out) {
    if (reset) { memset(&state, 0, sizeof(state)); state.prev_gain_Q16 = 65536; state.first_frame_after_reset = 1; }
    if (state.fs_kHz != rate) {
        state.first_frame_after_reset = 1;
        state.LastGainIndex = 10;
        state.lagPrev = 100;
        state.prevSignalType = TYPE_NO_VOICE_ACTIVITY;
        memset(state.outBuf, 0, sizeof(state.outBuf));
        memset(state.sLPC_Q14_buf, 0, sizeof(state.sLPC_Q14_buf));
    }
    state.fs_kHz = rate;
    state.subfr_length = rate * 5;
    state.nb_subfr = count;
    state.frame_length = count * state.subfr_length;
    state.ltp_mem_length = rate * 20;
    state.LPC_order = rate == 16 ? 16 : 10;
    state.psNLSF_CB = rate == 16 ? &silk_NLSF_CB_WB : &silk_NLSF_CB_NB_MB;
    state.pitch_lag_low_bits_iCDF = rate == 8 ? silk_uniform4_iCDF : rate == 12 ? silk_uniform6_iCDF : silk_uniform8_iCDF;
    state.pitch_contour_iCDF = rate == 8
        ? (count == 2 ? silk_pitch_contour_10_ms_NB_iCDF : silk_pitch_contour_NB_iCDF)
        : (count == 2 ? silk_pitch_contour_10_ms_iCDF : silk_pitch_contour_iCDF);
    state.VAD_flags[0] = activity;
    state.nFramesDecoded = 0;
    ec_dec decoder;
    ec_dec_init(&decoder, data, size);
    opus_int32 length;
    int result = silk_decode_frame(&state, &decoder, pcm, &length, lost, coding, 0);
    if (result != 0 || length != state.frame_length) abort();
    // dec_API removes gain clamping after each lost internal frame.
    if (lost) state.LastGainIndex = 10;
    *out++ = decoder.rng;
    *out++ = ec_tell(&decoder);
    *out++ = ec_tell_frac(&decoder);
    *out++ = state.LastGainIndex;
    *out++ = state.indices.signalType;
    for (int i=0; i<16; i++) *out++ = state.sLPC_Q14_buf[i];
    for (int i=0; i<320; i++) *out++ = state.outBuf[i];
    *out++ = state.prev_gain_Q16;
    *out++ = state.sPLC.pitchL_Q8;
    for (int i=0; i<5; i++) *out++ = state.sPLC.LTPCoef_Q14[i];
    for (int i=0; i<16; i++) *out++ = state.sPLC.prevLPC_Q12[i];
    *out++ = state.sPLC.last_frame_lost;
    *out++ = state.sPLC.rand_seed;
    *out++ = state.sPLC.randScale_Q14;
    *out++ = state.sPLC.conc_energy;
    *out++ = state.sPLC.conc_energy_shift;
    *out++ = state.sPLC.prevLTP_scale_Q14;
    *out++ = state.sPLC.prevGain_Q16[0];
    *out++ = state.sPLC.prevGain_Q16[1];
    *out++ = state.sPLC.fs_kHz;
    *out++ = state.sPLC.nb_subfr;
    *out++ = state.sPLC.subfr_length;
    *out++ = state.prevSignalType;
    *out++ = state.lossCnt;
    *out++ = state.first_frame_after_reset;
    for (int i=0; i<320; i++) *out++ = state.exc_Q14[i];
}
'''
files = ['celt/entdec.c', 'celt/entenc.c', 'celt/entcode.c'] + [
    'silk/' + name + '.c' for name in [
        'decode_frame', 'PLC', 'CNG', 'sum_sqr_shift',
        'decode_indices', 'decode_parameters', 'decode_pulses', 'decode_core',
        'decode_pitch', 'shell_coder', 'code_signs', 'gain_quant', 'lin2log', 'log2lin',
        'NLSF_unpack', 'NLSF_decode', 'NLSF_stabilize', 'NLSF2A', 'sort',
        'LPC_fit', 'LPC_inv_pred_gain', 'LPC_analysis_filter', 'bwexpander',
        'bwexpander_32', 'table_LSF_cos', 'tables_other', 'tables_gain',
        'tables_pitch_lag', 'tables_LTP', 'pitch_est_tables',
        'tables_NLSF_CB_NB_MB', 'tables_NLSF_CB_WB', 'tables_pulses_per_block']]
with tempfile.TemporaryDirectory(prefix='opura-plc-reference-') as temp:
    path = Path(temp)
    (path / 'probe.c').write_text(wrapper)
    subprocess.run(['cc', '-shared', '-fPIC', '-O2', '-fwrapv', '-DVAR_ARRAYS',
                    *['-I' + str(source / p) for p in ['include', 'celt', 'silk']],
                    str(path / 'probe.c'), *[str(source / f) for f in files],
                    '-o', str(path / 'probe.so')], check=True)
    lib = c.CDLL(str(path / 'probe.so'))
    lib.probe.argtypes = [c.POINTER(c.c_ubyte), c.c_uint] + [c.c_int] * 6 + [c.POINTER(c.c_int16), c.POINTER(c.c_int32)]
    lib.probe.restype = None
    print('# Opus v1.6.1 original silk_decode_frame + dec_API loss gain reset:', REVISION)
    print('# reset rate count activity coding lost HEX; next line: PCM[length] RANGE TELL FRAC GAIN_INDEX SIGNAL LPC[16] HISTORY[320] PREV_GAIN PLC_STATE[356]')
    random = 0x504c4321
    previous_rate = None
    needs_independent = True
    for case in range(240):
        reset = int(case % 60 == 0)
        rate = [8, 8, 12, 12, 16, 16, 12, 8][(case // 5) % 8]
        count = 2 if case % 3 == 0 else 4
        lost = int(case % 10 in [0, 3, 4, 5, 6])
        activity = int(case % 7 != 0)
        if reset or rate != previous_rate: needs_independent = True
        coding = 0 if needs_independent or case % 3 == 1 else 2
        # If reset immediately precedes a loss, that loss seeds the gain history
        # for later conditional symbols, as dec_API does.
        data = bytearray()
        for _ in range(2048):
            random ^= (random << 13) & 0xffffffff
            random ^= random >> 17
            random ^= (random << 5) & 0xffffffff
            data.append(random & 255)
        array = (c.c_ubyte * len(data))(*data)
        pcm, out = (c.c_int16 * 320)(), (c.c_int32 * 698)()
        lib.probe(array, len(data), reset, rate, count, activity, coding, lost, pcm, out)
        values = list(out)
        values[0] &= 0xffffffff
        print(reset, rate, count, activity, coding, lost, data.hex() if not lost else '-')
        print(*pcm[:rate * 5 * count], *values)
        if not lost: needs_independent = False
        previous_rate = rate
