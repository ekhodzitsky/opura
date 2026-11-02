#!/usr/bin/env python3
"""Optional unchanged scalar CELT VBR target, budget and feedback oracle."""
import argparse
from pathlib import Path
import subprocess
import tempfile
from silk_nsq_fixtures import REVISION
from silk_encoder_fec_policy import function


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source', type=Path)
    parser.add_argument('output', type=Path)
    parser.add_argument('--temporal-output', type=Path)
    args = parser.parse_args()
    source = args.source.resolve()
    if subprocess.check_output(['git', '-C', str(source), 'rev-parse', 'HEAD'], text=True).strip() != REVISION:
        raise ValueError('Expected pinned Opus v1.6.1')
    if subprocess.check_output(['git', '-C', str(source), 'status', '--porcelain'], text=True).strip():
        raise ValueError('Reference checkout must be unchanged')
    text = (source/'celt/celt_encoder.c').read_text()
    assert 'vbr_rate -= 8<<BITRES;' in text
    compute = function(text, 'static int compute_vbr(')
    initial = function(text[text.index('Computes the max bit-rate allowed'):], '      if (st->constrained_vbr)')
    minimum = text[text.index('   min_allowed = ((tell+total_boost'):text.index('   /* Variable bitrate */', text.index('   min_allowed = ((tell+total_boost'))]
    final = function(text[text.index('   /* Variable bitrate */'):], '   if (vbr_rate>0)')
    temporal = function(text[text.index('/* Temporal VBR (but not for LFE) */'):], '   if (!st->lfe)')
    program = r'''
#include "modes.h"
#include "analysis.h"
#include "mathops.h"
#include <stdio.h>
#if !OPUS_FAST_INT64
#error "Expected scalar fixed-point fast64 reference"
#endif
/* The oracle observes capacity only; no entropy bytes are being written. */
#define ec_enc_shrink(enc, capacity) ((void)(capacity))
COMPUTE
int main(int argc,char **argv){
 if(argc!=2&&argc!=3)return 1;FILE *out=fopen(argv[1],"w");if(!out)return 2;
 fprintf(out,"# Original CELT VBR/CVBR target, pre-cap and state REVISION\n");
 static const opus_int16 bands[]={0,1,2,3,4,5,6,7,8,10,12,14,16,20,24,28,34,40,48,60,78,100};
 CELTMode m={0};m.maxLM=3;m.nbEBands=21;m.eBands=bands;const CELTMode *mode=&m;
 for(int stream=0;stream<8;stream++){
  struct {int constrained_vbr,vbr_reservoir,vbr_drift,vbr_offset,vbr_count,lastCodedBands,intensity,lfe;opus_val16 stereo_saving;void *energy_mask;AnalysisInfo analysis;struct {int offset;}silk_info;} state={0},*st=&state;
  for(int frame=0;frame<1024;frame++){
   int LM=(frame+stream)%4,C=1+stream%2,hybrid=0,packet_size_cap=1275;
   int caps[]={2,3,8,16,32,64,128,256,637,1275};
   int rates[]={9601,9600,16000,24000,32001,48001,64001,128000,510000,750000};
   int capacity=caps[(frame*7+stream)%10],rate=rates[(frame/3+stream)%10];
   /* The public packet API owns a TOC: use original CELT signalling overhead. */
   int vbr_rate=((rate/(400>>LM))<<BITRES)-(8<<BITRES),nbCompressedBytes=capacity,nbFilledBytes=0,nbAvailableBytes=capacity,tell=1,tell0_frac=8,min_allowed;
   st->constrained_vbr=(frame/73+stream)%2;
INITIAL
   int precap=nbCompressedBytes;
   int last[]={0,1,8,13,19,21};st->lastCodedBands=last[(frame/7+stream)%6];st->intensity=(frame*5+stream)%22;
   int savings[]={0,25,26,27,128,256,1280};st->stereo_saving=savings[(frame/5+stream)%7];
   int estimates[]={0,720,721,3276,3277,8192,11469,16384};opus_val16 tf_estimate=estimates[(frame/11+stream)%8];
   int depths[]={-30,-1,0,1,4,10,20,32};celt_glog maxDepth=depths[(frame/13+stream)%8]*(1<<DB_SHIFT);
   celt_glog temporal_vbr=((frame/17+stream)%10-3)*(1<<(DB_SHIFT-1));
   int equiv_rate=IMIN(capacity*8*(400>>LM),rate)-(40*C+20)*((400>>LM)-50);
   int tot_boost=(frame*19)%1024,total_boost=(frame*31)%IMAX(1,precap*8);
   int pitch_change=frame%2; celt_glog surround_masking=0;
   int silence=(frame>=64&&frame<85)||(frame>=965&&frame<975);
   tell=silence?128:IMIN(precap*64-1,8+(frame*41)%(precap*64));
   int input_tell=tell;
   int base=vbr_rate-((40*C+20)<<BITRES);if(st->constrained_vbr)base+=st->vbr_offset>>(3-LM);
   int raw_target=compute_vbr(mode,&st->analysis,base,LM,equiv_rate,st->lastCodedBands,C,st->intensity,st->constrained_vbr,st->stereo_saving,tot_boost,tf_estimate,pitch_change,maxDepth,0,0,0,temporal_vbr);
MINIMUM
FINAL
   fprintf(out,"%d %d %d %d %d %d %d %d %d %d %d %d %d %d %d %d %d %d %d %d %d %d %d %d %d\n",stream,frame,LM,C,capacity,rate,st->constrained_vbr,precap,st->lastCodedBands,st->intensity,st->stereo_saving,tf_estimate,maxDepth,temporal_vbr,equiv_rate,tot_boost,total_boost,input_tell,silence,raw_target,nbCompressedBytes,st->vbr_reservoir,st->vbr_drift,st->vbr_offset,st->vbr_count);
  }
 }
 if(fclose(out))return 3;
 if(argc==3){
  FILE *out=fopen(argv[2],"w");if(!out)return 4;
  fprintf(out,"# Original CELT temporal spectral average REVISION\n");
  for(int stream=0;stream<4;stream++){
   struct {int lfe;celt_glog spec_avg;}state={0},*st=&state;
   for(int frame=0;frame<256;frame++){
    int C=1+stream%2,LM=frame%4,start=stream<2?0:17,end=stream%2?21:19,shortBlocks=LM&&frame%3==0;
    int nbEBands=21,i;celt_glog temporal_vbr=0,bandLogE[42];
    for(int j=0;j<42;j++)bandLogE[j]=((frame*719+j*317+stream*41)%(44*4096)-28*4096)*4096;
TEMPORAL
    fprintf(out,"%d %d %d %d %d %d %d",stream,frame,C,LM,start,end,shortBlocks);
    for(int j=0;j<42;j++)fprintf(out," %d",bandLogE[j]);
    fprintf(out," %d %d\n",temporal_vbr,st->spec_avg);
   }
  }
  if(fclose(out))return 5;
 }
 return 0;
}
'''
    for key, value in [('COMPUTE', compute), ('INITIAL', initial), ('MINIMUM', minimum), ('FINAL', final), ('TEMPORAL', temporal), ('REVISION', REVISION)]:
        program = program.replace(key, value)
    with tempfile.TemporaryDirectory(prefix='opura-celt-vbr-') as temp:
        folder = Path(temp)
        (folder/'probe.c').write_text(program)
        subprocess.run(['cc', '-O2', '-fwrapv', '-DVAR_ARRAYS', '-DOPUS_BUILD', '-DFIXED_POINT', '-DDISABLE_FLOAT_API',
                        *['-I'+str(source/p) for p in ['include', 'celt', 'silk', 'silk/fixed', 'src']],
                        str(folder/'probe.c'), str(source/'celt/mathops.c'), str(source/'celt/entcode.c'),
                        '-o', str(folder/'probe')], check=True)
        subprocess.run([str(folder/'probe'), str(args.output.resolve())] + ([str(args.temporal_output.resolve())] if args.temporal_output else []), check=True)
    print('Captured8192 unchanged CELT VBR/CVBR target and state decisions')


if __name__ == '__main__':
    main()
