#!/usr/bin/env python3
"""Capture unchanged native LBRR and main SILK frame/packet encoding.

Optional oracle only; no native source/library is a Cargo dependency.
"""
import argparse
from pathlib import Path
import re
import subprocess
import tempfile
from silk_nsq_fixtures import DRIVER, REVISION

from silk_encoder_channel_fixtures import WRAPPER as CHANNEL_WRAPPER

WRAPPER = CHANNEL_WRAPPER.replace('static FILE *trace_out;', 'static FILE *trace_out;static int native_overflow;static int failure[7];').replace(
    ' int result=__real_silk_encode_frame_FIX',
    ' int fec_enabled=s->sCmn.LBRR_enabled,fec_increase=s->sCmn.LBRR_GainIncreases;\n'
    ' int fec_reset=start==0||s->sCmn.LBRR_flags[start-1]==0;\n'
    ' int result=__real_silk_encode_frame_FIX')
WRAPPER = WRAPPER.replace('if(selected<0||e->error){fprintf(stderr,"native frame did not retain a valid trial\\n");exit(11);}', 'if(selected<0||e->error){ native_overflow=1;failure[0]=rate;failure[1]=count;failure[2]=before_counter;failure[3]=start;failure[4]=ec_tell(e);failure[5]=maximum;failure[6]=e->storage;return result;}')
WRAPPER = WRAPPER.replace(' int rate=s->sCmn.fs_kHz', ' if(native_overflow)return __real_silk_encode_frame_FIX(s,bytes,e,coding,maximum,fill);\n int rate=s->sCmn.fs_kHz')
WRAPPER = WRAPPER.replace(' fprintf(trace_out,"frame ', r''' int fec_flag=s->sCmn.LBRR_flags[start];
 fprintf(trace_out,"fec %d %d %d %d %d",fec_enabled,fec_increase,fec_reset,s->sCmn.LBRRprevLastGainIndex,fec_flag);
 if(fec_flag){
 SideInfoIndices *ix=&s->sCmn.indices_LBRR[start];int order=rate==16?16:10;
 fprintf(trace_out," %d %d",ix->signalType,ix->quantOffsetType);
 for(int i=0;i<4;i++)fprintf(trace_out," %d",i<count?ix->GainsIndices[i]:0);
 for(int i=0;i<17;i++)fprintf(trace_out," %d",i<=order?ix->NLSFIndices[i]:0);
 fprintf(trace_out," %d %d %d %d",ix->NLSFInterpCoef_Q2,ix->signalType==2?ix->lagIndex:0,ix->signalType==2?ix->contourIndex:0,ix->signalType==2?ix->PERIndex:0);
 for(int i=0;i<4;i++)fprintf(trace_out," %d",ix->signalType==2&&i<count?ix->LTPIndex[i]:0);
 fprintf(trace_out," %d %d",ix->signalType==2?ix->LTP_scaleIndex:0,ix->Seed);
 for(int i=0;i<length;i++)fprintf(trace_out," %d",s->sCmn.pulses_LBRR[start][i]);
 }
 fprintf(trace_out,"\n");
 fprintf(trace_out,"frame ''')

