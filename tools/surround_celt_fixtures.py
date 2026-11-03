#!/usr/bin/env python3
"""Optional actual CELT packet/PCM/state oracle with surround masks and LFE.

The temporary CELT source retains the already documented Q24 spectral-patch
local-variable correction. Mask reduction, LFE processing and native decoding
are otherwise unchanged; the external original checkout is never modified.
"""
import argparse
import ctypes as c
from pathlib import Path
import re
import subprocess
import tempfile
from audio_baseline import CFLAGS, REVISION, run
from surround_analysis_fixtures import fingerprint, signal

WRAPPER = r'''
void *opura_surround_celt_new(int rate,int channels) {
 CELTEncoder *state=calloc(1,celt_encoder_get_size(channels));
 if(!state)return NULL;
 if(celt_encoder_init(state,rate,channels,0)){free(state);return NULL;}
 celt_encoder_ctl(state,CELT_SET_SIGNALLING(0));
 celt_encoder_ctl(state,OPUS_SET_LSB_DEPTH(16));
 return state;
}
void opura_surround_celt_free(void *state) { free(state); }
int opura_surround_celt_controls(CELTEncoder *s,int reset,int lfe,int complexity,int prediction,int mode,int rate,int end,celt_glog *mask) {
 if(reset)celt_encoder_ctl(s,OPUS_RESET_STATE);
 int r=0;
 r|=celt_encoder_ctl(s,OPUS_SET_LFE(lfe));
 r|=celt_encoder_ctl(s,OPUS_SET_COMPLEXITY(complexity));
 r|=celt_encoder_ctl(s,CELT_SET_PREDICTION(prediction));
 r|=celt_encoder_ctl(s,CELT_SET_END_BAND(end));
 r|=celt_encoder_ctl(s,OPUS_SET_BITRATE(mode?rate:OPUS_BITRATE_MAX));
 r|=celt_encoder_ctl(s,OPUS_SET_VBR(mode!=0));
 r|=celt_encoder_ctl(s,OPUS_SET_VBR_CONSTRAINT(mode==2));
 r|=celt_encoder_ctl(s,OPUS_SET_ENERGY_MASK(mask));
 return r;
}
int opura_surround_celt_encode(CELTEncoder *s,const opus_int16 *pcm,int duration,int cap,unsigned char *out,int *state) {
 int result=celt_encode_with_ec(s,pcm,duration,out,cap,NULL);
 state[0]=s->lastCodedBands;state[1]=s->intensity;state[2]=s->spread_decision;
 state[3]=s->tapset_decision;state[4]=s->tonal_average;state[5]=s->hf_average;
 state[6]=s->spec_avg;state[7]=s->vbr_reservoir;state[8]=s->vbr_drift;
 state[9]=s->vbr_offset;state[10]=s->vbr_count;
 state[11]=s->prefilter_period;state[12]=s->prefilter_gain;
 state[13]=s->rng;
 return result;
}
'''


def compile_oracle(source, directory):
    if run(['git', '-C', source, 'rev-parse', 'HEAD']).strip() != REVISION or run(['git', '-C', source, 'status', '--porcelain']).strip():
        raise ValueError('Expected unchanged pinned reference')
    def manifest(file, name):
        match = re.search(r'^' + name + r' = (.*?)(?=\n\n|\Z)', (source/file).read_text(), re.M | re.S)
        if match is None:
            raise ValueError('Missing source manifest')
        return match[1].replace('\\', '').split()
    files = (manifest('silk_sources.mk', 'SILK_SOURCES') + manifest('silk_sources.mk', 'SILK_SOURCES_FIXED')
             + manifest('celt_sources.mk', 'CELT_SOURCES') + manifest('opus_sources.mk', 'OPUS_SOURCES'))
    original = (source/'celt/celt_encoder.c').read_text()
    old = 'opus_val16 x1, x2;'
    if original.count(old) != 1:
        raise ValueError('Spectral-patch correction target changed')
    wrapper = directory/'surround-celt.c'
    wrapper.write_text('#include <stdlib.h>\n' + original.replace(old, 'celt_glog x1, x2;') + WRAPPER)
    library = directory/'surround-celt.so'
    includes = ['-I' + str(source/p) for p in ['include', 'celt', 'silk', 'silk/fixed', 'src']]
    subprocess.run(['cc', *CFLAGS, *includes, str(wrapper),
                    *[str(source/f) for f in files if f != 'celt/celt_encoder.c'], '-lm', '-o', str(library)], check=True)
    lib = c.CDLL(str(library))
    signatures = [
        ('opura_surround_celt_new', [c.c_int, c.c_int], c.c_void_p),
        ('opura_surround_celt_free', [c.c_void_p], None),
        ('opura_surround_celt_controls', [c.c_void_p] + [c.c_int]*7 + [c.c_void_p], c.c_int),
        ('opura_surround_celt_encode', [c.c_void_p, c.c_void_p, c.c_int, c.c_int, c.c_void_p, c.c_void_p], c.c_int),
        ('opus_decoder_create', [c.c_int, c.c_int, c.c_void_p], c.c_void_p),
        ('opus_decoder_destroy', [c.c_void_p], None),
        ('opus_decoder_ctl', [c.c_void_p, c.c_int], c.c_int),
        ('opus_decode', [c.c_void_p, c.c_void_p, c.c_int, c.c_void_p, c.c_int, c.c_int], c.c_int),
    ]
    for name, args, result in signatures:
        function = getattr(lib, name)
        function.argtypes, function.restype = args, result
    return lib


