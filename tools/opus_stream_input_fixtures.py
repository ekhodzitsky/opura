#!/usr/bin/env python3
"""Optional original public Opus cross-mode speech prefill/input history oracle."""
import argparse
import ctypes as c
from pathlib import Path
import re
import subprocess
import tempfile
from audio_baseline import REVISION
from opus_pcm_fixtures import pcm
from silk_channel_layout_fixtures import WRAPPER as LAYOUT_WRAPPER

WRAPPER = LAYOUT_WRAPPER[:LAYOUT_WRAPPER.index('void *layout_new')]
WRAPPER += LAYOUT_WRAPPER[LAYOUT_WRAPPER.index('static opus_uint64 rs_hash'):LAYOUT_WRAPPER.index('int layout_probe')]
WRAPPER += r'''
void transform_inputs(const void*,opus_uint64*);
void call_trace_reset(void);
void stream_transform_inputs(OpusEncoder *st,opus_uint64 *out) {
 transform_inputs((const char*)st+st->celt_enc_offset,out);
}
opus_int __real_silk_InitEncoder(void*,int,int,silk_EncControlStruct*);
opus_int __wrap_silk_InitEncoder(void *state,int channels,int arch,silk_EncControlStruct *control) {
 mid_hash=14695981039346656037ULL;
 return __real_silk_InitEncoder(state,channels,arch,control);
}
void *stream_new(int Fs,int channels,int vbr) {
 int error;OpusEncoder *st=opus_encoder_create(Fs,channels,OPUS_APPLICATION_AUDIO,&error);
 if(!st||error)return NULL;
 if(opus_encoder_ctl(st,OPUS_SET_VBR(vbr)) || opus_encoder_ctl(st,OPUS_SET_VBR_CONSTRAINT(0)) ||
    opus_encoder_ctl(st,OPUS_SET_BITRATE(64000*channels)) ||
    opus_encoder_ctl(st,OPUS_SET_COMPLEXITY(0))) {opus_encoder_destroy(st);return NULL;}
 mid_target=&((silk_encoder*)((char*)st+st->silk_enc_offset))->state_Fxx[0];
 mid_hash=14695981039346656037ULL;
 return st;
}
int stream_probe(OpusEncoder *st,const opus_int16 *pcm,int n,int mode,int request,int band,int *out,opus_uint64 *hashes) {
 unsigned char packet[1276];
 if(band<0)band=mode==MODE_SILK_ONLY?OPUS_BANDWIDTH_WIDEBAND:OPUS_BANDWIDTH_FULLBAND;
 if(opus_encoder_ctl(st,OPUS_SET_FORCE_MODE(mode)) || opus_encoder_ctl(st,OPUS_SET_BANDWIDTH(band)) ||
    opus_encoder_ctl(st,OPUS_SET_FORCE_CHANNELS(request)))return -1;
 call_trace_reset();
 int length=opus_encode(st,pcm,n,packet,640);
 if(length<1)return length;
 silk_encoder *s=(silk_encoder*)((char*)st+st->silk_enc_offset);
 out[0]=1+((packet[0]>>2)&1);out[1]=st->silk_mode.toMono;
 out[2]=s->state_Fxx[0].sCmn.fs_kHz;out[3]=s->nPrevChannelsInternal;
 for(int ch=0;ch<2;ch++) {
  if(ch>=st->channels) {hashes[ch]=0;continue;}
  silk_resampler_state_struct *r=&s->state_Fxx[ch].sCmn.resampler_state;
  hashes[ch]=r->Fs_in_kHz?rs_hash(r):0;
 }
 hashes[2]=mid_hash;
 return length;
}
'''