WRAPPER = WRAPPER.replace('static OpusDecoder *decoder;', r"""
static OpusDecoder *decoder,*recovery;
static int recovery_index;
static void emit_recovery(FILE *out,unsigned char *packet,int length,int count,int fec){
 opus_int16 pcm[2880];int n=opus_decode(recovery,packet,length,pcm,count,fec);if(n!=count)exit(16);
 opus_uint32 range;if(opus_decoder_ctl(recovery,OPUS_GET_FINAL_RANGE(&range)))exit(17);
 uint64_t h=UINT64_C(14695981039346656037);for(int i=0;i<n;i++){unsigned v=(uint16_t)pcm[i];h=(h^(v&255))*UINT64_C(1099511628211);h=(h^(v>>8))*UINT64_C(1099511628211);}
 fprintf(out,"recover %d %d %u %llu\n",fec,n,range,(unsigned long long)h);
}
""")
WRAPPER = WRAPPER.replace('if(!decoder||error)exit(13);}', 'if(!decoder||error)exit(13);if(recovery)opus_decoder_destroy(recovery);recovery=opus_decoder_create(48000,1,&error);if(!recovery||error)exit(18);recovery_index=0;}')
WRAPPER = WRAPPER.replace('for(int i=0;i<length;i++)fprintf(out,"%02x",packet[i]);fprintf(out,"\\n");', 'for(int i=0;i<length;i++)fprintf(out,"%02x",packet[i]);fprintf(out,"\\n");int index=recovery_index++%3;if(index!=1){if(index==2)emit_recovery(out,packet,length,count,1);emit_recovery(out,packet,length,count,0);}')


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
    driver = driver.replace('if(!out)return 2;', 'if(!out)return 2;trace_out=tmpfile();if(!trace_out)return 14;\nfprintf(out,"# Unchanged public SILK channel frames '+REVISION+'\\n");')
    driver = driver.replace('OPUS_SET_COMPLEXITY(0)', 'OPUS_SET_COMPLEXITY(bandwidth==1101?0:bandwidth==1102?6:10)')
    driver = driver.replace('for(int bandwidth=1101;', 'for(int vbr=0;vbr<=1;vbr++)for(int bandwidth=1101;')
    driver = driver.replace('OPUS_SET_VBR(0)', 'OPUS_SET_VBR(vbr)')
    driver = driver.replace('ms<=20;ms+=10', 'ms<=60;ms=ms==10?20:ms+20').replace('input[960]', 'input[2880]')
    driver = driver.replace('if(opus_encoder_ctl(encoder,OPUS_SET_BITRATE(bitrate)))return 5;', 'if(opus_encoder_ctl(encoder,OPUS_SET_BITRATE(bitrate))||opus_encoder_ctl(encoder,OPUS_SET_PACKET_LOSS_PERC(20))||opus_encoder_ctl(encoder,OPUS_SET_INBAND_FEC(frame<8||frame>=10)))return 5;')
    driver = driver.replace('if(opus_encode(encoder,input,ms*48,packet,sizeof(packet))<1||!record.valid)return 6;\n   emit(out,frame==0);', 'int encoded_length=opus_encode(encoder,input,ms*48,packet,sizeof(packet));\n   if(encoded_length<1||!record.valid)return 6;\n   emit_packet(out,frame==0,packet,encoded_length);')
    # Retain the original source/bitrate workload, and report rather than hide
    # native internal entropy failures. Only valid whole packets become golden
    # snapshots; the failed packet trace is discarded and that stream ends.
    driver = driver.replace('record.valid=0;', 'record.valid=0;native_overflow=0;')
    driver = driver.replace('if(encoded_length<1||!record.valid)return 6;\n   emit_packet', 'if(encoded_length<1||!record.valid)return 6;\n   if(native_overflow){fprintf(out,"overflow");for(int k=0;k<7;k++)fprintf(out," %d",failure[k]);fprintf(out," %d ",encoded_length);for(int k=0;k<encoded_length;k++)fprintf(out,"%02x",packet[k]);fprintf(out,"\\n");fclose(trace_out);trace_out=tmpfile();if(!trace_out)return 14;break;}\n   rewind(trace_out);int byte;while((byte=fgetc(trace_out))!=EOF)if(fputc(byte,out)==EOF)return 15;fclose(trace_out);trace_out=tmpfile();if(!trace_out)return 14;\n   emit_packet')
    driver = driver.replace('if(fclose(out))return 7;', 'if(fclose(trace_out)||fclose(out))return 7;')
    names = ['silk_encode_frame_FIX', 'silk_LP_variable_cutoff', 'silk_encode_indices', 'silk_encode_pulses']
    with tempfile.TemporaryDirectory(prefix='opura-silk-fec-') as temp:
        folder = Path(temp)
        (folder/'probe.c').write_text(WRAPPER+'\n'+driver)
        subprocess.run(['cc', '-O2', '-fwrapv', '-DVAR_ARRAYS', '-DOPUS_BUILD', '-DFIXED_POINT', '-DDISABLE_FLOAT_API',
                        *['-I'+str(source/p) for p in ['include','celt','silk','silk/fixed','src']],
                        str(folder/'probe.c'), *[str(source/f) for f in files],
                        *['-Wl,--wrap='+name for name in names], '-lm', '-o', str(folder/'probe')], check=True)
        subprocess.run([str(folder/'probe'), str(args.output.resolve())], check=True)
    print('Captured native LBRR and main CBR/VBR frames and public packets')


if __name__ == '__main__':
    main()
