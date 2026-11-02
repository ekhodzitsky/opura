#!/usr/bin/env python3
"""Optional unchanged Opus Hybrid rate split and shared SILK cap oracle."""
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
    args = parser.parse_args()
    source = args.source.resolve()
    if subprocess.check_output(['git', '-C', str(source), 'rev-parse', 'HEAD'], text=True).strip() != REVISION:
        raise SystemExit('Expected pinned Opus v1.6.1')
    if subprocess.check_output(['git', '-C', str(source), 'status', '--porcelain'], text=True).strip():
        raise SystemExit('Reference checkout must be unchanged')
    opus = (source/'src/opus_encoder.c').read_text()
    split = function(opus, 'static int compute_silk_rate_for_hybrid(')
    begin = opus.index('        if (st->silk_mode.useCBR)')
    end = opus.index('        if (prefill &&', begin)
    caps = opus[begin:end]
    program = r'''
#include "main_FIX.h"
#include "API.h"
#include "opus_private.h"
#include <stdio.h>
typedef struct { int mode, Fs, use_vbr, stream_channels;
 struct { int useCBR, maxBits, bitRate, LBRR_coded; } silk_mode; } State;
'''+split+r'''
int main(int argc,char **argv) {
 if(argc!=2)return 1;FILE *out=fopen(argv[1],"w");if(!out)return 2;
 fprintf(out,"# Original Opus Hybrid rate allocation REVISION\n");
 int budgets[]={2,3,8,15,30,60,80,120,160,240,320,640,1275};
 int rates[]={1,7,100,799,800,6000,11999,12000,12001,15999,16000,16001,
  19999,20000,20001,23999,24000,24001,31999,32000,32001,63999,64000,64001,
  95999,96000,127999,128000,128001,255999,256000,510000,1020000};
 int count=0;
 for(int ms=10;ms<=20;ms+=10)for(int bw=1104;bw<=1105;bw++)for(int ch=1;ch<=2;ch++)
 for(int vbr=0;vbr<=1;vbr++)for(int fec=0;fec<=1;fec++)for(int b=0;b<13;b++) {
  int maximum=budgets[b]*8,capacity_rate=maximum*1000/ms;
  for(int r=0;r<35;r++) {
   int rate=r==33?capacity_rate:r==34?capacity_rate-1:rates[r];
   if(rate>capacity_rate)continue;
   State state={0},*st=&state;st->mode=MODE_HYBRID;st->Fs=48000;st->use_vbr=vbr;st->stream_channels=ch;
   st->silk_mode.useCBR=!vbr;st->silk_mode.LBRR_coded=fec;
   int frame_size=48*ms,curr_bandwidth=bw;
   st->silk_mode.bitRate=compute_silk_rate_for_hybrid(rate,bw,ms==20,vbr,fec,ch);
   st->silk_mode.maxBits=maximum;
CAPS
   if(st->silk_mode.useCBR)return 3;
   fprintf(out,"%d %d %d %d %d %d %d %d %d %d\n",ms,bw,ch,rate,budgets[b],vbr,fec,
     st->silk_mode.bitRate,rate-st->silk_mode.bitRate,st->silk_mode.maxBits);count++;
  }
 }
 if(fclose(out))return 4;printf("Captured %d original Hybrid rate/cap decisions\n",count);return 0;
}
'''
    program = program.replace('CAPS', caps).replace('REVISION', REVISION)
    with tempfile.TemporaryDirectory(prefix='opura-hybrid-rate-') as temp:
        folder = Path(temp)
        (folder/'probe.c').write_text(program)
        subprocess.run(['cc', '-O2', '-fwrapv', '-DVAR_ARRAYS', '-DOPUS_BUILD', '-DFIXED_POINT', '-DDISABLE_FLOAT_API',
                        *['-I'+str(source/p) for p in ['include', 'celt', 'silk', 'silk/fixed', 'src']],
                        str(folder/'probe.c'), '-o', str(folder/'probe')], check=True)
        subprocess.run([str(folder/'probe'), str(args.output.resolve())], check=True)


if __name__ == '__main__':
    main()
