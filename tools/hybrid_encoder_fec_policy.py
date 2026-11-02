#!/usr/bin/env python3
"""Optional original Hybrid FEC eligibility at an explicitly retained bandwidth."""
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
    parser.add_argument('--vbr', action='store_true')
    args = parser.parse_args()
    source = args.source.resolve()
    if subprocess.check_output(['git', '-C', str(source), 'rev-parse', 'HEAD'], text=True).strip() != REVISION:
        raise ValueError('Expected pinned Opus v1.6.1')
    if subprocess.check_output(['git', '-C', str(source), 'status', '--porcelain'], text=True).strip():
        raise ValueError('Reference checkout must be unchanged')
    opus = (source/'src/opus_encoder.c').read_text()
    originals = function(opus, 'static const opus_int32 fec_thresholds[]')+';\n'
    originals += function(opus, 'static int decide_fec(')+'\n'+function(opus, 'static opus_int32 compute_equiv_rate(')
    program = '#include "main_FIX.h"\n#include "opus_private.h"\n#include <stdio.h>\n'+originals+r'''
int main(int argc,char **argv){
 if(argc!=2)return 1;FILE *out=fopen(argv[1],"w");if(!out)return 2;
 fprintf(out,"# Original Hybrid equivalent-rate/FEC at retained fixed band REVISION\n");
 int budgets[]={3,8,12,16,20,24,28,30,32,36,40,48,60,80,120,160,240,320,640,1276};
 int losses[]={0,1,5,6,20,25,50,100},complexities[]={0,1,2,6,10};
 for(int bw=1104;bw<=1105;bw++)for(int ch=1;ch<=2;ch++)for(int ms=10;ms<=20;ms+=10)
 for(int previous=0;previous<=1;previous++)for(int enabled=0;enabled<=1;enabled++)for(int comp=0;comp<5;comp++)for(int loss=0;loss<8;loss++)for(int b=0;b<20;b++){
  int equiv=compute_equiv_rate(budgets[b]*8000/ms,ch,1000/ms,0,MODE_HYBRID,complexities[comp],losses[loss]);
  int band=bw,selected=decide_fec(enabled,losses[loss],previous,MODE_HYBRID,&band,equiv);
  fprintf(out,"%d %d %d %d %d %d %d %d %d\n",bw,ch,ms,previous,enabled,complexities[comp],losses[loss],budgets[b],selected && band==bw);
 }
 if(fclose(out))return 3;return 0;
}
'''
    if args.vbr:
        program = program.replace('1000/ms,0,MODE_HYBRID', '1000/ms,1,MODE_HYBRID')
        program = program.replace('retained fixed band REVISION', 'retained fixed band VBR REVISION')
    with tempfile.TemporaryDirectory(prefix='opura-hybrid-fec-policy-') as temp:
        folder = Path(temp)
        (folder/'probe.c').write_text(program.replace('REVISION', REVISION))
        subprocess.run(['cc', '-O2', '-fwrapv', '-DVAR_ARRAYS', '-DOPUS_BUILD', '-DFIXED_POINT', '-DDISABLE_FLOAT_API',
                        *['-I'+str(source/p) for p in ['include','celt','silk','silk/fixed','src']],
                        str(folder/'probe.c'), '-o', str(folder/'probe')], check=True)
        subprocess.run([str(folder/'probe'), str(args.output.resolve())], check=True)
    print('Captured25600 original fixed-band Hybrid FEC decisions')


if __name__ == '__main__':
    main()