def masks(channels, step, kind):
    return [0 if kind == 1 else ((step * 719 + band * 317 + channel * 151) % (4 * 4096) - 3 * 4096) * 4096
            for channel in range(channels) for band in range(21)]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source', type=Path)
    parser.add_argument('output', type=Path)
    args = parser.parse_args()
    if args.output.exists():
        raise ValueError('Output must be new')
    rows = 0
    with tempfile.TemporaryDirectory(prefix='opura-surround-celt-') as temporary:
        lib = compile_oracle(args.source.resolve(), Path(temporary))
        with args.output.open('x') as output:
            output.write('# Actual CELT masks/LFE ' + REVISION + '; temporary Q24 spectral-patch locals correction only\n')
            output.write('# case RATE CHANNELS LFE\n# frame STEP RESET LM COMPLEXITY PREDICTION MODE BITRATE CAP MASK_KIND INPUT_FNV HEX PCM_FNV STATE14...\n')
            for rate in [8000, 12000, 16000, 24000, 48000]:
                for channels, lfe in [(1, 0), (2, 0), (1, 1)]:
                    owner = lib.opura_surround_celt_new(rate, channels)
                    error = c.c_int()
                    decoder = lib.opus_decoder_create(48000, channels, c.byref(error))
                    if not owner or not decoder or error.value:
                        raise ValueError('Native allocation failed')
                    output.write(f'case {rate} {channels} {lfe}\n')
                    try:
                        for step in range(128):
                            lm, complexity, prediction = step % 4, (step // 9) % 11, (step // 23) % 3
                            mode = (step // 4) % 3
                            bitrate = [16000, 32000, 64000, 128000][(step // 12) % 4] * channels
                            cap = [8, 16, 32, 64, 128, 256][(step // 6) % 6]
                            kind = step % 7 if step % 7 < 2 else 2
                            reset = int(step == 64)
                            end = 13 if lfe or rate == 8000 else 17 if rate <= 16000 else 19 if rate == 24000 else 21
                            mask = (c.c_int32 * (21 * channels))(*masks(channels, step, kind)) if kind else None
                            if lib.opura_surround_celt_controls(owner, reset, lfe, complexity, prediction, mode, bitrate, end, mask):
                                raise ValueError('Native controls failed')
                            if reset and lib.opus_decoder_ctl(decoder, 4028):
                                raise ValueError('Native decoder reset failed')
                            values = signal(rate, channels, 120 << lm, step, step % 8)
                            pcm = (c.c_int16 * len(values))(*values)
                            data, state = (c.c_ubyte * 1275)(), (c.c_int32 * 14)()
                            n = lib.opura_surround_celt_encode(owner, pcm, (120 << lm) * rate // 48000, cap, data, state)
                            if n <= 0:
                                raise ValueError(f'Native encode failed {rate}/{channels}/{lfe}/{step}: {n}')
                            base = {13: 16, 17: 20, 19: 24, 21: 28}[end]
                            packet = bytes([((base + lm) << 3) | (4 if channels == 2 else 0)]) + bytes(data[:n])
                            encoded = (c.c_ubyte * len(packet)).from_buffer_copy(packet)
                            decoded = (c.c_int16 * (960 * channels))()
                            count = lib.opus_decode(decoder, encoded, len(packet), decoded, 960, 0)
                            if count != 120 << lm:
                                raise ValueError('Native decoder duration failed')
                            output.write(f'frame {step} {reset} {lm} {complexity} {prediction} {mode} {bitrate} {cap} {kind} {fingerprint(values,"<h")} {packet.hex()} {fingerprint(decoded[:count*channels],"<h")} '
                                         + ' '.join(map(str, state)) + '\n')
                            rows += 1
                    finally:
                        lib.opura_surround_celt_free(owner)
                        lib.opus_decoder_destroy(decoder)
            output.write(f'# rows={rows}\n')
    if rows != 1920:
        raise ValueError('Incomplete protocol')
    print(f'Captured{rows}actual native CELT mask/LFE packet/PCM/state operations', flush=True)


if __name__ == '__main__':
    main()
