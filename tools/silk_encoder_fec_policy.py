#!/usr/bin/env python3
"""Optional unchanged native FEC threshold and LBRR gain policy oracle."""
import argparse
from pathlib import Path
import subprocess
import tempfile
from silk_nsq_fixtures import REVISION


def function(text, marker):
    start = text.index(marker)
    brace = text.index('{', start)
    end, depth = brace+1, 1
    while depth:
        depth += (text[end] == '{')-(text[end] == '}')
        end += 1
    return text[start:end]


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
    codec = (source/'silk/control_codec.c').read_text()
    thresholds = function(opus, 'static const opus_int32 fec_thresholds[]')+';'
    decision = function(opus, 'static int decide_fec(')
    gains = function(codec[codec.rindex('static OPUS_INLINE opus_int silk_setup_LBRR('):], 'static OPUS_INLINE opus_int silk_setup_LBRR(\n    silk_encoder_state')
    # Only original decision functions and constants; no codec algorithm edits.
    program = '#include "main_FIX.h"\n#include "API.h"\n#include "opus_private.h"\n#include <stdio.h>\n'+thresholds+'\n'+decision+'\n'+gains+ r'''
int main(int argc,char **argv){
 if(argc!=2)return 1;FILE *out=fopen(argv[1],"w");if(!out)return 2;
 fprintf(out,"# Original fixed-bandwidth FEC eligibility and gain policy REVISION\n");
 int losses[]={0,1,5,10,20,25,50,100};int rates[]={5000,10000,11999,12000,12001,13000,14000,16000,18000,20000,32000,64000};
 for(int bw=1101;bw<=1103;bw++)for(int prev=0;prev<=1;prev++)for(int l=0;l<8;l++)for(int r=0;r<12;r++){
  int actual=bw;int selected=decide_fec(1,losses[l],prev,MODE_SILK_ONLY,&actual,rates[r]);
  silk_encoder_state s={0};silk_EncControlStruct c={0};s.LBRR_enabled=prev;s.PacketLoss_perc=losses[l];c.LBRR_coded=selected&&actual==bw;
  if(silk_setup_LBRR(&s,&c))return 3;
  fprintf(out,"%d %d %d %d %d %d %d\n",bw,prev,losses[l],rates[r],selected,actual,s.LBRR_GainIncreases);
 }
 if(fclose(out))return 4;return 0;
}
'''
    program = program.replace('REVISION', REVISION)
    with tempfile.TemporaryDirectory(prefix='opura-fec-policy-') as temp:
        folder = Path(temp)
        (folder/'probe.c').write_text(program)
        subprocess.run(['cc', '-O2', '-fwrapv', '-DVAR_ARRAYS', '-DOPUS_BUILD', '-DFIXED_POINT', '-DDISABLE_FLOAT_API',
                        *['-I'+str(source/p) for p in ['include', 'celt', 'silk', 'silk/fixed', 'src']],
                        str(folder/'probe.c'), '-o', str(folder/'probe')], check=True)
        subprocess.run([str(folder/'probe'), str(args.output.resolve())], check=True)
    print('Captured576 native FEC threshold/gain cases')


if __name__ == '__main__':
    main()
