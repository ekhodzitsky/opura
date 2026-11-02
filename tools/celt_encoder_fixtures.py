#!/usr/bin/env python3
"""Pinned original CELT component policies and tone/transient/TF diagnostics.

Optional external full-source probe, plus unchanged public Opus decoder PCM.
No foreign code is compiled or linked by ordinary Cargo builds/tests.
"""
import argparse
import ctypes as c
from pathlib import Path
import re
import math
import subprocess
import tempfile

REVISION='22244de5a79bd1d6d623c32e72bf1954b56235be'
parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('source',type=Path)
mode=parser.add_mutually_exclusive_group()
for name in ['analysis','analysis-math','analysis-tables','transients','tf-analysis','tf-packets','tf-math']:mode.add_argument('--'+name,action='store_true')
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
    int rate,C,first,previous,detect,streak;unsigned rng;
    opus_int32 preem[2],overlap[240],energy[42],residual[42],delayed;
};
void *baseline_create(int rate,int C,int detect){
    struct baseline *s=calloc(1,sizeof(*s));if(!s)return NULL;
    s->rate=rate;s->C=C;s->detect=detect;s->first=1;return s;
}
void baseline_reset(struct baseline *s){int rate=s->rate,C=s->C,detect=s->detect;memset(s,0,sizeof(*s));s->rate=rate;s->C=C;s->detect=detect;s->first=1;}
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
    ec_enc enc;ec_enc_init(&enc,data+1,bytes);unsigned budget=bytes*8;
    ec_enc_bit_logp(&enc,0,15);
    if(1+16<=budget)ec_enc_bit_logp(&enc,0,1);
    int allowed=LM && ec_tell(&enc)+3<=budget;
    int transient=0,channel=0;opus_val32 radius=0;opus_val16 estimate=0;
    if(((s->detect&1)&&allowed)||(s->detect&2)){
        opus_val16 frequency=tone_detect(in,C,N+120,&radius,48000);int weak=0;
        int detected=transient_analysis(in,N+120,C,&estimate,&channel,0,&weak,frequency,radius);
        transient=(s->detect&1)&&allowed&&detected;
        radius=MIN32(radius,QCONST32(1.f,29)-SHL32(estimate,15));
    }
    if(allowed)ec_enc_bit_logp(&enc,transient,3);
    compute_mdcts(m,transient?(1<<LM):0,in,freq,C,C,LM,upsample,0);
    compute_band_energies(m,freq,amplitudes,end,C,LM,0);
    normalise_bands(m,freq,X,amplitudes,end,C,1<<LM);
    amp2Log2(m,end,end,amplitudes,target,C);
    for(int ch=0;ch<C;ch++)for(int i=0;i<end;i++){
        int at=ch*21+i;
        if(ABS32(SUB32(target[at],s->energy[at]))<GCONST(2.f))
            target[at]-=MULT16_32_Q15(QCONST16(.25f,15),MAXG(-GCONST(.5f),MING(GCONST(.5f),s->residual[at])));
    }
    OPUS_CLEAR(s->residual,42);
    quant_coarse_energy(m,0,end,end,target,s->energy,budget,s->residual,&enc,C,LM,bytes,s->first,&s->delayed,1,0,0);
    /* Original two-pass scratch outside active bands is uninitialized and unused. */
    for(int i=0;i<42;i++)if(i>=C*21||i%21>=end)s->residual[i]=0;
    int tf[21]={0},requested_tf[21]={0},select=0;
    int analysis_enabled=(s->detect&2)&&bytes>=15*C&&radius<QCONST32(.98f,29);
    if(analysis_enabled){
        int importance[21];for(int i=0;i<21;i++)importance[i]=13;
        select=tf_analysis(m,end,transient,tf,IMAX(80,20480/bytes+2),X,N,LM,estimate,channel,importance);
    } else for(int i=0;i<end;i++)tf[i]=transient;
    for(int i=0;i<21;i++)requested_tf[i]=tf[i];
    tf_encode(0,end,transient,tf,LM,select,&enc);
    int spread=SPREAD_NORMAL;if(ec_tell(&enc)+4<=budget)ec_enc_icdf(&enc,spread,spread_icdf,5);
    int cap[21],offsets[21]={0},fine[21]={0},pvq[21]={0},priority[21]={0};init_caps(m,cap,LM,C);
    int total=bytes*64,tell=ec_tell_frac(&enc),logp=6;
    for(int i=0;i<end;i++)if(tell+(logp<<3)<total){ec_enc_bit_logp(&enc,0,logp);tell=ec_tell_frac(&enc);}
    int trim=5;if(tell+48<=total)ec_enc_icdf(&enc,trim,trim_icdf,7);
    int intensity=end,dual=0;opus_int32 balance=0;
    int bits=bytes*64-ec_tell_frac(&enc)-1;
    int anti=transient&&LM>=2&&bits>=((LM+2)<<3)?8:0;bits-=anti;
    int coded=clt_compute_allocation(m,0,end,offsets,cap,trim,&intensity,&dual,bits,&balance,pvq,fine,priority,C,LM,&enc,1,s->previous,end-1);
    quant_fine_energy(m,0,end,s->energy,s->residual,NULL,fine,&enc,C);
    unsigned seed=s->rng;unsigned char masks[42]={0};
    quant_all_bands(1,m,0,end,X,C==2?X+N:NULL,masks,amplitudes,pvq,transient?(1<<LM):0,spread,dual,intensity,tf,bytes*64-anti,balance,&enc,LM,coded,&seed,8,0,0);
    if(anti)ec_enc_bits(&enc,s->streak<2,1);
    if(ec_tell(&enc)>budget)return -2;
    quant_energy_finalise(m,0,end,s->energy,s->residual,fine,priority,budget-ec_tell(&enc),&enc,C);
    s->previous=s->previous?IMIN(s->previous+1,IMAX(s->previous-1,coded)):coded;
    s->rng=enc.rng;s->first=0;s->streak=transient||!allowed?IMIN(2,s->streak+1):0;
    out[0]=s->rng;out[1]=s->previous;out[2]=s->first;out[3]=s->delayed;out[4]=s->preem[0];out[5]=s->preem[1];
    for(int i=0;i<240;i++)out[6+i]=s->overlap[i];
    for(int i=0;i<42;i++){out[246+i]=s->energy[i];out[288+i]=s->residual[i];}
    out[330]=ec_tell(&enc);out[331]=ec_tell_frac(&enc);out[332]=transient;out[333]=s->streak;out[334]=!!analysis_enabled;out[335]=select;for(int i=0;i<21;i++)out[336+i]=requested_tf[i];for(int i=0;i<21;i++)out[357+i]=tf[i];
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

