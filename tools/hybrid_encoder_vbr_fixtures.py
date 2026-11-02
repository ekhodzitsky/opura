#!/usr/bin/env python3
"""Optional unchanged original Hybrid VBR target/minimum/length oracle."""
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
        raise ValueError('Expected pinned Opus v1.6.1')
    if subprocess.check_output(['git', '-C', str(source), 'status', '--porcelain'], text=True).strip():
        raise ValueError('Reference checkout must be unchanged')
    text = (source/'celt/celt_encoder.c').read_text()
    minimum = text[text.index('   min_allowed = ((tell+total_boost'):text.index('   /* Variable bitrate */', text.index('   min_allowed = ((tell+total_boost'))]
    cap_line = '     nbCompressedBytes = IMIN(nbCompressedBytes,packet_size_cap>>(3-LM));'
    assert cap_line in text
    base = '        base_target = IMAX(0, vbr_rate - ((9*C+4)<<BITRES));'
    assert base in text
    target = function(text, '{\n        target = base_target;')[1:-1]
    final = text[text.index('     target=target+tell;'):text.index('     /* By how much did we "miss"')]
    program = '#include "arch.h"\n#include "mathops.h"\n#include <stdio.h>\n#define BITRES 3\n'+r'''
int main(int argc,char **argv){
 if(argc!=2)return 1;FILE *out=fopen(argv[1],"w");if(!out)return 2;
 fprintf(out,"# Original unconstrained Hybrid VBR target and size REVISION\n");
 int capacities[]={2,3,8,32,64,128,256,637,638,1275};
 int rates[]={1,800,4000,8000,16000,64000,512000},offsets[]={32,100,240};
 int estimates[]={0,4095,4096,8192,11468,11469,11470,16384};
 for(int LM=2;LM<=3;LM++)for(int C=1;C<=2;C++)for(int cap=0;cap<10;cap++)
 for(int rate=0;rate<7;rate++)for(int off=0;off<3;off++)for(int est=0;est<8;est++)for(int scenario=0;scenario<3;scenario++){
  int capacity=capacities[cap],nbCompressedBytes=capacity,packet_size_cap=1275,hybrid=1;
  int tell0_frac=scenario==0?8:scenario==1?capacity*64/3:capacity*64-8;
  int tell=IMIN(capacity*64-1,tell0_frac+13+C*5),total_boost=scenario==0?0:capacity*(scenario==1?4:1);
  int vbr_rate=(rates[rate]/(400>>LM))<<BITRES,base_target,target,min_allowed,nbAvailableBytes;
  opus_val16 tf_estimate=estimates[est];struct {struct{int offset;}silk_info;}state,*st=&state;
  st->silk_info.offset=offsets[off];
MINIMUM
CAP
BASE
TARGET
FINAL
  fprintf(out,"%d %d %d %d %d %d %d %d %d %d\n",LM,C,capacity,rates[rate],offsets[off],tf_estimate,tell0_frac,tell,total_boost,nbAvailableBytes);
 }
 if(fclose(out))return 3;return 0;
}
'''
    for marker, value in [('MINIMUM', minimum), ('CAP', cap_line), ('BASE', base), ('TARGET', target), ('FINAL', final), ('REVISION', REVISION)]:
        program = program.replace(marker, value)
    with tempfile.TemporaryDirectory(prefix='opura-hybrid-vbr-') as temp:
        folder = Path(temp)
        (folder/'probe.c').write_text(program)
        subprocess.run(['cc', '-O2', '-fwrapv', '-DVAR_ARRAYS', '-DOPUS_BUILD', '-DFIXED_POINT', '-DDISABLE_FLOAT_API',
                        *['-I'+str(source/p) for p in ['include','celt','silk','silk/fixed','src']],
                        str(folder/'probe.c'), '-o', str(folder/'probe')], check=True)
        subprocess.run([str(folder/'probe'), str(args.output.resolve())], check=True)
    print('Captured20160 original Hybrid VBR size decisions')


if __name__ == '__main__':
    main()
