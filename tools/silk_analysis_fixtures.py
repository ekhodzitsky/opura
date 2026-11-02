#!/usr/bin/env python3
"""Record unchanged public SILK gain, spectral VQ, Burg and root-analysis calls.

Only this optional generator uses an external C compiler/libopus checkout.
"""
import argparse
from pathlib import Path
import re
import subprocess
import tempfile
from silk_nsq_fixtures import DRIVER, REVISION

WRAPPER = r'''
#include "main_FIX.h"
#include "opus.h"
#include <stdio.h>
#include <stdint.h>
static struct {
 int valid;
 struct {int rate,count,length,activity,tilt,quality[4];silk_VAD_state before,after;opus_int16 input[320];}v;
 struct {int rate,count,use,first,signal,activity,survivors,minimum,interpolation;opus_int16 input[384],previous[16],output[16],predictor[32];opus_int8 indices[17];}s;
 struct {int count,conditional,before,after;opus_int32 input[4],output[4];opus_int8 indices[4];}g;
 struct {int order,signal,mu,survivors,cost;opus_int16 input[16],weights[16],output[16];opus_int8 indices[17];}n;
 struct {int order,length,count,minimum,energy,q;opus_int16 input[384];opus_int32 output[16];}b;
 struct {int order;opus_int32 input[16],output[16];opus_int16 frequencies[16];}r;
}record;

static void vad_state(FILE *out,const silk_VAD_state *s){
 for(int i=0;i<2;i++)fprintf(out," %d",s->AnaState[i]);
 for(int i=0;i<2;i++)fprintf(out," %d",s->AnaState1[i]);
 for(int i=0;i<2;i++)fprintf(out," %d",s->AnaState2[i]);
 for(int i=0;i<4;i++)fprintf(out," %d",s->XnrgSubfr[i]);
 for(int i=0;i<4;i++)fprintf(out," %d",s->NrgRatioSmth_Q8[i]);fprintf(out," %d",s->HPstate);
 for(int i=0;i<4;i++)fprintf(out," %d",s->NL[i]);
 for(int i=0;i<4;i++)fprintf(out," %d",s->inv_NL[i]);
 for(int i=0;i<4;i++)fprintf(out," %d",s->NoiseLevelBias[i]);fprintf(out," %d",s->counter);
}
int __real_silk_VAD_GetSA_Q8_c(silk_encoder_state*,const opus_int16*);
int __wrap_silk_VAD_GetSA_Q8_c(silk_encoder_state *s,const opus_int16 *input){
 record.v.rate=s->fs_kHz;record.v.length=s->frame_length;record.v.count=s->nb_subfr;record.v.before=s->sVAD;
 memcpy(record.v.input,input,s->frame_length*sizeof(*input));
 int result=__real_silk_VAD_GetSA_Q8_c(s,input);
 record.v.after=s->sVAD;record.v.activity=s->speech_activity_Q8;record.v.tilt=s->input_tilt_Q15;
 for(int i=0;i<4;i++)record.v.quality[i]=s->input_quality_bands_Q15[i];return result;
}
void __real_silk_gains_quant(opus_int8*,opus_int32*,opus_int8*,opus_int,opus_int);
void __wrap_silk_gains_quant(opus_int8 *indices,opus_int32 *gains,opus_int8 *previous,opus_int conditional,opus_int count){
 record.valid=1;record.g.count=count;record.g.conditional=conditional;record.g.before=*previous;
 memcpy(record.g.input,gains,count*sizeof(*gains));
 __real_silk_gains_quant(indices,gains,previous,conditional,count);
 record.g.after=*previous;memcpy(record.g.output,gains,count*sizeof(*gains));memcpy(record.g.indices,indices,count);
}
opus_int32 __real_silk_NLSF_encode(opus_int8*,opus_int16*,const silk_NLSF_CB_struct*,const opus_int16*,opus_int,opus_int,opus_int);
opus_int32 __wrap_silk_NLSF_encode(opus_int8 *indices,opus_int16 *spectrum,const silk_NLSF_CB_struct *cb,const opus_int16 *weights,opus_int mu,opus_int survivors,opus_int signal){
 record.valid=1;record.n.order=cb->order;record.n.signal=signal;record.n.mu=mu;record.n.survivors=survivors;
 memcpy(record.n.input,spectrum,cb->order*sizeof(*spectrum));memcpy(record.n.weights,weights,cb->order*sizeof(*weights));
 opus_int32 cost=__real_silk_NLSF_encode(indices,spectrum,cb,weights,mu,survivors,signal);
 record.n.cost=cost;memcpy(record.n.output,spectrum,cb->order*sizeof(*spectrum));memcpy(record.n.indices,indices,cb->order+1);return cost;
}
void __real_silk_burg_modified_c(opus_int32*,opus_int*,opus_int32*,const opus_int16*,opus_int32,opus_int,opus_int,opus_int,int);
void __wrap_silk_burg_modified_c(opus_int32 *energy,opus_int *q,opus_int32 *a,const opus_int16 *x,opus_int32 minimum,opus_int length,opus_int count,opus_int order,int arch){
 record.valid=1;record.b.order=order;record.b.length=length;record.b.count=count;record.b.minimum=minimum;
 memcpy(record.b.input,x,length*count*sizeof(*x));
 __real_silk_burg_modified_c(energy,q,a,x,minimum,length,count,order,arch);
 record.b.energy=*energy;record.b.q=*q;memcpy(record.b.output,a,order*sizeof(*a));
}
void __real_silk_A2NLSF(opus_int16*,opus_int32*,opus_int);
void __wrap_silk_A2NLSF(opus_int16 *frequencies,opus_int32 *a,opus_int order){
 record.valid=1;record.r.order=order;memcpy(record.r.input,a,order*sizeof(*a));
 __real_silk_A2NLSF(frequencies,a,order);memcpy(record.r.output,a,order*sizeof(*a));memcpy(record.r.frequencies,frequencies,order*sizeof(*frequencies));
}

void __real_silk_find_LPC_FIX(silk_encoder_state*,opus_int16*,const opus_int16*,opus_int32);
void __wrap_silk_find_LPC_FIX(silk_encoder_state *s,opus_int16 *spectrum,const opus_int16 *input,opus_int32 minimum){
 record.s.rate=s->fs_kHz;record.s.count=s->nb_subfr;record.s.use=s->useInterpolatedNLSFs;record.s.first=s->first_frame_after_reset;record.s.minimum=minimum;
 int n=(s->subfr_length+s->predictLPCOrder)*s->nb_subfr;
 memcpy(record.s.input,input,n*sizeof(*input));memcpy(record.s.previous,s->prev_NLSFq_Q15,s->predictLPCOrder*sizeof(*input));
 __real_silk_find_LPC_FIX(s,spectrum,input,minimum);
 record.s.interpolation=s->indices.NLSFInterpCoef_Q2;
}
void __real_silk_process_NLSFs(silk_encoder_state*,opus_int16 (*)[16],opus_int16*,const opus_int16*);
void __wrap_silk_process_NLSFs(silk_encoder_state *s,opus_int16 predictor[2][16],opus_int16 *spectrum,const opus_int16 *previous){
 record.s.signal=s->indices.signalType;record.s.activity=s->speech_activity_Q8;record.s.survivors=s->NLSF_MSVQ_Survivors;
 __real_silk_process_NLSFs(s,predictor,spectrum,previous);
 memcpy(record.s.output,spectrum,s->predictLPCOrder*sizeof(*spectrum));memcpy(record.s.indices,s->indices.NLSFIndices,s->predictLPCOrder+1);
 memset(record.s.predictor,0,sizeof(record.s.predictor));
 for(int h=0;h<2;h++)for(int j=0;j<s->predictLPCOrder;j++)record.s.predictor[h*16+j]=predictor[h][j];
}
static void emit(FILE *out,int first){
 fprintf(out,"vad %d %d %d %d",record.v.rate,record.v.count,record.v.activity,record.v.tilt);
 for(int i=0;i<4;i++)fprintf(out," %d",record.v.quality[i]);
 vad_state(out,&record.v.before);
 for(int i=0;i<record.v.length;i++)fprintf(out," %d",record.v.input[i]);
 vad_state(out,&record.v.after);fprintf(out,"\n");


 int order=record.s.rate==16?16:10;
 fprintf(out,"spectrum %d %d %d %d %d %d %d %d %d %d",record.s.rate,record.s.count,record.s.use,record.s.first,record.s.signal,record.s.activity,record.s.survivors,record.s.minimum,record.s.interpolation,record.n.cost);
 for(int i=0;i<(record.s.rate*5+order)*record.s.count;i++)fprintf(out," %d",record.s.input[i]);
 for(int i=0;i<order;i++)fprintf(out," %d",record.s.previous[i]);
 for(int i=0;i<order;i++)fprintf(out," %d",record.s.output[i]);
 for(int i=0;i<order+1;i++)fprintf(out," %d",record.s.indices[i]);
 for(int h=0;h<2;h++)for(int i=0;i<order;i++)fprintf(out," %d",record.s.predictor[h*16+i]);fprintf(out,"\n");
 fprintf(out,"gain %d %d %d %d %d",first,record.g.count,record.g.conditional,record.g.before,record.g.after);
 for(int i=0;i<record.g.count;i++)fprintf(out," %d",record.g.input[i]);
 for(int i=0;i<record.g.count;i++)fprintf(out," %d",record.g.indices[i]);
 for(int i=0;i<record.g.count;i++)fprintf(out," %d",record.g.output[i]);fprintf(out,"\n");
 fprintf(out,"nlsf %d %d %d %d %d",record.n.order,record.n.signal,record.n.mu,record.n.survivors,record.n.cost);
 for(int i=0;i<record.n.order;i++)fprintf(out," %d",record.n.input[i]);
 for(int i=0;i<record.n.order;i++)fprintf(out," %d",record.n.weights[i]);
 for(int i=0;i<record.n.order+1;i++)fprintf(out," %d",record.n.indices[i]);
 for(int i=0;i<record.n.order;i++)fprintf(out," %d",record.n.output[i]);
 opus_int16 weights[16];silk_NLSF_VQ_weights_laroia(weights,record.n.input,record.n.order);
 for(int i=0;i<record.n.order;i++)fprintf(out," %d",weights[i]);fprintf(out,"\n");
 fprintf(out,"burg %d %d %d %d %d %d",record.b.order,record.b.length,record.b.count,record.b.minimum,record.b.energy,record.b.q);
 for(int i=0;i<record.b.length*record.b.count;i++)fprintf(out," %d",record.b.input[i]);
 for(int i=0;i<record.b.order;i++)fprintf(out," %d",record.b.output[i]);fprintf(out,"\n");
 fprintf(out,"root %d",record.r.order);
 for(int i=0;i<record.r.order;i++)fprintf(out," %d",record.r.input[i]);
 for(int i=0;i<record.r.order;i++)fprintf(out," %d",record.r.output[i]);
 for(int i=0;i<record.r.order;i++)fprintf(out," %d",record.r.frequencies[i]);fprintf(out,"\n");
}

static void root_probes(FILE *out){
 for(int order=10;order<=16;order+=6)for(int kind=0;kind<4;kind++){
  opus_int32 a[16]={0};opus_int16 frequencies[16];
  a[0]=kind==0?0:kind==1?131072:kind==2?-131072:196608;
  __wrap_silk_A2NLSF(frequencies,a,order);
  fprintf(out,"root %d",order);
  for(int i=0;i<order;i++)fprintf(out," %d",record.r.input[i]);
  for(int i=0;i<order;i++)fprintf(out," %d",record.r.output[i]);
  for(int i=0;i<order;i++)fprintf(out," %d",record.r.frequencies[i]);fprintf(out,"\n");
 }
}
'''


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source', type=Path)
    parser.add_argument('output', type=Path)
    args = parser.parse_args()
    source = args.source.resolve()
    if subprocess.check_output(['git','-C',str(source),'rev-parse','HEAD'],text=True).strip() != REVISION:
        raise SystemExit('Expected pinned Opus v1.6.1')
    if subprocess.check_output(['git','-C',str(source),'status','--porcelain'],text=True).strip():
        raise SystemExit('Reference checkout must be unchanged')
    def manifest(file,variable):
        match = re.search(r'^'+variable+r' = (.*?)(?=\n\n|\Z)',(source/file).read_text(),re.M|re.S)
        if match is None:raise SystemExit('Missing source manifest')
        return match[1].replace('\\','').split()
    files = (manifest('silk_sources.mk','SILK_SOURCES') + manifest('silk_sources.mk','SILK_SOURCES_FIXED') +
             manifest('celt_sources.mk','CELT_SOURCES') + manifest('opus_sources.mk','OPUS_SOURCES'))
    # Reuse the exact NSQ oracle's public encode workload, with a different trace
    # header and hooks; no codec algorithm or native analysis is replaced.
    driver = '\n'.join(line for line in DRIVER.splitlines() if not line.lstrip().startswith('fprintf(out,"#'))
    driver = driver.replace('if(!out)return 2;', 'if(!out)return 2;\nfprintf(out,"# Original public SILK analysis '+REVISION+'\\n");')
    driver = driver.replace('OPUS_SET_COMPLEXITY(0)','OPUS_SET_COMPLEXITY(bandwidth==1101?0:bandwidth==1102?6:10)')
    driver = driver.replace('if(fclose(out))return 7;', 'root_probes(out);if(fclose(out))return 7;')
    with tempfile.TemporaryDirectory(prefix='opura-silk-analysis-') as temp:
        folder = Path(temp);(folder/'probe.c').write_text(WRAPPER+'\n'+driver)
        names=['silk_gains_quant','silk_NLSF_encode','silk_burg_modified_c','silk_A2NLSF','silk_find_LPC_FIX','silk_process_NLSFs','silk_VAD_GetSA_Q8_c']
        subprocess.run(['cc','-O2','-fwrapv','-DVAR_ARRAYS','-DOPUS_BUILD','-DFIXED_POINT','-DDISABLE_FLOAT_API',
                        *['-I'+str(source/p) for p in ['include','celt','silk','silk/fixed','src']],
                        str(folder/'probe.c'),*[str(source/f) for f in files],
                        *['-Wl,--wrap='+name for name in names],'-lm','-o',str(folder/'probe')],check=True)
        subprocess.run([str(folder/'probe'),str(args.output.resolve())],check=True)
    print('Captured 72 native analysis groups and 8 root-recovery probes')


if __name__ == '__main__':main()
