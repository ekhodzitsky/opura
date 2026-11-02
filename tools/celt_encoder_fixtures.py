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
for name in ['analysis','analysis-math','analysis-tables','transients','tf-analysis','tf-packets','tf-math','dynalloc-tables','hybrid-dynalloc-analysis','dynalloc-analysis','dynalloc-packets','dynalloc-math','spreading-analysis','spreading-packets','spreading-tails','allocation-analysis-tables','allocation-analysis-math','stereo-analysis','trim-analysis','allocation-packets','allocation-tails','prefilter','comb-fields','pitch-packets']:mode.add_argument('--'+name,action='store_true')
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
int allocation_intensity(int rate,int previous);
void *prefilter_create(int C);
int prefilter_reset(CELTEncoder *s);
int prefilter_process(CELTEncoder *s,int LM,const int *input,const int *options,int *context,int *out);
void original_write_comb(ec_enc *enc,int bytes,int period,int gain,int tap);
opus_uint64 prefilter_history_hash(const opus_int32 *values,int length);
struct baseline {
    int rate,C,first,previous,detect,streak,spread,average,hf,tapset;unsigned rng;
    int intensity;opus_val16 saving;CELTEncoder *prefilter;
    opus_int32 preem[2],overlap[240],energy[42],residual[42],delayed;
};
void *baseline_create(int rate,int C,int detect){
    struct baseline *s=calloc(1,sizeof(*s));if(!s)return NULL;
    s->prefilter=prefilter_create(C);if(!s->prefilter){free(s);return NULL;}
    s->rate=rate;s->C=C;s->detect=detect;s->first=1;s->spread=SPREAD_NORMAL;s->average=256;return s;
}
int baseline_reset(struct baseline *s){int rate=s->rate,C=s->C,detect=s->detect;CELTEncoder *pf=s->prefilter;memset(s,0,sizeof(*s));s->prefilter=pf;s->rate=rate;s->C=C;s->detect=detect;s->first=1;s->spread=SPREAD_NORMAL;s->average=256;return prefilter_reset(pf);}
void baseline_set_flags(struct baseline *s,int flags){s->detect=flags;}
void baseline_destroy(struct baseline *s){if(s){free(s->prefilter);free(s);}}
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
    int allowed=LM && ec_tell(&enc)+3<=budget;
    int transient=0,channel=0;opus_val32 radius=0;opus_val16 estimate=0,frequency=-1;
    if(((s->detect&1)&&allowed)||(s->detect&62)){
        frequency=tone_detect(in,C,N+120,&radius,48000);int weak=0;
        int detected=transient_analysis(in,N+120,C,&estimate,&channel,0,&weak,frequency,radius);
        transient=(s->detect&1)&&allowed&&detected;
        radius=MIN32(radius,QCONST32(1.f,29)-SHL32(estimate,15));
    }
    opus_int32 raw_current[1920]={0};for(int ch=0;ch<C;ch++)OPUS_COPY(raw_current+ch*N,in+ch*(N+120)+120,N);
    int pf_options[8]={(s->detect&32)&&bytes>12*C&&1+16<=budget,10,bytes,estimate,frequency,radius,0,s->tapset};
    int pf_result[5];if(prefilter_process(s->prefilter,LM,raw_current,pf_options,in,pf_result))return -4;
    original_write_comb(&enc,bytes,pf_result[1],pf_result[2],pf_result[4]);
    allowed=LM&&ec_tell(&enc)+3<=budget;transient=transient&&allowed;
    if(allowed)ec_enc_bit_logp(&enc,transient,3);
    compute_mdcts(m,transient?(1<<LM):0,in,freq,C,C,LM,upsample,0);
    compute_band_energies(m,freq,amplitudes,end,C,LM,0);
    normalise_bands(m,freq,X,amplitudes,end,C,1<<LM);
    amp2Log2(m,end,end,amplitudes,target,C);
    int requests[21]={0},importance[21],spread_weight[21]={0};opus_int32 dyn_total=0,dyn_depth=0;
    for(int i=0;i<21;i++)importance[i]=13;
    if(s->detect&12){
        celt_glog comparison[42];OPUS_COPY(comparison,target,42);
        if(transient&&(s->detect&4)){
            celt_sig long_freq[1920]={0};celt_ener long_amplitudes[42]={0};
            compute_mdcts(m,0,in,long_freq,C,C,LM,upsample,0);
            compute_band_energies(m,long_freq,long_amplitudes,end,C,LM,0);
            amp2Log2(m,end,end,long_amplitudes,comparison,C);
            for(int ch=0;ch<C;ch++)for(int i=0;i<end;i++)comparison[ch*21+i]+=LM<<23;
        }
        celt_glog surround[21]={0};AnalysisInfo info;memset(&info,0,sizeof(info));
        dyn_depth=dynalloc_analysis(target,comparison,s->energy,21,0,end,C,requests,16,m->logN,transient,0,0,m->eBands,LM,bytes,&dyn_total,0,surround,&info,importance,spread_weight,frequency,radius);
    }

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
        int tf_importance[21];for(int i=0;i<21;i++)tf_importance[i]=(s->detect&4)?importance[i]:13;
        select=tf_analysis(m,end,transient,tf,IMAX(80,20480/bytes+2),X,N,LM,estimate,channel,tf_importance);
    } else for(int i=0;i<end;i++)tf[i]=transient;
    for(int i=0;i<21;i++)requested_tf[i]=tf[i];
    tf_encode(0,end,transient,tf,LM,select,&enc);
    if(ec_tell(&enc)+4<=budget){
        if((s->detect&8)&&!transient&&bytes>=10*C)
            s->spread=spreading_decision(m,X,&s->average,s->spread,&s->hf,&s->tapset,pf_result[0]&&!transient,end,C,1<<LM,spread_weight);
        else s->spread=SPREAD_NORMAL;
        ec_enc_icdf(&enc,s->spread,spread_icdf,5);
    } else s->spread=SPREAD_NORMAL;
    int spread=s->spread;
    int cap[21],offsets[21]={0},fine[21]={0},pvq[21]={0},priority[21]={0};init_caps(m,cap,LM,C);
    int total=bytes*64,tell=ec_tell_frac(&enc),logp=6,total_boost=0;
    for(int i=0;i<end;i++){
        int width=C*(m->eBands[i+1]-m->eBands[i])<<LM;
        int quanta=IMIN(width<<3,IMAX(48,width)),loop_logp=logp,boost=0,j;
        for(j=0;tell+(loop_logp<<3)<total-total_boost&&boost<cap[i];j++){
            int flag=(s->detect&4)&&j<requests[i];ec_enc_bit_logp(&enc,flag,loop_logp);tell=ec_tell_frac(&enc);
            if(!flag)break;
            boost+=quanta;total_boost+=quanta;loop_logp=1;
        }
        if(j)logp=IMAX(2,logp-1);
        offsets[i]=boost;
    }
    int rate=(bytes*8*50<<(3-LM))-(40*C+20)*((400>>LM)-50);
    int intensity=end,dual=0;
    if((s->detect&16)&&C==2){
        if(LM)dual=stereo_analysis(m,X,LM,N);
        s->intensity=IMIN(end,IMAX(0,allocation_intensity((opus_val16)(rate/1000),s->intensity)));
        intensity=s->intensity;
    }
    int requested_dual=dual,trim_fits=tell+48<=total-total_boost,trim=5;
    if(trim_fits){
        if(s->detect&16){
            AnalysisInfo info;memset(&info,0,sizeof(info));
            trim=alloc_trim_analysis(m,X,target,end,LM,C,N,&info,&s->saving,estimate,intensity,0,rate,0);
        }
        ec_enc_icdf(&enc,trim,trim_icdf,7);
    }
    opus_int32 balance=0;
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
    for(int i=0;i<5;i++)out[455+i]=pf_result[i];
    for(int ch=0;ch<2;ch++){
        opus_uint64 raw_hash=ch<C?prefilter_history_hash(s->prefilter->in_mem+C*120+ch*1024,1024):0;
        opus_uint64 overlap_hash=ch<C?prefilter_history_hash(s->prefilter->in_mem+ch*120,120):0;
        out[460+ch*4]=(opus_uint32)raw_hash;out[461+ch*4]=(opus_uint32)(raw_hash>>32);
        out[462+ch*4]=(opus_uint32)overlap_hash;out[463+ch*4]=(opus_uint32)(overlap_hash>>32);
    }
    out[447]=s->saving;out[448]=s->intensity;out[449]=trim;out[450]=intensity;out[451]=dual;out[452]=trim_fits;out[453]=requested_dual;out[454]=rate;
    out[378]=dyn_depth;out[379]=dyn_total;out[443]=s->spread;out[444]=s->average;out[445]=s->hf;out[446]=s->tapset;
    for(int i=0;i<21;i++){out[380+i]=requests[i];out[401+i]=i<end?importance[i]:0;out[422+i]=spread_weight[i];}

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
comb_signature='      if (pf_on==0)'
comb_begin=encoder_source.index(comb_signature)
comb_first=original_function(encoder_source,comb_signature)
comb_next=comb_begin+len(comb_first)
if not encoder_source[comb_next:].startswith(' else {'):raise SystemExit('Original comb branch changed')
comb_brace=encoder_source.index('{',comb_next);comb_depth=1;comb_end=comb_brace+1
while comb_depth:
    comb_depth+=(encoder_source[comb_end]=='{')-(encoder_source[comb_end]=='}');comb_end+=1
