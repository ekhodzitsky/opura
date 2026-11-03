#!/usr/bin/env python3
"""Optional original surround stream-policy wire oracle, outside Cargo.

Records original self-delimited TOCs for coupled/LFE policy checks, without
requiring encoder audio bytes to match the original Q24 narrowing behavior.
"""
import argparse
import ctypes as c
from pathlib import Path
import tempfile
from audio_baseline import REVISION, compile_reference
from mapped_audio_fixtures import setup
from multistream_rate_fixtures import DURATIONS


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source',type=Path)
    parser.add_argument('output',type=Path)
    args=parser.parse_args()
    if args.output.exists():raise ValueError('Output must be new')
    rows=0
    with tempfile.TemporaryDirectory(prefix='opura-surround-policy-') as temporary:
        lib=compile_reference(args.source.resolve(),Path(temporary));setup(lib)
        lib.opus_packet_parse_impl.argtypes=[c.c_void_p,c.c_int,c.c_int]+[c.c_void_p]*7
        lib.opus_packet_parse_impl.restype=c.c_int
        with args.output.open('x') as output:
            output.write('# Original surround coupled/LFE wire policy '+REVISION+'; caller requests channels1\n')
            output.write('# case RATE CHANNELS APPLICATION STREAMS COUPLED\n# frame DURATION48 TOCS_UPPER6...\n')
            for rate in [8000,12000,16000,24000,48000]:
                for channels in range(3,9):
                    for application in [2049,2048,2051,2052,2053]:
                        streams,coupled,error=c.c_int(),c.c_int(),c.c_int()
                        mapping=(c.c_ubyte*channels)()
                        owner=lib.opus_multistream_surround_encoder_create(rate,channels,1,c.byref(streams),c.byref(coupled),mapping,application,c.byref(error))
                        if not owner or error.value:raise ValueError('Native factory failed')
                        output.write(f'case {rate} {channels} {application} {streams.value} {coupled.value}\n')
                        try:
                            for request,value in [(4002,32000*channels),(4006,0),(4010,0),(4022,1)]:
                                if lib.opus_multistream_encoder_ctl(owner,request,c.c_int(value)):raise ValueError('Native controls failed')
                            for duration in DURATIONS:
                                if application==2052 and duration<480:continue
                                n=duration*rate//48000
                                values=[(frame*(73+19*channel))%8000-4000 for frame in range(n) for channel in range(channels)]
                                pcm=(c.c_int16*len(values))(*values);data=(c.c_ubyte*8192)()
                                size=lib.opus_multistream_encode(owner,pcm,n,data,len(data))
                                if size<=0:raise ValueError(f'Native policy encode failed:{size}')
                                offset=0;tocs=[]
                                for stream in range(streams.value):
                                    toc=c.c_ubyte();consumed=c.c_int32();sizes=(c.c_int16*48)()
                                    count=lib.opus_packet_parse_impl(c.byref(data,offset),size-offset,int(stream+1<streams.value),c.byref(toc),None,sizes,None,c.byref(consumed),None,None)
                                    if count<1 or consumed.value<=0:raise ValueError('Native framing failed')
                                    offset+=consumed.value;tocs.append(toc.value&252)
                                if offset!=size:raise ValueError('Native parsing did not consume packet')
                                output.write(f'frame {duration} '+' '.join(map(str,tocs))+'\n');rows+=1
                        finally:lib.opus_multistream_encoder_destroy(owner)
            output.write(f'# rows={rows}\n')
    if rows!=1290:raise ValueError('Incomplete policy matrix')
    print(f'Captured{rows}original native surround wire-policy packets',flush=True)


if __name__=='__main__':main()
