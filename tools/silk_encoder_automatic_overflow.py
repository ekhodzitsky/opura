#!/usr/bin/env python3
"""Optional original restricted-SILK short-budget overflow/PLC reproduction."""
import argparse
from pathlib import Path
import re
import subprocess
import tempfile
from silk_nsq_fixtures import REVISION

PROGRAM = r'''

#include "main_FIX.h"
#include "opus.h"
#include "opus_private.h"
#include <stdio.h>
opus_int __real_silk_encode_frame_FIX(silk_encoder_state_FIX*,opus_int32*,ec_enc*,opus_int,opus_int,opus_int);
opus_int __wrap_silk_encode_frame_FIX(silk_encoder_state_FIX*s,opus_int32*b,ec_enc*e,opus_int coding,opus_int maximum,opus_int fill){
 int before=ec_tell(e);int ret=__real_silk_encode_frame_FIX(s,b,e,coding,maximum,fill);
 printf("channel%d rate%d frame%d before%d soft%d after%d error%d ret%d\n",s->sCmn.channelNb,s->sCmn.fs_kHz,s->sCmn.frameCounter,before,maximum,ec_tell(e),e->error,ret);return ret;
}
int main(void){int error=0;OpusEncoder*e=opus_encoder_create(48000,2,OPUS_APPLICATION_RESTRICTED_SILK,&error);if(!e||error)return 1;
 if(opus_encoder_ctl(e,OPUS_SET_FORCE_MODE(MODE_SILK_ONLY))||opus_encoder_ctl(e,OPUS_SET_FORCE_CHANNELS(2))||opus_encoder_ctl(e,OPUS_SET_BITRATE(27200))||opus_encoder_ctl(e,OPUS_SET_VBR(0))||opus_encoder_ctl(e,OPUS_SET_COMPLEXITY(0))||opus_encoder_ctl(e,OPUS_SET_INBAND_FEC(1))||opus_encoder_ctl(e,OPUS_SET_PACKET_LOSS_PERC(20))||opus_encoder_ctl(e,OPUS_SET_DTX(1)))return 2;
 short pcm[960];for(int i=0;i<480;i++)for(int ch=0;ch<2;ch++){int phase=(i+ch*17)%80;pcm[2*i+ch]=((phase<80-phase?phase:80-phase)-20)*600;}
 for(int f=0;f<4;f++){unsigned char out[34];int n=opus_encode(e,pcm,480,out,sizeof(out));const unsigned char *frames[48];opus_int16 sizes[48];unsigned char toc=0;int count=n>0?opus_packet_parse(out,n,&toc,frames,sizes,NULL):0;
 opus_uint32 range=0;opus_encoder_ctl(e,OPUS_GET_FINAL_RANGE(&range));
 int plc=count==1&&sizes[0]==1&&frames[0][0]==0;
 printf("packet%d bytes%d toc%d frames%d size%d plc%d range%u\n",f,n,n>0?out[0]:-1,count,count>0?sizes[0]:0,plc,range);
 if(n!=34||count!=1||(f>=2&&(!plc||range)))return 3;}
 opus_encoder_destroy(e);return 0;
}
'''


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source', type=Path)
    parser.add_argument('output', type=Path)
    args = parser.parse_args()
    source = args.source.resolve()
    if subprocess.check_output(['git','-C',str(source),'rev-parse','HEAD'],text=True).strip() != REVISION:
        raise SystemExit('Expected pinned Opus v1.6.1')
    if subprocess.check_output(['git','-C',str(source),'status','--porcelain'],text=True).strip():
        raise SystemExit('Reference checkout must be unchanged')
    def manifest(file, variable):
        match = re.search(r'^'+variable+r' = (.*?)(?=\n\n|\Z)', (source/file).read_text(), re.M|re.S)
        if match is None:
            raise SystemExit('Missing native source manifest')
        return match[1].replace('\\','').split()
    files = (manifest('silk_sources.mk','SILK_SOURCES') + manifest('silk_sources.mk','SILK_SOURCES_FIXED') +
             manifest('celt_sources.mk','CELT_SOURCES') + manifest('opus_sources.mk','OPUS_SOURCES'))
    with tempfile.TemporaryDirectory(prefix='opura-automatic-overflow-') as temp:
        folder = Path(temp)
        (folder/'probe.c').write_text(PROGRAM)
        subprocess.run(['cc','-O2','-fwrapv','-DVAR_ARRAYS','-DOPUS_BUILD','-DFIXED_POINT','-DDISABLE_FLOAT_API',
                        *['-I'+str(source/p) for p in ['include','celt','silk','silk/fixed','src']],
                        str(folder/'probe.c'),*[str(source/p) for p in files],
                        '-Wl,--wrap=silk_encode_frame_FIX','-lm','-o',str(folder/'probe')],check=True)
        with args.output.open('w') as output:
            output.write('# Original restricted-SILK WB stereo 27200bps/10ms/complexity0 overflow '+REVISION+'\n')
            output.flush()
            subprocess.run([str(folder/'probe')],stdout=output,check=True)
    print('Reproduced4 native short-budget packets, including2 PLC fallbacks')


if __name__ == '__main__':
    main()