def original_function(text,signature):
    if text.count(signature)!=1:raise SystemExit('Original analysis signature changed')
    start=text.index(signature);at=text.index('{',start);depth=1;end=at+1
    while depth:
        depth+=(text[end]=='{')-(text[end]=='}');end+=1
    return text[start:end]
encoder_source=(source/'celt/celt_encoder.c').read_text()
observed=original_function(encoder_source,'static int transient_analysis(')
observed=observed.replace('static int transient_analysis(','static int transient_analysis_observed(',1)
needle='   tf_max = MAX16(0,celt_sqrt(27*mask_metric)-42);'
if observed.count(needle)!=1:raise SystemExit('Original metric observation site changed')
observed=observed.replace(needle,'   observed_metric=mask_metric;\n'+needle,1)
# A separate observed copy adds one read-only assignment. The unmodified original
# function is called too and every ordinary output is checked for equality.
wrapper+='\nstatic int observed_metric;\n'+observed+r'''
void analysis_constants(int *out){out[0]=QCONST32(1.999999f,29);out[1]=QCONST32(3.999999f,29);out[2]=QCONST32(.98f,29);out[3]=QCONST16(.026f,13);out[4]=QCONST16(.0069,14);out[5]=QCONST32(.139,28);}
int analysis_math(int op,int a,int b){return op?frac_div32_q29(a,b):acos_approx(a);}
int analysis_cos(int phase){return celt_cos_norm(phase);}
int analysis_probe(int LM,int C,int allow,const int *input,int *out){
    int n=(120<<LM)+120;opus_val32 radius;opus_val16 frequency=tone_detect(input,C,n,&radius,48000);
    opus_val16 estimate=0;int channel=0,weak=0;
    out[0]=transient_analysis(input,n,C,&estimate,&channel,allow,&weak,frequency,radius);
    out[1]=weak;out[2]=estimate;out[3]=channel;out[4]=frequency;out[5]=radius;
    opus_val16 observed_estimate=0;int observed_channel=0,observed_weak=0;
    int decision=transient_analysis_observed(input,n,C,&observed_estimate,&observed_channel,allow,&observed_weak,frequency,radius);
    if(decision!=out[0]||observed_weak!=weak||observed_estimate!=estimate||observed_channel!=channel)return -1;
    out[6]=observed_metric;return 0;
}
'''
observed_tf=original_function(encoder_source,'static int tf_analysis(')
observed_tf=observed_tf.replace('static int tf_analysis(','static int tf_analysis_observed(',1)
needle='   /* Search for the optimal tf resolution, including tf_select */'
if observed_tf.count(needle)!=1:raise SystemExit('Original TF metric observation site changed')
observed_tf=observed_tf.replace(needle,'   for(int observed_i=0;observed_i<len;observed_i++)observed_tf_metric[observed_i]=metric[observed_i];\n'+needle,1)
wrapper+='\nstatic int observed_tf_metric[21];\n'+observed_tf+r'''
int tf_bias_constant(void){return QCONST16(.04f,15);}
int tf_bias(int estimate){return MULT16_16_Q14(QCONST16(.04f,15),MAX16(-QCONST16(.25f,14),QCONST16(.5f,14)-estimate));}
int tf_l1(const int *input,int n,int level,int bias){return l1_metric(input,n,level,bias);}

int tf_probe(int LM,int end,int transient,int lambda,int estimate,const int *input,const int *importance,int *out){
    int error;const CELTMode *m=opus_custom_mode_create(48000,960,&error);if(!m||error)return -1;
    int original[21]={0},observed[21]={0};
    int select=tf_analysis(m,end,transient,original,lambda,(celt_norm*)input,120<<LM,LM,estimate,0,(int*)importance);
    int check=tf_analysis_observed(m,end,transient,observed,lambda,(celt_norm*)input,120<<LM,LM,estimate,0,(int*)importance);
    if(select!=check)return -2;
    out[0]=select;
    for(int i=0;i<21;i++){
        if(original[i]!=observed[i])return -3;
        out[1+i]=i<end?observed_tf_metric[i]:0;
        out[22+i]=original[i];
    }
    return 0;
}
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
    for name,types,result in [('baseline_create',[c.c_int,c.c_int,c.c_int],ptr),('baseline_reset',[ptr],None),('baseline_destroy',[ptr],None),
        ('baseline_encode',[ptr,c.c_int,i16,c.c_int,u8,i32],c.c_int),('opus_decoder_create',[c.c_int,c.c_int,i32],ptr),('opus_decoder_destroy',[ptr],None),
        ('decode_packet',[ptr,u8,c.c_int,i16,u32],c.c_int),('reset_decoder',[ptr],c.c_int),('decoder_energy',[ptr,i32],None)]:
        fn=getattr(lib,name);fn.argtypes=types;fn.restype=result
    if args.tf_math:
        lib.tf_bias.argtypes=[c.c_int];lib.tf_bias.restype=c.c_int
        lib.tf_l1.argtypes=[i32,c.c_int,c.c_int,c.c_int];lib.tf_l1.restype=c.c_int
        print('# Original TF bias and L1 metrics:',REVISION)
        print('# bias START COUNT FNV1A64(le-i32); vector N INPUT[N]; l1 LEVEL BIAS VALUE (25 per vector)')
        for start in range(0,16385,256):
            count=min(256,16385-start);fingerprint=14695981039346656037
            for estimate in range(start,start+count):
                value=lib.tf_bias(estimate)
                for byte in (value&0xffffffff).to_bytes(4,'little'):fingerprint=((fingerprint^byte)*1099511628211)&0xffffffffffffffff
            print('bias',start,count,fingerprint)
        random=0x4c314e4d
        for n in [1,2,4,8,16,22,32,44,64,88,128,176]:
            for pattern in range(8):
                values=[]
                for i in range(n):
                    random^=(random<<13)&0xffffffff;random^=random>>17;random^=(random<<5)&0xffffffff
                    values.append(0 if pattern==0 else 1 if pattern==1 else -1 if pattern==2 else 1023 if pattern==3 else -1025 if pattern==4 else 16384 if pattern==5 else -16384 if pattern==6 else c.c_int32(random).value>>7)
                print('vector',n,*values)
                for level in range(5):
                    for bias in [-328,-1,0,1,655]:
                        print('l1',level,bias,lib.tf_l1((c.c_int32*n)(*values),n,level,bias))
        raise SystemExit
    if args.tf_analysis:
        lib.tf_probe.argtypes=[c.c_int]*5+[i32,i32,i32];lib.tf_probe.restype=c.c_int
        lib.tf_bias_constant.restype=c.c_int
        lib.analysis_cos.argtypes=[c.c_int];lib.analysis_cos.restype=c.c_int
        print('# Original L1/Haar TF analysis and read-only metrics:',REVISION)
        print('# frame LM END OPTIONS INPUT[N]; tf TRANSIENT LAMBDA ESTIMATE IMPORTANCE[21] SELECT METRICS[21] DECISIONS[21]')
        print('constant',lib.tf_bias_constant())
        random=0x5446414e;frames=0;analyses=0
        def next_tf_random():
            global random
            random^=(random<<13)&0xffffffff;random^=random>>17;random^=(random<<5)&0xffffffff
            return random
        bands=[0,1,2,3,4,5,6,7,8,10,12,14,16,20,24,28,34,40,48,60,78,100]
        for lm in range(4):
            n=120<<lm
            for end in range(1,22):
                for pattern in range(8):
                    values=[0]*n
                    for band in range(end):
                        start=bands[band]<<lm;stop=bands[band+1]<<lm;width=stop-start
                        if pattern==0:raw=[0]*width
                        elif pattern==1:raw=[32767]+[0]*(width-1)
                        elif pattern==2:raw=[0]*(width-1)+[-32767]
                        elif pattern==3:raw=[32767]*width
                        elif pattern==4:raw=[32767 if i%2 else -32767 for i in range(width)]
                        elif pattern==5:raw=[lib.analysis_cos((i+1)*(13*band+1)*1024) for i in range(width)]
                        elif pattern==6:raw=[next_tf_random()%65536-32768 for _ in range(width)]
                        else:raw=[32767 if i==width//3 else -16384 if i==2*width//3 else 0 for i in range(width)]
                        energy=sum(x*x for x in raw);den=math.isqrt(energy)+1
                        values[start:stop]=[(x*(1<<24))//den for x in raw]
                    # Input after the active band end is deliberately nonphysical.
                    values[bands[end]<<lm:]=[2147483647]*(n-(bands[end]<<lm))
                    modes=[0,1] if lm else [0]
                    print('frame',lm,end,6*len(modes),*values);frames+=1
                    for transient in modes:
                        for case,(penalty,estimate) in enumerate([(0,0),(1,4096),(80,8192),(147,12288),(1367,16384),(65535,0)]):
                            importance=([0]*21 if case==0 else [1]*21 if case==1 else [13]*21 if case==2 else [208]*21 if case==3 else [255]*21 if case==4 else [next_tf_random()%256 for _ in range(21)])
                            out=(c.c_int32*43)()
                            error=lib.tf_probe(lm,end,transient,penalty,estimate,(c.c_int32*n)(*values),(c.c_int32*21)(*importance),out)
                            if error:raise SystemExit(f'Observed TF changed original decisions: {lm,end,pattern,case,error}')
                            print('tf',transient,penalty,estimate,*importance,*out);analyses+=1
        print(f'# frames={frames} analyses={analyses}')
        raise SystemExit
    if args.analysis_tables:
        constants=(c.c_int32*6)();lib.analysis_constants.argtypes=[i32];lib.analysis_constants(constants)
        values=[int(x) for x in re.search(r'static const unsigned char inv_table\[128\] = \{(.*?)\};',encoder_source,re.S)[1].replace('\n','').split(',') if x.strip()]
        if len(values)!=128:raise SystemExit('Original inverse table shape changed')
        print('// Original tone/transient constants, pinned Opus v1.6.1. See THIRD_PARTY_NOTICES.md.')
        print('pub(super) const INV: [u8;128] = '+str(values)+';')
        for name,value in zip(['TONE_LIMIT','COMPLEX_SCALE','TONE_GUARD','TONE_GUARD_FREQ','TF_SCALE','TF_OFFSET'],constants):print(f'pub(super) const {name}: i32 = {value};')
        raise SystemExit
    if args.analysis_math:
        lib.analysis_math.argtypes=[c.c_int]*3;lib.analysis_math.restype=c.c_int32
        print('# Original tone acos approximation and wide Q29 division:',REVISION)
        print('# acos START STEP COUNT FNV1A64(le-i32); divide A B RESULT')
        constants=(c.c_int32*6)();lib.analysis_constants.argtypes=[i32];lib.analysis_constants(constants)
        print('constants',*constants)
        inverse=[int(x) for x in re.search(r'static const unsigned char inv_table\[128\] = \{(.*?)\};',encoder_source,re.S)[1].replace('\n','').split(',') if x.strip()]
        print('inverse',*inverse)
        values=range(-(1<<29),(1<<29)+1,16384)
        for at in range(0,len(values),1024):
            block=values[at:at+1024];fingerprint=14695981039346656037
            for x in block:
                value=lib.analysis_math(0,x,0)
                for byte in (value&0xffffffff).to_bytes(4,'little'):fingerprint=((fingerprint^byte)*1099511628211)&0xffffffffffffffff
            print('acos',block[0],16384,len(block),fingerprint)
        for b in [1,2,3,5,7,8,15,16,127,128,32767,32768,65535,65536,1<<20,1<<29,(1<<30)-1,1<<30,2147483647]:
            for a in sorted(set([-2*b+1,-b-1,-b,-b+1,-1,0,1,b-1,b,b+1,2*b-1])):
                if -2147483648<=a<=2147483647 and abs(a)<2*b:print('divide',a,b,lib.analysis_math(1,a,b))
        random=0x544f4e45
        for _ in range(4096):
            random^=(random<<13)&0xffffffff;random^=random>>17;random^=(random<<5)&0xffffffff;b=1+random%2147483647
            random^=(random<<13)&0xffffffff;random^=random>>17;random^=(random<<5)&0xffffffff
            low=max(-2147483648,-2*b+1);high=min(2147483647,2*b-1);a=low+random%(high-low+1)
            print('divide',a,b,lib.analysis_math(1,a,b))
        raise SystemExit
    if args.analysis:
        lib.analysis_probe.argtypes=[c.c_int]*3+[i32,i32];lib.analysis_probe.restype=c.c_int
        lib.analysis_cos.argtypes=[c.c_int];lib.analysis_cos.restype=c.c_int
        print('# Original tone_detect/transient_analysis and read-only metric:',REVISION)
        print('# frame LM C INPUT[C*((120<<LM)+120)]; analysis ALLOW_WEAK TRANSIENT WEAK TF_ESTIMATE CHANNEL FREQUENCY_Q13 TONEISHNESS_Q29 MASK_METRIC (two per frame)')
        random=0x5452414e;rows=0
        def next_random():
            global random
            random^=(random<<13)&0xffffffff;random^=random>>17;random^=(random<<5)&0xffffffff
            return random
        def emit(lm,channels,values):
            global rows
            n=(120<<lm)+120
            if len(values)!=channels*n:raise SystemExit('Unexpected analysis input shape')
            print('frame',lm,channels,*values)
            for weak in [0,1]:
                out=(c.c_int32*7)()
                if lib.analysis_probe(lm,channels,weak,(c.c_int32*len(values))(*values),out):raise SystemExit('Read-only analysis observation changed original outputs')
                print('analysis',weak,*out)
            rows+=1
        for lm in range(4):
            n=(120<<lm)+120
            for channels in [1,2]:
                for amplitude in [0,1,2,15,1<<12,1<<20,1<<24,1<<27,1<<28]:
                    for pattern in range(12):
                        values=[]
                        for channel in range(channels):
                            for i in range(n):
                                if pattern==0:value=0
                                elif pattern==1:value=amplitude
                                elif pattern==2:value=-amplitude
                                elif pattern==3:value=amplitude if i%2 else -amplitude
                                elif pattern==4:value=amplitude if i==n//2 else 0
                                elif pattern==5:value=amplitude if i>=n//2 else 0
                                elif pattern==6:value=(amplitude if i%2 else -amplitude) if n//4<=i<n//2 else 0
                                elif pattern==7:value=next_random()%(2*amplitude+1)-amplitude
                                elif pattern==8:value=amplitude if i<12 else 0
                                elif pattern==9:value=amplitude if i==n-1 else 0
                                elif pattern==10:value=(amplitude if i%2 else -amplitude) if n//4<=i<n//2 else amplitude//16
                                else:value=next_random()%(2*amplitude+1)-amplitude if channel else 0
                                if channel and pattern in [3,4,5,6]:value=-value
                                values.append(value)
                        emit(lm,channels,values)
                for increment in [0,1,16,64,128,256,512,540,541,542,543,544,545,546,547,548,600,1024,2048,8192,16384,32768,65536]:
                    for amplitude in [1<<12,1<<20,1<<24,1<<27,1<<28]:
                        values=[(lib.analysis_cos((i+channel*3)*increment)*amplitude)>>15 for channel in range(channels) for i in range(n)]
                        emit(lm,channels,values)
        # Exact native strict-threshold regressions, found by bounded burst search.
        for lm,start,width,low in [(0,12,8,29),(1,177,12,14)]:
            n=(120<<lm)+120
            values=[(256 if start<=i<start+width else low)*(1<<14)*(1 if i%2 else -1) for i in range(n)]
            emit(lm,1,values)
        # Odd stereo operands straddle the rounded Q14 analysis boundary.
        n=240;left=[((lib.analysis_cos(i*1024)*256)>>15)*16384+8191 for i in range(n)]
        emit(0,2,left+[x+2 for x in left])
        # Seeded regression for rounding the energy of paired HPF samples.
        seed=162919406;values=[]
        for _ in range(1080):
            seed^=(seed<<13)&0xffffffff;seed^=seed>>17;seed^=(seed<<5)&0xffffffff
            values.append(c.c_int32(seed).value>>3)
        emit(3,1,values)
        print(f'# frames={rows} analyses={rows*2}')
        raise SystemExit
    print('# Original '+('TF/transient ' if args.tf_packets else 'tone/transient ' if args.transients else 'fixed-policy ')+'CELT components and public Opus decoding:',REVISION)
    print('# frame RESET INPUT_HZ C LM PAYLOAD_BYTES HEX; input PCM; state RANGE PREVIOUS FIRST DELAYED PREEM[2] OVERLAP[240] ENERGY[42] RESIDUAL[42] TELL FRAC; decoded RATE API COUNT RANGE ENERGY[42] PCM[COUNT*API] (ten per frame)')
    if args.tf_packets:print('# TF state tail: TRANSIENT STREAK ENABLED SELECT REQUESTED_TF[21] EFFECTIVE_TF[21]')
    random=0x50434d45;rows=0;history_differences=0
    def next_random():
        global random
        random^=(random<<13)&0xffffffff;random^=random>>17;random^=(random<<5)&0xffffffff
        return random
    for rate in [8000,12000,16000,24000,48000]:
        for channels in [1,2]:
            for sequence in range(4 if (args.transients or args.tf_packets) else 3):
                encoder=lib.baseline_create(rate,channels,3 if args.tf_packets else int(args.transients))
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
                        reset=frame in [0,13];lm=(frame//6)%4 if sequence>=2 else (frame+sequence)%4;n=120<<lm;count=n*rate//48000
                        budgets=([2,3,4,8,14,15,16,29,30,31,32,64,128,256,512,1275] if args.tf_packets and sequence==0 else [2,3,4,8,16,32,64,128,256,512,1275] if sequence==0 else [8,16,32,64,128,256] if sequence==2 else [32,64,128,256,512,1275])
                        budget=budgets[frame%len(budgets)]
                        if reset:
                            lib.baseline_reset(encoder)
                            for output,api,d in decoders:
                                if lib.reset_decoder(d):raise SystemExit('Original reset failed')
                        samples=[]
                        for i in range(count):
                            for channel in range(channels):
                                pattern=frame%8
                                if sequence==3:value=(32767 if i==count//2 else 0)*(-1 if channel else 1)
                                elif sequence==2:value=((phase+i+channel*31)%239-119)*200
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
                        data=(c.c_ubyte*1276)();state=(c.c_int32*378)()
                        length=lib.baseline_encode(encoder,lm,(c.c_int16*len(samples))(*samples),budget,data,state)
                        if length!=budget+1:raise SystemExit(f'Original baseline failed {rate,channels,sequence,frame,budget,length}')
                        values=list(state);values[0]=c.c_uint32(values[0]).value
                        print('frame',int(reset),rate,channels,lm,budget,bytes(data[:length]).hex());print('input',*samples);print('state',*(values if args.tf_packets else values[:334] if args.transients else values[:332]))
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
