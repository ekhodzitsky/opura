#!/usr/bin/env python3
"""Capture actual public SILK scalar or delayed NSQ calls without source edits.

The linker wrapper records input/output around the unchanged original function.
Only optional fixture generation needs a C compiler and external pinned libopus.
"""
import argparse
from pathlib import Path
import re
import subprocess
import tempfile

REVISION = '22244de5a79bd1d6d623c32e72bf1954b56235be'
WRAPPER = r'''
#include "main.h"
#include "opus.h"
#include <stdio.h>
#include <stdint.h>
#define ARGS const silk_encoder_state *s,silk_nsq_state *n,SideInfoIndices *ix,const opus_int16 *x,opus_int8 *pulse,const opus_int16 *a,const opus_int16 *b,const opus_int16 *ar,const opus_int *harm,const opus_int *tilt,const opus_int32 *lf,const opus_int32 *gain,const opus_int *pitch,opus_int lambda,opus_int scale
void __real_silk_NSQ_c(ARGS);
static struct {
 int valid,rate,subframes,order,signal,offset,seed,interpolation,lambda,scale,states,warping,after_seed;
 int coefficients[168];
 silk_nsq_state before;
 int input[320],pulses[320],pcm[320];
 uint64_t after;
} record;
static uint64_t hash;
static void add(int value) {
 unsigned v=(unsigned)value;
 for(int j=0;j<4;j++)hash=(hash^((v>>(8*j))&255))*UINT64_C(1099511628211);
}
static void state(const silk_nsq_state *n,FILE *out) {
 #define VALUE(v) do { if(out)fprintf(out," %d",(int)(v));else add((int)(v)); } while(0)
 for(int i=0;i<640;i++)VALUE(n->xq[i]);
 for(int i=0;i<640;i++)VALUE(n->sLTP_shp_Q14[i]);
 for(int i=0;i<96;i++)VALUE(n->sLPC_Q14[i]);
 for(int i=0;i<24;i++)VALUE(n->sAR2_Q14[i]);
 VALUE(n->sLF_AR_shp_Q14);VALUE(n->sDiff_shp_Q14);VALUE(n->lagPrev);
 VALUE(n->sLTP_buf_idx);VALUE(n->sLTP_shp_buf_idx);VALUE(n->rand_seed);
 VALUE(n->prev_gain_Q16);VALUE(n->rewhite_flag);
 #undef VALUE
}
void __wrap_silk_NSQ_c(ARGS) {
 record.valid=1;record.rate=s->fs_kHz;record.subframes=s->nb_subfr;record.order=s->shapingLPCOrder;
 record.signal=ix->signalType;record.offset=ix->quantOffsetType;record.seed=ix->Seed;
 record.interpolation=ix->NLSFInterpCoef_Q2;record.lambda=lambda;record.scale=scale;
 record.states=s->nStatesDelayedDecision;record.warping=s->warping_Q16;record.before=*n;
 memset(record.coefficients,0,sizeof(record.coefficients));
 for(int h=0;h<2;h++)if(h==1||ix->NLSFInterpCoef_Q2!=4)
  for(int j=0;j<s->predictLPCOrder;j++)record.coefficients[h*16+j]=a[h*16+j];
 for(int k=0;k<s->nb_subfr;k++) {
  if(ix->signalType==2)for(int j=0;j<5;j++)record.coefficients[32+k*5+j]=b[k*5+j];
  for(int j=0;j<s->shapingLPCOrder;j++)record.coefficients[52+k*24+j]=ar[k*24+j];
  record.coefficients[148+k]=harm[k];record.coefficients[152+k]=tilt[k];
  record.coefficients[156+k]=lf[k];record.coefficients[160+k]=gain[k];record.coefficients[164+k]=pitch[k];
 }
 for(int j=0;j<s->frame_length;j++)record.input[j]=x[j];
 __real_silk_NSQ_c(s,n,ix,x,pulse,a,b,ar,harm,tilt,lf,gain,pitch,lambda,scale);
 for(int j=0;j<s->frame_length;j++){record.pulses[j]=pulse[j];record.pcm[j]=n->xq[s->ltp_mem_length+j];}
 record.after_seed=ix->Seed;hash=UINT64_C(14695981039346656037);state(n,NULL);record.after=hash;
}
static void emit(FILE *out,int first) {
 int length=record.rate*record.subframes*5;
 fprintf(out,"%d %d %d %d %d %d %d %d %d %d %d %d %d %llu",first,record.rate,record.subframes,record.order,record.signal,record.offset,record.seed,record.interpolation,record.lambda,record.scale,record.states,record.warping,record.after_seed,(unsigned long long)record.after);
 for(int i=0;i<168;i++)fprintf(out," %d",record.coefficients[i]);
 state(&record.before,out);
 for(int i=0;i<length;i++)fprintf(out," %d",record.input[i]);
 for(int i=0;i<length;i++)fprintf(out," %d",record.pulses[i]);
 for(int i=0;i<length;i++)fprintf(out," %d",record.pcm[i]);
 fprintf(out,"\n");
}
int main(int argc,char **argv) {
 if(argc!=2)return 1;
 FILE *out=fopen(argv[1],"w");if(!out)return 2;
 fprintf(out,"# Actual public restricted-SILK complexity0 scalar NSQ calls; original pinned v1.6.1\n");
 fprintf(out,"# RESET RATE SUBFRAMES ORDER SIGNAL OFFSET SEED INTERP LAMBDA SCALE STATES WARP AFTER_SEED AFTER_FNV64 COEFS[168] BEFORE[1408] INPUT[n] PULSES[n] PCM[n]\n");
 unsigned random=0x4e535130;
 for(int bandwidth=1101;bandwidth<=1103;bandwidth++)for(int ms=10;ms<=20;ms+=10) {
  int error;OpusEncoder *encoder=opus_encoder_create(48000,1,OPUS_APPLICATION_RESTRICTED_SILK,&error);
  if(!encoder||error)return 3;
  error=opus_encoder_ctl(encoder,OPUS_SET_COMPLEXITY(0));
  error|=opus_encoder_ctl(encoder,OPUS_SET_VBR(0));
  error|=opus_encoder_ctl(encoder,OPUS_SET_BANDWIDTH(bandwidth));
  error|=opus_encoder_ctl(encoder,OPUS_SET_MAX_BANDWIDTH(bandwidth));
  if(error)return 4;
  for(int frame=0;frame<12;frame++) {
   opus_int16 input[960];unsigned char packet[1276];
   int bitrate=frame<6?32000:frame<10?40000:6000+(bandwidth-1101)*4000;
   if(opus_encoder_ctl(encoder,OPUS_SET_BITRATE(bitrate)))return 5;
   for(int j=0;j<ms*48;j++) {
    int t=frame*ms*48+j;int phase=t%240;
    random=1664525*random+1013904223;
    int v=frame<2?0:frame>=10?(int)((random>>16)&32767)-16384:
      ((phase<120?phase:240-phase)-60)*(frame<6?120:240)+(int)((random>>16)&511)-256;
    input[j]=(opus_int16)v;
   }
   record.valid=0;
   if(opus_encode(encoder,input,ms*48,packet,sizeof(packet))<1||!record.valid)return 6;
   emit(out,frame==0);
  }
  opus_encoder_destroy(encoder);
 }
 if(fclose(out))return 7;
 return 0;
}
'''


