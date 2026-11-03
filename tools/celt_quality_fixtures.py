#!/usr/bin/env python3
"""Optional pristine/corrected-Q24 native CELT quality and spectral-patch oracle."""
import argparse
import ctypes as c
from pathlib import Path
import re
import subprocess
import tempfile
from audio_baseline import REVISION
WRAPPER = r'''
#include "celt_encoder.c"
#include "entdec.h"
#include "opus.h"
#include <stdlib.h>
#if !OPUS_FAST_INT64 || DB_SHIFT != 24
#error "Expected scalar fixed fast64 Q24"
#endif
static unsigned char quality_bytes[1275];static int quality_length;static opus_uint32 quality_range;static opus_uint64 quality_pcm_hash;static OpusDecoder *quality_decoder;
int quality_packet(unsigned char *out,opus_uint32 *range,opus_uint64 *hash){memcpy(out,quality_bytes,quality_length);*range=quality_range;*hash=quality_pcm_hash;return quality_length;}
void *quality_new(int Fs,int C,int complexity) {
 CELTEncoder *s=calloc(1,celt_encoder_get_size(C));if(!s)return NULL;
 if(celt_encoder_init(s,Fs,C,0)){free(s);return NULL;}
 celt_encoder_ctl(s,CELT_SET_SIGNALLING(0));celt_encoder_ctl(s,OPUS_SET_COMPLEXITY(complexity));
 celt_encoder_ctl(s,OPUS_SET_LSB_DEPTH(16));
 celt_encoder_ctl(s,CELT_SET_END_BAND(Fs==8000?13:Fs<=16000?17:Fs==24000?19:21));
 int error;quality_decoder=opus_decoder_create(48000,C,&error);if(!quality_decoder||error){free(s);return NULL;}
 return s;
}
void quality_free(void *s){opus_decoder_destroy(quality_decoder);quality_decoder=NULL;free(s);}
int quality_patch(int C,int start,int end,const int *current,const int *previous) {
 return patch_transient_decision((celt_glog*)current,(celt_glog*)previous,21,start,end,C);
}
int quality_encode(CELTEncoder *s,const opus_int16 *pcm,int LM,int bytes,int *out) {
 int N=120<<LM,C=s->channels;unsigned char data[1275];celt_sig in[2160];
 for(int ch=0;ch<C;ch++) {
  opus_val32 mem=s->preemph_memE[ch];
  OPUS_COPY(in+ch*(N+120),&s->in_mem[C*120+(1+ch)*COMBFILTER_MAXPERIOD-120],120);
  celt_preemphasis(pcm+ch,in+ch*(N+120)+120,N,C,s->upsample,s->mode->preemph,&mem,0);
 }
 opus_val32 tone;opus_val16 freq=tone_detect(in,C,N+120,&tone,48000),estimate=0;int chan=0,weak=0;
 out[0]=s->complexity>=1?transient_analysis(in,N+120,C,&estimate,&chan,0,&weak,freq,tone):0;
 int ret=celt_encode_with_ec(s,pcm,N/s->upsample,data,bytes,NULL);if(ret<0)return ret;
 ec_dec dec;ec_dec_init(&dec,data,ret);int quiet=ec_dec_bit_logp(&dec,15),tell=quiet?ret*8:ec_tell(&dec);
 if(tell+16<=ret*8){if(ec_dec_bit_logp(&dec,1)) {
  int octave=ec_dec_uint(&dec,6);ec_dec_bits(&dec,4+octave);ec_dec_bits(&dec,3);
  if(ec_tell(&dec)+2<=ret*8)ec_dec_icdf(&dec,tapset_icdf,2);
 }tell=ec_tell(&dec);}
 out[1]=LM&&tell+3<=ret*8?ec_dec_bit_logp(&dec,3):0;
 memcpy(quality_bytes,data,ret);quality_length=ret;quality_range=s->rng;
 unsigned char packet[1276];int base=s->end==13?16:s->end==17?20:s->end==19?24:28;
 packet[0]=((base+LM)<<3)|(C==2?4:0);memcpy(packet+1,data,ret);
 opus_int16 decoded[1920];int samples=opus_decode(quality_decoder,packet,ret+1,decoded,N,0);if(samples!=N)return -9;
 quality_pcm_hash=14695981039346656037ULL;
 for(int i=0;i<N*C;i++){opus_uint16 v=(opus_uint16)decoded[i];quality_pcm_hash^=v&255;quality_pcm_hash*=1099511628211ULL;quality_pcm_hash^=v>>8;quality_pcm_hash*=1099511628211ULL;}
 out[2]=s->spread_decision;out[3]=s->tonal_average;out[4]=s->hf_average;out[5]=s->tapset_decision;out[6]=s->spec_avg;
 return ret;
}
'''
def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source',type=Path);parser.add_argument('output',type=Path)
    parser.add_argument('--patch-output',type=Path,required=True)
    parser.add_argument('--correct-q24',action='store_true',help='private source copy: retain spectral-patch x1/x2 as celt_glog instead of int16')
    args=parser.parse_args();source=args.source.resolve()
    if subprocess.check_output(['git','-C',str(source),'rev-parse','HEAD'],text=True).strip()!=REVISION:
        raise ValueError('Expected pinned Opus1.6.1')
    if subprocess.check_output(['git','-C',str(source),'status','--porcelain'],text=True).strip():
        raise ValueError('Reference checkout must be unchanged')
    def manifest(file, variable):
        match = re.search(r'^'+variable+r' = (.*?)(?=\n\n|\Z)', (source/file).read_text(), re.M | re.S)
        if match is None:
            raise ValueError('Missing native source manifest')
        return match[1].replace('\\', '').split()

    files = (manifest('silk_sources.mk', 'SILK_SOURCES') + manifest('silk_sources.mk', 'SILK_SOURCES_FIXED') +
             manifest('celt_sources.mk', 'CELT_SOURCES') + manifest('opus_sources.mk', 'OPUS_SOURCES'))
    with tempfile.TemporaryDirectory(prefix='opura-long-input-') as temp:
        folder = Path(temp)
        wrapper=WRAPPER
        if args.correct_q24:
            original=(source/'celt/celt_encoder.c').read_text()
            old='opus_val16 x1, x2;'
            if original.count(old)!=1:raise ValueError('Unexpected spectral-patch declaration')
            (folder/'corrected-celt.c').write_text(original.replace(old,'celt_glog x1, x2;'))
            wrapper=wrapper.replace('#include "celt_encoder.c"','#include "corrected-celt.c"')
            start=original.index('static int patch_transient_decision(')
            end=original.index('\n/** Apply window',start)
            pristine=original[start:end].replace('patch_transient_decision','pristine_patch_transient_decision',1)
            wrapper+='\n'+pristine+'\nint quality_patch_pristine(int C,int start,int end,const int *current,const int *previous){return pristine_patch_transient_decision((celt_glog*)current,(celt_glog*)previous,21,start,end,C);}'
        else:
            wrapper+='\nint quality_patch_pristine(int C,int start,int end,const int *current,const int *previous){return patch_transient_decision((celt_glog*)current,(celt_glog*)previous,21,start,end,C);}'

        (folder/'probe.c').write_text(wrapper)
        subprocess.run(['cc', '-shared', '-fPIC', '-O2', '-fwrapv', '-DVAR_ARRAYS', '-DOPUS_BUILD',
                        '-DFIXED_POINT', '-DDISABLE_FLOAT_API',
                        *['-I'+str(source/p) for p in ['include', 'celt', 'silk', 'silk/fixed', 'src']],
                        str(folder/'probe.c'), *[str(source/f) for f in files if f not in ['celt/celt_encoder.c']],
                        '-lm', '-o', str(folder/'probe.so')], check=True)
        lib=c.CDLL(str(folder/'probe.so'))
        lib.quality_new.argtypes=[c.c_int]*3;lib.quality_new.restype=c.c_void_p
        lib.quality_free.argtypes=[c.c_void_p]
        lib.quality_encode.argtypes=[c.c_void_p,c.POINTER(c.c_int16),c.c_int,c.c_int,c.POINTER(c.c_int)];lib.quality_encode.restype=c.c_int
        lib.quality_patch.argtypes=[*[c.c_int]*3,c.POINTER(c.c_int),c.POINTER(c.c_int)];lib.quality_patch.restype=c.c_int
        lib.quality_packet.argtypes=[c.POINTER(c.c_ubyte),c.POINTER(c.c_uint32),c.POINTER(c.c_uint64)];lib.quality_packet.restype=c.c_int
        lib.quality_patch_pristine.argtypes=lib.quality_patch.argtypes;lib.quality_patch_pristine.restype=c.c_int
        patch=['# '+('Corrected Q24 local types; private copy of ' if args.correct_q24 else 'Pristine narrowed local types; original ')+REVISION,'# C START END CURRENT42 PREVIOUS42 PRISTINE_PATCH Q24_PATCH']
        for channels in [1,2]:
            for end in [13,17,19,21]:
                for start in [0,2,6]:
                    for case in range(32):
                        old=[((i*case*137)%29-14)*(1<<23) for i in range(42)]
                        new=[((i*case*313+case*5)%29-14)*(1<<23) for i in range(42)]
                        if case<4:old=[0]*42;new=[(1<<24)+case-1]*42
                        result=lib.quality_patch(channels,start,end,(c.c_int*42)(*new),(c.c_int*42)(*old))
                        pristine=lib.quality_patch_pristine(channels,start,end,(c.c_int*42)(*new),(c.c_int*42)(*old))
                        patch.append(' '.join(map(str,[channels,start,end,*new,*old,pristine,result])))
        args.patch_output.write_text('\n'.join(patch)+'\n')
        rows=['# '+('Corrected Q24 local types; actual CELT ' if args.correct_q24 else 'Pristine actual CELT ')+REVISION,'# FS API LM COMPLEX CAP INDEX SEED PATTERN TEMPORAL FINAL_TRANSIENT SPREAD AVG HF TAP SPEC_AVG RANGE PCM_FNV PACKET_HEX']
        patched=0
        for rate in [8000,12000,16000,24000,48000]:
            for channels in [1,2]:
                for lm in range(4):
                    for complexity in range(11):
                        for nominal_cap in [24,160,2]:
                            state=lib.quality_new(rate,channels,complexity)
                            if not state:raise ValueError('Native init failed')
                            try:
                                for index in range(8):
                                    cap=(2 if index%2==0 else 160) if nominal_cap==2 else nominal_cap
                                    n=(120<<lm)*rate//48000;seed=(0x7175616c+index*982451653+rate*lm)&0xffffffff
                                    rng=seed;values=[]
                                    scale=32767 if nominal_cap==2 else [0,128,512,2048,8192,30000,32,24000][index]
                                    for i in range(n*channels):
                                        rng^=(rng<<13)&0xffffffff;rng^=rng>>17;rng^=(rng<<5)&0xffffffff
                                        x=(32767 if rng&1 else -32768) if nominal_cap==2 else (rng&65535)-32768
                                        if index==7 and nominal_cap!=2:x=32767 if (i//channels)%80<40 else -32768
                                        values.append(x*scale//32768)
                                    out=(c.c_int*7)();ret=lib.quality_encode(state,(c.c_int16*len(values))(*values),lm,cap,out)
                                    if ret<0:raise ValueError(f'Native encode failed:{ret}')
                                    patched+=not out[0] and bool(out[1])
                                    packet=(c.c_ubyte*1275)();rng_out=c.c_uint32();pcm_out=c.c_uint64();length=lib.quality_packet(packet,c.byref(rng_out),c.byref(pcm_out))
                                    rows.append(' '.join(map(str,[rate,channels,lm,complexity,cap,index,seed,index,*out,rng_out.value,pcm_out.value,bytes(packet[:length]).hex()])))
                            finally:lib.quality_free(state)
        args.output.write_text('\n'.join(rows)+'\n');print('Captured',len(rows)-2,'native CELT calls;',patched,'patched;',len(patch)-2,'helper cases')
if __name__=='__main__':main()
