#!/usr/bin/env python3
"""Optional unchanged native channel-history resampling and input-state priming oracle."""
import argparse
from pathlib import Path
import subprocess
import tempfile
from silk_nsq_fixtures import REVISION

WRAPPER = r'''
static unsigned random_step(unsigned *r){*r=1664525*(*r)+1013904223;return *r;}
static uint64_t fingerprint(opus_int16 *x,int n){uint64_t h=UINT64_C(14695981039346656037);for(int i=0;i<n;i++){unsigned v=(uint16_t)x[i];h=(h^(v&255))*UINT64_C(1099511628211);h=(h^(v>>8))*UINT64_C(1099511628211);}return h;}
int main(int argc,char **argv){
 if(argc!=2)return 1;FILE *out=fopen(argv[1],"w");if(!out)return 2;
 fprintf(out,"# Original setup_resamplers history and primed PCM REVISION\n");
 int rates[]={8,12,16},apis[]={8000,12000,16000,24000,48000};unsigned id=0;
 for(int old=0;old<3;old++)for(int next=0;next<3;next++)for(int a=0;a<5;a++)for(int ms=10;ms<=20;ms+=10){
  silk_encoder_state_FIX s={0};s.sCmn.fs_kHz=rates[old];s.sCmn.nb_subfr=ms/5;s.sCmn.API_fs_Hz=apis[a];s.sCmn.prev_API_fs_Hz=apis[a];
  unsigned seed_history=0x48495354u^id,seed_audio=0x42575253u^id,random=seed_history;
  for(int i=0;i<720;i++)s.x_buf[i]=(int)(random_step(&random)>>16)-32768;
  random=seed_audio;opus_int16 input[960],pcm[320];int n=ms*(apis[a]/1000);
  for(int i=0;i<n;i++)input[i]=(int)(random_step(&random)>>16)-32768;
  if(silk_resampler_init(&s.sCmn.resampler_state,apis[a],rates[old]*1000,1)||silk_resampler(&s.sCmn.resampler_state,pcm,input,n))return 3;
  if(silk_setup_resamplers(&s,rates[next]))return 4;
  fprintf(out,"%d %d %d %d %u %u %llu",rates[old],rates[next],apis[a],ms,seed_history,seed_audio,(unsigned long long)fingerprint(s.x_buf,720));
  for(int block=0;block<8;block++){
   int length=(block%2?20:10)*(apis[a]/1000);
   for(int i=0;i<length;i++)input[i]=(int)(random_step(&random)>>16)-32768;
   if(silk_resampler(&s.sCmn.resampler_state,pcm,input,length))return 5;
   fprintf(out," %llu",(unsigned long long)fingerprint(pcm,(block%2?20:10)*rates[next]));
  }
  fprintf(out,"\n");id++;
 }
 if(fclose(out))return 6;return 0;
}
'''


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
    codec = (source/'silk/control_codec.c').read_text()
    setup = function(codec[codec.rindex('static opus_int silk_setup_resamplers('):], 'static opus_int silk_setup_resamplers(')
    headers = '#include "main_FIX.h"\n#include "API.h"\n#include "stack_alloc.h"\n#include "tuning_parameters.h"\n#include <stdio.h>\n#include <stdint.h>\n#define silk_encoder_state_Fxx silk_encoder_state_FIX\n'
    program = (headers+setup+WRAPPER).replace('REVISION', REVISION)
    files = ['resampler.c','resampler_down2_3.c','resampler_down2.c','resampler_private_AR2.c','resampler_private_down_FIR.c','resampler_private_IIR_FIR.c','resampler_private_up2_HQ.c','resampler_rom.c']
    with tempfile.TemporaryDirectory(prefix='opura-silk-rate-') as temp:
        folder = Path(temp)
        (folder/'probe.c').write_text(program)
        subprocess.run(['cc', '-O2', '-fwrapv', '-DVAR_ARRAYS', '-DOPUS_BUILD', '-DFIXED_POINT', '-DDISABLE_FLOAT_API',
                        *['-I'+str(source/p) for p in ['include', 'celt', 'silk', 'silk/fixed', 'src']],
                        str(folder/'probe.c'), *[str(source/'silk'/f) for f in files], '-o', str(folder/'probe')], check=True)
        subprocess.run([str(folder/'probe'), str(args.output.resolve())], check=True)
    print('Captured90 native rate migration histories and720 primed resampler PCM results')


if __name__ == '__main__':
    main()