# Shared deterministic public encoder workload for other analysis oracles.
DRIVER = WRAPPER[WRAPPER.index("int main("):]

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source', type=Path)
    parser.add_argument('output', type=Path)
    parser.add_argument('--delayed', action='store_true')
    args = parser.parse_args()
    source = args.source.resolve()
    if subprocess.check_output(['git','-C',str(source),'rev-parse','HEAD'],text=True).strip() != REVISION:
        raise SystemExit('Expected pinned Opus v1.6.1')
    if subprocess.check_output(['git','-C',str(source),'status','--porcelain'],text=True).strip():
        raise SystemExit('Reference checkout must be unchanged')
    def manifest(file, variable):
        match = re.search(r'^'+variable+r' = (.*?)(?=\n\n|\Z)',(source/file).read_text(),re.M|re.S)
        if match is None:
            raise SystemExit('Missing source manifest')
        return match[1].replace('\\','').split()
    files = (manifest('silk_sources.mk','SILK_SOURCES') + manifest('silk_sources.mk','SILK_SOURCES_FIXED') +
             manifest('celt_sources.mk','CELT_SOURCES') + manifest('opus_sources.mk','OPUS_SOURCES'))
    with tempfile.TemporaryDirectory(prefix='opura-native-nsq-') as temp:
        folder = Path(temp)
        wrapper = WRAPPER.replace("original pinned v1.6.1", "original pinned v1.6.1 "+REVISION)
        name = 'silk_NSQ_c'
        if args.delayed:
            name = 'silk_NSQ_del_dec_c'
            wrapper = wrapper.replace('silk_NSQ_c', name).replace('OPUS_SET_COMPLEXITY(0)', 'OPUS_SET_COMPLEXITY(bandwidth==1101?2:bandwidth==1102?6:10)')
            wrapper = wrapper.replace('complexity0 scalar', 'complexity2/6/10 delayed-decision')
        (folder/'probe.c').write_text(wrapper)
        subprocess.run(['cc','-O2','-fwrapv','-DVAR_ARRAYS','-DOPUS_BUILD','-DFIXED_POINT','-DDISABLE_FLOAT_API',
                        *['-I'+str(source/p) for p in ['include','celt','silk','silk/fixed','src']],
                        str(folder/'probe.c'),*[str(source/f) for f in files],'-Wl,--wrap='+name,'-lm','-o',str(folder/'probe')],check=True)
        subprocess.run([str(folder/'probe'),str(args.output.resolve())],check=True)
    print('Captured 72 final '+('delayed-decision' if args.delayed else 'scalar')+' NSQ calls from unchanged public SILK encoder')


if __name__ == '__main__':
    main()
