#!/usr/bin/env python3
"""Optional original CELT band analysis and refined scalar math oracle."""
import argparse
import ctypes as c
from pathlib import Path
import subprocess
import tempfile

REVISION='22244de5a79bd1d6d623c32e72bf1954b56235be'


def function(text,signature):
    if text.count(signature)!=1:raise SystemExit('Unexpected original function '+signature)
    start=text.index(signature);end=text.index('{',start)+1;depth=1
    while depth:
        if text[end]=='{':depth+=1
        elif text[end]=='}':depth-=1
        end+=1
    return text[start:end]


def array(text,signature):
    start=text.index(signature);return text[start:text.index(';',start)+1]


def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('source',type=Path)
    parser.add_argument('--math',action='store_true');args=parser.parse_args();source=args.source.resolve()
    if subprocess.check_output(['git','-C',str(source),'rev-parse','HEAD'],text=True).strip()!=REVISION:
        raise SystemExit('Expected pinned Opus1.6.1')
    if subprocess.check_output(['git','-C',str(source),'status','--porcelain'],text=True).strip():
        raise SystemExit('Reference checkout must be clean')
    bands=(source/'celt/bands.c').read_text().split('#else /* FIXED_POINT */')[0]
    quant=(source/'celt/quant_bands.c').read_text()
    wrapper='''
#include "arch.h"
#include "mathops.h"
#include "modes.h"
#include "quant_bands.h"
#include "os_support.h"
#if !OPUS_FAST_INT64 || DB_SHIFT != 24 || NORM_SHIFT != 24
#error "Canonical scalar fixed-point fast64 Q24 required"
#endif
'''+array((source/'celt/modes.c').read_text(),'static const opus_int16 eband5ms[]')+'\n'+array(quant,'const signed char eMeans[25]')+'''
extern const unsigned char band_allocation[];
#include "static_modes_fixed.h"
'''+function(bands,'void compute_band_energies(')+'\n'+function(bands,'void normalise_bands(')+'\n'+function(quant,'void amp2Log2(')+'''
void probe(int lm,int channels,int effective_end,int end,const opus_int32 *spectrum,opus_int32 *energy,opus_int32 *normalized,opus_int32 *logs) {
    CELTMode mode={0};mode.shortMdctSize=120;mode.nbEBands=21;mode.eBands=eband5ms;mode.logN=logN400;
    compute_band_energies(&mode,spectrum,energy,effective_end,channels,lm,0);
    normalise_bands(&mode,spectrum,normalized,energy,effective_end,channels,1<<lm);
    amp2Log2(&mode,effective_end,end,energy,logs,channels);
}
int scalar(int op,int value) {return op==0?celt_sqrt32(value):celt_rcp_norm32(value);}
'''
    with tempfile.TemporaryDirectory(prefix='opura-celt-band-analysis-') as temp:
        path=Path(temp);(path/'probe.c').write_text(wrapper)
        subprocess.run(['cc','-shared','-fPIC','-O2','-fwrapv','-DFIXED_POINT','-DVAR_ARRAYS','-DOPUS_BUILD','-DDISABLE_FLOAT_API',
                        '-I'+str(source/'include'),'-I'+str(source/'celt'),str(path/'probe.c'),str(source/'celt/mathops.c'),
                        str(source/'celt/entcode.c'),'-o',str(path/'probe.so')],check=True)
        lib=c.CDLL(str(path/'probe.so'));i32=c.POINTER(c.c_int32)
        lib.probe.argtypes=[c.c_int]*4+[i32]*4;lib.probe.restype=None
        lib.scalar.argtypes=[c.c_int,c.c_int];lib.scalar.restype=c.c_int
        seed=0x42414e44
        def random_i32():
            nonlocal seed
            seed^=(seed<<13)&0xffffffff;seed^=seed>>17;seed^=(seed<<5)&0xffffffff
            return c.c_int32(seed).value
        if args.math:
            print('# Original celt_sqrt32/celt_rcp_norm32:',REVISION)
            print('# block OP START STEP COUNT FNV1A64(le-i32); case OP INPUT RESULT;0=sqrt32,1=rcp_norm32')
            for op,start,step,count in [(0,0,1,65536),(1,1<<30,32768,32768)]:
                for block in range(0,count,1024):
                    first=start+block*step;fingerprint=14695981039346656037
                    for value in range(first,first+1024*step,step):
                        result=lib.scalar(op,value)
                        for byte in (result&0xffffffff).to_bytes(4,'little'):
                            fingerprint=((fingerprint^byte)*1099511628211)&0xffffffffffffffff
                    print('block',op,first,step,1024,fingerprint)
            values={0,1,2,0x3fffffff,0x40000000,0x40000001,0x7fffffff}
            for bit in range(31):values.update(v for v in range((1<<bit)-3,(1<<bit)+4) if 0<=v<=0x7fffffff)
            for _ in range(4096):values.add(random_i32()&0x7fffffff)
            for value in sorted(values):
                print('case',0,value,lib.scalar(0,value))
                if value>=1<<30:print('case',1,value,lib.scalar(1,value))
            return
        print('# Original compute_band_energies/normalise_bands/amp2Log2:',REVISION)
        print('# frame LM CHANNELS EFFECTIVE_END END SPECTRUM[N*C] AMPLITUDES[21*C] NORMALIZED[N*C] LOGS[21*C]')
        for lm in range(4):
            n=120<<lm
            for channels in [1,2]:
                for effective_end,end in [(1,1),(13,13),(17,17),(19,21),(21,21),(1,21),(5,13),(13,21)]:
                    for pattern in range(10):
                        values=[]
                        for i in range(n*channels):
                            if pattern==0:value=0
                            elif pattern==1:value=1 if i%n==0 else 0
                            elif pattern==2:value=1<<24
                            elif pattern==3:value=-(1<<24)
                            elif pattern==4:value=1 if i%2 else -1
                            elif pattern in [5,6,7]:value=random_i32()>>[8,16,24][pattern-5]
                            elif pattern==8:value=(1<<29) if i%n==0 else 0
                            else:value=(1<<26) if i%2 else -(1<<26)
                            values.append(value)
                        data=(c.c_int32*len(values))(*values);energy=(c.c_int32*(21*channels))()
                        normalized=(c.c_int32*(n*channels))();logs=(c.c_int32*(21*channels))()
                        lib.probe(lm,channels,effective_end,end,data,energy,normalized,logs)
                        if list(data)!=values:raise SystemExit('Unexpected original input mutation')
                        print('frame',lm,channels,effective_end,end,*values,*energy,*normalized,*logs)


if __name__=='__main__':main()
