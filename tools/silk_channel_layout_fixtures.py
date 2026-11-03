#!/usr/bin/env python3
"""Optional unchanged public Opus speech channel/resampler history oracle."""
import argparse
import ctypes as c
from pathlib import Path
import re
import subprocess
import tempfile
from audio_baseline import REVISION
from opus_pcm_fixtures import pcm

WRAPPER = r'''
#include "opus_encoder.c"
#include "main_FIX.h"
#if !OPUS_FAST_INT64 || DB_SHIFT != 24
#error "Expected scalar fast64 fixed-point Q24 reference"
#endif

static silk_encoder_state_FIX *mid_target;
static opus_uint64 mid_hash;
void __real_silk_encode_do_VAD_FIX(silk_encoder_state_FIX*,opus_int);
void __wrap_silk_encode_do_VAD_FIX(silk_encoder_state_FIX *s,opus_int activity) {
 if(s==mid_target)for(int i=0;i<s->sCmn.frame_length;i++) {
  opus_uint16 v=(opus_uint16)s->sCmn.inputBuf[i+1];
  mid_hash^=v&255;mid_hash*=1099511628211ULL;
  mid_hash^=v>>8;mid_hash*=1099511628211ULL;
 }
 __real_silk_encode_do_VAD_FIX(s,activity);
}
void *layout_new(int Fs,int band,int vbr) {
 int error;OpusEncoder *st=opus_encoder_create(Fs,2,OPUS_APPLICATION_AUDIO,&error);
 if(!st||error)return NULL;
 if(opus_encoder_ctl(st,OPUS_SET_FORCE_MODE(MODE_SILK_ONLY)) ||
    opus_encoder_ctl(st,OPUS_SET_BANDWIDTH(OPUS_BANDWIDTH_NARROWBAND+band)) ||
    opus_encoder_ctl(st,OPUS_SET_MAX_BANDWIDTH(OPUS_BANDWIDTH_NARROWBAND+band)) ||
    opus_encoder_ctl(st,OPUS_SET_VBR(vbr)) || opus_encoder_ctl(st,OPUS_SET_BITRATE(48000)) ||
    opus_encoder_ctl(st,OPUS_SET_COMPLEXITY(0))) {opus_encoder_destroy(st);return NULL;}
 mid_target=&((silk_encoder*)((char*)st+st->silk_enc_offset))->state_Fxx[0];
 mid_hash=14695981039346656037ULL;
 return st;
}
static opus_uint64 rs_hash(const silk_resampler_state_struct *s) {
 opus_int32 v[103];int n=0;
 for(int i=0;i<6;i++)v[n++]=s->sIIR[i];
 for(int i=0;i<8;i++)v[n++]=s->resampler_function==2?s->sFIR.i16[i]:0;
 for(int i=0;i<36;i++)v[n++]=s->resampler_function==3?s->sFIR.i32[i]:0;
 for(int i=0;i<48;i++)v[n++]=s->delayBuf[i];
 v[n++]=s->resampler_function;v[n++]=s->invRatio_Q16;v[n++]=s->inputDelay;
 v[n++]=s->FIR_Order;v[n++]=s->FIR_Fracs;
 opus_uint64 h=14695981039346656037ULL;
 for(int i=0;i<n;i++)for(int b=0;b<4;b++){h^=((opus_uint32)v[i]>>(8*b))&255;h*=1099511628211ULL;}
 return h;
}
int layout_probe(OpusEncoder *st,const opus_int16 *pcm,int n,int request,int dtx,int fec,int *out,opus_uint64 *hashes) {
 unsigned char packet[1276];
 if(opus_encoder_ctl(st,OPUS_SET_FORCE_CHANNELS(request)) || opus_encoder_ctl(st,OPUS_SET_DTX(dtx)) ||
    opus_encoder_ctl(st,OPUS_SET_INBAND_FEC(fec)) || opus_encoder_ctl(st,OPUS_SET_PACKET_LOSS_PERC(20)))return -1;
 int length=opus_encode(st,pcm,n,packet,n*6000/st->Fs);
 if(length<1)return length;
 silk_encoder *s=(silk_encoder*)((char*)st+st->silk_enc_offset);
 out[0]=1+((packet[0]>>2)&1);out[1]=st->silk_mode.toMono;
 out[2]=s->sStereo.sMid[0];out[3]=s->sStereo.sMid[1];
 out[4]=s->state_Fxx[0].sCmn.fs_kHz;
 for(int ch=0;ch<2;ch++) {
  silk_resampler_state_struct *r=&s->state_Fxx[ch].sCmn.resampler_state;
  hashes[ch]=r->Fs_in_kHz?rs_hash(r):0;
 }
 hashes[2]=mid_hash;
 return length;
}

void restart_probe(const int *v,int *out) {
 silk_encoder enc;silk_EncControlStruct ctrl;
 int ret=silk_InitEncoder(&enc,2,0,&ctrl);(void)ret;
 silk_encoder *psEnc=&enc;silk_EncControlStruct *encControl=&ctrl;
 ctrl.nChannelsAPI=2;ctrl.nChannelsInternal=2;enc.nChannelsInternal=1;
 stereo_enc_state *s=&enc.sStereo;int n=0;
 for(int i=0;i<2;i++)s->pred_prev_Q13[i]=v[n++];
 for(int i=0;i<2;i++)s->sMid[i]=v[n++];
 for(int i=0;i<2;i++)s->sSide[i]=v[n++];
 for(int i=0;i<4;i++)s->mid_side_amp_Q0[i]=v[n++];
 s->smth_width_Q14=v[n++];s->width_prev_Q14=v[n++];s->silent_side_len=v[n++];
 RESTART_BLOCK
 n=0;
 for(int i=0;i<2;i++)out[n++]=s->pred_prev_Q13[i];
 for(int i=0;i<2;i++)out[n++]=s->sMid[i];
 for(int i=0;i<2;i++)out[n++]=s->sSide[i];
 for(int i=0;i<4;i++)out[n++]=s->mid_side_amp_Q0[i];
 out[n++]=s->smth_width_Q14;out[n++]=s->width_prev_Q14;out[n++]=s->silent_side_len;
}
'''


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source', type=Path)
    parser.add_argument('output', type=Path)
    parser.add_argument('--restart-output', type=Path)
    args = parser.parse_args()
    source = args.source.resolve()
    if subprocess.check_output(['git', '-C', str(source), 'rev-parse', 'HEAD'], text=True).strip() != REVISION:
        raise ValueError('Expected pinned Opus v1.6.1')
    if subprocess.check_output(['git', '-C', str(source), 'status', '--porcelain'], text=True).strip():
        raise ValueError('Reference checkout must be unchanged')

    def manifest(file, variable):
        match = re.search(r'^'+variable+r' = (.*?)(?=\n\n|\Z)', (source/file).read_text(), re.M | re.S)
        if match is None:
            raise ValueError('Missing source manifest')
        return match[1].replace('\\', '').split()

    files = (manifest('silk_sources.mk', 'SILK_SOURCES') + manifest('silk_sources.mk', 'SILK_SOURCES_FIXED') +
             manifest('celt_sources.mk', 'CELT_SOURCES') + manifest('opus_sources.mk', 'OPUS_SOURCES'))
    with tempfile.TemporaryDirectory(prefix='opura-silk-layout-') as temp:
        folder = Path(temp)
        original = (source/'silk/enc_API.c').read_text()
        start = original.index('    if( encControl->nChannelsInternal > psEnc->nChannelsInternal )')
        end = original.index('    transition =', start)
        (folder/'probe.c').write_text(WRAPPER.replace('RESTART_BLOCK', original[start:end]))
        subprocess.run(['cc', '-shared', '-fPIC', '-O2', '-fwrapv', '-DVAR_ARRAYS', '-DOPUS_BUILD',
                        '-DFIXED_POINT', '-DDISABLE_FLOAT_API',
                        *['-I'+str(source/p) for p in ['include', 'celt', 'silk', 'silk/fixed', 'src']],
                        str(folder/'probe.c'), *[str(source/f) for f in files if f != 'src/opus_encoder.c'],
                        '-Wl,--wrap=silk_encode_do_VAD_FIX', '-lm', '-o', str(folder/'probe.so')], check=True)
        lib = c.CDLL(str(folder/'probe.so'))
        lib.layout_new.argtypes = [c.c_int, c.c_int, c.c_int]
        lib.layout_new.restype = c.c_void_p
        lib.layout_probe.argtypes = [c.c_void_p, c.POINTER(c.c_int16), *[c.c_int]*4,
                                     c.POINTER(c.c_int), c.POINTER(c.c_uint64)]
        lib.layout_probe.restype = c.c_int
        lib.opus_encoder_destroy.argtypes = [c.c_void_p]
        lib.opus_encoder_destroy.restype = None
        rows = ['# Original public Opus '+REVISION+' AUDIO forced SILK; scalar fixed fast64 Q24',
                '# RATE BAND MS VBR INDEX REQUEST DTX FEC SEED PATTERN WIRE TO_MONO MID0 MID1 INTERNAL RS0_HASH RS1_HASH MID_PCM_HASH']
        for rate in [8000, 12000, 16000, 24000, 48000]:
            for band, internal in enumerate([8000, 12000, 16000]):
                if internal > rate:
                    continue
                for ms in [10, 20, 40, 60]:
                    for vbr in [0, 1]:
                        state = lib.layout_new(rate, band, vbr)
                        if not state:
                            raise ValueError('Cannot create reference encoder')
                        try:
                            for i in range(48):
                                request = 1 + ((i//5 + vbr) % 2)
                                dtx, fec = int(8 <= i < 38), int(i >= 10)
                                seed = (0x73696c6b + i*982451653 + rate*ms) & 0xffffffff
                                pattern = 0 if 8 <= i < 32 else 1 if i % 7 == 0 else 3
                                n = rate*ms//1000
                                samples = (c.c_int16*(2*n))(*pcm(seed, 2*n, pattern))
                                out, hashes = (c.c_int*5)(), (c.c_uint64*3)()
                                result = lib.layout_probe(state, samples, n, request, dtx, fec, out, hashes)
                                if result < 1 or out[4] != internal//1000:
                                    raise ValueError(f'Unexpected native configuration: {rate}/{band}/{ms}/{i}: {result}/{list(out)}')
                                rows.append(' '.join(map(str, [rate, band, ms, vbr, i, request, dtx, fec,
                                                              seed, pattern, *out, *hashes])))
                        finally:
                            lib.opus_encoder_destroy(state)
        if args.restart_output:
            lib.restart_probe.argtypes = [c.POINTER(c.c_int), c.POINTER(c.c_int)]
            lib.restart_probe.restype = None
            restarts = ['# Original enc_API.c mono-to-stereo initializer; before13 after13; '+REVISION]
            for i in range(32):
                before = [i*103-1700, 1200-i*79, -15000+i*701, 13000-i*313,
                          400-i*79, -300+i*53, i*123, 1+i*177, i*301, 1+i*89,
                          i*501, i*377, 1+i*19]
                after = (c.c_int*13)()
                lib.restart_probe((c.c_int*13)(*before), after)
                restarts.append(' '.join(map(str, [*before, *after])))
            args.restart_output.write_text('\n'.join(restarts)+'\n')
        args.output.write_text('\n'.join(rows)+'\n')
        print('Captured', len(rows)-2, 'original public speech channel/input histories')


if __name__ == '__main__':
    main()
