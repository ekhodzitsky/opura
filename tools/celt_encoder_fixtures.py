#!/usr/bin/env python3
"""Pinned original CELT components under Opura's initial long-frame policy.

Optional external full-source probe, plus unchanged public Opus decoder PCM.
No foreign code is compiled or linked by ordinary Cargo builds/tests.
"""
import argparse
import ctypes as c
from pathlib import Path
import re
import subprocess
import tempfile

REVISION='22244de5a79bd1d6d623c32e72bf1954b56235be'
parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('source',type=Path)
args=parser.parse_args();source=args.source.resolve()
if subprocess.check_output(['git','-C',str(source),'rev-parse','HEAD'],text=True).strip()!=REVISION:
    raise SystemExit('Expected pinned Opus v1.6.1')
if subprocess.check_output(['git','-C',str(source),'status','--porcelain'],text=True).strip():
    raise SystemExit('Reference checkout must be clean')
def manifest(file,variable):
    match=re.search(r'^'+variable+r' = (.*?)(?=\n\n|\Z)',(source/file).read_text(),re.M|re.S)
    if match is None:raise SystemExit('Missing source manifest '+variable)
    return match[1].replace('\\','').split()

wrapper=r'''
#include "opus.h"
#include "celt_encoder.c"
#if !OPUS_FAST_INT64 || DB_SHIFT != 24
#error "Canonical fast64 fixed-point Q24 required"
#endif
struct baseline {
    int rate,C,first,previous;unsigned rng;
    opus_int32 preem[2],overlap[240],energy[42],residual[42],delayed;
};
void *baseline_create(int rate,int C){
    struct baseline *s=calloc(1,sizeof(*s));if(!s)return NULL;
    s->rate=rate;s->C=C;s->first=1;return s;
}
void baseline_reset(struct baseline *s){int rate=s->rate,C=s->C;memset(s,0,sizeof(*s));s->rate=rate;s->C=C;s->first=1;}
void baseline_destroy(struct baseline *s){free(s);}
int baseline_encode(struct baseline *s,int LM,const opus_int16 *pcm,int bytes,unsigned char *data,int *out){
    int error;const CELTMode *m=opus_custom_mode_create(48000,960,&error);
    if(!m||error)return -1;
    int N=120<<LM,C=s->C,upsample=48000/s->rate;
    int end=s->rate==8000?13:s->rate<=16000?17:s->rate==24000?19:21;
    int config=s->rate==8000?16:s->rate<=16000?20:s->rate==24000?24:28;
    celt_sig in[2160]={0},freq[1920]={0};celt_norm X[1920]={0};celt_ener amplitudes[42]={0};celt_glog target[42]={0};
    for(int ch=0;ch<C;ch++){
        OPUS_COPY(in+ch*(N+120),s->overlap+ch*120,120);
        celt_preemphasis(pcm+ch,in+ch*(N+120)+120,N,C,upsample,m->preemph,s->preem+ch,0);
        OPUS_COPY(s->overlap+ch*120,in+ch*(N+120)+N,120);
    }
    compute_mdcts(m,0,in,freq,C,C,LM,upsample,0);
    compute_band_energies(m,freq,amplitudes,end,C,LM,0);
    normalise_bands(m,freq,X,amplitudes,end,C,1<<LM);
    amp2Log2(m,end,end,amplitudes,target,C);
    for(int ch=0;ch<C;ch++)for(int i=0;i<end;i++){
        int at=ch*21+i;
        if(ABS32(SUB32(target[at],s->energy[at]))<GCONST(2.f))
            target[at]-=MULT16_32_Q15(QCONST16(.25f,15),MAXG(-GCONST(.5f),MING(GCONST(.5f),s->residual[at])));
    }
    OPUS_CLEAR(s->residual,42);
    ec_enc enc;ec_enc_init(&enc,data+1,bytes);unsigned budget=bytes*8;
    ec_enc_bit_logp(&enc,0,15);
    if(1+16<=budget)ec_enc_bit_logp(&enc,0,1);
    if(LM && ec_tell(&enc)+3<=budget)ec_enc_bit_logp(&enc,0,3);
    quant_coarse_energy(m,0,end,end,target,s->energy,budget,s->residual,&enc,C,LM,bytes,s->first,&s->delayed,1,0,0);
    /* Original two-pass scratch outside active bands is uninitialized and unused. */
    for(int i=0;i<42;i++)if(i>=C*21||i%21>=end)s->residual[i]=0;
    int tf[21]={0};tf_encode(0,end,0,tf,LM,0,&enc);
    int spread=SPREAD_NORMAL;if(ec_tell(&enc)+4<=budget)ec_enc_icdf(&enc,spread,spread_icdf,5);
    int cap[21],offsets[21]={0},fine[21]={0},pvq[21]={0},priority[21]={0};init_caps(m,cap,LM,C);
    int total=bytes*64,tell=ec_tell_frac(&enc),logp=6;
    for(int i=0;i<end;i++)if(tell+(logp<<3)<total){ec_enc_bit_logp(&enc,0,logp);tell=ec_tell_frac(&enc);}
    int trim=5;if(tell+48<=total)ec_enc_icdf(&enc,trim,trim_icdf,7);
    int intensity=end,dual=0;opus_int32 balance=0;
    int coded=clt_compute_allocation(m,0,end,offsets,cap,trim,&intensity,&dual,bytes*64-ec_tell_frac(&enc)-1,&balance,pvq,fine,priority,C,LM,&enc,1,s->previous,end-1);
    quant_fine_energy(m,0,end,s->energy,s->residual,NULL,fine,&enc,C);
    unsigned seed=s->rng;unsigned char masks[42]={0};
    quant_all_bands(1,m,0,end,X,C==2?X+N:NULL,masks,amplitudes,pvq,0,spread,dual,intensity,tf,bytes*64,balance,&enc,LM,coded,&seed,8,0,0);
    if(ec_tell(&enc)>budget)return -2;
    quant_energy_finalise(m,0,end,s->energy,s->residual,fine,priority,budget-ec_tell(&enc),&enc,C);
    s->previous=s->previous?IMIN(s->previous+1,IMAX(s->previous-1,coded)):coded;
    s->rng=enc.rng;s->first=0;
    out[0]=s->rng;out[1]=s->previous;out[2]=s->first;out[3]=s->delayed;out[4]=s->preem[0];out[5]=s->preem[1];
    for(int i=0;i<240;i++)out[6+i]=s->overlap[i];
    for(int i=0;i<42;i++){out[246+i]=s->energy[i];out[288+i]=s->residual[i];}
    out[330]=ec_tell(&enc);out[331]=ec_tell_frac(&enc);
    ec_enc_done(&enc);if(ec_get_error(&enc))return -3;
    data[0]=((config+LM)<<3)|(C==2?4:0);
    return bytes+1;
}
int decode_packet(OpusDecoder *s,const unsigned char *data,int length,opus_int16 *pcm,unsigned *range){
    int count=opus_decode(s,data,length,pcm,5760,0);
    if(count<0)return count;
    if(opus_decoder_ctl(s,OPUS_GET_FINAL_RANGE(range))!=OPUS_OK)return -999;
    return count;
}
int reset_decoder(OpusDecoder *s){return opus_decoder_ctl(s,OPUS_RESET_STATE);}
'''
files=manifest('silk_sources.mk','SILK_SOURCES')+manifest('silk_sources.mk','SILK_SOURCES_FIXED')+manifest('celt_sources.mk','CELT_SOURCES')+manifest('opus_sources.mk','OPUS_SOURCES')
with tempfile.TemporaryDirectory(prefix='opura-celt-encoder-') as temp:
    path=Path(temp);(path/'probe.c').write_text(wrapper)
    # Appended read-only observations; every original decoding statement is intact.
    (path/'celt_decoder.c').write_text((source/'celt/celt_decoder.c').read_text()+r'''
void baseline_decoder_energy(const CELTDecoder *s,int *out){
    const celt_glog *e=(const celt_glog*)(s->_decode_mem+(DECODE_BUFFER_SIZE+s->overlap)*s->channels);
    for(int i=0;i<42;i++)out[i]=e[i];
}
''')
    (path/'opus_decoder.c').write_text((source/'src/opus_decoder.c').read_text()+r'''
extern void baseline_decoder_energy(const CELTDecoder*,int*);
void decoder_energy(const OpusDecoder *s,int *out){baseline_decoder_energy((const CELTDecoder*)((const char*)s+s->celt_dec_offset),out);}
''')
    subprocess.run(['cc','-shared','-fPIC','-O2','-fwrapv','-DVAR_ARRAYS','-DOPUS_BUILD','-DFIXED_POINT','-DDISABLE_FLOAT_API','-DRESYNTH',
                    *['-I'+str(source/p) for p in ['include','celt','silk','silk/fixed','src']],str(path/'probe.c'),str(path/'celt_decoder.c'),str(path/'opus_decoder.c'),
                    *[str(source/f) for f in files if f not in ['celt/celt_encoder.c','celt/celt_decoder.c','src/opus_decoder.c']],'-lm','-o',str(path/'probe.so')],check=True)
    lib=c.CDLL(str(path/'probe.so'));ptr=c.c_void_p;i16=c.POINTER(c.c_int16);u8=c.POINTER(c.c_ubyte);u32=c.POINTER(c.c_uint32);i32=c.POINTER(c.c_int)
    for name,types,result in [('baseline_create',[c.c_int,c.c_int],ptr),('baseline_reset',[ptr],None),('baseline_destroy',[ptr],None),
        ('baseline_encode',[ptr,c.c_int,i16,c.c_int,u8,i32],c.c_int),('opus_decoder_create',[c.c_int,c.c_int,i32],ptr),('opus_decoder_destroy',[ptr],None),
        ('decode_packet',[ptr,u8,c.c_int,i16,u32],c.c_int),('reset_decoder',[ptr],c.c_int),('decoder_energy',[ptr,i32],None)]:
        fn=getattr(lib,name);fn.argtypes=types;fn.restype=result
    print('# Original fixed-policy CELT components and public Opus decoding:',REVISION)
    print('# frame RESET INPUT_HZ C LM PAYLOAD_BYTES HEX; input PCM; state RANGE PREVIOUS FIRST DELAYED PREEM[2] OVERLAP[240] ENERGY[42] RESIDUAL[42] TELL FRAC; decoded RATE API COUNT RANGE ENERGY[42] PCM[COUNT*API] (ten per frame)')
    random=0x50434d45;rows=0;history_differences=0
    def next_random():
        global random
        random^=(random<<13)&0xffffffff;random^=random>>17;random^=(random<<5)&0xffffffff
        return random
    for rate in [8000,12000,16000,24000,48000]:
        for channels in [1,2]:
            for sequence in range(3):
                encoder=lib.baseline_create(rate,channels)
                decoders=[]
                if not encoder:raise SystemExit('Encoder allocation failed')
                for output in [8000,12000,16000,24000,48000]:
                    for api in [1,2]:
                        error=c.c_int();d=lib.opus_decoder_create(output,api,c.byref(error))
                        if not d or error.value:raise SystemExit('Original decoder creation failed')
                        decoders.append((output,api,d))
                phase=0
                try:
                    for frame in range(24):
                        reset=frame in [0,13];lm=(frame//6)%4 if sequence==2 else (frame+sequence)%4;n=120<<lm;count=n*rate//48000
                        budget=([2,3,4,8,16,32,64,128,256,512,1275] if sequence==0 else [8,16,32,64,128,256] if sequence==2 else [32,64,128,256,512,1275])[frame%(11 if sequence==0 else 6)]
                        if reset:
                            lib.baseline_reset(encoder)
                            for output,api,d in decoders:
                                if lib.reset_decoder(d):raise SystemExit('Original reset failed')
                        samples=[]
                        for i in range(count):
                            for channel in range(channels):
                                pattern=frame%8
                                if sequence==2:value=((phase+i+channel*31)%239-119)*200
                                elif pattern==0:value=0
                                elif pattern==1:value=32767 if (i+channel)%2 else -32768
                                elif pattern==2:value=c.c_int16(next_random()).value
                                elif pattern==3:value=(1 if i%17==0 else 0)*(-1 if channel else 1)
                                elif pattern==4:value=((i+frame*7+channel*31)%239-119)*200
                                elif pattern==5:value=32767 if i==count//2 else 0
                                elif pattern==6:value=32767 if channel==0 else -32768
                                else:value=(next_random()%8192-4096)*(-1 if channel else 1)
                                samples.append(value)
                        phase+=count
                        data=(c.c_ubyte*1276)();state=(c.c_int32*332)()
                        length=lib.baseline_encode(encoder,lm,(c.c_int16*len(samples))(*samples),budget,data,state)
                        if length!=budget+1:raise SystemExit(f'Original baseline failed {rate,channels,sequence,frame,budget,length}')
                        values=list(state);values[0]=c.c_uint32(values[0]).value
                        print('frame',int(reset),rate,channels,lm,budget,bytes(data[:length]).hex());print('input',*samples);print('state',*values)
                        for output,api,d in decoders:
                            pcm=(c.c_int16*11520)();final=c.c_uint32();actual=lib.decode_packet(d,data,length,pcm,c.byref(final))
                            if actual!=n*output//48000 or final.value!=values[0]:raise SystemExit(f'Original packet decode mismatch {rate,channels,sequence,frame,output,api,actual}')
                            energies=(c.c_int32*42)();lib.decoder_energy(d,energies)
                            if api==channels and list(energies[:channels*21])!=values[246:246+channels*21]:history_differences+=1
                            print('decoded',output,api,actual,final.value,*energies,*pcm[:actual*api])
                        rows+=1
                finally:
                    lib.baseline_destroy(encoder)
                    for output,api,d in decoders:lib.opus_decoder_destroy(d)
    print(f'# rows={rows} public-decodes={rows*10} native-energy-history-differences={history_differences}')
