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
parser.add_argument('--loss',action='store_true',help='Generate periodic/noise PLC and recovery streams')
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
int decode_loss(OpusDecoder *d,const unsigned char *data,int len,int requested,opus_int16 *pcm,opus_uint32 *range) {
    int count=opus_decode(d,data,len,pcm,requested,0);
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
void opura_celt_loss_snapshot(const CELTDecoder *st,opus_int32 *out) {
    const celt_glog *energy=(const celt_glog*)(st->_decode_mem+(DECODE_BUFFER_SIZE+st->overlap)*st->channels);
    const opus_int16 *lpc=(const opus_int16*)(energy+168);
    out[0]=st->rng;out[1]=st->last_pitch_index;out[2]=st->loss_duration;out[3]=st->plc_duration;
    out[4]=st->skip_plc;out[5]=st->last_frame_type;out[6]=st->prefilter_and_fold;out[7]=st->end;
    for(int c=0;c<st->channels;c++) {
        const celt_sig *memory=st->_decode_mem+c*(DECODE_BUFFER_SIZE+st->overlap);
        for(int i=0;i<24;i++) out[8+c*24+i]=lpc[c*24+i];
        for(int i=0;i<2048;i++) out[8+st->channels*24+c*2048+i]=memory[i];
        for(int i=0;i<120;i++) out[8+st->channels*(24+2048)+c*120+i]=memory[2048+i];
    }
}
''')
    (path/'opus_decoder.c').write_text((source/'src/opus_decoder.c').read_text()+r'''
extern void opura_celt_snapshot(const CELTDecoder *,opus_int32 *);
void snapshot(const OpusDecoder *st,opus_int32 *out) {
    opura_celt_snapshot((const CELTDecoder*)((const char*)st+st->celt_dec_offset),out);
}
extern void opura_celt_loss_snapshot(const CELTDecoder *,opus_int32 *);
void snapshot_loss(const OpusDecoder *st,opus_int32 *out) {
    opura_celt_loss_snapshot((const CELTDecoder*)((const char*)st+st->celt_dec_offset),out);
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
                              ('repack',[u8,ints,c.c_int,u8],c.c_int),('opus_packet_pad',[u8,c.c_int,c.c_int],c.c_int),('snapshot',[ptr,ints],None),
                              ('decode_loss',[ptr,u8,c.c_int,c.c_int,i16,u32],c.c_int),('snapshot_loss',[ptr,ints],None)]:
        fn=getattr(lib,name);fn.argtypes=types;fn.restype=result
    if args.loss:
        print('# Public CELT PLC/DTX/recovery PCM and independent complete state:',REVISION)
        print('# frame KIND RESET(0=keep,1=ctl,2=fresh) RATE API_CHANNELS DURATION48 HEX_OR_DASH FINAL_RANGE N PCM[N*C]')
        print('# state ENERGY[42] PREVIOUS[42] PREVIOUS2[42] BACKGROUND[42] DEEMPHASIS[2] POSTFILTER[6] OVERLAP[60*C] HISTORY[1026*C]')
        print('# plc RNG PITCH LOSS_DURATION PLC_DURATION SKIP FRAME_TYPE PREFILTER END LPC[24*C] HISTORY[2048*C] FUTURE[120*C]')
        for rate in [8000,12000,16000,24000,48000]:
            for api in [1,2]:
                for sequence in range(3):
                    error=c.c_int();encoder=lib.opus_encoder_create(48000,2,2051,c.byref(error));decoder=lib.opus_decoder_create(rate,api,c.byref(error))
                    if not encoder or not decoder or error.value:raise SystemExit('Loss stream initialization failed')
                    random=0x4c4f5353+sequence;phase=other=0
                    def emit(kind,packet,duration,reset=False,repeats=1):
                        if reset and lib.reset(decoder):raise SystemExit('Loss reset failed')
                        pcm=(c.c_int16*11520)();final=c.c_uint32();data=(c.c_ubyte*len(packet)).from_buffer_copy(packet) if packet is not None else None
                        for _ in range(repeats):
                            actual=lib.decode_loss(decoder,data,len(packet) if packet is not None else 0,duration*rate//48000,pcm,c.byref(final))
                        if actual!=duration*rate//48000:raise SystemExit('Public loss decode failed '+kind)
                        print('frame',kind,int(reset),rate,api,duration,packet.hex() if packet is not None else '-',final.value,actual,*pcm[:actual*api])
                        state=(c.c_int32*(176+1086*api))();lib.snapshot(decoder,state);print('state',*state)
                        plc=(c.c_int32*(8+2192*api))();lib.snapshot_loss(decoder,plc);values=list(plc);values[0]=c.c_uint32(values[0]).value;print('plc',*values)
                    try:
                        emit('loss',None,960,2)
                        emit('empty',bytes([0xf8]),960)
                        emit('short',bytes([0x88,0xff]),240)
                        # Long burst crosses the100ms periodic/noise threshold;
                        # one recovery frame keeps noise PLC enabled until the
                        # second consecutive normal frame. Later loss sizes and
                        # implicit headers exercise scheduling/config changes.
                        events=['normal','normal','normal',*['loss']*7,'normal','loss','normal','normal','loss','normal','empty','short','normal','mixed','loss','normal','multi','normal']
                        for index,kind in enumerate(events):
                            lm=sequence if sequence<2 else (index//4)%4
                            if sequence==0:lm=3
                            n=120<<lm
                            band=[1101,1103,1104,1105][(index//3+sequence)%4]
                            channels=1 if index%7 in [0,1,5] else 2
                            if lib.configure(encoder,band,channels,[32000,64000,128000][sequence],index%2):raise SystemExit('Loss encoder configuration failed')
                            frames=[]
                            durations=[n]
                            duration=n
                            if kind=='loss':
                                duration=960 if index<10 else [120,240,360,480,600,840,960,1440,5760][(index+sequence)%9]
                                durations=[];remaining=duration
                                while remaining:
                                    chunk=next(v for v in [960,480,240,120] if v<=remaining);durations.append(chunk);remaining-=chunk
                            for chunk in durations:
                                samples=[]
                                amplitude=[160,120,80,40,20,5,100,180][(index+sequence)%8]
                                for j in range(chunk):
                                    phase=(phase+1)%239;other=(other+1)%173
                                    random^=(random<<13)&0xffffffff;random^=random>>17;random^=(random<<5)&0xffffffff
                                    left=((phase if phase<120 else 238-phase)-60)*amplitude+random%128-64
                                    right=((other if other<87 else 172-other)-43)*amplitude
                                    if sequence==1:right=-left
                                    if index in [2,15,22] and sequence==2:left=right=0
                                    samples.extend([left,right])
                                data=(c.c_ubyte*1275)();length=lib.opus_encode(encoder,(c.c_int16*len(samples))(*samples),chunk,data,1275)
                                if length<3:raise SystemExit('Expected normal CELT frame')
                                packet=bytes(data[:length])
                                if packet[0]>>3<16:raise SystemExit('Expected CELT loss stream')
                                frames.append(packet)
                            if kind=='loss':packet=None
                            elif kind=='empty':packet=bytes([frames[0][0]&0xfc])
                            elif kind=='short':packet=bytes([frames[0][0]&0xfc,0x55])
                            elif kind=='mixed':
                                packets=[bytes([frames[0][0]&0xfc]),frames[0],bytes([frames[0][0]&0xfc,0xff])];combined=b''.join(packets);out=(c.c_ubyte*65536)()
                                length=lib.repack((c.c_ubyte*len(combined)).from_buffer_copy(combined),(c.c_int*3)(*map(len,packets)),3,out)
                                if length<0:raise SystemExit('Mixed repacketization failed')
                                packet=bytes(out[:length]);duration=3*n
                            elif kind=='multi':
                                count=6 if n==960 else 5760//n;packet=bytes([(frames[0][0]&0xfc)|3,count])+bytes([0xff]*count);duration=n*count
                            else:packet=frames[0]
                            emit(kind,packet,duration)
                        if rate==48000 and api==2 and sequence==2:
                            emit('repeat210',None,5760,repeats=210)
                            emit('recovery',frames[-1],n)
                        emit('reset_loss',None,960,reset=True)
                        emit('reset_recovery',frames[-1],n)
                    finally:
                        lib.opus_encoder_destroy(encoder);lib.opus_decoder_destroy(decoder)
        raise SystemExit
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
