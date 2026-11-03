#!/usr/bin/env python3
"""Optional original native generic multistream bitrate-allocation oracle.

Includes the pristine external translation unit in a temporary wrapper solely
for observing its internal allocation and normalized total. No native code is
linked into Cargo, copied into the repository, or used by ordinary tests.
"""
import argparse
import ctypes as c
from pathlib import Path
import re
import subprocess
import tempfile
from audio_baseline import CFLAGS,REVISION,run

LAYOUTS = [(1,0,[0]),(1,1,[0,1]),(2,1,[2,0,1,255,0]),
           (2,0,[0,1,1,255]),(5,3,[0,6,1,2,3,4,5,7]),(3,0,[2,1,0]),
           (255,0,list(range(255))),(127,127,list(range(254))+[255])]
DURATIONS = [120,240,480,960,1920,2880,3840,4800,5760]


def compile_oracle(source,directory):
    if run(['git','-C',source,'rev-parse','HEAD']).strip()!=REVISION:
        raise ValueError('Expected pinned original native revision')
    if run(['git','-C',source,'status','--porcelain']).strip():
        raise ValueError('Native reference checkout must be unchanged')
    def manifest(file,name):
        match=re.search(r'^'+name+r' = (.*?)(?=\n\n|\Z)',(source/file).read_text(),re.M|re.S)
        if match is None: raise ValueError('Missing source manifest '+name)
        return match[1].replace('\\','').split()
    files=manifest('silk_sources.mk','SILK_SOURCES')+manifest('silk_sources.mk','SILK_SOURCES_FIXED')+manifest('celt_sources.mk','CELT_SOURCES')+manifest('opus_sources.mk','OPUS_SOURCES')
    wrapper=directory/'rates.c'
    wrapper.write_text('''#include "opus_multistream_encoder.c"
#if !OPUS_FAST_INT64
#error "Expected original fixed fast64 arithmetic"
#endif
int opura_ms_rates(OpusMSEncoder *st,int frame_size,opus_int32 *rates) {
 return rate_allocation(st,rates,frame_size);
}
int opura_ms_total(OpusMSEncoder *st) { return st->bitrate_bps; }
''')
    library=directory/'rates.so'
    includes=['-I'+str(source/p) for p in ['include','celt','silk','silk/fixed','src']]
    subprocess.run(['cc',*CFLAGS,*includes,str(wrapper),*[str(source/f) for f in files if f!='src/opus_multistream_encoder.c'],'-lm','-o',str(library)],check=True)
    return c.CDLL(str(library))


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source',type=Path)
    parser.add_argument('output',type=Path)
    args=parser.parse_args()
    rows=0
    with tempfile.TemporaryDirectory(prefix='opura-multistream-rate-') as temp:
        lib=compile_oracle(args.source.resolve(),Path(temp))
        lib.opus_multistream_encoder_create.argtypes=[c.c_int,c.c_int,c.c_int,c.c_int,c.c_void_p,c.c_int,c.c_void_p]
        lib.opus_multistream_encoder_create.restype=c.c_void_p
        lib.opus_multistream_encoder_ctl.argtypes=[c.c_void_p,c.c_int]
        lib.opus_multistream_encoder_ctl.restype=c.c_int
        lib.opus_multistream_encoder_destroy.argtypes=[c.c_void_p]
        lib.opura_ms_total.argtypes=[c.c_void_p]
        lib.opura_ms_total.restype=c.c_int
        lib.opura_ms_rates.argtypes=[c.c_void_p,c.c_int,c.c_void_p]
        lib.opura_ms_rates.restype=c.c_int
        with args.output.open('x') as out:
            out.write('# Original generic multistream rate allocation '+REVISION+'\n')
            out.write('# layout RATE STREAMS COUPLED MAP(comma-separated)\n')
            out.write('# rates DURATION48 REQUESTED NORMALIZED SUM STREAM_RATES...\n')
            for rate in [8000,12000,16000,24000,48000]:
                for streams,coupled,mapping in LAYOUTS:
                    native_map=(c.c_ubyte*len(mapping))(*mapping)
                    error=c.c_int()
                    owner=lib.opus_multistream_encoder_create(rate,len(mapping),streams,coupled,native_map,2049,c.byref(error))
                    if not owner or error.value:raise ValueError('Native encoder creation failed')
                    try:
                        out.write(f'layout {rate} {streams} {coupled} '+','.join(map(str,mapping))+'\n')
                        for duration in DURATIONS:
                            for requested in [1,499,500,64000,96000,510400,1500000,750000*len(mapping),191250000]:
                                if lib.opus_multistream_encoder_ctl(owner,4002,c.c_int(requested)):
                                    raise ValueError('Native bitrate setting failed')
                                rates=(c.c_int32*streams)()
                                total=lib.opura_ms_rates(owner,duration*rate//48000,rates)
                                normalized=lib.opura_ms_total(owner)
                                if total!=sum(rates) or any(n<500 for n in rates):
                                    raise ValueError('Invalid native allocation result')
                                out.write(f'rates {duration} {requested} {normalized} {total} '+' '.join(map(str,rates))+'\n')
                                rows+=1
                    finally:
                        lib.opus_multistream_encoder_destroy(owner)
            out.write(f'# rows={rows}\n')
    print(f'Captured {rows} original native generic multistream allocation cases')


if __name__=='__main__':main()
