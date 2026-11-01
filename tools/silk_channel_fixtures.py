#!/usr/bin/env python3
"""Optional pinned upstream entropy-to-raw-synthesis oracle; no Cargo dependency."""
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
# Use the original complete routines, with only the caller glue reproduced here.
# This deliberately stops before CNG/PLC and resampling, like ChannelState.
wrapper = r'''
#include "main.h"
static silk_decoder_state state;
static ec_dec decoder;
void begin(unsigned char *data, unsigned len, int reset, int rate) {
    if (reset) { memset(&state, 0, sizeof(state)); state.prev_gain_Q16 = 65536; }
    if (state.fs_kHz != rate) {
        state.first_frame_after_reset = 1;
        state.LastGainIndex = 10;
        memset(state.outBuf, 0, sizeof(state.outBuf));
        memset(state.sLPC_Q14_buf, 0, sizeof(state.sLPC_Q14_buf));
    }
    state.fs_kHz = rate;
    state.subfr_length = rate * 5;
    state.ltp_mem_length = rate * 20;
    state.LPC_order = rate == 16 ? 16 : 10;
    state.psNLSF_CB = rate == 16 ? &silk_NLSF_CB_WB : &silk_NLSF_CB_NB_MB;
    state.pitch_lag_low_bits_iCDF = rate == 8 ? silk_uniform4_iCDF : rate == 12 ? silk_uniform6_iCDF : silk_uniform8_iCDF;
    ec_dec_init(&decoder, data, len);
}
void step(int count, int activity, int coding, opus_int16 *pcm, opus_uint32 *out) {
    silk_decoder_control control = {0};
    opus_int16 pulses[320];
    state.nb_subfr = count;
    state.frame_length = count * state.subfr_length;
    state.pitch_contour_iCDF = state.fs_kHz == 8
        ? (count == 2 ? silk_pitch_contour_10_ms_NB_iCDF : silk_pitch_contour_NB_iCDF)
        : (count == 2 ? silk_pitch_contour_10_ms_iCDF : silk_pitch_contour_iCDF);
    state.VAD_flags[0] = activity != 0;
    silk_decode_indices(&state, &decoder, 0, activity == 2, coding);
    silk_decode_pulses(&decoder, pulses, state.indices.signalType,
                       state.indices.quantOffsetType, state.frame_length);
    silk_decode_parameters(&state, &control, coding);
    silk_decode_core(&state, &control, pcm, pulses, 0);
    int remaining = state.ltp_mem_length - state.frame_length;
    memmove(state.outBuf, state.outBuf + state.frame_length, remaining * sizeof(*pcm));
    memcpy(state.outBuf + remaining, pcm, state.frame_length * sizeof(*pcm));
    state.first_frame_after_reset = 0;
    out[0] = decoder.rng;
    out[1] = ec_tell(&decoder);
    out[2] = ec_tell_frac(&decoder);
    out[3] = state.indices.signalType;
    out[4] = state.indices.NLSFInterpCoef_Q2;
}
'''
files = ['celt/entdec.c', 'celt/entenc.c', 'celt/entcode.c'] + [
    'silk/' + name + '.c' for name in [
        'decode_indices', 'decode_parameters', 'decode_pulses', 'decode_core',
        'decode_pitch', 'shell_coder', 'code_signs', 'gain_quant', 'lin2log', 'log2lin',
        'NLSF_unpack', 'NLSF_decode', 'NLSF_stabilize', 'NLSF2A', 'sort',
        'LPC_fit', 'LPC_inv_pred_gain', 'LPC_analysis_filter', 'bwexpander',
        'bwexpander_32', 'table_LSF_cos', 'tables_other', 'tables_gain',
        'tables_pitch_lag', 'tables_LTP', 'pitch_est_tables',
        'tables_NLSF_CB_NB_MB', 'tables_NLSF_CB_WB', 'tables_pulses_per_block']]
with tempfile.TemporaryDirectory(prefix='opura-channel-reference-') as temp:
    path = Path(temp)
    (path / 'probe.c').write_text(wrapper)
    subprocess.run(['cc', '-shared', '-fPIC', '-O2', '-fwrapv', '-DVAR_ARRAYS',
                    *['-I' + str(source / p) for p in ['include', 'celt', 'silk']],
                    str(path / 'probe.c'), *[str(source / f) for f in files],
                    '-o', str(path / 'probe.so')], check=True)
    lib = c.CDLL(str(path / 'probe.so'))
    lib.begin.argtypes = [c.POINTER(c.c_ubyte), c.c_uint, c.c_int, c.c_int]
    lib.begin.restype = None
    lib.step.argtypes = [c.c_int] * 3 + [c.POINTER(c.c_int16), c.POINTER(c.c_uint32)]
    lib.step.restype = None
    print('# Opus v1.6.1 decode_indices/pulses/parameters/core + PCM history:', REVISION)
    print('# Synthetic streams; NOT complete Opus packets or post-CNG PCM.')
    print('# stream RESET RATE HEX; three rows: COUNT ACTIVITY CODING RANGE TELL TELL_FRAC SIGNAL INTERP PCM[rate*5*count]')
    random = 0x63686e6c
    for block in range(48):
        data = bytearray()
        for _ in range(1024):
            random ^= (random << 13) & 0xffffffff
            random ^= random >> 17
            random ^= (random << 5) & 0xffffffff
            data.append(random & 255)
        if block < 3:
            data = bytearray([block * 127] * 1024)
        rate = [8, 8, 12, 12, 16, 16, 12, 8][block % 8]
        reset = int(block % 8 == 0)
        array = (c.c_ubyte * len(data))(*data)
        lib.begin(array, len(data), reset, rate)
        print('stream', reset, rate, data.hex())
        for step in range(3):
            count = 2 if (block + step) % 3 == 0 else 4
            activity = (block + step) % 3
            coding = block % 2 if step == 0 else 2
            pcm, out = (c.c_int16 * 320)(), (c.c_uint32 * 5)()
            lib.step(count, activity, coding, pcm, out)
            if out[1] > len(data) * 8:
                raise SystemExit('Oracle exceeded entropy budget')
            print(count, activity, coding, *out, *pcm[:rate * 5 * count])
