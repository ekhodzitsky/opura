#!/usr/bin/env python3
"""Optional original native multistream PCM/range/gain oracle; outside Cargo.

Generate only after benchmark jobs terminate. Inputs are deterministic integer
signals; expected output is obtained from the unchanged public native decoder.
"""
import argparse
import ctypes as c
from pathlib import Path
import struct
import tempfile
from audio_baseline import REVISION, compile_reference

LAYOUTS = [(1,0,[0]),(1,1,[0,1]),(2,1,[0,1,2]),
           (2,1,[2,0,1,255,0]),(2,0,[255]),
           (5,3,[0,6,1,2,3,4,5,7]),(3,0,[2,0,2,255]),(2,1,[1])]


def checked(result):
    if result < 0: raise ValueError(f'Native multistream failure {result}')
    return result


def fingerprint(samples):
    result = 14695981039346656037
    for value in samples:
        for byte in struct.pack('<h',value):
            result = ((result^byte)*1099511628211)&0xffffffffffffffff
    return result


def packets(lib, streams, coupled):
    coded = streams+coupled
    mapping = (c.c_ubyte*coded)(*range(coded))
    error = c.c_int()
    owner = lib.opus_multistream_encoder_create(48000,coded,streams,coupled,mapping,2048,c.byref(error))
    checked(error.value)
    if not owner: raise ValueError('Native encoder allocation failed')
    result = []
    try:
        for request,value in [(4010,10),(4006,0),(4014,20),(4012,1),(4024,3001)]:
            checked(lib.opus_multistream_encoder_ctl(owner,request,c.c_int(value)))
        for index in range(12):
            if index==8:
                # Active FEC/loss deliberately keeps native VoIP in SILK even
                # at high rates. Disable it for actual later CELT transitions.
                for request,value in [(4012,0),(4014,0),(4024,3002)]:
                    checked(lib.opus_multistream_encoder_ctl(owner,request,c.c_int(value)))
            bitrate = (24000 if index<4 else 32000 if index<8 else 128000)*coded
            checked(lib.opus_multistream_encoder_ctl(owner,4002,c.c_int(bitrate)))
            checked(lib.opus_multistream_encoder_ctl(owner,4008,c.c_int(1103 if index<4 else 1105)))
            pcm = (c.c_int16*(960*coded))()
            seed = 6716+index
            for frame in range(960):
                for channel in range(coded):
                    seed = (1664525*seed+1013904223)&0xffffffff
                    phase = (index*960+frame+channel*37)%240
                    triangle = phase if phase<120 else 240-phase
                    pcm[frame*coded+channel] = (triangle-60)*(110-channel*3)+((seed>>24)-128)*3
            out = (c.c_ubyte*65536)()
            n = checked(lib.opus_multistream_encode(owner,pcm,960,out,len(out)))
            if n==0: raise ValueError('Native encoder emitted no packet')
            result.append(bytes(out[:n]))
    finally:
        lib.opus_multistream_encoder_destroy(owner)
    return result



