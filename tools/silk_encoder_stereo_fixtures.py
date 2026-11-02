#!/usr/bin/env python3
"""Trace unchanged public native SILK L/R-to-M/S analysis and stereo signaling."""
import argparse
from pathlib import Path
import re
import subprocess
import tempfile
from silk_nsq_fixtures import REVISION

WRAPPER = r'''
#include "main.h"
#include "opus.h"
#include <stdio.h>
#include <stdlib.h>
static FILE *out;
static void state(const stereo_enc_state *s){
 for(int i=0;i<2;i++)fprintf(out," %d",s->pred_prev_Q13[i]);
 for(int i=0;i<2;i++)fprintf(out," %d",s->sMid[i]);
 for(int i=0;i<2;i++)fprintf(out," %d",s->sSide[i]);
 for(int i=0;i<4;i++)fprintf(out," %d",s->mid_side_amp_Q0[i]);
 fprintf(out," %d %d %d",s->smth_width_Q14,s->width_prev_Q14,s->silent_side_len);
}
void __real_silk_stereo_LR_to_MS(stereo_enc_state*,opus_int16*,opus_int16*,opus_int8 (*)[3],opus_int8*,opus_int32*,opus_int32,opus_int,opus_int,opus_int,opus_int);
void __wrap_silk_stereo_LR_to_MS(stereo_enc_state *s,opus_int16 *left,opus_int16 *right,opus_int8 ix[2][3],opus_int8 *only,opus_int32 *rates,opus_int32 total,opus_int activity,opus_int mono,opus_int rate,opus_int length){
 stereo_enc_state before=*s;opus_int16 l[320],r[320];memcpy(l,left,length*sizeof(*left));memcpy(r,right,length*sizeof(*right));
 __real_silk_stereo_LR_to_MS(s,left,right,ix,only,rates,total,activity,mono,rate,length);
 fprintf(out,"%d %d %d %d %d %d %d %d",rate,length,total,activity,mono,*only,rates[0],rates[1]);
 state(&before);state(s);
 for(int n=0;n<2;n++)for(int i=0;i<3;i++)fprintf(out," %d",ix[n][i]);
 for(int i=0;i<length;i++)fprintf(out," %d",l[i]);for(int i=0;i<length;i++)fprintf(out," %d",r[i]);
 for(int i=0;i<length;i++)fprintf(out," %d",left[i-1]);for(int i=0;i<length;i++)fprintf(out," %d",right[i-1]);
 unsigned char bytes[32];ec_enc e;ec_enc_init(&e,bytes,32);silk_stereo_encode_pred(&e,ix);silk_stereo_encode_mid_only(&e,*only);
 fprintf(out," %u %u %u",e.rng,ec_tell(&e),ec_tell_frac(&e));ec_enc_done(&e);fprintf(out," ");for(int i=0;i<32;i++)fprintf(out,"%02x",bytes[i]);fprintf(out,"\n");
}
int main(int argc,char **argv){
 if(argc!=2)return 1;out=fopen(argv[1],"w");if(!out)return 2;fprintf(out,"# Original public encoder stereo analysis %s\n","REVISION");unsigned random=0x53544552;
 for(int bandwidth=1101;bandwidth<=1103;bandwidth++)for(int ms=10;ms<=20;ms+=10){
  int error;OpusEncoder *encoder=opus_encoder_create(48000,2,OPUS_APPLICATION_RESTRICTED_SILK,&error);if(!encoder||error)return 3;
  if(opus_encoder_ctl(encoder,OPUS_SET_FORCE_CHANNELS(2))||opus_encoder_ctl(encoder,OPUS_SET_VBR(0))||opus_encoder_ctl(encoder,OPUS_SET_BANDWIDTH(bandwidth))||opus_encoder_ctl(encoder,OPUS_SET_MAX_BANDWIDTH(bandwidth))||opus_encoder_ctl(encoder,OPUS_SET_COMPLEXITY(6)))return 4;
  for(int frame=0;frame<24;frame++){
   int bitrate=frame<4?64000:frame<14?10000:64000;if(opus_encoder_ctl(encoder,OPUS_SET_BITRATE(bitrate)))return 5;
   opus_int16 pcm[1920];unsigned char packet[1276];
   for(int i=0;i<ms*48;i++){
    int phase=(frame*ms*48+i)%240;int value=((phase<120?phase:240-phase)-60)*200;
    random=1664525*random+1013904223;pcm[2*i]=frame<2?0:frame<14?value:(int)(random>>16)-32768;
    random=1664525*random+1013904223;pcm[2*i+1]=frame<2?0:frame<14?value:(int)(random>>16)-32768;
   }
   if(opus_encode(encoder,pcm,ms*48,packet,sizeof(packet))<1)return 6;
  }
  opus_encoder_destroy(encoder);
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
            raise SystemExit('Missing source manifest')
        return match[1].replace('\\', '').split()
    files = (manifest('silk_sources.mk', 'SILK_SOURCES') + manifest('silk_sources.mk', 'SILK_SOURCES_FIXED') +
             manifest('celt_sources.mk', 'CELT_SOURCES') + manifest('opus_sources.mk', 'OPUS_SOURCES'))
    with tempfile.TemporaryDirectory(prefix='opura-silk-stereo-encoder-') as temp:
        folder = Path(temp)
        (folder/'probe.c').write_text(WRAPPER.replace('"REVISION"', '"'+REVISION+'"'))
        subprocess.run(['cc', '-O2', '-fwrapv', '-DVAR_ARRAYS', '-DOPUS_BUILD', '-DFIXED_POINT', '-DDISABLE_FLOAT_API',
                        *['-I'+str(source/p) for p in ['include','celt','silk','silk/fixed','src']],
                        str(folder/'probe.c'), *[str(source/f) for f in files], '-Wl,--wrap=silk_stereo_LR_to_MS', '-lm', '-o', str(folder/'probe')], check=True)
        subprocess.run([str(folder/'probe'), str(args.output.resolve())], check=True)
    print('Captured complete unchanged public stereo analysis/state/entropy traces')


if __name__ == '__main__':
    main()
