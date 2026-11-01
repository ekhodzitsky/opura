#!/usr/bin/env python3
"""Original classic CELT PLC downsampling/whitening and pitch-search oracle.

Compact deterministic integer signal recipes are fixture inputs; every expected
whitened sample and period comes from unchanged pinned scalar pitch/LPC code.
"""
import argparse
import ctypes as c
from pathlib import Path
import subprocess
import tempfile
REVISION='22244de5a79bd1d6d623c32e72bf1954b56235be'
parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('source',type=Path)
args=parser.parse_args();source=args.source.resolve()
if subprocess.check_output(['git','-C',str(source),'rev-parse','HEAD'],text=True).strip()!=REVISION:raise SystemExit('Expected pinned Opus v1.6.1')
if subprocess.check_output(['git','-C',str(source),'status','--porcelain'],text=True).strip():raise SystemExit('Reference checkout must be clean')
wrapper=r'''
#include "arch.h"
#include "pitch.h"
#if !OPUS_FAST_INT64
#error "Canonical pitch fixtures require scalar fast-64-bit math"
#endif
void analyse(int *left,int *right,int channels,int *out) {
    celt_sig *history[2]={left,right};opus_int16 lp[1024];int index;
    pitch_downsample(history,lp,1024,channels,2,0);
    pitch_search(lp+360,lp,1328,620,&index,0);
    out[0]=720-index;
    for(int i=0;i<1024;i++) out[i+1]=lp[i];
}
'''
def trunc(a,b):return -(abs(a)//b) if a<0 else a//b
def signal(period,shape,amplitude,offset,seed):
    values=[]
    for i in range(2048):
        seed=(1664525*seed+1013904223)&0xffffffff
        phase=(i+offset)%period
        if shape==0:
            triangle=min(phase,period-phase)
            value=trunc(amplitude*(4*triangle-period),period)
        elif shape==1:value=amplitude if phase<period//2 else -amplitude
        elif shape==2:value=amplitude if phase==0 else 0
        elif shape==3:value=trunc(amplitude*(2*phase-period),period)
        elif shape==4:value=(c.c_int32(seed).value*amplitude)>>31
        elif shape==5:value=0
        elif shape==6:value=amplitude
        else:value=amplitude if i%2 else -amplitude
        values.append(value)
    return values
with tempfile.TemporaryDirectory(prefix='opura-celt-pitch-') as temp:
    path=Path(temp);(path/'probe.c').write_text(wrapper)
    subprocess.run(['cc','-shared','-fPIC','-O2','-fwrapv','-DFIXED_POINT','-DVAR_ARRAYS','-DOPUS_BUILD','-DDISABLE_FLOAT_API',
                    '-I'+str(source/'include'),'-I'+str(source/'celt'),str(path/'probe.c'),
                    *[str(source/'celt'/f) for f in ['pitch.c','celt_lpc.c','mathops.c','entcode.c']],'-o',str(path/'probe.so')],check=True)
    lib=c.CDLL(str(path/'probe.so'));ptr=c.POINTER(c.c_int32);lib.analyse.argtypes=[ptr,ptr,c.c_int,ptr];lib.analyse.restype=None
    print('# Original classic PLC pitch pipeline:',REVISION)
    print('# pitch INPUT_PERIOD SHAPE AMPLITUDE CHANNELS SIDE_MODE OFFSET SEED PERIOD WHITENED[1024]')
    def emit(period,shape,amplitude,channels,side,offset,seed):
        left=signal(period,shape,amplitude,offset,seed)
        right=left if side==0 else [-v for v in left] if side==1 else signal(period if side==2 else period//2+37,shape,amplitude,offset+period//3,seed^0x9e3779b9)
        out=(c.c_int32*1025)();lib.analyse((c.c_int32*2048)(*left),(c.c_int32*2048)(*right),channels,out)
        print('pitch',period,shape,amplitude,channels,side,offset,seed,*out)
    for period in range(100,721):
        for channels in [1,2]:
            emit(period,period%4,[1,4096,1<<20,536870911][(period//4)%4],channels,period%4,period//3,0x50495443+period)
    for period in [100,101,119,120,127,128,173,239,255,256,360,511,512,719,720]:
        for shape in range(8):
            for channels in [1,2]:
                for side in range(4):
                    emit(period,shape,[0,1,31,4095,4096,1<<20,1<<28,536870911][shape],channels,side,period//2,0x434f4e43+period+shape)
