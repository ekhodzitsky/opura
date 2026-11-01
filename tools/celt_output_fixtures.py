#!/usr/bin/env python3
"""Generate classic CELT spectral scaling and de-emphasis reference data.

Optional only. Original function bodies are copied verbatim from the clean
external checkout into a temporary probe, with original headers/tables. No C
source is linked or included in ordinary Rust builds/tests.
"""
import argparse
import ctypes as c
from pathlib import Path
import re
import subprocess
import tempfile
REVISION='22244de5a79bd1d6d623c32e72bf1954b56235be'
parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('source',type=Path);parser.add_argument('--tables',action='store_true')
args=parser.parse_args();source=args.source.resolve()
if subprocess.check_output(['git','-C',str(source),'rev-parse','HEAD'],text=True).strip()!=REVISION:
    raise SystemExit('Expected pinned Opus v1.6.1')
if subprocess.check_output(['git','-C',str(source),'status','--porcelain'],text=True).strip():
    raise SystemExit('Reference checkout must be clean')

def array(text,signature):
    at=text.index(signature);end=text.index(';',at)+1
    body=text[at:end]
    numbers=re.sub(r'/\*.*?\*/','',body[body.index('{')+1:body.rindex('}')],flags=re.S)
    return body,[int(v) for v in re.findall(r'-?\d+',numbers)]

def function(text,signature):
    if text.count(signature)!=1:raise SystemExit('Unexpected original function signature: '+signature)
    start=text.index(signature);brace=text.index('{',start);depth=1;end=brace+1
    while depth:
        if text[end]=='{':depth+=1
        elif text[end]=='}':depth-=1
        end+=1
    return text[start:end]

bands_source=(source/'celt/bands.c').read_text();decoder_source=(source/'celt/celt_decoder.c').read_text()
band_definition,bands=array((source/'celt/modes.c').read_text(),'static const opus_int16 eband5ms[]')
mean_definition,means=array((source/'celt/quant_bands.c').read_text(),'const signed char eMeans[25]')
mode=(source/'celt/static_modes_fixed.h').read_text().split('static const CELTMode mode48000_960_120 = {')[1]
coef=[int(v) for v in re.search(r'\{([^{}]+)\},\s*/\* preemph',mode)[1].split(',') if v.strip()]
if len(bands)!=22 or len(means)!=25 or coef!=[27853,0,4096,8192]:raise SystemExit('Unexpected standard output tables')
if args.tables:
    print('// Generated from pinned Opus v1.6.1 by tools/celt_output_fixtures.py --tables.')
    print('// Copyright (c) 2007-2008 CSIRO; 2007-2009 Xiph.Org Foundation.')
    print('// See THIRD_PARTY_NOTICES.md.')
    print('pub(super) const BANDS: [u16;22] = '+str(bands)+';')
    print('pub(super) const MEANS: [i8;21] = '+str(means[:21])+';')
    print('pub(super) const PREEMPHASIS: i16 = '+str(coef[0])+';')
    raise SystemExit(0)
