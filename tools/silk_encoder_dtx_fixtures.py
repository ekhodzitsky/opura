#!/usr/bin/env python3
"""Capture unchanged native SILK VAD/DTX counters and outer digital-silence clock."""
import argparse
from pathlib import Path
import re
import subprocess
import tempfile
from silk_nsq_fixtures import REVISION

WRAPPER = r'''
#include "main_FIX.h"
#include "API.h"
#include "opus.h"
#include <stdio.h>
#include <stdlib.h>
static FILE *out;
OUTER_FUNCTION
void __real_silk_encode_do_VAD_FIX(silk_encoder_state_FIX*,opus_int);
void __wrap_silk_encode_do_VAD_FIX(silk_encoder_state_FIX *s,opus_int activity){
 int before=s->sCmn.noSpeechCounter,flag=s->sCmn.inDTX;
 __real_silk_encode_do_VAD_FIX(s,activity);
 fprintf(out,"vad %d %d %d %d %d\n",before,flag,s->sCmn.speech_activity_Q8>=13,s->sCmn.noSpeechCounter,s->sCmn.inDTX);
}
int main(int argc,char **argv){
 if(argc!=2)return 1;out=fopen(argv[1],"w");if(!out)return 2;
 fprintf(out,"# Unchanged native DTX VAD and outer clock %s\n","REVISION");
 for(int profile=0;profile<2;profile++)for(int channels=1;channels<=2;channels++)for(int ms=10;ms<=60;ms=ms==10?20:ms+20){
  int error;OpusEncoder *e=opus_encoder_create(48000,channels,OPUS_APPLICATION_RESTRICTED_SILK,&error);if(!e||error)return 3;
  if(opus_encoder_ctl(e,OPUS_SET_FORCE_CHANNELS(channels))||opus_encoder_ctl(e,OPUS_SET_VBR(0))||opus_encoder_ctl(e,OPUS_SET_BANDWIDTH(OPUS_BANDWIDTH_WIDEBAND))||opus_encoder_ctl(e,OPUS_SET_BITRATE(24000*channels))||opus_encoder_ctl(e,OPUS_SET_COMPLEXITY(0)))return 4;
  for(int f=0;f<150;f++){
   int enabled=!(f>=85&&f<95)&&!(profile&&f>=5&&f<9);if(opus_encoder_ctl(e,OPUS_SET_DTX(enabled)))return 5;
   opus_int16 pcm[5760];unsigned char packet[1276];
   for(int i=0;i<48*ms;i++)for(int c=0;c<channels;c++){
    int phase=(f*48*ms+i)%240;
    pcm[i*channels+c]=f<5||f>=110?((phase<120?phase:240-phase)-60)*200:f<55?0:(i+c)%2;
   }
   int length=opus_encode(e,pcm,ms*48,packet,sizeof(packet));if(length<1)return 6;
   fprintf(out,"public %d %d %d %d %d\n",channels,ms,f,enabled,length==1);
  }
  opus_encoder_destroy(e);
 }
 for(int ms=10;ms<=60;ms=ms==10?20:ms+20){
  int count=0;
  for(int f=0;f<100;f++){
   int active=f==0||f==70,before=count;
   int dtx=decide_dtx_mode(active,&count,2*ms);
   fprintf(out,"clock %d %d %d %d %d\n",before,ms,active,count,dtx);
  }
 }
 opus_int16 quiet[100]={-1,-1,-1};opus_int32 energy,a[10];opus_int q;
 silk_burg_modified(&energy,&q,a,quiet,63388,50,2,10,0);
 fprintf(out,"burg %d %d",energy,q);for(int i=0;i<10;i++)fprintf(out," %d",a[i]);fprintf(out,"\n");
 if(fclose(out))return 7;return 0;
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
    with tempfile.TemporaryDirectory(prefix='opura-silk-dtx-') as temp:
        folder = Path(temp)
        original = (source/'src/opus_encoder.c').read_text()
        start = original.index('static int decide_dtx_mode(')
        brace = original.index('{', start)
        depth, end = 1, brace+1
        while depth:
            depth += (original[end] == '{')-(original[end] == '}')
            end += 1
        probe = WRAPPER.replace('OUTER_FUNCTION', original[start:end])
        (folder/'probe.c').write_text(probe.replace('"REVISION"', '"'+REVISION+'"'))
        subprocess.run(['cc', '-O2', '-fwrapv', '-DVAR_ARRAYS', '-DOPUS_BUILD', '-DFIXED_POINT', '-DDISABLE_FLOAT_API',
                        *['-I'+str(source/p) for p in ['include','celt','silk','silk/fixed','src']],
                        str(folder/'probe.c'), *[str(source/f) for f in files], '-Wl,--wrap=silk_encode_do_VAD_FIX', '-lm', '-o', str(folder/'probe')], check=True)
        subprocess.run([str(folder/'probe'), str(args.output.resolve())], check=True)
    print('Captured unchanged native VAD/DTX and outer-clock traces')


if __name__ == '__main__':
    main()
