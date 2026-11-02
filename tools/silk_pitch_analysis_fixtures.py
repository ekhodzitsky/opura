#!/usr/bin/env python3
"""Capture complete pitch/LTP analysis around unchanged public SILK encoding.

Optional pinned native oracle; never a build or ordinary-test dependency.
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
static struct {
 int valid,rate,count,length,complexity,activity,tilt,first,signal,previous_signal,previous_lag,previous_correlation;
 int after_signal,after_correlation,gain,index,contour,lags[4];
 opus_int16 input[672],residual[672];
 struct {int valid,before,after,gain,periodicity;opus_int32 xx[100],xt[20],inverse[4];opus_int8 indices[4];opus_int16 coefs[20],input[672],normalized[384];}ltp;
}record;
void __real_silk_find_pitch_lags_FIX(silk_encoder_state_FIX*,silk_encoder_control_FIX*,opus_int16*,const opus_int16*,int);
void __wrap_silk_find_pitch_lags_FIX(silk_encoder_state_FIX *s,silk_encoder_control_FIX *c,opus_int16 *res,const opus_int16 *x,int arch){
 record.ltp.valid=0;record.valid=1;record.rate=s->sCmn.fs_kHz;record.count=s->sCmn.nb_subfr;
 record.length=s->sCmn.ltp_mem_length+s->sCmn.frame_length+s->sCmn.la_pitch;
 record.complexity=s->sCmn.Complexity;record.activity=s->sCmn.speech_activity_Q8;record.tilt=s->sCmn.input_tilt_Q15;
 record.first=s->sCmn.first_frame_after_reset;record.signal=s->sCmn.indices.signalType;
 record.previous_signal=s->sCmn.prevSignalType;record.previous_lag=s->sCmn.prevLag;record.previous_correlation=s->LTPCorr_Q15;
 memcpy(record.input,x,record.length*sizeof(*x));
 __real_silk_find_pitch_lags_FIX(s,c,res,x,arch);
 memcpy(record.residual,res,record.length*sizeof(*res));record.gain=c->predGain_Q16;
 record.after_signal=s->sCmn.indices.signalType;record.after_correlation=s->LTPCorr_Q15;
 record.index=s->sCmn.indices.lagIndex;record.contour=s->sCmn.indices.contourIndex;
 for(int i=0;i<record.count;i++)record.lags[i]=c->pitchL[i];
}

void __real_silk_quant_LTP_gains(opus_int16*,opus_int8*,opus_int8*,opus_int32*,opus_int*,const opus_int32*,const opus_int32*,opus_int,opus_int,int);
void __wrap_silk_quant_LTP_gains(opus_int16 *b,opus_int8 *indices,opus_int8 *periodicity,opus_int32 *sum,opus_int *gain,const opus_int32 *xx,const opus_int32 *xt,opus_int sf,opus_int count,int arch){
 record.ltp.valid=1;record.ltp.before=*sum;
 memcpy(record.ltp.xx,xx,25*count*sizeof(*xx));memcpy(record.ltp.xt,xt,5*count*sizeof(*xt));
 __real_silk_quant_LTP_gains(b,indices,periodicity,sum,gain,xx,xt,sf,count,arch);
 record.ltp.after=*sum;record.ltp.gain=*gain;record.ltp.periodicity=*periodicity;
 memcpy(record.ltp.indices,indices,count);memcpy(record.ltp.coefs,b,5*count*sizeof(*b));
}
void __real_silk_LTP_analysis_filter_FIX(opus_int16*,const opus_int16*,const opus_int16*,const opus_int*,const opus_int32*,opus_int,opus_int,opus_int);
void __wrap_silk_LTP_analysis_filter_FIX(opus_int16 *out,const opus_int16 *x,const opus_int16 *b,const opus_int *lags,const opus_int32 *gains,opus_int sf,opus_int count,opus_int prefix){
 memcpy(record.ltp.input,x-(20*record.rate-prefix),record.length*sizeof(*x));
 memcpy(record.ltp.inverse,gains,count*sizeof(*gains));
 __real_silk_LTP_analysis_filter_FIX(out,x,b,lags,gains,sf,count,prefix);
 memcpy(record.ltp.normalized,out,count*(sf+prefix)*sizeof(*out));
}
static void emit(FILE *out,int first){
 (void)first;
 fprintf(out,"pitch %d %d %d %d %d %d %d %d %d %d %d %d %d %d %d %d",record.rate,record.count,record.complexity,record.activity,record.tilt,record.first,record.signal,record.previous_signal,record.previous_lag,record.previous_correlation,record.after_signal,record.after_correlation,record.gain,record.index,record.contour,record.length);
 for(int i=0;i<record.count;i++)fprintf(out," %d",record.lags[i]);
 for(int i=0;i<record.length;i++)fprintf(out," %d",record.input[i]);
 for(int i=0;i<record.length;i++)fprintf(out," %d",record.residual[i]);fprintf(out,"\n");
 if(record.ltp.valid){
  fprintf(out,"ltp %d %d %d %d %d",record.ltp.before,record.ltp.after,record.ltp.gain,record.ltp.periodicity,record.count);
  for(int i=0;i<record.count;i++)fprintf(out," %d",record.ltp.indices[i]);
  for(int i=0;i<record.count*5;i++)fprintf(out," %d",record.ltp.coefs[i]);
  for(int i=0;i<record.count*25;i++)fprintf(out," %d",record.ltp.xx[i]);
  for(int i=0;i<record.count*5;i++)fprintf(out," %d",record.ltp.xt[i]);
  for(int i=0;i<record.count;i++)fprintf(out," %d",record.ltp.inverse[i]);
  for(int i=0;i<record.length;i++)fprintf(out," %d",record.ltp.input[i]);
  int order=record.rate==16?16:10;
  for(int i=0;i<record.count*(5*record.rate+order);i++)fprintf(out," %d",record.ltp.normalized[i]);fprintf(out,"\n");
 }
}
'''


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source', type=Path)
    parser.add_argument('output', type=Path)
    args = parser.parse_args()
    source = args.source.resolve()
    if subprocess.check_output(['git', '-C', str(source), 'rev-parse', 'HEAD'], text=True).strip() != REVISION:
        raise SystemExit('Expected pinned Opus v1.6.1')
    if subprocess.check_output(['git', '-C', str(source), 'status', '--porcelain'], text=True).strip():
        raise SystemExit('Reference checkout must be unchanged')
    def manifest(file, variable):
        match = re.search(r'^'+variable+r' = (.*?)(?=\n\n|\Z)', (source/file).read_text(), re.M|re.S)
        if match is None:
            raise SystemExit('Missing source manifest')
        return match[1].replace('\\', '').split()
    files = (manifest('silk_sources.mk', 'SILK_SOURCES') + manifest('silk_sources.mk', 'SILK_SOURCES_FIXED') +
             manifest('celt_sources.mk', 'CELT_SOURCES') + manifest('opus_sources.mk', 'OPUS_SOURCES'))
    driver = '\n'.join(line for line in DRIVER.splitlines() if not line.lstrip().startswith('fprintf(out,"#'))
    driver = driver.replace('if(!out)return 2;', 'if(!out)return 2;\nfprintf(out,"# Public native SILK pitch '+REVISION+'\\n");')
    driver = driver.replace('OPUS_SET_COMPLEXITY(0)', 'OPUS_SET_COMPLEXITY(bandwidth==1101?0:bandwidth==1102?6:10)')
    with tempfile.TemporaryDirectory(prefix='opura-silk-pitch-analysis-') as temp:
        folder = Path(temp)
        (folder/'probe.c').write_text(WRAPPER+'\n'+driver)
        subprocess.run(['cc', '-O2', '-fwrapv', '-DVAR_ARRAYS', '-DOPUS_BUILD', '-DFIXED_POINT', '-DDISABLE_FLOAT_API',
                        *['-I'+str(source/p) for p in ['include','celt','silk','silk/fixed','src']],
                        str(folder/'probe.c'), *[str(source/f) for f in files],
                        '-Wl,--wrap=silk_find_pitch_lags_FIX', '-Wl,--wrap=silk_quant_LTP_gains',
                        '-Wl,--wrap=silk_LTP_analysis_filter_FIX', '-lm', '-o', str(folder/'probe')], check=True)
        subprocess.run([str(folder/'probe'), str(args.output.resolve())], check=True)
    print('Captured 72 native pitch calls and their voiced LTP fits/residuals')


if __name__ == '__main__':
    main()
