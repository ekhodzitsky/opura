#!/usr/bin/env python3
"""Optional unchanged public Hybrid DTX suppression and layer-call order oracle."""
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
#include "opus_private.h"
#include "celt.h"
#include <stdio.h>
static int silk_zero,celt_calls;
int __real_silk_Encode(void*,silk_EncControlStruct*,const opus_res*,int,ec_enc*,opus_int32*,int,int);
int __wrap_silk_Encode(void *s,silk_EncControlStruct *ctl,const opus_res *pcm,int n,ec_enc *enc,opus_int32 *bytes,int prefill,int activity){
 int ret=__real_silk_Encode(s,ctl,pcm,n,enc,bytes,prefill,activity);
 if(!prefill)silk_zero=(*bytes==0);return ret;
}
int __real_celt_encode_with_ec(OpusCustomEncoder*,const opus_res*,int,unsigned char*,int,ec_enc*);
int __wrap_celt_encode_with_ec(OpusCustomEncoder *s,const opus_res *pcm,int n,unsigned char *out,int bytes,ec_enc *enc){
 celt_calls++;return __real_celt_encode_with_ec(s,pcm,n,out,bytes,enc);
}
int main(int argc,char **argv){
 if(argc!=2)return 1;FILE *out=fopen(argv[1],"w");if(!out)return 2;
 fprintf(out,"# Unchanged original Hybrid DTX and layer call order REVISION\n");
 int complexities[]={0,6,10};
 for(int rate=24000;rate<=48000;rate*=2)for(int bw=1104;bw<=1105;bw++){
  if(rate==24000 && bw==1105)continue;
  for(int ch=1;ch<=2;ch++)for(int ms=10;ms<=20;ms+=10)for(int comp=0;comp<3;comp++)for(int profile=0;profile<2;profile++){
   int error;OpusEncoder *s=opus_encoder_create(rate,ch,OPUS_APPLICATION_AUDIO,&error);if(!s||error)return 3;
   if(opus_encoder_ctl(s,OPUS_SET_FORCE_MODE(MODE_HYBRID))||opus_encoder_ctl(s,OPUS_SET_FORCE_CHANNELS(ch))||
      opus_encoder_ctl(s,OPUS_SET_VBR(0))||opus_encoder_ctl(s,OPUS_SET_BITRATE(32000*ch))||
      opus_encoder_ctl(s,OPUS_SET_BANDWIDTH(bw))||opus_encoder_ctl(s,OPUS_SET_MAX_BANDWIDTH(bw))||
      opus_encoder_ctl(s,OPUS_SET_COMPLEXITY(complexities[comp])))return 4;
   int n=rate*ms/1000;
   for(int f=0;f<150;f++){
    int enabled=!(f>=85&&f<95)&&!(profile&&f>=5&&f<9);
    if(opus_encoder_ctl(s,OPUS_SET_DTX(enabled)))return 5;
    opus_int16 pcm[1920];unsigned char packet[1276];
    for(int i=0;i<n;i++)for(int c=0;c<ch;c++){
     int phase=(f*n+i)%240;
     pcm[i*ch+c]=f<5||f>=110?((phase<120?phase:240-phase)-60)*200:f<55?0:(i+c)%2;
    }
    silk_zero=celt_calls=0;int length=opus_encode(s,pcm,n,packet,sizeof(packet));
    if(length<1 || (packet[0]>>3)<12 || (packet[0]>>3)>15)return 6;
    fprintf(out,"%d %d %d %d %d %d %d %d %d %d %d\n",rate,bw,ch,ms,complexities[comp],profile,f,enabled,length==1,silk_zero,celt_calls);
   }
   opus_encoder_destroy(s);
  }
 }
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
            raise ValueError('Missing source manifest')
        return match[1].replace('\\', '').split()
    files = (manifest('silk_sources.mk', 'SILK_SOURCES')+manifest('silk_sources.mk', 'SILK_SOURCES_FIXED')+
             manifest('celt_sources.mk', 'CELT_SOURCES')+manifest('opus_sources.mk', 'OPUS_SOURCES'))
    with tempfile.TemporaryDirectory(prefix='opura-hybrid-dtx-') as temp:
        folder = Path(temp)
        (folder/'probe.c').write_text(WRAPPER.replace('REVISION', REVISION))
        subprocess.run(['cc', '-O2', '-fwrapv', '-DVAR_ARRAYS', '-DOPUS_BUILD', '-DFIXED_POINT', '-DDISABLE_FLOAT_API',
                        *['-I'+str(source/p) for p in ['include','celt','silk','silk/fixed','src']],
                        str(folder/'probe.c'), *[str(source/f) for f in files], '-Wl,--wrap=silk_Encode',
                        '-Wl,--wrap=celt_encode_with_ec', '-lm', '-o', str(folder/'probe')], check=True)
        subprocess.run([str(folder/'probe'), str(args.output.resolve())], check=True)
    print('Captured10800 original forced-Hybrid DTX decisions and layer calls')


if __name__ == '__main__':
    main()
