#!/usr/bin/env python3
"""Real CELT-only streams and public opus_decode PCM/final range from pinned Opus.

Optional developer oracle: compile unchanged external scalar fixed-point source
in a temporary directory; no reference library is linked or loaded by Cargo.
"""
import argparse
import ctypes as c
from pathlib import Path
import re
import subprocess
import tempfile
REVISION='22244de5a79bd1d6d623c32e72bf1954b56235be'
parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('source',type=Path)
args=parser.parse_args();source=args.source.resolve()
if subprocess.check_output(['git','-C',str(source),'rev-parse','HEAD'],text=True).strip()!=REVISION:
    raise SystemExit('Expected pinned Opus v1.6.1')
if subprocess.check_output(['git','-C',str(source),'status','--porcelain'],text=True).strip():
    raise SystemExit('Reference checkout must be clean')
def manifest(file,variable):
    match=re.search(r'^'+variable+r' = (.*?)(?=\n\n|\Z)',(source/file).read_text(),re.M|re.S)
    if match is None:raise SystemExit('Missing source manifest '+variable)
    return match[1].replace('\\','').split()
wrapper=r'''
#include "opus.h"
#include "opus_private.h"
#include <stdlib.h>
int configure(OpusEncoder *e,int bandwidth,int channels,int bitrate,int vbr) {
    int ret=0;
    ret|=opus_encoder_ctl(e,OPUS_SET_FORCE_MODE(MODE_CELT_ONLY));
    ret|=opus_encoder_ctl(e,OPUS_SET_BANDWIDTH(bandwidth));
    ret|=opus_encoder_ctl(e,OPUS_SET_MAX_BANDWIDTH(bandwidth));
    ret|=opus_encoder_ctl(e,OPUS_SET_FORCE_CHANNELS(channels));
    ret|=opus_encoder_ctl(e,OPUS_SET_BITRATE(bitrate));
    ret|=opus_encoder_ctl(e,OPUS_SET_VBR(vbr));
    ret|=opus_encoder_ctl(e,OPUS_SET_COMPLEXITY(5));
    ret|=opus_encoder_ctl(e,OPUS_SET_DTX(0));
    return ret;
}
int reset(OpusDecoder *d) {return opus_decoder_ctl(d,OPUS_RESET_STATE);}
int decode(OpusDecoder *d,const unsigned char *data,int len,opus_int16 *pcm,opus_uint32 *range) {
    int count=opus_decode(d,data,len,pcm,5760,0);
    if(count>=0 && opus_decoder_ctl(d,OPUS_GET_FINAL_RANGE(range))!=OPUS_OK) abort();
    return count;
}
int repack(const unsigned char *packets,const int *lengths,int count,unsigned char *out) {
    OpusRepacketizer *r=opus_repacketizer_create();if(!r)return OPUS_ALLOC_FAIL;
    int result=OPUS_OK,offset=0;
    for(int i=0;i<count && result==OPUS_OK;i++) {result=opus_repacketizer_cat(r,packets+offset,lengths[i]);offset+=lengths[i];}
    if(result==OPUS_OK) result=opus_repacketizer_out(r,out,65536);
    opus_repacketizer_destroy(r);return result;
}
'''
files=manifest('silk_sources.mk','SILK_SOURCES')+manifest('silk_sources.mk','SILK_SOURCES_FIXED')+manifest('celt_sources.mk','CELT_SOURCES')+manifest('opus_sources.mk','OPUS_SOURCES')
with tempfile.TemporaryDirectory(prefix='opura-celt-packets-') as temp:
    path=Path(temp);(path/'probe.c').write_text(wrapper)
    # Append read-only observers; every original decoding statement stays intact.
    (path/'celt_decoder.c').write_text((source/'celt/celt_decoder.c').read_text()+r'''
void opura_celt_snapshot(const CELTDecoder *st,opus_int32 *out) {
    const celt_glog *energy=(const celt_glog*)(st->_decode_mem+(DECODE_BUFFER_SIZE+st->overlap)*st->channels);
    for(int i=0;i<168;i++) out[i]=energy[i];
    out[168]=st->preemph_memD[0];out[169]=st->preemph_memD[1];
    out[170]=st->postfilter_period_old;out[171]=st->postfilter_gain_old;out[172]=st->postfilter_tapset_old;
    out[173]=st->postfilter_period;out[174]=st->postfilter_gain;out[175]=st->postfilter_tapset;
    for(int c=0;c<st->channels;c++) {
        const celt_sig *memory=st->_decode_mem+c*(DECODE_BUFFER_SIZE+st->overlap);
        for(int i=0;i<60;i++) out[176+c*60+i]=memory[DECODE_BUFFER_SIZE+i];
        for(int i=0;i<1026;i++) out[176+st->channels*60+c*1026+i]=memory[DECODE_BUFFER_SIZE-1026+i];
    }
}
''')
    (path/'opus_decoder.c').write_text((source/'src/opus_decoder.c').read_text()+r'''
extern void opura_celt_snapshot(const CELTDecoder *,opus_int32 *);
void snapshot(const OpusDecoder *st,opus_int32 *out) {
    opura_celt_snapshot((const CELTDecoder*)((const char*)st+st->celt_dec_offset),out);
}
''')
    subprocess.run(['cc','-shared','-fPIC','-O2','-fwrapv','-DVAR_ARRAYS','-DOPUS_BUILD','-DFIXED_POINT','-DDISABLE_FLOAT_API',
                    *['-I'+str(source/p) for p in ['include','celt','silk','silk/fixed','src']],str(path/'probe.c'),
                    str(path/'celt_decoder.c'),str(path/'opus_decoder.c'),
                    *[str(source/f) for f in files if f not in ['celt/celt_decoder.c','src/opus_decoder.c']],'-lm','-o',str(path/'probe.so')],check=True)
    lib=c.CDLL(str(path/'probe.so'));ptr=c.c_void_p;i16=c.POINTER(c.c_int16);u8=c.POINTER(c.c_ubyte);u32=c.POINTER(c.c_uint32);ints=c.POINTER(c.c_int)
    for name,types,result in [('opus_encoder_create',[c.c_int,c.c_int,c.c_int,ints],ptr),('opus_decoder_create',[c.c_int,c.c_int,ints],ptr),
                              ('opus_encoder_destroy',[ptr],None),('opus_decoder_destroy',[ptr],None),
                              ('configure',[ptr,*[c.c_int]*4],c.c_int),('reset',[ptr],c.c_int),
                              ('opus_encode',[ptr,i16,c.c_int,u8,c.c_int],c.c_int),('decode',[ptr,u8,c.c_int,i16,u32],c.c_int),
                              ('repack',[u8,ints,c.c_int,u8],c.c_int),('opus_packet_pad',[u8,c.c_int,c.c_int],c.c_int),('snapshot',[ptr,ints],None)]:
        fn=getattr(lib,name);fn.argtypes=types;fn.restype=result
    print('# Real CELT-only encoder streams; public fixed-point opus_decode:',REVISION)
    print('# packet RESET RATE API_CHANNELS HEX FINAL_RANGE SAMPLES_PER_CHANNEL PCM[N*C]')
    print('# state ENERGY[42] PREVIOUS[42] PREVIOUS2[42] BACKGROUND[42] DEEMPHASIS[2] POSTFILTER[old/current period,gain,tap] OVERLAP[60*C] HISTORY[1026*C]')
    for rate in [8000,12000,16000,24000,48000]:
        for api in [1,2]:
            for sequence in range(3):
                error=c.c_int();encoder=lib.opus_encoder_create(48000,2,2051,c.byref(error))
                if not encoder or error.value:raise SystemExit('Encoder initialization failed')
                decoder=lib.opus_decoder_create(rate,api,c.byref(error))
                if not decoder or error.value:raise SystemExit('Decoder initialization failed')
                random=0x43454c54+sequence;phase=other=0
                try:
                    for index in range(32):
                        reset=index in [0,16]
                        if reset and lib.reset(decoder):raise SystemExit('Reset failed')
                        if index<24:lm=index//6;count=1
                        else:count=[2,3,4,6,8,12,24,48][index-24];lm=3 if count<=6 else 0
                        n=120<<lm
                        band=[1101,1103,1104,1105][(index//3+sequence)%4] if sequence else 1105
                        channels=1 if index%12 in [0,1,2,7,8] else 2
                        bitrate=[24000,48000,96000,192000][(index//2+sequence)%4]
                        if lib.configure(encoder,band,channels,bitrate,index%2):raise SystemExit('Encoder configuration failed')
                        frames=[]
                        for part in range(count):
                            samples=[]
                            for j in range(n):
                                phase=(phase+1)%239;other=(other+1)%173
                                random^=(random<<13)&0xffffffff;random^=random>>17;random^=(random<<5)&0xffffffff
                                left=((phase if phase<120 else 238-phase)-60)*160+random%1024-512
                                right=((other if other<87 else 172-other)-43)*190
                                if index%7==0:right=left
                                if index in [0,11,12,20,28]:left=right=0
                                if index==29:left=10000 if j<2 else 0;right=-left
                                samples.extend([left,right])
                            data=(c.c_ubyte*1275)();length=lib.opus_encode(encoder,(c.c_int16*len(samples))(*samples),n,data,1275)
                            if length<3:raise SystemExit('Expected normal CELT payload')
                            packet=bytes(data[:length])
                            if packet[0]>>3<16:raise SystemExit('Expected CELT-only packet')
                            frames.append(packet)
                        combined=b''.join(frames);data=(c.c_ubyte*65536)()
                        length=lib.repack((c.c_ubyte*len(combined)).from_buffer_copy(combined),(c.c_int*len(frames))(*map(len,frames)),len(frames),data)
                        if length<0:raise SystemExit('Repacketization failed')
                        if index==31:
                            if lib.opus_packet_pad(data,length,length+257):raise SystemExit('Padding failed')
                            length+=257
                        pcm=(c.c_int16*11520)();final=c.c_uint32();actual=lib.decode(decoder,data,length,pcm,c.byref(final))
                        if actual!=n*count*rate//48000:raise SystemExit('Public CELT decode failed')
                        print('packet',int(reset),rate,api,bytes(data[:length]).hex(),final.value,actual,*pcm[:actual*api])
                        state=(c.c_int32*(176+1086*api))();lib.snapshot(decoder,state)
                        print('state',*state)
                finally:
                    lib.opus_encoder_destroy(encoder);lib.opus_decoder_destroy(decoder)
