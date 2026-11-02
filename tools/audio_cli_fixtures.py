#!/usr/bin/env python3
"""Small Ogg/PCM audio-command fixtures from unchanged pinned public libopus.

Optional external generator; normal Cargo tests need no reference executable.
Ogg fixture pages use an independent bitwise CRC, not the Rust lookup kernel.
"""
import argparse
import ctypes as c
import json
from pathlib import Path
import re
import struct
import subprocess
import tempfile
REVISION='22244de5a79bd1d6d623c32e72bf1954b56235be'
parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('source',type=Path);parser.add_argument('output',type=Path)
args=parser.parse_args();source=args.source.resolve()
if subprocess.check_output(['git','-C',str(source),'rev-parse','HEAD'],text=True).strip()!=REVISION:raise SystemExit('Expected pinned Opus v1.6.1')
if subprocess.check_output(['git','-C',str(source),'status','--porcelain'],text=True).strip():raise SystemExit('Reference checkout must be clean')
def manifest(file,variable):
    match=re.search(r'^'+variable+r' = (.*?)(?=\n\n|\Z)',(source/file).read_text(),re.M|re.S)
    if match is None:raise SystemExit('Source manifest missing')
    return match[1].replace('\\','').split()
def page(packet,sequence,granule,flags):
    laces=bytes([255]*(len(packet)//255)+[len(packet)%255])
    header=b'OggS'+bytes([0,flags])+struct.pack('<QIII',granule,0x41554449,sequence,0)+bytes([len(laces)])
    data=bytearray(header+laces+packet);crc=0
    for byte in data:
        crc^=byte<<24
        for _ in range(8):crc=((crc<<1)^(0x04c11db7 if crc&0x80000000 else 0))&0xffffffff
    data[22:26]=struct.pack('<I',crc)
    return bytes(data)
wrapper=r'''
#include "opus.h"
#include "opus_private.h"
#include "arch.h"
#if !OPUS_FAST_INT64
#error "Canonical fixed fast64 reference required"
#endif
int configure(OpusEncoder *s,int mode,int channels,int bitrate,int bandwidth,int *lookahead){
    int r=0;
    r|=opus_encoder_ctl(s,OPUS_SET_FORCE_MODE(mode));r|=opus_encoder_ctl(s,OPUS_SET_FORCE_CHANNELS(channels));
    r|=opus_encoder_ctl(s,OPUS_SET_BANDWIDTH(bandwidth));r|=opus_encoder_ctl(s,OPUS_SET_MAX_BANDWIDTH(bandwidth));
    r|=opus_encoder_ctl(s,OPUS_SET_BITRATE(bitrate));r|=opus_encoder_ctl(s,OPUS_SET_VBR(0));
    r|=opus_encoder_ctl(s,OPUS_SET_COMPLEXITY(5));r|=opus_encoder_ctl(s,OPUS_SET_DTX(0));
    r|=opus_encoder_ctl(s,OPUS_GET_LOOKAHEAD(lookahead));return r;
}
'''
files=manifest('silk_sources.mk','SILK_SOURCES')+manifest('silk_sources.mk','SILK_SOURCES_FIXED')+manifest('celt_sources.mk','CELT_SOURCES')+manifest('opus_sources.mk','OPUS_SOURCES')
args.output.mkdir(parents=True,exist_ok=True)
with tempfile.TemporaryDirectory(prefix='opura-audio-fixtures-') as temp:
    path=Path(temp);(path/'probe.c').write_text(wrapper)
    subprocess.run(['cc','-shared','-fPIC','-O2','-fwrapv','-DVAR_ARRAYS','-DOPUS_BUILD','-DFIXED_POINT','-DDISABLE_FLOAT_API',*['-I'+str(source/p) for p in ['include','celt','silk','silk/fixed','src']],str(path/'probe.c'),*[str(source/f) for f in files],'-lm','-o',str(path/'probe.so')],check=True)
    lib=c.CDLL(str(path/'probe.so'));ptr=c.c_void_p;i32=c.POINTER(c.c_int);i16=c.POINTER(c.c_int16);u8=c.POINTER(c.c_ubyte)
    for name,types,result in [('opus_encoder_create',[c.c_int,c.c_int,c.c_int,i32],ptr),('opus_decoder_create',[c.c_int,c.c_int,i32],ptr),('configure',[ptr,c.c_int,c.c_int,c.c_int,c.c_int,i32],c.c_int),('opus_encode',[ptr,i16,c.c_int,u8,c.c_int32],c.c_int32),('opus_decode',[ptr,u8,c.c_int32,i16,c.c_int,c.c_int],c.c_int),('opus_encoder_destroy',[ptr],None),('opus_decoder_destroy',[ptr],None)]:
        fn=getattr(lib,name);fn.argtypes=types;fn.restype=result
    cases=[]
    for name,mode,channels,bitrate,bandwidth,application in [('silk-mono',1000,1,16000,1103,2049),('hybrid-stereo',1001,2,40000,1105,2049),('celt-stereo',1002,2,96000,1105,2051)]:
        error=c.c_int();encoder=lib.opus_encoder_create(48000,channels,application,c.byref(error))
        if not encoder or error.value:raise SystemExit('Original encoder creation failed')
        decoder=lib.opus_decoder_create(48000,channels,c.byref(error))
        if not decoder or error.value:raise SystemExit('Original decoder creation failed')
        try:
            skip=c.c_int()
            if lib.configure(encoder,mode,channels,bitrate,bandwidth,c.byref(skip)):raise SystemExit('Original encoder setup failed')
            count=4663;original=[];random=0x41554449
            for i in range(count):
                for ch in range(channels):
                    random=(1664525*random+1013904223)&0xffffffff
                    value=((i+ch*31)%239-119)*100+(c.c_int32(random).value>>21)
                    original.append(max(-32768,min(32767,value)))
            head=b'OpusHead'+bytes([1,channels])+struct.pack('<HIhB',skip.value,48000,0,0)
            vendor=b'pinned libopus 1.6.1';tags=b'OpusTags'+struct.pack('<I',len(vendor))+vendor+struct.pack('<I',0)
            encoded=bytearray(page(head,0,0,2)+page(tags,1,0,0));decoded=[];modes=[]
            frames=(count+skip.value+959)//960
            for frame in range(frames):
                chunk=original[frame*960*channels:(frame+1)*960*channels];chunk+=[0]*(960*channels-len(chunk))
                data=(c.c_ubyte*1276)();length=lib.opus_encode(encoder,(c.c_int16*len(chunk))(*chunk),960,data,1276)
                if length<1:raise SystemExit('Original encode failed')
                packet=bytes(data[:length]);config=packet[0]>>3;actual=1000 if config<12 else 1001 if config<16 else 1002
                if actual!=mode:raise SystemExit(f'Original forced mode changed: {name,actual}')
                modes.append(actual)
                pcm=(c.c_int16*11520)();samples=lib.opus_decode(decoder,data,length,pcm,5760,0)
                if samples!=960:raise SystemExit('Original decode failed')
                decoded.extend(pcm[:samples*channels])
                eos=frame+1==frames;granule=count+skip.value if eos else (frame+1)*960
                encoded.extend(page(packet,frame+2,granule,4 if eos else 0))
            (args.output/(name+'.opus')).write_bytes(encoded)
            expected=decoded[skip.value*channels:(skip.value+count)*channels]
            (args.output/(name+'.pcm')).write_bytes(struct.pack('<'+'h'*len(expected),*expected))
            cases.append({'name':name,'channels':channels,'samples':count,'pre_skip':skip.value,'packets':frames,'mode':mode,'bitrate':bitrate})
        finally:lib.opus_encoder_destroy(encoder);lib.opus_decoder_destroy(decoder)
    (args.output/'manifest.json').write_text(json.dumps({'reference_revision':REVISION,'sample_rate':48000,'cases':cases},indent=2)+'\n')
