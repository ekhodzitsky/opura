#!/usr/bin/env python3
"""Optional pinned native self-delimited framing byte/consumed-offset oracle.

Uses original internal framing routines in a temporary external reference build;
no foreign sources enter Opura or Cargo. Run only outside timed benchmarks.
"""
import argparse
import ctypes as c
from pathlib import Path
import tempfile
from audio_baseline import REVISION, compile_reference


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source', type=Path)
    parser.add_argument('output', type=Path)
    args = parser.parse_args()
    boundaries = [0,1,251,252,253,254,255,256,1274,1275]
    rows = 0
    with tempfile.TemporaryDirectory(prefix='opura-multistream-framing-') as temp:
        lib = compile_reference(args.source.resolve(), Path(temp))
        lib.opus_repacketizer_create.argtypes = []
        lib.opus_repacketizer_create.restype = c.c_void_p
        lib.opus_repacketizer_destroy.argtypes = [c.c_void_p]
        lib.opus_repacketizer_cat.argtypes = [c.c_void_p,c.c_void_p,c.c_int]
        lib.opus_repacketizer_cat.restype = c.c_int
        lib.opus_repacketizer_out_range_impl.argtypes = [c.c_void_p,c.c_int,c.c_int,c.c_void_p,c.c_int,c.c_int,c.c_int,c.c_void_p,c.c_int]
        lib.opus_repacketizer_out_range_impl.restype = c.c_int
        lib.opus_packet_parse_impl.argtypes = [c.c_void_p,c.c_int,c.c_int,c.c_void_p,c.c_void_p,c.c_void_p,c.c_void_p,c.c_void_p,c.c_void_p,c.c_void_p]
        lib.opus_packet_parse_impl.restype = c.c_int
        lib.opus_packet_get_samples_per_frame.argtypes = [c.c_void_p,c.c_int]
        lib.opus_packet_get_samples_per_frame.restype = c.c_int
        with args.output.open('x') as out:
            out.write('# Original native self-delimited framing '+REVISION+'\n')
            out.write('# TOC COUNT PATTERN EXTRA LENGTH FNV1A64 CONSUMED\n')
            for config in range(32):
                for channels in [1,2]:
                    toc = config*8+(channels-1)*4
                    probe = (c.c_ubyte*1)(toc)
                    maximum = 5760//lib.opus_packet_get_samples_per_frame(probe,48000)
                    for count in sorted(set([1,min(2,maximum),min(3,maximum),maximum])):
                        for pattern in range(8):
                            rp = lib.opus_repacketizer_create()
                            if not rp: raise ValueError('Native repacketizer allocation failed')
                            try:
                                packets = []
                                for frame in range(count):
                                    length = ([0,1,251,252,1275][pattern] if pattern<5 else
                                              [0,1275][frame%2] if pattern==5 else
                                              boundaries[(frame+config)%len(boundaries)] if pattern==6 else
                                              (frame*17+toc)%1276)
                                    payload = bytes((i*37+frame*19+toc*3)&255 for i in range(length))
                                    packet = (c.c_ubyte*(length+1)).from_buffer_copy(bytes([toc])+payload)
                                    packets.append(packet)
                                    if lib.opus_repacketizer_cat(rp,packet,length+1):
                                        raise ValueError('Native packet append failed')
                                result = (c.c_ubyte*65536)()
                                n = lib.opus_repacketizer_out_range_impl(rp,0,count,result,len(result),1,0,None,0)
                                if n<=0: raise ValueError('Native self-delimited output failed')
                                for extra in [0,1,2,254,255,256,511]:
                                    length = lib.opus_repacketizer_out_range_impl(rp,0,count,result,n+extra,1,int(extra>0),None,0)
                                    if length!=n+extra: raise ValueError('Native padded output failed')
                                    result[length] = 0xe0
                                    result[length+1] = 99
                                    sizes = (c.c_int16*48)()
                                    consumed = c.c_int32()
                                    parsed = lib.opus_packet_parse_impl(result,length+2,1,None,None,sizes,None,c.byref(consumed),None,None)
                                    if parsed!=count or consumed.value!=length:
                                        raise ValueError('Native parser disagrees with native output')
                                    expected_sizes = [len(p)-1 for p in packets]
                                    if list(sizes[:count])!=expected_sizes:
                                        raise ValueError('Native frame sizes changed')
                                    fingerprint = 14695981039346656037
                                    for byte in result[:length]:
                                        fingerprint = ((fingerprint^byte)*1099511628211)&0xffffffffffffffff
                                    out.write(f'{toc} {count} {pattern} {extra} {length} {fingerprint} {consumed.value}\n')
                                    rows += 1
                            finally:
                                lib.opus_repacketizer_destroy(rp)
            out.write(f'# rows={rows}\n')
    print(f'Captured {rows} native self-delimited framing/consumed-offset results')


if __name__ == '__main__':
    main()