comb_block=encoder_source[comb_begin:comb_end]
wrapper+=r'''
void original_write_comb(ec_enc *enc,int bytes,int period,int gain,int tap){
    int hybrid=0,tell=1,total_bits=bytes*8,pf_on=gain!=0;
    int pitch_index=period,qg=gain?gain/3072-1:0,prefilter_tapset=tap;
'''+comb_block+r'''
}
int comb_field_probe(int bytes,int period,int gain,int tap,unsigned char *data,int *out){
    ec_enc enc;ec_enc_init(&enc,data,bytes);ec_enc_bit_logp(&enc,0,15);
    original_write_comb(&enc,bytes,period,gain,tap);
    out[0]=enc.rng;out[1]=ec_tell(&enc);out[2]=ec_tell_frac(&enc);
    ec_enc_done(&enc);return ec_get_error(&enc);
}
'''
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
void dynalloc_constants(int *out){out[0]=-GCONST(31.9f);out[1]=GCONST(.0062f);out[2]=QCONST16(120/M_PI,9);}
int dynalloc_probe(int LM,int C,int start,int end,int bytes,int lsb,int flags,int frequency,int tone,
    const int *current,const int *comparison,const int *previous,int *surround,int *out){
    int error;const CELTMode *m=opus_custom_mode_create(48000,960,&error);if(!m||error)return -1;
    int requests[21]={0},importance[21]={0},spread[21]={0};opus_int32 total=0;AnalysisInfo info;memset(&info,0,sizeof(info));
    out[0]=dynalloc_analysis(current,comparison,previous,21,start,end,C,requests,lsb,m->logN,flags&1,!!(flags&2),!!(flags&4),m->eBands,LM,bytes,&total,!!(flags&8),surround,&info,importance,spread,frequency,tone);
    out[1]=total;
    for(int i=0;i<21;i++){out[2+i]=requests[i];out[23+i]=i>=start&&i<end?importance[i]:0;out[44+i]=i>=start&&i<end?spread[i]:0;}
    return 0;
}
int spreading_defaults(int *out){
    CELTEncoder *s=malloc(celt_encoder_get_size(2));if(!s)return -1;
    int error=celt_encoder_init(s,48000,2,0);
    if(!error){out[0]=s->spread_decision;out[1]=s->tonal_average;out[2]=s->hf_average;out[3]=s->tapset_decision;}
    free(s);return error;
}
int spreading_tail(int LM,int C,int bytes,int remaining,unsigned char *data,int *out){
    int error;const CELTMode *m=opus_custom_mode_create(48000,960,&error);if(!m||error)return -1;
    int initial[4];if(spreading_defaults(initial))return -2;
    int decision=initial[0],average=initial[1],hf=initial[2],tapset=initial[3];
    celt_norm X[1920]={0};int weights[21];for(int i=0;i<21;i++)weights[i]=32;
    ec_enc enc;ec_enc_init(&enc,data,bytes);int left=bytes*8-1-remaining;
    while(left){int bits=IMIN(25,left);ec_enc_bits(&enc,(1u<<bits)-1,bits);left-=bits;}
    if(ec_tell(&enc)+4<=bytes*8){
        if(bytes>=10*C)decision=spreading_decision(m,X,&average,decision,&hf,&tapset,0,21,C,1<<LM,weights);
        else decision=SPREAD_NORMAL;
        ec_enc_icdf(&enc,decision,spread_icdf,5);
    } else decision=SPREAD_NORMAL;
    out[0]=decision;out[1]=average;out[2]=hf;out[3]=tapset;out[4]=enc.rng;out[5]=ec_tell(&enc);out[6]=ec_tell_frac(&enc);
    ec_enc_done(&enc);return ec_get_error(&enc);
}
int spreading_probe(int LM,int C,int end,int update,int average,int hf,int tapset,int previous,const int *input,const int *weights,int *out){
    int error;const CELTMode *m=opus_custom_mode_create(48000,960,&error);if(!m||error)return -1;
    out[0]=spreading_decision(m,input,&average,previous,&hf,&tapset,update,end,C,1<<LM,weights);
    out[1]=average;out[2]=hf;out[3]=tapset;return 0;
}
int dynalloc_importance(int follower){return PSHR32(13*celt_exp2_db(MING(follower,GCONST(4.f))),16);}
int dynalloc_median(int n,const int *input){return n==3?median_of_3(input):median_of_5(input);}
void dynalloc_caps(int LM,int C,int *out){
    int error;const CELTMode *m=opus_custom_mode_create(48000,960,&error);init_caps(m,out,LM,C);
}


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
observed_stereo=original_function(encoder_source,'static int stereo_analysis(').replace('static int stereo_analysis(','static int stereo_analysis_observed(',1)
needle='   return MULT16_32_Q15((m->eBands[13]<<(LM+1))+thetas, sumMS)'
if observed_stereo.count(needle)!=1:raise SystemExit('Original stereo observation changed')
observed_stereo=observed_stereo.replace(needle,'   observed_lr_cost=MULT16_32_Q15(m->eBands[13]<<(LM+1),sumLR);\n   observed_ms_cost=MULT16_32_Q15((m->eBands[13]<<(LM+1))+thetas,sumMS);\n'+needle,1)
observed_trim=original_function(encoder_source,'static int alloc_trim_analysis(').replace('static int alloc_trim_analysis(','static int alloc_trim_analysis_observed(',1)
needle='#ifdef FIXED_POINT\n   trim_index = PSHR32(trim, 8);'
if observed_trim.count(needle)!=1:raise SystemExit('Original trim observation changed')
observed_trim=observed_trim.replace(needle,'   observed_policy_trim=trim;observed_policy_tilt=diff;\n'+needle,1)
wrapper+='\nstatic int observed_lr_cost,observed_ms_cost,observed_policy_trim,observed_policy_tilt;\n'+observed_stereo+'\n'+observed_trim+r'''
int stereo_policy_probe(int LM,const int *input,int *out){
    int error;const CELTMode *m=opus_custom_mode_create(48000,960,&error);if(!m||error)return -1;
    int original=stereo_analysis(m,input,LM,120<<LM),observed=stereo_analysis_observed(m,input,LM,120<<LM);
    if(original!=observed)return -2;
    out[0]=original;out[1]=observed_lr_cost;out[2]=observed_ms_cost;return 0;
}
int trim_policy_probe(int LM,int C,int end,int intensity,int rate,int estimate,int surround,int saving,const int *input,const int *logs,int *out){
    int error;const CELTMode *m=opus_custom_mode_create(48000,960,&error);if(!m||error)return -1;
    AnalysisInfo info;memset(&info,0,sizeof(info));opus_val16 original_saving=saving,observed_saving=saving;
    int original=alloc_trim_analysis(m,input,logs,end,LM,C,120<<LM,&info,&original_saving,estimate,intensity,surround,rate,0);
    int observed=alloc_trim_analysis_observed(m,input,logs,end,LM,C,120<<LM,&info,&observed_saving,estimate,intensity,surround,rate,0);
    if(original!=observed||original_saving!=observed_saving)return -2;
    out[0]=original;out[1]=observed_policy_trim;out[2]=observed_policy_tilt;out[3]=original_saving;return 0;
}
'''
policy_arrays=[]
for original in ['intensity_thresholds','intensity_histeresis']:
    match=re.search(original+r'\[21\]\s*=\s*(?:/\*.*?\*/\s*)?\{(.*?)\};',encoder_source,re.S)
    if match is None:raise SystemExit('Original intensity array changed')
    values=[int(x) for x in match[1].split(',') if x.strip()]
    if len(values)!=21:raise SystemExit('Original intensity array shape changed')
    policy_arrays.append(values)
