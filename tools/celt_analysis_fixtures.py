#!/usr/bin/env python3
"""Optional original CELT channel/subframe MDCT oracle, never a Cargo dependency."""
import argparse
import ctypes as c
from pathlib import Path
import subprocess
import tempfile

REVISION='22244de5a79bd1d6d623c32e72bf1954b56235be'


def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('source',type=Path)
    source=parser.parse_args().source.resolve()
    if subprocess.check_output(['git','-C',str(source),'rev-parse','HEAD'],text=True).strip()!=REVISION:
        raise SystemExit('Expected pinned Opus1.6.1')
    if subprocess.check_output(['git','-C',str(source),'status','--porcelain'],text=True).strip():
        raise SystemExit('Reference checkout must be clean')
    text=(source/'celt/celt_encoder.c').read_text();signature='static void compute_mdcts('
    if text.count(signature)!=1:raise SystemExit('Unexpected original compute_mdcts signature')
    start=text.index(signature);end=text.index('{',start)+1;depth=1
    while depth:
        if text[end]=='{':depth+=1
        elif text[end]=='}':depth-=1
        end+=1
    wrapper='''
#include "arch.h"
#include "mdct.h"
#include "modes.h"
#include "os_support.h"
extern const opus_int16 eband5ms[];
extern const unsigned char band_allocation[];
#include "static_modes_fixed.h"
#if !OPUS_FAST_INT64
#error "Canonical scalar fixed-point fast64 required"
#endif
'''+text[start:end]+'''
void probe(int lm,int factor,int input_channels,int coded_channels,int short_blocks,opus_int32 *input,opus_int32 *output) {
    CELTMode mode={0};mode.overlap=120;mode.shortMdctSize=120;mode.maxLM=3;mode.window=window120;
    mode.mdct.n=1920;mode.mdct.maxshift=3;mode.mdct.trig=mdct_twiddles960;
    mode.mdct.kfft[0]=&fft_state48000_960_0;mode.mdct.kfft[1]=&fft_state48000_960_1;
    mode.mdct.kfft[2]=&fft_state48000_960_2;mode.mdct.kfft[3]=&fft_state48000_960_3;
    compute_mdcts(&mode,short_blocks?(1<<lm):0,input,output,coded_channels,input_channels,lm,factor,0);
}
'''
    with tempfile.TemporaryDirectory(prefix='opura-celt-analysis-') as temp:
        path=Path(temp);(path/'probe.c').write_text(wrapper)
        subprocess.run(['cc','-shared','-fPIC','-O2','-fwrapv','-DFIXED_POINT','-DVAR_ARRAYS','-DOPUS_BUILD','-DDISABLE_FLOAT_API',
                        '-I'+str(source/'include'),'-I'+str(source/'celt'),str(path/'probe.c'),str(source/'celt/mdct.c'),
                        str(source/'celt/kiss_fft.c'),'-o',str(path/'probe.so')],check=True)
        lib=c.CDLL(str(path/'probe.so'));i32=c.POINTER(c.c_int32)
        lib.probe.argtypes=[c.c_int]*5+[i32]*2;lib.probe.restype=None
        print('# Original compute_mdcts/clt_mdct_forward_c standard fixed-point:',REVISION)
        print('# frame LM FACTOR INPUT_CHANNELS CODED_CHANNELS SHORT INPUT[...] OUTPUT[CODED_CHANNELS*(120<<LM)]')
        seed=0x414e414c
        def random_i32():
            nonlocal seed
            seed^=(seed<<13)&0xffffffff;seed^=seed>>17;seed^=(seed<<5)&0xffffffff
            return c.c_int32(seed).value
        for lm in range(4):
            n=120<<lm
            for factor in [1,2,3,4,6]:
                for cc,channels in [(1,1),(2,1),(2,2)]:
                    for short in [0,1]:
                        for pattern in range(9):
                            values=[]
                            for channel in range(cc):
                                for i in range(n+120):
                                    if pattern==0:value=0
                                    elif pattern==1:value=-(1<<26) if i==60+channel else 0
                                    elif pattern==2:value=0x7fffffff
                                    elif pattern==3:value=-0x80000000
                                    elif pattern==4:value=0x7fffffff if (i+channel)%2 else -0x80000000
                                    elif pattern in [5,6,7]:value=random_i32()>>[0,8,24][pattern-5]
                                    else:value=3 if channel else -1
                                    values.append(value)
                            data=(c.c_int32*len(values))(*values);output=(c.c_int32*(n*cc))()
                            lib.probe(lm,factor,cc,channels,short,data,output)
                            if list(data)!=values:raise SystemExit('Unexpected original input mutation')
                            print('frame',lm,factor,cc,channels,short,*values,*output[:n*channels])


if __name__=='__main__':main()
