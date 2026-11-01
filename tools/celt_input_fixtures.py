#!/usr/bin/env python3
"""Optional original standard signed16 CELT pre-emphasis oracle.

Copies one unchanged original function body into a temporary external probe.
No foreign implementation enters ordinary Cargo builds or tests.
"""
import argparse
import ctypes as c
from pathlib import Path
import subprocess
import tempfile

REVISION='22244de5a79bd1d6d623c32e72bf1954b56235be'


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source',type=Path)
    source=parser.parse_args().source.resolve()
    if subprocess.check_output(['git','-C',str(source),'rev-parse','HEAD'],text=True).strip()!=REVISION:
        raise SystemExit('Pinned original Opus1.6.1 required')
    if subprocess.check_output(['git','-C',str(source),'status','--porcelain'],text=True).strip():
        raise SystemExit('Reference checkout must be clean')
    text=(source/'celt/celt_encoder.c').read_text()
    signature='void celt_preemphasis('
    if text.count(signature)!=1:raise SystemExit('Unexpected original pre-emphasis function')
    start=text.index(signature);end=text.index('{',start)+1;depth=1
    while depth:
        if text[end]=='{':depth+=1
        elif text[end]=='}':depth-=1
        end+=1
    wrapper='''
#include "arch.h"
#include "mathops.h"
#include "os_support.h"
#if !OPUS_FAST_INT64 || SIG_SHIFT != 12 || RES_SHIFT != 0
#error "Canonical signed16 fixed-point fast64 input required"
#endif
'''+text[start:end]+'''
void probe(int n,int factor,int channels,int clip,const opus_int16 *pcm,opus_int32 *output,opus_int32 *memory) {
    const opus_int16 coef[4]={27853,0,4096,8192};
    for(int channel=0;channel<channels;channel++)
        celt_preemphasis(pcm+channel,output+channel*n,n,channels,factor,coef,memory+channel,clip);
}
'''
    with tempfile.TemporaryDirectory(prefix='opura-celt-input-') as temp:
        path=Path(temp);(path/'probe.c').write_text(wrapper)
        subprocess.run(['cc','-shared','-fPIC','-O2','-fwrapv','-DFIXED_POINT','-DOPUS_BUILD','-DDISABLE_FLOAT_API',
                        '-I'+str(source/'include'),'-I'+str(source/'celt'),str(path/'probe.c'),'-o',str(path/'probe.so')],check=True)
        lib=c.CDLL(str(path/'probe.so'))
        lib.probe.argtypes=[c.c_int]*4+[c.POINTER(c.c_int16),c.POINTER(c.c_int32),c.POINTER(c.c_int32)]
        lib.probe.restype=None
        print('# Original standard celt_preemphasis signed16 Q12:',REVISION)
        print('# frame RESET LM FACTOR CHANNELS CLIP PCM[N/factor*channels] PLANAR_SIGNAL[N*channels] MEMORY[2]')
        seed=0x50524545
        def random_i16():
            nonlocal seed
            seed^=(seed<<13)&0xffffffff;seed^=seed>>17;seed^=(seed<<5)&0xffffffff
            return c.c_int16(seed).value
        memory=(c.c_int32*2)()
        def emit(reset,lm,factor,channels,pattern,clip):
            n=120<<lm
            if reset:memory[0]=memory[1]=0
            values=[]
            for i in range(n//factor*channels):
                if pattern==0:value=0
                elif pattern==1:value=32767 if i==0 else 0
                elif pattern==2:value=-32768 if i==n//factor*channels-1 else 0
                elif pattern==3:value=32767
                elif pattern==4:value=-32768
                elif pattern==5:value=-32768 if i%2 else 32767
                elif pattern==6:value=1 if i%2 else -1
                else:value=random_i16()>>(pattern%3)
                values.append(value)
            data=(c.c_int16*len(values))(*values);output=(c.c_int32*(n*channels))()
            lib.probe(n,factor,channels,clip,data,output,memory)
            if list(data)!=values:raise SystemExit('Reference input unexpectedly changed')
            print('frame',int(reset),lm,factor,channels,clip,*values,*output,*memory)
        for lm in range(4):
            for factor in [1,2,3,4,6]:
                for channels in [1,2]:
                    for pattern in range(8):emit(True,lm,factor,channels,pattern,pattern%2)
        for sequence in range(4):
            for index in range(48):
                emit(index in [0,17,33],(index+sequence)%4,[1,2,3,4,6][(index+sequence)%5],
                     1+(index//3+sequence)%2,(index+sequence)%10,index%2)


if __name__=='__main__':main()
