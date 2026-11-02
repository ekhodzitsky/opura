#!/usr/bin/env python3
"""Optional unchanged native Hybrid Q15 gain and high-band/stereo fade oracle."""
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
    window = function((source/'celt/static_modes_fixed.h').read_text(), 'static const celt_coef window120[120]')+';'
    functions = '\n'.join(function(opus, 'static void '+name+'(') for name in ['gain_fade', 'stereo_fade'])
    gain = opus[opus.index('               HB_gain = Q15ONE -'):opus.index('\n', opus.index('               HB_gain = Q15ONE -'))]
    program = '#include "arch.h"\n#include "mathops.h"\n#include <stdint.h>\n#include <stdio.h>\n'+window+'\n'+functions+r'''
int main(int argc,char **argv){
 if(argc!=2)return 1;FILE *out=fopen(argv[1],"w");if(!out)return 2;
 fprintf(out,"# Original Hybrid Q15 gain and fades REVISION\n");
 int rates[]={-1000,-400,0,1,500,1000,2048,4096,8192,16383,16384,32767,32768,50000,1020000};
 for(int i=0;i<15;i++) { int celt_rate=rates[i];opus_val16 HB_gain;
GAIN
 fprintf(out,"gain %d %d\n",celt_rate,HB_gain); }
 int values[]={-32768,-1,0,1,8192,16384,32766,32767};
 uint32_t seed=0x46414445;
 for(int kind=0;kind<2;kind++)for(int rate=24000;rate<=48000;rate*=2)for(int ch=1;ch<=2;ch++)for(int ms=10;ms<=20;ms+=10)
 for(int g1=0;g1<8;g1++)for(int g2=0;g2<8;g2++) {
  if(kind && (ch!=2 || g1<2 || g2<2))continue;
  int n=rate*ms/1000,old=values[g1],next=values[g2];uint32_t before=seed;opus_res pcm[1920];
  for(int i=0;i<n*ch;i++){seed=1664525*seed+1013904223;pcm[i]=(opus_int16)(seed>>16);}
  if(old<Q15ONE||next<Q15ONE) {
   if(kind)stereo_fade(pcm,pcm,old,next,120,n,ch,window120,rate);
   else gain_fade(pcm,pcm,old,next,120,n,ch,window120,rate);
  }
  uint64_t hash=UINT64_C(14695981039346656037);
  for(int i=0;i<n*ch;i++){uint16_t v=(uint16_t)pcm[i];hash=(hash^(v&255))*UINT64_C(1099511628211);hash=(hash^(v>>8))*UINT64_C(1099511628211);}
  fprintf(out,"fade %d %d %d %d %d %d %u %llu\n",kind,rate,ch,ms,old,next,before,(unsigned long long)hash);
 }
 if(fclose(out))return 3;return 0;
}
'''
    program = program.replace('GAIN', gain).replace('REVISION', REVISION)
    with tempfile.TemporaryDirectory(prefix='opura-hybrid-fades-') as temp:
        folder = Path(temp)
        (folder/'probe.c').write_text(program)
        subprocess.run(['cc', '-O2', '-fwrapv', '-DVAR_ARRAYS', '-DOPUS_BUILD', '-DFIXED_POINT', '-DDISABLE_FLOAT_API',
                        *['-I'+str(source/p) for p in ['include', 'celt', 'silk', 'silk/fixed', 'src']],
                        str(folder/'probe.c'), '-o', str(folder/'probe')], check=True)
        subprocess.run([str(folder/'probe'), str(args.output.resolve())], check=True)
    print('Captured 15 native gains and 656 native in-place fade fingerprints')


if __name__ == '__main__':
    main()