wrapper='''
#include "arch.h"
#include "mathops.h"
#include "modes.h"
#include "quant_bands.h"
#include "stack_alloc.h"
#include "os_support.h"
#if !OPUS_FAST_INT64
#error "Canonical output oracle requires scalar fast-64-bit math"
#endif
'''+band_definition+'\n'+mean_definition+'\n'+function(bands_source,'void denormalise_bands(')+'\n'+function(decoder_source,'static void deemphasis_stereo_simple(')+'\n'+function(decoder_source,'void deemphasis(')+r'''
void spectrum_probe(int lm,int factor,int start,int end,int silence,const opus_int32 *values,const opus_int32 *energy,opus_int32 *out) {
    CELTMode mode={0};mode.shortMdctSize=120;mode.eBands=eband5ms;
    denormalise_bands(&mode,values,out,energy,start,end,1<<lm,factor,silence);
}
void output_probe(int count,int channels,int factor,opus_int32 *left,opus_int32 *right,opus_int32 *memory,opus_int16 *out) {
    opus_int32 *input[2]={left,right};
    const opus_int16 coef[4]={27853,0,4096,8192};
    deemphasis(input,out,count,channels,factor,coef,memory,0);
}
'''
with tempfile.TemporaryDirectory(prefix='opura-celt-output-') as temp:
    path=Path(temp);(path/'probe.c').write_text(wrapper)
    subprocess.run(['cc','-shared','-fPIC','-O2','-fwrapv','-DFIXED_POINT','-DVAR_ARRAYS','-DOPUS_BUILD','-DDISABLE_FLOAT_API',
                    '-I'+str(source/'include'),'-I'+str(source/'celt'),str(path/'probe.c'),'-o',str(path/'probe.so')],check=True)
    lib=c.CDLL(str(path/'probe.so'));i32=c.POINTER(c.c_int32);i16=c.POINTER(c.c_int16)
    lib.spectrum_probe.argtypes=[c.c_int]*5+[i32]*3;lib.spectrum_probe.restype=None
    lib.output_probe.argtypes=[c.c_int]*3+[i32]*3+[i16];lib.output_probe.restype=None
    print('# Original classic denormalise_bands/deemphasis:',REVISION)
    print('# spectrum LM FACTOR START END SILENCE ENERGIES_Q24[21] INPUT_Q24[N] OUTPUT[N]')
    print('# frame RESET FACTOR CHANNELS N INPUT_LEFT[N] INPUT_RIGHT[N if stereo] PCM[N/factor*channels] MEMORY[2]')
    random=0x4f555450
    def next_random():
        global random
        random ^= (random<<13)&0xffffffff;random ^= random>>17;random ^= (random<<5)&0xffffffff
        return c.c_int32(random).value
    for lm in range(4):
        n=120<<lm
        for factor in [1,2,3,4,6]:
            for index in range(32):
                start,end=[(0,21),(0,13),(0,17),(0,19),(17,21),(5,13),(21,21),(0,1)][index%8]
                silence=int(index%13==0)
                energy=[c.c_int32(([-32,-28,-20,-9,0,14,18,28,127,-128][(index+i)%10]<<24)+[0,1,8191,8192,0xffffff][(index+i)%5]).value for i in range(21)]
                values=[]
                for i in range(n):
                    values.append([0,(1<<24) if i==0 else 0,1<<24,-(1<<24),0x7fffffff,-0x80000000,next_random()>>8,next_random()][index%8])
                out=(c.c_int32*n)()
                lib.spectrum_probe(lm,factor,start,end,silence,(c.c_int32*n)(*values),(c.c_int32*21)(*energy),out)
                print('spectrum',lm,factor,start,end,silence,*energy,*values,*out)
    for sequence in range(8):
        memory=(c.c_int32*2)()
        for index in range(48):
            reset=index in [0,17,33]
            if reset:memory[0]=memory[1]=0
            n=120<<((index+sequence)%4);channels=1+(index//3+sequence)%2;factor=[1,2,3,4,6][(index+sequence)%5]
            signals=[]
            for channel in range(2):
                values=[]
                for i in range(n):
                    pattern=(index+sequence+channel)%10
                    if pattern==0:value=0
                    elif pattern==1:value=(1<<25) if i==0 else 0
                    elif pattern==2:value=-100000
                    elif pattern==3:value=0x7fffffff
                    elif pattern==4:value=-0x80000000
                    elif pattern==5:value=536870911 if i%2 else -536870911
                    else:value=next_random()>>[0,4,12,20][pattern-6]
                    values.append(value)
                signals.append(values)
            out=(c.c_int16*1920)()
            lib.output_probe(n,channels,factor,(c.c_int32*n)(*signals[0]),(c.c_int32*n)(*signals[1]),memory,out)
            print('frame',int(reset),factor,channels,n,*signals[0],*(signals[1] if channels==2 else []),*out[:n//factor*channels],*memory)
