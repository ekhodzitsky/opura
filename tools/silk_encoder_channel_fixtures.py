#!/usr/bin/env python3
"""Capture complete unchanged native SILK frame calls and public packet decoding.

Optional oracle only; no native source/library is a Cargo dependency.
"""
import argparse
from pathlib import Path
import re
import subprocess
import tempfile
from silk_nsq_fixtures import DRIVER, REVISION

WRAPPER = r'''
#include "main_FIX.h"
#include "opus.h"
#include <stdio.h>
#include <stdlib.h>
#include <stdint.h>
static FILE *trace_out;
static struct {int valid,active,attempts;SideInfoIndices indices;opus_int16 input[320];}record;
static struct {ec_enc encoder;SideInfoIndices indices;opus_int8 pulses[320];unsigned char bytes[1275];}trial[8];
static uint64_t hash;
static void add(int value){unsigned v=(unsigned)value;for(int j=0;j<4;j++)hash=(hash^((v>>(8*j))&255))*UINT64_C(1099511628211);}
static uint64_t nsq_hash(const silk_nsq_state *n){
 hash=UINT64_C(14695981039346656037);
 for(int i=0;i<640;i++)add(n->xq[i]);for(int i=0;i<640;i++)add(n->sLTP_shp_Q14[i]);
 for(int i=0;i<96;i++)add(n->sLPC_Q14[i]);for(int i=0;i<24;i++)add(n->sAR2_Q14[i]);
 add(n->sLF_AR_shp_Q14);add(n->sDiff_shp_Q14);add(n->lagPrev);add(n->sLTP_buf_idx);add(n->sLTP_shp_buf_idx);add(n->rand_seed);add(n->prev_gain_Q16);add(n->rewhite_flag);return hash;
}
void __real_silk_encode_indices(silk_encoder_state*,ec_enc*,opus_int,opus_int,opus_int);
void __wrap_silk_encode_indices(silk_encoder_state *s,ec_enc *e,opus_int frame,opus_int lbrr,opus_int coding){
 if(record.active)record.indices=lbrr?s->indices_LBRR[frame]:s->indices;
 __real_silk_encode_indices(s,e,frame,lbrr,coding);
}
void __real_silk_encode_pulses(ec_enc*,const opus_int,const opus_int,opus_int8*,const opus_int);
void __wrap_silk_encode_pulses(ec_enc *e,const opus_int signal,const opus_int offset,opus_int8 *p,const opus_int length){
 __real_silk_encode_pulses(e,signal,offset,p,length);
 if(record.active){int n=record.attempts++;if(n>=8){fprintf(stderr,"too many native trials\n");exit(9);}trial[n].encoder=*e;trial[n].indices=record.indices;memcpy(trial[n].pulses,p,length);memcpy(trial[n].bytes,e->buf,e->offs);}
}
void __real_silk_LP_variable_cutoff(silk_LP_state*,opus_int16*,const opus_int);
void __wrap_silk_LP_variable_cutoff(silk_LP_state *s,opus_int16 *x,const opus_int n){
 __real_silk_LP_variable_cutoff(s,x,n);if(record.active)memcpy(record.input,x,n*sizeof(*x));
}
static int same(const ec_enc *a,const ec_enc *b){
 return a->rng==b->rng && a->val==b->val && a->offs==b->offs && a->end_offs==b->end_offs && a->nbits_total==b->nbits_total && a->rem==b->rem && a->ext==b->ext && a->end_window==b->end_window && a->nend_bits==b->nend_bits && a->error==b->error;
}
opus_int __real_silk_encode_frame_FIX(silk_encoder_state_FIX*,opus_int32*,ec_enc*,opus_int,opus_int,opus_int);
opus_int __wrap_silk_encode_frame_FIX(silk_encoder_state_FIX *s,opus_int32 *bytes,ec_enc *e,opus_int coding,opus_int maximum,opus_int fill){
 int rate=s->sCmn.fs_kHz,count=s->sCmn.nb_subfr,length=s->sCmn.frame_length,start=s->sCmn.nFramesEncoded;
 int before_counter=s->sCmn.frameCounter,first=s->sCmn.first_frame_after_reset,active=s->sCmn.indices.signalType!=0;
 int prefix_tell=ec_tell(e),prefix_frac=ec_tell_frac(e);unsigned prefix_range=e->rng;
 opus_int16 history[400];memcpy(history,s->x_buf,25*rate*sizeof(*history));
 record.valid=1;record.active=1;record.attempts=0;
 int result=__real_silk_encode_frame_FIX(s,bytes,e,coding,maximum,fill);record.active=0;
 if(s->sCmn.prefillFlag){fprintf(stderr,"unexpected native prefill\n");exit(10);}
 int selected=-1;for(int j=0;j<record.attempts;j++)if(same(e,&trial[j].encoder)&&!memcmp(e->buf,trial[j].bytes,e->offs))selected=j;
 if(selected<0||e->error){fprintf(stderr,"native frame did not retain a valid trial\n");exit(11);}
 fprintf(trace_out,"frame %d %d %d %d %d %d %d %d %d %d %d %d %d %d %d %d %d %u %d %d %d %d %d %d %d %d %u %d %d %llu",rate,count,s->sCmn.Complexity,s->sCmn.SNR_dB_Q7,s->sCmn.useCBR,fill,coding,maximum,s->sCmn.speech_activity_Q8,s->sCmn.input_tilt_Q15,active,s->sCmn.PacketLoss_perc,s->sCmn.nFramesPerPacket,s->sCmn.LBRR_flag,before_counter,first,prefix_tell,prefix_range,prefix_frac,start,s->sCmn.input_quality_bands_Q15[0],s->sCmn.input_quality_bands_Q15[1],s->sCmn.input_quality_bands_Q15[2],s->sCmn.input_quality_bands_Q15[3],ec_tell(e),ec_tell_frac(e),e->rng,s->sShape.LastGainIndex,record.attempts,(unsigned long long)nsq_hash(&s->sCmn.sNSQ));
 for(int i=0;i<25*rate;i++)fprintf(trace_out," %d",history[i]);
 for(int i=0;i<length;i++)fprintf(trace_out," %d",record.input[i]);
 SideInfoIndices *ix=&trial[selected].indices;int order=rate==16?16:10;
 fprintf(trace_out," %d %d",ix->signalType,ix->quantOffsetType);
 for(int i=0;i<4;i++)fprintf(trace_out," %d",i<count?ix->GainsIndices[i]:0);
 for(int i=0;i<17;i++)fprintf(trace_out," %d",i<=order?ix->NLSFIndices[i]:0);
 fprintf(trace_out," %d %d %d %d",ix->NLSFInterpCoef_Q2,ix->signalType==2?ix->lagIndex:0,ix->signalType==2?ix->contourIndex:0,ix->signalType==2?ix->PERIndex:0);
 for(int i=0;i<4;i++)fprintf(trace_out," %d",ix->signalType==2&&i<count?ix->LTPIndex[i]:0);
 fprintf(trace_out," %d %d",ix->signalType==2?ix->LTP_scaleIndex:0,ix->Seed);
 for(int i=0;i<length;i++)fprintf(trace_out," %d",trial[selected].pulses[i]);
 ec_enc copy=*e;unsigned char buffer[1275];memcpy(buffer,e->buf,e->storage);copy.buf=buffer;
 unsigned size=(ec_tell(e)+7)>>3;ec_enc_shrink(&copy,size);ec_enc_done(&copy);if(copy.error){fprintf(stderr,"native copy finalization error\n");exit(12);}
 fprintf(trace_out," ");for(unsigned i=0;i<size;i++)fprintf(trace_out,"%02x",buffer[i]);fprintf(trace_out,"\n");
 return result;
}
static OpusDecoder *decoder;
static void emit_packet(FILE *out,int first,unsigned char *packet,int length){
 if(first){if(decoder)opus_decoder_destroy(decoder);int error;decoder=opus_decoder_create(48000,1,&error);if(!decoder||error)exit(13);}
 opus_int16 pcm[2880];int count=opus_decode(decoder,packet,length,pcm,2880,0);if(count<1)exit(14);
 opus_uint32 range;if(opus_decoder_ctl(decoder,OPUS_GET_FINAL_RANGE(&range)))exit(15);
 uint64_t fingerprint=UINT64_C(14695981039346656037);for(int i=0;i<count;i++){unsigned v=(uint16_t)pcm[i];fingerprint=(fingerprint^(v&255))*UINT64_C(1099511628211);fingerprint=(fingerprint^(v>>8))*UINT64_C(1099511628211);}
 fprintf(out,"packet %d %d %u %llu ",first,count,range,(unsigned long long)fingerprint);for(int i=0;i<length;i++)fprintf(out,"%02x",packet[i]);fprintf(out,"\n");
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
    driver = '\n'.join(line for line in DRIVER.splitlines() if not line.lstrip().startswith('fprintf(out,"#'))
    driver = driver.replace('if(!out)return 2;', 'if(!out)return 2;trace_out=out;\nfprintf(out,"# Unchanged public SILK channel frames '+REVISION+'\\n");')
    driver = driver.replace('OPUS_SET_COMPLEXITY(0)', 'OPUS_SET_COMPLEXITY(bandwidth==1101?0:bandwidth==1102?6:10)')
    driver = driver.replace('for(int bandwidth=1101;', 'for(int vbr=0;vbr<=1;vbr++)for(int bandwidth=1101;')
    driver = driver.replace('OPUS_SET_VBR(0)', 'OPUS_SET_VBR(vbr)')
    driver = driver.replace('ms<=20;ms+=10', 'ms<=60;ms=ms==10?20:ms+20').replace('input[960]', 'input[2880]')
    driver = driver.replace('if(opus_encoder_ctl(encoder,OPUS_SET_BITRATE(bitrate)))return 5;', 'if(opus_encoder_ctl(encoder,OPUS_SET_BITRATE(bitrate))||opus_encoder_ctl(encoder,OPUS_SET_PACKET_LOSS_PERC(frame>=6?20:0)))return 5;')
    driver = driver.replace('if(opus_encode(encoder,input,ms*48,packet,sizeof(packet))<1||!record.valid)return 6;\n   emit(out,frame==0);', 'int encoded_length=opus_encode(encoder,input,ms*48,packet,sizeof(packet));\n   if(encoded_length<1||!record.valid)return 6;\n   emit_packet(out,frame==0,packet,encoded_length);')
    names = ['silk_encode_frame_FIX', 'silk_LP_variable_cutoff', 'silk_encode_indices', 'silk_encode_pulses']
    with tempfile.TemporaryDirectory(prefix='opura-silk-channel-') as temp:
        folder = Path(temp)
        (folder/'probe.c').write_text(WRAPPER+'\n'+driver)
        subprocess.run(['cc', '-O2', '-fwrapv', '-DVAR_ARRAYS', '-DOPUS_BUILD', '-DFIXED_POINT', '-DDISABLE_FLOAT_API',
                        *['-I'+str(source/p) for p in ['include','celt','silk','silk/fixed','src']],
                        str(folder/'probe.c'), *[str(source/f) for f in files],
                        *['-Wl,--wrap='+name for name in names], '-lm', '-o', str(folder/'probe')], check=True)
        subprocess.run([str(folder/'probe'), str(args.output.resolve())], check=True)
    print('Captured complete native CBR/VBR channel frames and public PCM packets')


if __name__ == '__main__':
    main()