def features(lib, data, streams):
    """Observe original native TOCs/LBRR from independently parsed subpackets."""
    source = (c.c_ubyte*len(data)).from_buffer_copy(data)
    offset = 0
    modes = [0,0,0]
    backups = 0
    for stream in range(streams):
        frames = (c.c_void_p*48)()
        sizes = (c.c_int16*48)()
        toc = c.c_ubyte()
        consumed = c.c_int32()
        count = checked(lib.opus_packet_parse_impl(c.byref(source,offset),len(data)-offset,int(stream+1<streams),c.byref(toc),frames,sizes,None,c.byref(consumed),None,None))
        if count<1 or consumed.value<=0: raise ValueError('Invalid native subpacket geometry')
        modes[2 if toc.value&128 else 1 if toc.value&96==96 else 0] += 1
        # Public LBRR inspection takes ordinary framing. The independent native
        # parser supplies the exact first frame; its SILK internal duration and
        # all header flags remain intact in this single-frame wrapper.
        first = bytes([toc.value&252])+c.string_at(frames[0],sizes[0])
        packet = (c.c_ubyte*len(first)).from_buffer_copy(first)
        backups += int(checked(lib.opus_packet_has_lbrr(packet,len(first)))!=0)
        offset += consumed.value
    if offset!=len(data): raise ValueError('Unconsumed native packet bytes')
    return modes,backups


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source',type=Path)
    parser.add_argument('output',type=Path)
    args = parser.parse_args()
    rows = 0
    with tempfile.TemporaryDirectory(prefix='opura-multistream-decoder-') as temp:
        lib = compile_reference(args.source.resolve(),Path(temp))
        lib.opus_multistream_encoder_create.argtypes = [c.c_int,c.c_int,c.c_int,c.c_int,c.c_void_p,c.c_int,c.c_void_p]
        lib.opus_multistream_encoder_create.restype = c.c_void_p
        lib.opus_multistream_decoder_create.argtypes = [c.c_int,c.c_int,c.c_int,c.c_int,c.c_void_p,c.c_void_p]
        lib.opus_multistream_decoder_create.restype = c.c_void_p
        for name in ['encoder','decoder']:
            getattr(lib,'opus_multistream_'+name+'_destroy').argtypes = [c.c_void_p]
            getattr(lib,'opus_multistream_'+name+'_ctl').argtypes = [c.c_void_p,c.c_int]
            getattr(lib,'opus_multistream_'+name+'_ctl').restype = c.c_int
        lib.opus_multistream_encode.argtypes = [c.c_void_p,c.c_void_p,c.c_int,c.c_void_p,c.c_int]
        lib.opus_multistream_encode.restype = c.c_int
        lib.opus_multistream_decode.argtypes = [c.c_void_p,c.c_void_p,c.c_int,c.c_void_p,c.c_int,c.c_int]
        lib.opus_multistream_decode.restype = c.c_int
        lib.opus_packet_parse_impl.argtypes = [c.c_void_p,c.c_int,c.c_int,c.c_void_p,c.c_void_p,c.c_void_p,c.c_void_p,c.c_void_p,c.c_void_p,c.c_void_p]
        lib.opus_packet_parse_impl.restype = c.c_int
        lib.opus_packet_has_lbrr.argtypes = [c.c_void_p,c.c_int]
        lib.opus_packet_has_lbrr.restype = c.c_int
        modes = [0,0,0]
        fec_backups = 0
        with args.output.open('x') as out:
            out.write('# Original public multistream PCM/range/gain '+REVISION+'\n')
            out.write('# case RATE STREAMS COUPLED MAP(comma-separated)\n')
            out.write('# frame KIND RESET GAIN_Q8 DURATION48 HEX(-=loss) RANGE SAMPLES_PER_CHANNEL PCM16LE_FNV1A64\n')
            for streams,coupled,mapping in LAYOUTS:
                encoded = packets(lib,streams,coupled)
                for index,data in enumerate(encoded):
                    observed,backups = features(lib,data,streams)
                    modes = [a+b for a,b in zip(modes,observed)]
                    if index in [2,6]: fec_backups += backups
                for rate in [8000,12000,16000,24000,48000]:
                    native_map = (c.c_ubyte*len(mapping))(*mapping)
                    error = c.c_int()
                    owner = lib.opus_multistream_decoder_create(rate,len(mapping),streams,coupled,native_map,c.byref(error))
                    checked(error.value)
                    if not owner: raise ValueError('Native decoder allocation failed')
                    try:
                        out.write(f'case {rate} {streams} {coupled} '+','.join(map(str,mapping))+'\n')
                        gains = [0,1536,1536,-1536,-1,1,32767,-32768,0,2048,-512,0]
                        for index,data in enumerate(encoded):
                            reset = int(index==9)
                            gain = gains[index]
                            checked(lib.opus_multistream_decoder_ctl(owner,4034,c.c_int(gain)))
                            if reset: checked(lib.opus_multistream_decoder_ctl(owner,4028))
                            operations = ['plc'] if index in [1,5] else ['fec','normal'] if index in [2,6] else ['normal']
                            for kind in operations:
                                duration = 120 if index==5 else 960
                                packet = None if kind=='plc' else (c.c_ubyte*len(data)).from_buffer_copy(data)
                                pcm = (c.c_int16*(5760*len(mapping)))()
                                n = checked(lib.opus_multistream_decode(owner,packet,0 if packet is None else len(data),pcm,duration*rate//48000,int(kind=='fec')))
                                if n!=duration*rate//48000: raise ValueError('Native output duration differs')
                                final_range = c.c_uint32()
                                checked(lib.opus_multistream_decoder_ctl(owner,4031,c.byref(final_range)))
                                digest = fingerprint(pcm[:n*len(mapping)])
                                out.write(f'frame {kind} {reset} {gain} {duration} '+('-' if packet is None else data.hex())+f' {final_range.value} {n} {digest}\n')
                                rows += 1
                    finally:
                        lib.opus_multistream_decoder_destroy(owner)
            if not all(modes) or fec_backups==0:
                raise ValueError(f'Fixture lacks actual SILK/Hybrid/CELT or FEC backup coverage: modes={modes}, fec_step_lbrr_streams={fec_backups}')
            out.write(f'# native_mode_packets={modes} fec_step_lbrr_streams={fec_backups}\n')
            out.write(f'# rows={rows}\n')
    print(f'Captured {rows} public native multistream PCM/range/gain operations')


if __name__ == '__main__':
    main()