CELT_WRAPPER = r'''
#include "celt_encoder.c"
static opus_uint64 calls[40];
static int call_count;
void call_trace_reset(void) {call_count=0;}
int call_trace_get(opus_uint64 *out) {for(int i=0;i<call_count;i++)out[i]=calls[i];return call_count;}
int __real_celt_encode_with_ec(CELTEncoder*,const opus_res*,int,unsigned char*,int,ec_enc*);
int __wrap_celt_encode_with_ec(CELTEncoder *st,const opus_res *pcm,int n,unsigned char *data,int cap,ec_enc *enc) {
 if(call_count+5>40)return -3;
 opus_uint64 h=14695981039346656037ULL;
 for(int i=0;i<n*st->channels;i++) {
  opus_uint16 v=(opus_uint16)pcm[i];
  h^=v&255;h*=1099511628211ULL;h^=v>>8;h*=1099511628211ULL;
 }
 calls[call_count++]=n*st->upsample;calls[call_count++]=st->start;
 calls[call_count++]=st->force_intra?0:st->disable_pf?1:2;
 calls[call_count++]=cap;calls[call_count++]=h;
 return __real_celt_encode_with_ec(st,pcm,n,data,cap,enc);
}
void transform_inputs(const void *state,opus_uint64 *out) {
 const CELTEncoder *s=(const CELTEncoder*)state;
 for(int c=0;c<s->channels;c++) {
  opus_uint64 h=14695981039346656037ULL;
  opus_int32 v=s->vbr_count;
  for(int b=0;b<4;b++){h^=((opus_uint32)v>>(8*b))&255;h*=1099511628211ULL;}
  v=s->preemph_memE[c];
  for(int b=0;b<4;b++){h^=((opus_uint32)v>>(8*b))&255;h*=1099511628211ULL;}
  for(int i=0;i<s->mode->overlap;i++) {
   v=s->in_mem[c*s->mode->overlap+i];
   for(int b=0;b<4;b++){h^=((opus_uint32)v>>(8*b))&255;h*=1099511628211ULL;}
  }
  out[c]=h;
 }
}
'''


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source', type=Path)
    parser.add_argument('output', type=Path)
    parser.add_argument('--band-output', type=Path)
    parser.add_argument('--celt-output', type=Path)
    parser.add_argument('--call-output', type=Path)
    parser.add_argument('--band-call-output', type=Path)
    args = parser.parse_args()
    if args.band_call_output and not args.band_output:
        parser.error('--band-call-output requires --band-output')
    source = args.source.resolve()
    if subprocess.check_output(['git', '-C', str(source), 'rev-parse', 'HEAD'], text=True).strip() != REVISION:
        raise ValueError('Expected pinned Opus v1.6.1')
    if subprocess.check_output(['git', '-C', str(source), 'status', '--porcelain'], text=True).strip():
        raise ValueError('Reference checkout must be unchanged')

    def manifest(file, variable):
        match = re.search(r'^'+variable+r' = (.*?)(?=\n\n|\Z)', (source/file).read_text(), re.M | re.S)
        if match is None:
            raise ValueError('Missing native source manifest')
        return match[1].replace('\\', '').split()

    files = (manifest('silk_sources.mk', 'SILK_SOURCES') + manifest('silk_sources.mk', 'SILK_SOURCES_FIXED') +
             manifest('celt_sources.mk', 'CELT_SOURCES') + manifest('opus_sources.mk', 'OPUS_SOURCES'))
    with tempfile.TemporaryDirectory(prefix='opura-stream-input-') as temp:
        folder = Path(temp)
        (folder/'probe.c').write_text(WRAPPER)
        (folder/'transform.c').write_text(CELT_WRAPPER)
        subprocess.run(['cc', '-shared', '-fPIC', '-O2', '-fwrapv', '-DVAR_ARRAYS', '-DOPUS_BUILD',
                        '-DFIXED_POINT', '-DDISABLE_FLOAT_API',
                        *['-I'+str(source/p) for p in ['include', 'celt', 'silk', 'silk/fixed', 'src']],
                        str(folder/'probe.c'), str(folder/'transform.c'), *[str(source/f) for f in files if f not in ['src/opus_encoder.c','celt/celt_encoder.c']],
                        '-Wl,--wrap=silk_encode_do_VAD_FIX', '-Wl,--wrap=silk_InitEncoder', '-Wl,--wrap=celt_encode_with_ec',
                        '-lm', '-o', str(folder/'probe.so')], check=True)
        lib = c.CDLL(str(folder/'probe.so'))
        lib.stream_new.argtypes = [c.c_int]*3
        lib.stream_new.restype = c.c_void_p
        lib.stream_probe.argtypes = [c.c_void_p, c.POINTER(c.c_int16), *[c.c_int]*4,
                                    c.POINTER(c.c_int), c.POINTER(c.c_uint64)]
        lib.stream_probe.restype = c.c_int
        lib.opus_encoder_destroy.argtypes = [c.c_void_p]
        lib.stream_transform_inputs.argtypes = [c.c_void_p,c.POINTER(c.c_uint64)]
        lib.call_trace_get.argtypes = [c.POINTER(c.c_uint64)]
        lib.call_trace_get.restype = c.c_int
        def call_row():
            values = (c.c_uint64*40)()
            count = lib.call_trace_get(values)
            return ' '.join(map(str,[count//5,*values[:count]]))
        call_rows = ['# Original CELT calls per Opus packet: count; samples48/start/prediction/capacity/PCM16_FNV per call']
        band_call_rows = call_rows.copy()
        transform_rows = ['# Native CELT input state after the corresponding original cross-mode row; VBR_COUNT + MEM_E + IN_MEM120 FNV32 per API channel']
        rows = ['# Original public Opus '+REVISION+' AUDIO mode switching; fixed scalar fast64 Q24',
                '# RATE API VBR INDEX MS MODE REQUEST SEED PATTERN WIRE TO_MONO INTERNAL PREV_SPEECH RS0 RS1 MID_PCM_HASH']
        for rate in [8000, 12000, 16000, 24000, 48000]:
            for channels in [1, 2]:
                for vbr in [0, 1]:
                    state = lib.stream_new(rate, channels, vbr)
                    if not state:
                        raise ValueError('Cannot create reference encoder')
                    try:
                        for i in range(64):
                            ms = 10 if i % 2 == 0 else 20
                            mode = [1002, 1000, 1001, 1002][i//4 % 4]
                            request = 1 + (channels-1)*(i//7 % 2)
                            seed = (0x7374726d + i*982451653 + rate*ms) & 0xffffffff
                            pattern = 1 if i % 7 == 0 else 3
                            n = rate*ms//1000
                            samples = (c.c_int16*(channels*n))(*pcm(seed, channels*n, pattern))
                            out, hashes = (c.c_int*4)(), (c.c_uint64*3)()
                            result = lib.stream_probe(state, samples, n, mode, request, -1, out, hashes)
                            if result < 1:
                                raise ValueError(f'Native encode failed {rate}/{channels}/{vbr}/{i}: {result}')
                            rows.append(' '.join(map(str,[rate,channels,vbr,i,ms,mode,request,seed,pattern,*out,*hashes])))
                            transform = (c.c_uint64*2)()
                            lib.stream_transform_inputs(state,transform)
                            transform_rows.append(' '.join(map(str,transform)))
                            call_rows.append(call_row())
                    finally:
                        lib.opus_encoder_destroy(state)
        # Add short direct speech-to-CELT switches, which do not use an
        # outgoing speech packet/tail and therefore require a primary prefill.
        rate = 48000
        for channels in [1, 2]:
            for vbr in [0, 1]:
                state = lib.stream_new(rate, channels, vbr)
                if not state:
                    raise ValueError('Cannot create reference encoder')
                try:
                    for i in range(32):
                        ms = 5 if i % 8 == 5 else 10 if i % 2 == 0 else 20
                        mode = [1000,1000,1000,1000,1000,1002,1001,1001][i % 8]
                        request = 1 + (channels-1)*(i//7 % 2)
                        seed = (0x7374726d + i*982451653 + rate*ms) & 0xffffffff
                        pattern = 1 if i % 7 == 0 else 3
                        n = rate*ms//1000
                        samples = (c.c_int16*(channels*n))(*pcm(seed,channels*n,pattern))
                        out, hashes = (c.c_int*4)(), (c.c_uint64*3)()
                        result = lib.stream_probe(state,samples,n,mode,request,-1,out,hashes)
                        if result < 1:
                            raise ValueError('Native short transition encode failed')
                        rows.append(' '.join(map(str,[rate,channels,vbr,i,ms,mode,request,seed,pattern,*out,*hashes])))
                        transform = (c.c_uint64*2)()
                        lib.stream_transform_inputs(state,transform)
                        transform_rows.append(' '.join(map(str,transform)))
                        call_rows.append(call_row())
                finally:
                    lib.opus_encoder_destroy(state)
        if args.band_output:
            bands = ['# Original public Opus '+REVISION+' AUDIO SILK bandwidth prefill histories',
                     '# Same columns; desired WB for frames64..191, NB otherwise;20ms']
            for rate in [16000, 48000]:
                for channels in [1, 2]:
                    for vbr in [0, 1]:
                        state = lib.stream_new(rate, channels, vbr)
                        if not state:
                            raise ValueError('Cannot create reference encoder')
                        try:
                            for i in range(384):
                                band = 2 if 64 <= i < 192 else 0
                                seed = (0x62616e64 + i*982451653 + rate*20) & 0xffffffff
                                pattern = 0 if 40 <= i < 80 or 160 <= i < 224 or 304 <= i < 352 else 3
                                n = rate//50
                                samples = (c.c_int16*(channels*n))(*pcm(seed, channels*n, pattern))
                                out, hashes = (c.c_int*4)(), (c.c_uint64*3)()
                                result = lib.stream_probe(state, samples, n, 1000, channels, 1101+band, out, hashes)
                                if result < 1:
                                    raise ValueError(f'Native bandwidth encode failed {rate}/{channels}/{vbr}/{i}: {result}')
                                bands.append(' '.join(map(str,[rate,channels,vbr,i,20,1000,channels,seed,pattern,*out,*hashes])))
                                band_call_rows.append(call_row())
                        finally:
                            lib.opus_encoder_destroy(state)
            args.band_output.write_text('\n'.join(bands)+'\n')
            print('Captured',len(bands)-2,'original bandwidth prefill input histories')
        if args.call_output:
            args.call_output.write_text('\n'.join(call_rows)+'\n')
        if args.band_call_output:
            args.band_call_output.write_text('\n'.join(band_call_rows)+'\n')
        if args.celt_output:
            args.celt_output.write_text('\n'.join(transform_rows)+'\n')
        args.output.write_text('\n'.join(rows)+'\n')
        print('Captured',len(rows)-2,'original cross-mode speech input histories')


if __name__ == '__main__':
    main()
