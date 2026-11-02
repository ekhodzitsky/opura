#!/usr/bin/env python3
"""Optional unchanged native SILK bandwidth controller and LP filter oracle."""
import argparse
from pathlib import Path
import re
import subprocess
import tempfile
from silk_nsq_fixtures import REVISION

WRAPPER = r'''
#include "main.h"
#include "API.h"
#include <stdio.h>
#include <stdint.h>
static uint64_t fingerprint(opus_int16 *x,int n){
 uint64_t h=UINT64_C(14695981039346656037);for(int i=0;i<n;i++){unsigned v=(uint16_t)x[i];h=(h^(v&255))*UINT64_C(1099511628211);h=(h^(v>>8))*UINT64_C(1099511628211);}return h;
}
int main(int argc,char **argv){
 if(argc!=2)return 1;FILE *out=fopen(argv[1],"w");if(!out)return 2;
 fprintf(out,"# Unchanged native SILK bandwidth controller and LP PCM REVISION\n");
 int rates[]={0,8,12,16},apis[]={8000,12000,16000,24000,48000},directions[]={-2,0,1},positions[]={0,1,31,32,33,63,64,65,127,128,191,255,256};
 int durations[]={10,20,40,60},budgets[]={1,127,128,1023,6400,10200};int id=0;
 for(int current=0;current<4;current++)for(int api=0;api<5;api++)for(int min=1;min<4;min++)for(int max=min;max<4;max++)for(int desired=min;desired<=max;desired++)for(int dir=0;dir<3;dir++)for(int allow=0;allow<=1;allow++)for(int can=0;can<=1;can++){
  silk_encoder_state s={0};silk_EncControlStruct c={0};int pos=positions[id%13],saved=rates[(id/13)%4];
  s.fs_kHz=rates[current];s.API_fs_Hz=apis[api];s.minInternal_fs_Hz=rates[min]*1000;s.maxInternal_fs_Hz=rates[max]*1000;s.desiredInternal_fs_Hz=rates[desired]*1000;s.allow_bandwidth_switch=allow;
  s.sLP.transition_frame_no=pos;s.sLP.mode=directions[dir];s.sLP.saved_fs_kHz=saved;s.sLP.In_LP_State[0]=-120000-id*100;s.sLP.In_LP_State[1]=240000+id*200;
  c.opusCanSwitch=can;c.payloadSize_ms=durations[id%4];c.maxBits=budgets[id%6];
  fprintf(out,"control %d %d %d %d %d %d %d %d %d %d %d %d %d %d",rates[current],apis[api],rates[min],rates[max],rates[desired],allow,can,c.payloadSize_ms,c.maxBits,pos,directions[dir],saved,s.sLP.In_LP_State[0],s.sLP.In_LP_State[1]);
  int result=silk_control_audio_bandwidth(&s,&c);
  fprintf(out," %d %d %d %d %d %d %d %d\n",result,c.switchReady,c.maxBits,s.sLP.transition_frame_no,s.sLP.mode,s.sLP.saved_fs_kHz,s.sLP.In_LP_State[0],s.sLP.In_LP_State[1]);id++;
 }
 for(int rate=1;rate<4;rate++)for(int ms=10;ms<=20;ms+=10)for(int dir=0;dir<3;dir++)for(int pos=0;pos<=256;pos++){
  silk_LP_state s={0};s.transition_frame_no=pos;s.mode=directions[dir];s.saved_fs_kHz=rates[rate];
  s.In_LP_State[0]=(opus_int32)(0x7fffffffu-(unsigned)id*91719u);s.In_LP_State[1]=(opus_int32)(0x80000000u+(unsigned)id*7919u);
  unsigned random=0x42574c50u^(unsigned)id;int kind=id%6;opus_int16 pcm[320];int n=ms*rates[rate];
  fprintf(out,"filter %d %d %d %d %d %d %d %u %d",rates[rate],ms,pos,directions[dir],s.saved_fs_kHz,s.In_LP_State[0],s.In_LP_State[1],random,kind);
  for(int i=0;i<n;i++){random=1664525*random+1013904223;pcm[i]=kind==0?0:kind==1?-32768:kind==2?32767:kind==3?(i%2?-32768:32767):kind==4?(i%64)*1024-32768:(int)(random>>16)-32768;}
  silk_LP_variable_cutoff(&s,pcm,n);
  fprintf(out," %d %d %d %d %d %llu\n",s.transition_frame_no,s.mode,s.saved_fs_kHz,s.In_LP_State[0],s.In_LP_State[1],(unsigned long long)fingerprint(pcm,n));id++;
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
    with tempfile.TemporaryDirectory(prefix='opura-silk-bandwidth-') as temp:
        folder = Path(temp)
        (folder/'probe.c').write_text(WRAPPER.replace('REVISION', REVISION))
        subprocess.run(['cc', '-O2', '-fwrapv', '-DVAR_ARRAYS', '-DOPUS_BUILD', '-DFIXED_POINT', '-DDISABLE_FLOAT_API',
                        *['-I'+str(source/p) for p in ['include','celt','silk','silk/fixed','src']],
                        str(folder/'probe.c'), *[str(source/f) for f in files], '-lm', '-o', str(folder/'probe')], check=True)
        subprocess.run([str(folder/'probe'), str(args.output.resolve())], check=True)
    print('Captured2400 native bandwidth decisions and4626 complete LP PCM/state fingerprints')


if __name__ == '__main__':
    main()
