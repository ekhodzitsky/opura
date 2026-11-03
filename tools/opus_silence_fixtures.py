#!/usr/bin/env python3
"""Optional unchanged native AUDIO CELT silence/resume packet oracle."""
import argparse
import ctypes as c
from pathlib import Path
import re
import subprocess
import tempfile
from audio_baseline import REVISION
from opus_pcm_fixtures import pcm
from opus_stream_input_fixtures import WRAPPER, CELT_WRAPPER

WRAPPER += r'''
#include "entdec.h"
int silence_probe(OpusEncoder *s,const opus_int16 *pcm,int n,int wire,unsigned char *packet,opus_uint32 *range,int *quiet) {
 if(opus_encoder_ctl(s,OPUS_SET_FORCE_MODE(MODE_CELT_ONLY)) ||
    opus_encoder_ctl(s,OPUS_SET_BANDWIDTH(OPUS_BANDWIDTH_FULLBAND)) ||
    opus_encoder_ctl(s,OPUS_SET_FORCE_CHANNELS(wire)))return -1;
 call_trace_reset();
 int bytes=opus_encode(s,pcm,n,packet,640);
 if(bytes<3)return bytes;
 ec_dec dec;ec_dec_init(&dec,packet+1,bytes-1);*quiet=ec_dec_bit_logp(&dec,15);
 if(opus_encoder_ctl(s,OPUS_GET_FINAL_RANGE(range)))return -1;
 return bytes;
}
'''
def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source', type=Path)
    parser.add_argument('output', type=Path)
    args = parser.parse_args()
    source = args.source.resolve()
    if subprocess.check_output(['git', '-C', str(source), 'rev-parse', 'HEAD'], text=True).strip() != REVISION:
        raise ValueError('Expected pinned Opus v1.6.1')
    if subprocess.check_output(['git', '-C', str(source), 'status', '--porcelain'], text=True).strip():
        raise ValueError('Reference checkout must be unchanged')

    def manifest(file, variable):
        match = re.search(r'^'+variable+r' = (.*?)(?=\n\n|\Z)', (source/file).read_text(), re.M | re.S)
        if match is None:
            raise ValueError('Missing native source manifest')
        return match[1].replace('\\', '').split()

    files = (manifest('silk_sources.mk', 'SILK_SOURCES') + manifest('silk_sources.mk', 'SILK_SOURCES_FIXED') +
             manifest('celt_sources.mk', 'CELT_SOURCES') + manifest('opus_sources.mk', 'OPUS_SOURCES'))
    with tempfile.TemporaryDirectory(prefix='opura-stream-input-') as temp:
        folder = Path(temp)
        (folder/'probe.c').write_text(WRAPPER)
        (folder/'transform.c').write_text(CELT_WRAPPER)
        subprocess.run(['cc', '-shared', '-fPIC', '-O2', '-fwrapv', '-DVAR_ARRAYS', '-DOPUS_BUILD',
                        '-DFIXED_POINT', '-DDISABLE_FLOAT_API',
                        *['-I'+str(source/p) for p in ['include', 'celt', 'silk', 'silk/fixed', 'src']],
                        str(folder/'probe.c'), str(folder/'transform.c'), *[str(source/f) for f in files if f not in ['src/opus_encoder.c','celt/celt_encoder.c']],
                        '-Wl,--wrap=silk_encode_do_VAD_FIX', '-Wl,--wrap=silk_InitEncoder', '-Wl,--wrap=celt_encode_with_ec',
                        '-lm', '-o', str(folder/'probe.so')], check=True)
        lib=c.CDLL(str(folder/'probe.so'))
        lib.stream_new.argtypes=[c.c_int]*3
        lib.stream_new.restype=c.c_void_p
        lib.silence_probe.argtypes=[c.c_void_p,c.POINTER(c.c_int16),c.c_int,c.c_int,c.POINTER(c.c_ubyte),c.POINTER(c.c_uint32),c.POINTER(c.c_int)]
        lib.silence_probe.restype=c.c_int
        lib.opus_encoder_destroy.argtypes=[c.c_void_p]
        rows=['# Original public AUDIO forced CELT silence/resume '+REVISION,
              '# FS API VBR SAMPLES48 INDEX WIRE SEED PATTERN SILENCE RANGE PACKET_HEX']
        for rate in [8000,12000,16000,24000,48000]:
            for channels in [1,2]:
                for vbr in [0,1]:
                    for samples48 in [120,240,480,960]:
                        state=lib.stream_new(rate,channels,vbr)
                        if not state:raise ValueError('Native encoder init failed')
                        try:
                            for i in range(32):
                                wire=1+(channels-1)*(i//8%2)
                                pattern=3 if 8<=i<12 or 24<=i<28 else 0
                                seed=(0x73696c65+i*982451653+rate*samples48)&0xffffffff
                                n=rate*samples48//48000
                                values=(c.c_int16*(n*channels))(*pcm(seed,n*channels,pattern))
                                packet=(c.c_ubyte*1276)();rng=c.c_uint32();quiet=c.c_int()
                                length=lib.silence_probe(state,values,n,wire,packet,c.byref(rng),c.byref(quiet))
                                if length<3:raise ValueError(f'Native encode failed {rate}/{channels}/{vbr}/{samples48}/{i}: {length}')
                                rows.append(' '.join(map(str,[rate,channels,vbr,samples48,i,wire,seed,pattern,quiet.value,rng.value,bytes(packet[:length]).hex()])))
                        finally:lib.opus_encoder_destroy(state)
        args.output.write_text('\n'.join(rows)+'\n')
        print('Captured',len(rows)-2,'native silence/resume packets')


if __name__ == '__main__':
    main()
