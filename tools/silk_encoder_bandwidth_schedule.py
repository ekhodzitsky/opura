#!/usr/bin/env python3
"""Optional native packet bandwidth/LP/activity scheduling oracle (no Cargo dependency)."""
import argparse
from pathlib import Path
import subprocess
import tempfile
from silk_nsq_fixtures import REVISION

WRAPPER = r'''
#include "main.h"
#include "API.h"
#include "tuning_parameters.h"
#include <stdio.h>
#include <stdint.h>
static uint64_t add(uint64_t h,opus_int32 v){unsigned u=(unsigned)v;for(int j=0;j<4;j++){h=(h^(u&255))*UINT64_C(1099511628211);u>>=8;}return h;}
int main(int argc,char **argv){
 if(argc!=2)return 1;FILE *out=fopen(argv[1],"w");if(!out)return 2;
 fprintf(out,"# Native packet bandwidth/LP/activity scheduling REVISION\n");
 int apis[]={8000,12000,16000,24000,48000},rates[]={8,12,16},durations[]={10,20,40,60},bits[]={16,127,512,4096,10200};
 int id=0;
 for(int api=0;api<5;api++)for(int initial=0;initial<3;initial++)for(int channels=1;channels<=2;channels++)for(int profile=0;profile<2;profile++,id++){
  silk_encoder_state s[2]={{0},{0}};int current=rates[initial],allowed=0,elapsed=0,can=0,changes=0,readies=0,omitted=0;uint64_t hash=UINT64_C(14695981039346656037);
  for(int packet=0;packet<900;packet++){
   int targets[]={rates[initial],8,16,12,8,16};int desired=targets[packet/150];if(packet>=225&&packet<235)desired=16;
   if(desired*1000>apis[api])desired=apis[api]/1000;
   silk_EncControlStruct c={0};c.payloadSize_ms=profile?durations[packet%4]:60;c.maxBits=bits[packet%5];c.opusCanSwitch=can;
   int selected=current;
   for(int channel=0;channel<channels;channel++){
    s[channel].fs_kHz=packet?current:0;s[channel].API_fs_Hz=apis[api];s[channel].minInternal_fs_Hz=8000;s[channel].maxInternal_fs_Hz=16000;s[channel].desiredInternal_fs_Hz=desired*1000;s[channel].allow_bandwidth_switch=allowed;
    int rate=silk_control_audio_bandwidth(&s[channel],&c);if(channel==0)selected=rate;
   }
   if(selected!=current){changes++;for(int channel=0;channel<channels;channel++)s[channel].sLP.In_LP_State[0]=s[channel].sLP.In_LP_State[1]=0;}
   current=selected;readies+=c.switchReady!=0;
   hash=add(hash,current);hash=add(hash,c.switchReady);hash=add(hash,c.maxBits);
   int ms=c.payloadSize_ms==10?10:20;
   for(int frame=0;frame<c.payloadSize_ms/ms;frame++)for(int channel=0;channel<channels;channel++){
    if(channel==1&&packet%73<35){omitted++;continue;}
    unsigned random=0x73636864u^(unsigned)(id*100000+packet*8+frame*2+channel);opus_int16 pcm[320];
    for(int i=0;i<ms*current;i++){random=1664525u*random+1013904223u;pcm[i]=(int)(random>>16)-32768;}
    silk_LP_variable_cutoff(&s[channel].sLP,pcm,ms*current);
    for(int i=0;i<ms*current;i++)hash=add(hash,pcm[i]);
   }
   int activity=packet%91<5?0:255-packet%17;
   opus_int threshold=silk_SMLAWB(SILK_FIX_CONST(SPEECH_ACTIVITY_DTX_THRES,8),SILK_FIX_CONST((1-SPEECH_ACTIVITY_DTX_THRES)/MAX_BANDWIDTH_SWITCH_DELAY_MS,16+8),elapsed);
   if(activity<threshold){allowed=1;elapsed=0;}else{allowed=0;elapsed+=c.payloadSize_ms;}
   can=c.switchReady;
   hash=add(hash,allowed);hash=add(hash,elapsed);hash=add(hash,can);
   for(int channel=0;channel<channels;channel++){
    hash=add(hash,s[channel].sLP.transition_frame_no);hash=add(hash,s[channel].sLP.mode);hash=add(hash,s[channel].sLP.In_LP_State[0]);hash=add(hash,s[channel].sLP.In_LP_State[1]);
   }
  }
  fprintf(out,"schedule %d %d %d %d %llu %d %d %d\n",apis[api],rates[initial],channels,profile,(unsigned long long)hash,changes,readies,omitted);
 }
 if(fclose(out))return 3;return 0;
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
    files = ['silk/control_audio_bandwidth.c', 'silk/LP_variable_cutoff.c', 'silk/biquad_alt.c', 'silk/tables_other.c']
    with tempfile.TemporaryDirectory(prefix='opura-silk-bandwidth-schedule-') as temp:
        folder = Path(temp)
        (folder/'probe.c').write_text(WRAPPER.replace('REVISION', REVISION))
        subprocess.run(['cc', '-O2', '-fwrapv', '-DVAR_ARRAYS', '-DOPUS_BUILD', '-DFIXED_POINT', '-DDISABLE_FLOAT_API',
                        *['-I'+str(source/p) for p in ['include','celt','silk','silk/fixed','src']],
                        str(folder/'probe.c'), *[str(source/f) for f in files], '-lm', '-o', str(folder/'probe')], check=True)
        subprocess.run([str(folder/'probe'), str(args.output.resolve())], check=True)
    print('Captured60 native bandwidth/LP/activity schedules of900 packets each')


if __name__ == '__main__':
    main()