wrapper+='\nstatic const opus_val16 policy_thresholds[21]={' + ','.join(map(str,policy_arrays[0])) + '};\n'
wrapper+='static const opus_val16 policy_hysteresis[21]={' + ','.join(map(str,policy_arrays[1])) + '};\n'
wrapper+=r'''
void allocation_policy_constants(int *out){out[0]=EPSILON;out[1]=QCONST16(.707107f,15);out[2]=QCONST32(1.001f,20);out[3]=celt_log2(QCONST32(1.001f,20));}
int allocation_policy_defaults(int *out){
    CELTEncoder *s=malloc(celt_encoder_get_size(2));if(!s)return -1;
    int error=celt_encoder_init(s,48000,2,0);
    if(!error){out[0]=s->stereo_saving;out[1]=s->intensity;}
    free(s);return error;
}
int allocation_intensity(int rate,int previous){return hysteresis_decision(rate,policy_thresholds,policy_hysteresis,21,previous);}
void allocation_correlation_logs(int sum,int minimum,int *out){
    opus_val16 raw=celt_log2(QCONST32(1.001f,20)-MULT16_16(sum,sum));
    opus_val16 raw2=MAX16(HALF16(raw),celt_log2(QCONST32(1.001f,20)-MULT16_16(minimum,minimum)));
    out[0]=PSHR32(raw-QCONST16(6.f,10),2);out[1]=PSHR32(raw2-QCONST16(6.f,10),2);out[2]=-HALF16(out[1]);
}
int allocation_equivalent_rate(int LM,int C,int bytes){return (bytes*8*50<<(3-LM))-(40*C+20)*((400>>LM)-50);}

'''
# Reuse component assembly with unchanged original helpers at the exact entropy
# cursor. Prefix bytes are artificial resource tests, not complete PCM packets.
allocation_stage=wrapper[wrapper.index('    int cap[21],offsets'):wrapper.index('    quant_fine_energy(m,0,end')]
wrapper+=r'''
int allocation_tail(int LM,int C,int bytes,int remaining,int boost,int saving,unsigned char *data,int *out){
    int error;const CELTMode *m=opus_custom_mode_create(48000,960,&error);if(!m||error)return -1;
    int N=120<<LM,end=21;struct baseline state={0},*s=&state;
    s->detect=16|(boost?4:0);s->previous=21;s->intensity=8;s->saving=saving;
    celt_norm X[1920]={0};celt_glog target[42]={0};int requests[21];
    for(int i=0;i<21;i++){requests[i]=boost?255:0;for(int ch=0;ch<C;ch++)X[ch*N+(m->eBands[i]<<LM)]=QCONST32(1.f,24);}
    int transient=0;opus_val16 estimate=8192;
    ec_enc enc;ec_enc_init(&enc,data,bytes);int left=bytes*8-1-remaining;
    while(left){int bits=IMIN(25,left);ec_enc_bits(&enc,(1u<<bits)-1,bits);left-=bits;}
'''+allocation_stage+r'''
    out[0]=s->saving;out[1]=s->intensity;out[2]=trim;out[3]=intensity;out[4]=dual;out[5]=trim_fits;
    out[6]=enc.rng;out[7]=ec_tell(&enc);out[8]=ec_tell_frac(&enc);out[9]=coded;out[10]=balance;
    for(int i=0;i<21;i++){out[11+i]=offsets[i];out[32+i]=fine[i];out[53+i]=pvq[i];out[74+i]=priority[i];}
    ec_enc_done(&enc);return ec_get_error(&enc);
}
'''
wrapper+=r'''
opus_uint64 prefilter_history_hash(const opus_int32 *values,int length){
    opus_uint64 hash=14695981039346656037ULL;
    for(int i=0;i<length;i++)for(int byte=0;byte<4;byte++)hash=(hash^(((opus_uint32)values[i]>>(8*byte))&255))*1099511628211ULL;
    return hash;
}
void prefilter_destroy(CELTEncoder *s){free(s);}
void *prefilter_create(int C){
    CELTEncoder *s=malloc(celt_encoder_get_size(C));if(!s)return NULL;
    if(celt_encoder_init(s,48000,C,0)){free(s);return NULL;}return s;
}
int prefilter_reset(CELTEncoder *s){return celt_encoder_ctl(s,OPUS_RESET_STATE);}
void prefilter_observe(CELTEncoder *s,int *out){
    int C=s->channels;out[0]=s->prefilter_period;out[1]=s->prefilter_gain;out[2]=s->prefilter_tapset;
    for(int i=0;i<C*120;i++)out[3+i]=s->in_mem[i];
    for(int i=0;i<C*1024;i++)out[3+C*120+i]=s->in_mem[C*120+i];
}
int prefilter_process(CELTEncoder *s,int LM,const int *input,const int *options,int *context,int *out){
    int C=s->channels,N=120<<LM;opus_val16 gain;int pitch,qgain;
    for(int ch=0;ch<C;ch++){
        for(int i=0;i<120;i++)context[ch*(N+120)+i]=2147483647; /* Unused caller overlap must be replaced. */
        OPUS_COPY(context+ch*(N+120)+120,input+ch*N,N);
    }
    s->loss_rate=options[6];
    out[0]=run_prefilter(s,context,s->in_mem+C*120,C,N,options[7],&pitch,&gain,&qgain,options[0],options[1],options[3],options[2],&s->analysis,options[4],options[5]);
    s->prefilter_period=pitch;s->prefilter_gain=gain;s->prefilter_tapset=options[7];
    out[1]=pitch;out[2]=gain;out[3]=qgain;out[4]=options[7];return 0;
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
    for name,types,result in [('baseline_create',[c.c_int,c.c_int,c.c_int],ptr),('baseline_reset',[ptr],c.c_int),('baseline_destroy',[ptr],None),
        ('baseline_set_flags',[ptr,c.c_int],None),
        ('baseline_encode',[ptr,c.c_int,i16,c.c_int,u8,i32],c.c_int),('opus_decoder_create',[c.c_int,c.c_int,i32],ptr),('opus_decoder_destroy',[ptr],None),
        ('decode_packet',[ptr,u8,c.c_int,i16,u32],c.c_int),('reset_decoder',[ptr],c.c_int),('decoder_energy',[ptr,i32],None)]:
        fn=getattr(lib,name);fn.argtypes=types;fn.restype=result
    if args.comb_fields:
        lib.comb_field_probe.argtypes=[c.c_int]*4+[u8,i32];lib.comb_field_probe.restype=c.c_int
        print('# Unchanged original fresh-prefix comb encoding block:',REVISION)
        print('# comb BYTES PERIOD GAIN TAP RANGE TELL FRAC HEX')
        rows=0
        for budget in [3,4,8,16]:
            for period in range(15,1023):
                for gain in range(3072,24577,3072):
                    for tap in range(3):
                        data=(c.c_ubyte*budget)();out=(c.c_int32*3)()
                        if lib.comb_field_probe(budget,period,gain,tap,data,out):raise SystemExit('Original comb field failed')
                        print('comb',budget,period,gain,tap,c.c_uint32(out[0]).value,out[1],out[2],bytes(data).hex());rows+=1
        for budget in [2,3,4,8,16]:
            for period in [0,15,1022,1023]:
                for tap in range(3):
                    data=(c.c_ubyte*budget)();out=(c.c_int32*3)()
                    if lib.comb_field_probe(budget,period,0,tap,data,out):raise SystemExit('Original disabled comb field failed')
                    print('comb',budget,period,0,tap,c.c_uint32(out[0]).value,out[1],out[2],bytes(data).hex());rows+=1
        print(f'# rows={rows}')
        raise SystemExit
    if args.prefilter:
        lib.prefilter_create.argtypes=[c.c_int];lib.prefilter_create.restype=ptr
        lib.prefilter_destroy.argtypes=[ptr];lib.prefilter_destroy.restype=None
        lib.prefilter_reset.argtypes=[ptr];lib.prefilter_reset.restype=c.c_int
        lib.prefilter_observe.argtypes=[ptr,i32];lib.prefilter_observe.restype=None
        lib.prefilter_process.argtypes=[ptr,c.c_int,i32,i32,i32,i32];lib.prefilter_process.restype=c.c_int
        lib.analysis_cos.argtypes=[c.c_int];lib.analysis_cos.restype=c.c_int
        print('# Original unchanged run_prefilter and complete histories:',REVISION)
        print('# frame RESET LM C ENABLED COMPLEXITY BYTES TF FREQ RADIUS LOSS TAP; input[C*N]; result ON PERIOD GAIN QGAIN TAP; filtered[C*(N+120)]; overlap[C*120]; raw[C*1024]')
        configs=[]
        for complexity in [0,4,5,10]:configs.append([1,complexity,128,0,-1,0,0,0])
        for budget in [0,1,12,24,25,26,34,35,36,1275]:configs.append([1,10,budget,0,-1,0,0,0])
        for estimate in [0,1,16055,16056,16057,16383,16384]:configs.append([1,10,128,estimate,-1,0,0,0])
        for loss in [0,1,2,3,4,5,8,9,100]:configs.append([1,10,128,0,-1,0,loss,0])
        for radius in [531502207,531502208,531502209,536870912]:
            for frequency in [-1,0,50,51,3194,3195,25735,25736]:configs.append([1,10,128,0,frequency,radius,0,0])
        configs+=[[0,0,0,16384,25736,536870912,100,0],[0,10,128,0,-1,0,0,0]]
        rows=0;active=0;random=0x50524546
        for channels in [1,2]:
            for sequence in range(8):
                state=lib.prefilter_create(channels)
                if not state:raise SystemExit('Original prefilter creation failed')
                phase=0
                try:
                    initial=(c.c_int32*(3+channels*1144))();lib.prefilter_observe(state,initial)
                    if any(initial):raise SystemExit('Unexpected original initial prefilter state')
                    for frame in range(128):
                        reset=frame in [0,67];lm=(frame//8)%4 if sequence%2 else frame%4;n=120<<lm
                        if reset:
                            if lib.prefilter_reset(state):raise SystemExit('Original prefilter reset failed')
                        options=list(configs[(frame+sequence*11)%len(configs)]);options[7]=(frame//3+sequence)%3
                        period=[31,60,120,239,511,1022][(frame//16+sequence)%6]
                        amplitude=[1,4096,1<<20,1<<27,536870911][(frame//8+sequence)%5]
                        if sequence==0:options=[1,10,128,0,(51472+period//2)//period,536870912,0,options[7]]
                        if sequence==6:options=[1,10,[24,25,34,35][frame%4],0,-1,0,0,options[7]]
                        if sequence==7:options=[1,[0,4,5,10][frame%4],128,[16055,16056,16057,16384][(frame//4)%4],(51472+(period*(1+(frame%8>=4)))//2)//(period*(1+(frame%8>=4))),536870912,[0,3,5,9,100][(frame//16)%5],options[7]]
                        if frame in [11,37,92]:options[0]=0
                        values=[]
                        for channel in range(channels):
                            for i in range(n):
                                random^=(random<<13)&0xffffffff;random^=random>>17;random^=(random<<5)&0xffffffff
                                at=(phase+i+(period//3 if channel and frame%4==2 else 0))%period
                                if sequence in [0,3]:value=(lib.analysis_cos((phase+i)*131072//period)*amplitude)>>15
                                elif sequence==1:value=amplitude*(2*at-period)//period
                                elif sequence==2:value=(c.c_int32(random).value*amplitude)>>31
                                elif sequence==4:value=amplitude if at in [0,period//3] else 0
                                elif sequence==6:
                                    weight=1+(frame//16)%7
                                    base=amplitude*(4*min(at,period-at)-period)//period
                                    noise=(c.c_int32(random).value*amplitude)>>31
                                    value=(base*weight+noise*(8-weight))//8
                                elif sequence==7:value=(lib.analysis_cos((phase+i)*131072//period)*amplitude)>>15
                                else:value=amplitude if i%2 else -amplitude
                                if channel and frame%4==1:value=-value
                                if channel and frame%4==3:value=0
                                values.append(value)
                        phase+=n
                        context=(c.c_int32*(channels*(n+120)))();out=(c.c_int32*5)();history=(c.c_int32*(3+channels*1144))()
                        if lib.prefilter_process(state,lm,(c.c_int32*len(values))(*values),(c.c_int32*8)(*options),context,out):raise SystemExit('Original prefilter failed')
                        lib.prefilter_observe(state,history)
                        print('frame',int(reset),lm,channels,*options);print('input',*values);print('result',*out);print('filtered',*context)
                        print('overlap',*history[3:3+channels*120]);print('raw',*history[3+channels*120:])
                        if list(history[:3])!=[out[1],out[2],out[4]]:raise SystemExit('Original prefilter history mismatch')
                        rows+=1;active+=out[0]
                finally:lib.prefilter_destroy(state)
        print(f'# rows={rows} active={active}')
        raise SystemExit
    if args.stereo_analysis or args.trim_analysis:
        lib.stereo_policy_probe.argtypes=[c.c_int,i32,i32];lib.stereo_policy_probe.restype=c.c_int
        lib.trim_policy_probe.argtypes=[c.c_int]*8+[i32]*3;lib.trim_policy_probe.restype=c.c_int
        lib.analysis_cos.argtypes=[c.c_int];lib.analysis_cos.restype=c.c_int
        bands=[0,1,2,3,4,5,6,7,8,10,12,14,16,20,24,28,34,40,48,60,78,100]
        random=0x53544552
        def next_policy_random():
            global random
            random^=(random<<13)&0xffffffff;random^=random>>17;random^=(random<<5)&0xffffffff
            return random
        def vector(width,pattern,band):
            if pattern==0:raw=[0]*width
            elif pattern==1:raw=[32767]+[0]*(width-1)
            elif pattern==2:raw=[0]*(width-1)+[-32767]
            elif pattern==3:raw=[32767]*width
            elif pattern==4:raw=[32767 if i%2 else -32767 for i in range(width)]
            elif pattern==5:raw=[lib.analysis_cos((i+1)*(band*13+1)*1024) for i in range(width)]
            elif pattern==6:raw=[next_policy_random()%65536-32768 for _ in range(width)]
            else:raw=[32767 if i%4==0 else -16384 if i%4==1 else 0 for i in range(width)]
            den=math.isqrt(sum(x*x for x in raw))+1
            return [(x*(1<<24))//den for x in raw]
        def pair(lm,end,pattern,link,amplitude):
            n=120<<lm;values=[2147483647]*(2*n)
            for band in range(end):
                start=bands[band]<<lm;stop=bands[band+1]<<lm;width=stop-start
                left=vector(width,pattern,band)
                if link==0:right=left[:]
                elif link==1:right=[-x for x in left]
                elif link==2:right=vector(width,6,band)
                elif link==3:right=[0]*width
                elif link==4:right=left[1:]+left[:1]
                elif link==5:right=[x>>1 for x in left]
                else:right=left[:] if band<8 else left[1:]+left[:1]
                values[start:stop]=[(x*amplitude)>>24 for x in left]
                values[n+start:n+stop]=[(x*amplitude)>>24 for x in right]
            return values
        frames=0;analyses=0;overflow=0
        if args.stereo_analysis:
            print('# Original stereo L1 model and read-only costs:',REVISION)
            print('# frame LM INPUT[2*N]; stereo DUAL LR_COST MS_COST')
            for lm in range(4):
                for pattern in range(8):
                    for link in range(7):
                        for amplitude in [0,1,2,1023,1024,4096,1<<20,1<<23,1<<24]:
                            values=pair(lm,13,pattern,link,amplitude);out=(c.c_int32*3)()
                            error=lib.stereo_policy_probe(lm,(c.c_int32*len(values))(*values),out)
                            if error:raise SystemExit('Original stereo observer changed decisions')
                            print('frame',lm,*values);print('stereo',*out);frames+=1
            print(f'# frames={frames}')
            raise SystemExit
        print('# Original trim analysis and read-only raw/tilt diagnostics:',REVISION)
        print('# frame LM C END CASES INPUT[C*N] LOGS[42]; trim/overflow SAVING RATE TF_EST INTENSITY SURROUND INDEX RAW_Q8 TILT_Q19 NEXT_SAVING')
        for lm in range(4):
            n=120<<lm
            for channels in [1,2]:
                for end in range(2 if channels==1 else 8,22):
                    for pattern in range(8):
                        values=[2147483647]*n if channels==1 else pair(lm,end,pattern,[0,0,1,2,6,5,4,3][pattern],1<<24)
                        logs=[2147483647]*42
                        for channel in range(channels):
                            for band in range(end-1):
                                weight=2+2*band-end
                                if pattern==0:value=0
                                elif pattern==1:value=-(18<<24)
                                elif pattern==2:value=(2*band-end)*(1<<23)
                                elif pattern==3:value=(end-2*band)*(1<<23)
                                elif pattern==4:value=(32 if band%2 else -32)<<24
                                elif pattern in [5,7]:value=(32 if weight>=0 else -32)*(1<<24)*(-1 if pattern==7 and channel else 1)
                                else:value=next_policy_random()%((64<<24)+1)-(32<<24)
                                logs[channel*21+band]=value
                        total=sum((logs[channel*21+band]>>5)*(2+2*band-end) for channel in range(channels) for band in range(end-1))
                        valid=-2147483648<=total<=2147483647
                        print('frame',lm,channels,end,16,*values,*logs);frames+=1
                        for case in range(16):
                            saving=[0,1,63,64,128,255,256,511,512,768,1022,1280][case%12]
                            rate=[-2147483648,0,63999,64000,64001,65023,65024,79999,80000,80001,128000,2147483647][(case+pattern)%12]
                            estimate=[0,1,63,64,65,127,128,8191,8192,8193,16383,16384][case%12]
                            intensity=[0,1,7,8,end-1,end][case%6] if channels==2 else [0,end][case%2]
                            surround=[-(4<<24),-65537,-65536,-1,0,1,65535,65536,65537,4<<24][case%10]
                            out=(c.c_int32*4)()
                            error=lib.trim_policy_probe(lm,channels,end,intensity,rate,estimate,surround,saving,(c.c_int32*len(values))(*values),(c.c_int32*42)(*logs),out)
                            if error:raise SystemExit('Original trim observer changed index/saving')
                            print('trim' if valid else 'overflow',saving,rate,estimate,intensity,surround,*out)
                            analyses+=int(valid);overflow+=int(not valid)
        # Negative LF averaging must truncate before taking the absolute value.
        values=[2147483647]*240;values[:8]=[1<<24]*8;values[120:128]=[-(1<<24)]*7+[-(1<<24)+(1<<14)]
        logs=[2147483647]*42
        for channel in range(2):logs[channel*21:channel*21+7]=[0]*7
        out=(c.c_int32*4)()
        error=lib.trim_policy_probe(0,2,8,0,80000,0,0,1280,(c.c_int32*240)(*values),(c.c_int32*42)(*logs),out)
        if error:raise SystemExit('Original negative LF averaging regression failed')
        print('frame',0,2,8,1,*values,*logs);print('trim',1280,80000,0,0,0,*out);frames+=1;analyses+=1
        # Stateful saving rise, saturation and intensity-dependent collapse.
        values=[2147483647]*1920
        for band in range(21):
            start=bands[band]<<3;stop=bands[band+1]<<3
            values[start:stop]=[1<<24]+[0]*(stop-start-1)
            values[960+start:960+stop]=([1<<24]+[0]*(stop-start-1)) if band<8 else ([0,1<<24]+[0]*(stop-start-2))
        logs=[0]*42;saving=0
        print('sequence',3,2,21,40,*values,*logs);frames+=1
        for frame in range(40):
            intensity=21 if frame==20 else 0;out=(c.c_int32*4)()
            error=lib.trim_policy_probe(3,2,21,intensity,80000,0,0,saving,(c.c_int32*1920)(*values),(c.c_int32*42)(*logs),out)
            if error:raise SystemExit('Original trim saving sequence observer mismatch')
            print('trim',saving,80000,0,intensity,0,*out);saving=out[3];analyses+=1
        print(f'# frames={frames} analyses={analyses} overflow-rejections={overflow}')
        raise SystemExit
    if args.allocation_analysis_math:
        lib.allocation_policy_constants.argtypes=[i32];constants=(c.c_int32*4)();lib.allocation_policy_constants(constants)
        lib.allocation_policy_defaults.argtypes=[i32];defaults=(c.c_int32*2)()
        if lib.allocation_policy_defaults(defaults):raise SystemExit('Original allocation history observation failed')
        lib.allocation_correlation_logs.argtypes=[c.c_int,c.c_int,i32]
        lib.allocation_intensity.argtypes=[c.c_int,c.c_int];lib.allocation_intensity.restype=c.c_int
        lib.allocation_equivalent_rate.argtypes=[c.c_int]*3;lib.allocation_equivalent_rate.restype=c.c_int
        print('# Original allocation analysis scalar math:',REVISION)
        print('constants',*constants);print('defaults',*defaults);print('thresholds',*policy_arrays[0]);print('hysteresis',*policy_arrays[1])
        def fingerprint(values):
            value=14695981039346656037
            for x in values:
                for byte in (x&0xffffffff).to_bytes(4,'little'):value=((value^byte)*1099511628211)&0xffffffffffffffff
            return value
        maximum=0;minimum=10000
        for correlation in range(1025):
            values=[]
            for low in range(correlation+1):
                out=(c.c_int32*3)();lib.allocation_correlation_logs(correlation,low,out);values.extend(out)
                maximum=max(maximum,out[2]);minimum=min(minimum,out[2])
            print('correlation',correlation,correlation+1,fingerprint(values))
        for lm in range(4):
            for channels in [1,2]:
                for start in range(2,1276,128):
                    count=min(128,1276-start)
                    print('equivalent',lm,channels,start,count,fingerprint([lib.allocation_equivalent_rate(lm,channels,b) for b in range(start,start+count)]))
        for previous in range(22):
            for start in range(-32768,32768,256):
                print('intensity',previous,start,256,fingerprint([lib.allocation_intensity(rate,previous) for rate in range(start,start+256)]))
        print(f'# saving-target={minimum}..{maximum} correlation-pairs=525825 equivalent-rates=10192 intensity-cases=1441792')
        if minimum<0 or maximum>1280:raise SystemExit('Native saving target exceeds checked history domain')
        raise SystemExit
    if args.allocation_analysis_tables:
        lib.allocation_policy_constants.argtypes=[i32];values=(c.c_int32*4)();lib.allocation_policy_constants(values)
        defaults=(c.c_int32*2)();lib.allocation_policy_defaults.argtypes=[i32]
        if lib.allocation_policy_defaults(defaults):raise SystemExit('Original allocation history observation failed')
        print('// Original CELT trim/stereo constants, pinned Opus v1.6.1.')
        print('// See THIRD_PARTY_NOTICES.md.')
        print(f'// Native initial saving/intensity: {defaults[0]}/{defaults[1]}; guard logQ10: {values[3]}.')
        for name,value in zip(['MODEL_EPSILON','MID_SCALE_Q15','CORRELATION_GUARD_Q20'],values):print(f'pub(super) const {name}: i32 = {value};')
        for name,values in zip(['INTENSITY_THRESHOLDS','INTENSITY_HYSTERESIS'],policy_arrays):print(f'pub(super) const {name}: [i16;21] = '+str(values)+';')
        raise SystemExit
    if args.allocation_tails:
        lib.allocation_tail.argtypes=[c.c_int]*6+[u8,i32];lib.allocation_tail.restype=c.c_int
        print('# Original isolated allocation entropy tails:',REVISION)
        print('# tail LM C BYTES REMAINING BOOST INITIAL_SAVING SAVING DESIRED TRIM INTENSITY DUAL FITS RANGE TELL FRAC CODED BALANCE OFFSETS[21] FINE[21] PVQ[21] PRIORITY[21] HEX')
        rows=0
        for lm in range(4):
            for channels in [1,2]:
                budget=32*channels
                for boost in [0,1]:
                    for saving in [0,704]:
                        for remaining in range(33):
                            data=(c.c_ubyte*budget)();out=(c.c_int32*95)()
                            error=lib.allocation_tail(lm,channels,budget,remaining,boost,saving,data,out)
                            if error:raise SystemExit(f'Original allocation tail failed {lm,channels,remaining,boost,saving,error}')
                            values=list(out);values[6]=c.c_uint32(values[6]).value
                            print('tail',lm,channels,budget,remaining,boost,saving,*values,bytes(data).hex());rows+=1
        print(f'# rows={rows}')
        raise SystemExit
    if args.spreading_tails:
        lib.spreading_tail.argtypes=[c.c_int]*4+[u8,i32];lib.spreading_tail.restype=c.c_int
        print('# Original isolated entropy tail and spreading history:',REVISION)
        print('# tail LM C BYTES REMAINING DECISION AVERAGE HF TAPSET RANGE TELL FRAC HEX')
        for lm in range(4):
            for channels in [1,2]:
                budget=10*channels
                for remaining in range(9):
                    data=(c.c_ubyte*budget)();out=(c.c_int32*7)()
                    error=lib.spreading_tail(lm,channels,budget,remaining,data,out)
                    if error:raise SystemExit(f'Original isolated spreading tail failed {lm,channels,remaining,error}')
                    values=list(out);values[4]=c.c_uint32(values[4]).value
                    print('tail',lm,channels,budget,remaining,*values,bytes(data).hex())
        raise SystemExit
    if args.spreading_analysis:
        lib.spreading_probe.argtypes=[c.c_int]*8+[i32]*3;lib.spreading_probe.restype=c.c_int
        lib.analysis_cos.argtypes=[c.c_int];lib.analysis_cos.restype=c.c_int
        print('# Original pulse spreading/state analysis:',REVISION)
        print('# frame LM C END CASES INPUT[C*N]; spread UPDATE AVERAGE HF TAPSET PREVIOUS WEIGHTS[21] DECISION AVERAGE HF TAPSET')
        bands=[0,1,2,3,4,5,6,7,8,10,12,14,16,20,24,28,34,40,48,60,78,100]
        lib.spreading_defaults.argtypes=[i32];lib.spreading_defaults.restype=c.c_int
        defaults=(c.c_int32*4)()
        if lib.spreading_defaults(defaults):raise SystemExit('Original native initial history observation failed')
        print('defaults',*defaults)
        random=0x53505244;frames=0;analyses=0
        def next_spread_random():
            global random
            random^=(random<<13)&0xffffffff;random^=random>>17;random^=(random<<5)&0xffffffff
            return random
        def normalized(lm,channels,end,pattern):
            n=120<<lm;values=[2147483647]*(n*channels)
            for channel in range(channels):
                for band in range(end):
                    start=bands[band]<<lm;stop=bands[band+1]<<lm;width=stop-start
                    if pattern>=8:
                        threshold=[2048,512,128,2048][pattern-8]
                        q=max(1,threshold//width);x=math.isqrt(q*32768)
                        if x*x<q*32768:x+=1
                        value=x*1024-(1 if pattern==11 else 0)
                        vector=[value]*width
                    else:
                        raw=[]
                        for i in range(width):
                            if pattern==0:value=0
                            elif pattern==1:value=32767 if i==0 else 0
                            elif pattern==2:value=-32767 if i==width-1 else 0
                            elif pattern==3:value=32767
                            elif pattern==4:value=32767 if (i+channel)%2 else -32767
                            elif pattern==5:value=lib.analysis_cos((i+channel*3+1)*(13*band+1)*1024)
                            elif pattern==6:value=next_spread_random()%65536-32768
                            else:value=(32767 if i%4==0 else -32767 if i%4==1 else 0)
                            raw.append(value)
                        den=math.isqrt(sum(x*x for x in raw))+1
                        vector=[(x*(1<<24))//den for x in raw]
                    values[channel*n+start:channel*n+stop]=vector
            return values
        def emit(lm,channels,end,values,cases):
            global frames,analyses
            print('frame',lm,channels,end,len(cases),*values);frames+=1
            for update,average,hf,tapset,previous,weights in cases:
                out=(c.c_int32*4)()
                error=lib.spreading_probe(lm,channels,end,update,average,hf,tapset,previous,(c.c_int32*len(values))(*values),(c.c_int32*21)(*weights),out)
                if error:raise SystemExit('Original spreading analysis failed')
                print('spread',update,average,hf,tapset,previous,*weights,*out);analyses+=1
        for lm in range(4):
            for channels in [1,2]:
                for end in range(1,22):
                    for pattern in range(12):
                        cases=[]
                        for case in range(16):
                            weights=[255]*21
                            for band in range(end):weights[band]=[0,1,2,4,8,16,32][(case+band)%7] if case%3 else 32
                            if ((bands[end]-bands[end-1])<<lm)>8:weights[end-1]=max(1,weights[end-1])
                            cases.append((case%2,[0,79,80,128,255,256,383,384,512,767,768][case%11],[0,17,18,22,23,48,64][(case+pattern)%7],case%3,(case+pattern)%4,weights))
                        emit(lm,channels,end,normalized(lm,channels,end,pattern),cases)
        # Exhaust every checked tonal-history value for exact hysteresis/tie edges.
        for pattern in [0,3,11]:
            cases=[(0,average,0,0,previous,[32]*21) for average in range(769) for previous in range(4)]
            if pattern==3:cases.extend([(1,256,44,1,2,[32]*21),(1,256,36,1,2,[32]*21)])
            emit(3,1,21,normalized(3,1,21,pattern),cases)
        print(f'# frames={frames} analyses={analyses}')
        raise SystemExit
    if args.dynalloc_math:
        lib.dynalloc_importance.argtypes=[c.c_int];lib.dynalloc_importance.restype=c.c_int
        lib.dynalloc_median.argtypes=[c.c_int,i32];lib.dynalloc_median.restype=c.c_int
        lib.dynalloc_constants.argtypes=[i32];constants=(c.c_int32*3)();lib.dynalloc_constants(constants)
        print('# Original dynamic importance, medians and constants:',REVISION)
        print('constants',*constants)
        inputs=set([0,4<<24,(4<<24)+1,32<<24])
        for index in range(4097):
            for delta in [-8193,-8192,-8191,-1,0,1,8191,8192,8193]:
                value=(index<<14)+delta
                if 0<=value<=4<<24:inputs.add(value)
        for value in sorted(inputs):print('importance',value,lib.dynalloc_importance(value))
        for n in [3,5]:
            for encoded in range(3**n):
                values=[];remaining=encoded
                for _ in range(n):values.append([-32<<24,0,32<<24][remaining%3]);remaining//=3
                print('median',n,*values,lib.dynalloc_median(n,(c.c_int32*n)(*values)))
        print(f'# importance={len(inputs)} medians={3**3+3**5}')
        raise SystemExit
    if args.hybrid_dynalloc_analysis:
        lib.dynalloc_probe.argtypes=[c.c_int]*9+[i32]*5;lib.dynalloc_probe.restype=c.c_int
        print('# Original Hybrid dynamic analysis policy:',REVISION)
        frames=0;analyses=0
        for lm in [2,3]:
            threshold=30+5*lm
            for channels in [1,2]:
                for end in [19,21]:
                    for pattern in range(4):
                        arrays=[]
                        for kind in range(3):
                            values=[2147483647]*42
                            for channel in range(channels):
                                for band in range(end):
                                    if pattern==0:value=0
                                    elif pattern==1:value=((24 if band==end-2 else -12)+channel*2-kind)*(1<<24)
                                    elif pattern==2:value=((band%5)*4-8+channel*3-kind)*(1<<24)
                                    else:value=(28 if (band+channel+kind)%2 else -28)*(1<<24)
                                    values[channel*21+band]=value
                            arrays.append(values)
                        print('frame',lm,channels,17,end,24,*arrays[0],*arrays[1],*arrays[2]);frames+=1
                        for transient in [0,1]:
                            for budget in [threshold-1,threshold,threshold+1,64,128,1275]:
                                for variable in [0,1]:
                                    flags=transient|variable*2
                                    surround=[0]*21;out=(c.c_int32*65)()
                                    frequency=-1;tone=0
                                    error=lib.dynalloc_probe(lm,channels,17,end,budget,16,flags,frequency,tone,*[(c.c_int32*42)(*v) for v in arrays],(c.c_int32*21)(*surround),out)
                                    if error:raise SystemExit('Original Hybrid dynamic analysis failed')
                                    print('dyn',budget,16,flags,frequency,tone,*surround,*out);analyses+=1
        print(f'# frames={frames} analyses={analyses}')
        raise SystemExit
    if args.dynalloc_analysis:
        lib.dynalloc_probe.argtypes=[c.c_int]*9+[i32]*5;lib.dynalloc_probe.restype=c.c_int
        lib.dynalloc_caps.argtypes=[c.c_int,c.c_int,i32]
        print('# Original dynamic allocation analysis:',REVISION)
        print('# caps LM C VALUES[21]; frame LM C START END CASES CURRENT[42] COMPARISON[42] PREVIOUS[42]; dyn BYTES LSB FLAGS FREQ TONE SURROUND[21] DEPTH TOTAL REQUESTS[21] IMPORTANCE[21] SPREAD[21]')
        for lm in range(4):
            for channels in [1,2]:
                caps=(c.c_int32*21)();lib.dynalloc_caps(lm,channels,caps);print('caps',lm,channels,*caps)
        random=0x44594e41;frames=0;analyses=0
        def next_dyn_random():
            global random
            random^=(random<<13)&0xffffffff;random^=random>>17;random^=(random<<5)&0xffffffff
            return random
        for lm in range(4):
            threshold=30+5*lm
            for channels in [1,2]:
                for end in range(3,22):
                    for start in sorted(set([0,end//2,end-1])):
                        for pattern in range(6):
                            arrays=[]
                            for kind in range(3):
                                values=[2147483647]*42
                                for channel in range(channels):
                                    for band in range(end):
                                        if pattern==0:value=0
                                        elif pattern==1:value=([-32,-28,-14,0,16,32][(end+channel+kind)%6])<<24
                                        elif pattern==2:value=((band%7)*2-6+channel*4-kind)*(1<<22)
                                        elif pattern==3:value=(32 if (band+channel+kind)%2 else -32)<<24
                                        elif pattern==4:value=next_dyn_random()%(64*(1<<24)+1)-(32<<24)
                                        else:value=((band%5)*4-8)*(1<<24) if channel==0 else (16-kind*2)*(1<<24)
                                        values[channel*21+band]=value
                                arrays.append(values)
                            print('frame',lm,channels,start,end,8,*arrays[0],*arrays[1],*arrays[2]);frames+=1
                            for case in range(8):
                                transient=int(bool(lm) and case>=4 and case!=7)
                                flags=transient|([0,0,2,6,0,2,6,8][case])
                                budget=[threshold-1,threshold,threshold+1,64,threshold,1275,64,1275][case]
                                lsb=[8,16,24][(case+pattern)%3]
                                tone=[0,526133503,526133504,526133505,0,0,1<<29,0][case]
                                frequency=[-1,0,213,8192,25736][(pattern+case+end)%5]
                                surround=[2147483647]*21
                                for band in range(start,end):surround[band]=0 if case<2 else (1<<22) if case==2 else (4<<24) if case in [3,4] else next_dyn_random()%((4<<24)+1)
                                out=(c.c_int32*65)()
                                error=lib.dynalloc_probe(lm,channels,start,end,budget,lsb,flags,frequency,tone,*[(c.c_int32*42)(*v) for v in arrays],(c.c_int32*21)(*surround),out)
                                if error:raise SystemExit('Original dynamic analysis failed')
                                print('dyn',budget,lsb,flags,frequency,tone,*surround,*out);analyses+=1
        # The second channel has no rise; native backward following retains the
        # last rising-band index from the first channel.
        current=[0]*20+[16<<24]+[16<<24]*18+[0]*3
        comparison=current[:];previous=[0]*42;surround=[0]*21
        print('frame',1,2,0,21,1,*current,*comparison,*previous);frames+=1
        out=(c.c_int32*65)()
        error=lib.dynalloc_probe(1,2,0,21,128,16,2,-1,0,(c.c_int32*42)(*current),(c.c_int32*42)(*comparison),(c.c_int32*42)(*previous),(c.c_int32*21)(*surround),out)
        if error:raise SystemExit('Original retained-channel follower regression failed')
        print('dyn',128,16,2,-1,0,*surround,*out);analyses+=1
        print(f'# frames={frames} analyses={analyses}')
        raise SystemExit
    if args.dynalloc_tables:
        lib.dynalloc_constants.argtypes=[i32];values=(c.c_int32*3)();lib.dynalloc_constants(values)
        print('// Original CELT dynamic-analysis constants, pinned Opus v1.6.1.')
        print('// See THIRD_PARTY_NOTICES.md.')
        for name,value in zip(['INITIAL_DEPTH','NOISE_SLOPE','TONE_BIN_Q9'],values):print(f'pub(super) const {name}: i32 = {value};')
        raise SystemExit
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
    print('# Original '+('pitch/allocation/spreading/dynamic/TF/transient ' if args.pitch_packets else 'allocation/spreading/dynamic/TF/transient ' if args.allocation_packets else 'spreading/dynamic/TF/transient ' if args.spreading_packets else 'dynamic/TF/transient ' if args.dynalloc_packets else 'TF/transient ' if args.tf_packets else 'tone/transient ' if args.transients else 'fixed-policy ')+'CELT components and public Opus decoding:',REVISION)
    print('# frame RESET INPUT_HZ C LM PAYLOAD_BYTES HEX; input PCM; state RANGE PREVIOUS FIRST DELAYED PREEM[2] OVERLAP[240] ENERGY[42] RESIDUAL[42] TELL FRAC; decoded RATE API COUNT RANGE ENERGY[42] PCM[COUNT*API] (ten per frame)')
    if args.tf_packets:print('# TF state tail: TRANSIENT STREAK ENABLED SELECT REQUESTED_TF[21] EFFECTIVE_TF[21]')
    if args.dynalloc_packets:print('# Dynamic state tail adds DEPTH TOTAL RAW_REQUESTS[21] IMPORTANCE[21] SPREAD_WEIGHTS[21]')
    if args.spreading_packets:print('# Frame tail FLAGS: bit0 transient,bit1 TF,bit2 dynamic,bit3 spreading; base policies8,10,13,15; bit3 toggled off on frames7/11/17/23')
    if args.spreading_packets:print('# State after first332: TRANSIENT STREAK TF_ENABLED TF_SELECT REQUESTED_TF[21] EFFECTIVE_TF[21] DEPTH TOTAL RAW_REQUESTS[21] IMPORTANCE[21] SPREAD_WEIGHTS[21] DECISION TONAL_AVERAGE HF_AVERAGE TAPSET')
    if args.allocation_packets:print('# Frame FLAGS bits0..4 transient/TF/dynamic/spreading/allocation; base16,17,18,20,24,31; allocation off frames7/11/17/23. State tail after447: SAVING DESIRED_INTENSITY TRIM EFFECTIVE_INTENSITY DUAL TRIM_FITS REQUESTED_DUAL EQUIVALENT_RATE')
    if args.pitch_packets:
        lib.analysis_cos.argtypes=[c.c_int];lib.analysis_cos.restype=c.c_int
        print('# FLAGS bit5 pitch prefilter, base32/33/34/36/40/48/47/63; pitch off7/11/17/23. State after455: PF_ON PERIOD GAIN QGAIN TAP RAW0_LO RAW0_HI OVERLAP0_LO OVERLAP0_HI RAW1_LO RAW1_HI OVERLAP1_LO OVERLAP1_HI')
    random=0x50434d45;rows=0;history_differences=0
    def next_random():
        global random
        random^=(random<<13)&0xffffffff;random^=random>>17;random^=(random<<5)&0xffffffff
        return random
    for rate in [8000,12000,16000,24000,48000]:
        for channels in [1,2]:
            for sequence in range(8 if args.pitch_packets else 6 if args.allocation_packets else 4 if (args.transients or args.tf_packets or args.dynalloc_packets or args.spreading_packets) else 3):
                flags=[32,33,34,36,40,48,47,63][sequence] if args.pitch_packets else [16,17,18,20,24,31][sequence] if args.allocation_packets else [8,10,13,15][sequence] if args.spreading_packets else 7 if args.dynalloc_packets else 3 if args.tf_packets else int(args.transients)
                encoder=lib.baseline_create(rate,channels,flags)
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
                        reset=frame in [0,13];lm=3 if args.pitch_packets and sequence==4 else (frame//6)%4 if sequence>=2 else (frame+sequence)%4;n=120<<lm;count=n*rate//48000
                        budgets=([2,3,4,11,12,13,14,15,23,24,25,26,32,64,128,256,512,1275] if args.pitch_packets and sequence==0 else [2,3,4,8,9,10,11,14,15,16,20,19,21,29,30,31,32,64,128,256,512,1275] if (args.spreading_packets or args.allocation_packets or args.pitch_packets) and sequence==0 else [2,3,4,8,14,15,16,29,30,31,32,64,128,256,512,1275] if (args.tf_packets or args.dynalloc_packets) and sequence==0 else [2,3,4,8,16,32,64,128,256,512,1275] if sequence==0 else [8,16,32,64,128,256] if sequence==2 else [32,64,128,256,512,1275])
                        budget=budgets[frame%len(budgets)]
                        if reset:
                            if lib.baseline_reset(encoder):raise SystemExit('Original baseline reset failed')
                            for output,api,d in decoders:
                                if lib.reset_decoder(d):raise SystemExit('Original reset failed')
                        frame_flags=flags^(32 if args.pitch_packets and frame in [7,11,17,23] else 16 if args.allocation_packets and frame in [7,11,17,23] else 8 if args.spreading_packets and frame in [7,11,17,23] else 0)
                        if args.spreading_packets or args.allocation_packets or args.pitch_packets:lib.baseline_set_flags(encoder,frame_flags)
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
                                if args.allocation_packets and sequence==5 and frame==0:value=(c.c_int16(next_random()).value if channel==0 else 0)
                                if args.pitch_packets and sequence in [0,1,4,5,6,7]:
                                    period=max(16,rate//[100,200,400][frame//8]);at=(phase+i+(period//3 if channel and frame%4==2 else 0))%period
                                    value=(lib.analysis_cos((phase+i)*131072//period)*[24000,30000,16000][frame//8])>>15
                                    if sequence==1 and i==count//2:value=32767
                                    if sequence==4:
                                        value=(lib.analysis_cos((phase+i)*131072//period)*18000)>>15
                                        for high_frequency in [10000,14000,18000]:value+=(lib.analysis_cos((phase+i)*high_frequency*131072//rate)*2000)>>15
                                    if sequence==6:value=(value*3+(c.c_int16(next_random()).value//8))//4
                                    if sequence==7:value=(value+((at*2-period)*16000//period))//2
                                    if channel and frame%4==1:value=-value
                                    if channel and frame%4==3:value=0
                                samples.append(value)
                        phase+=count
                        data=(c.c_ubyte*1276)();state=(c.c_int32*468)()
                        length=lib.baseline_encode(encoder,lm,(c.c_int16*len(samples))(*samples),budget,data,state)
                        if length!=budget+1:raise SystemExit(f'Original baseline failed {rate,channels,sequence,frame,budget,length}')
                        values=list(state);values[0]=c.c_uint32(values[0]).value
                        for index in range(460,468):values[index]=c.c_uint32(values[index]).value
                        print('frame',int(reset),rate,channels,lm,budget,bytes(data[:length]).hex(),*([frame_flags] if args.spreading_packets or args.allocation_packets or args.pitch_packets else []));print('input',*samples);print('state',*(values if args.pitch_packets else values[:455] if args.allocation_packets else values[:447] if args.spreading_packets else values[:443] if args.dynalloc_packets else values[:378] if args.tf_packets else values[:334] if args.transients else values[:332]))
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
