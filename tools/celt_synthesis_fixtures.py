#!/usr/bin/env python3
"""Generate complete classic CELT synthesis-backend PCM and persistent state.

Optional oracle over unchanged original celt_synthesis/comb/de-emphasis and
supporting functions, original standard mode tables, mdct.c and kiss_fft.c.
The inputs are decoded normalized spectra, not encoded Opus packets.
"""
import argparse
import ctypes as c
from pathlib import Path
import math
import subprocess
import tempfile
REVISION='22244de5a79bd1d6d623c32e72bf1954b56235be'
parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('source',type=Path)
args=parser.parse_args();source=args.source.resolve()
if subprocess.check_output(['git','-C',str(source),'rev-parse','HEAD'],text=True).strip()!=REVISION:
    raise SystemExit('Expected pinned Opus v1.6.1')
if subprocess.check_output(['git','-C',str(source),'status','--porcelain'],text=True).strip():
    raise SystemExit('Reference checkout must be clean')
def function(text,signature,occurrence=0):
    starts=[];at=0
    while (at:=text.find(signature,at))>=0:starts.append(at);at+=len(signature)
    start=starts[occurrence];brace=text.index('{',start);depth=1;end=brace+1
    while depth:
        if text[end]=='{':depth+=1
        elif text[end]=='}':depth-=1
        end+=1
    return text[start:end]
def declaration(text,signature):
    start=text.index(signature);return text[start:text.index(';',start)+1]
celt=(source/'celt/celt.c').read_text();decoder=(source/'celt/celt_decoder.c').read_text();modes=(source/'celt/modes.c').read_text()
wrapper='''
#include "arch.h"
#include "mathops.h"
#include "modes.h"
#include "quant_bands.h"
#include "bands.h"
#include "celt.h"
#include "pitch.h"
#include "stack_alloc.h"
#include "os_support.h"
#if !OPUS_FAST_INT64
#error "Canonical synthesis oracle requires scalar fast-64-bit math"
#endif
'''+declaration(modes,'static const opus_int16 eband5ms[]')+'\n'+declaration(modes,'static const unsigned char band_allocation[]')+'\n'+declaration((source/'celt/quant_bands.c').read_text(),'const signed char eMeans[25]')+'\n#include "static_modes_fixed.h"\n'+function((source/'celt/bands.c').read_text(),'void denormalise_bands(')+'\n'+function(decoder,'void celt_synthesis(')+'\n'+function(celt,'void comb_filter_const_c(',1)+'\n'+function(celt,'void comb_filter(')+'\n'+function(decoder,'static void deemphasis_stereo_simple(')+'\n'+function(decoder,'void deemphasis(')+r'''
void synthesize(int lm,int factor,int C,int CC,int start,int end,int transient,int silence,int period,int gain,int tap,
                opus_int32 *X,opus_int32 *energy,opus_int32 *overlap,opus_int32 *history,int *parameters,opus_int32 *memory,opus_int16 *pcm) {
    opus_int32 buffer[2][2046]={{0}};
    opus_int32 *out[2]={buffer[0]+1026,buffer[1]+1026};
    int N=120<<lm;
    for(int c=0;c<CC;c++) {
        for(int i=0;i<1026;i++) buffer[c][i]=history[1026*c+i];
        for(int i=0;i<60;i++) out[c][i]=overlap[60*c+i];
    }
    celt_synthesis(&mode48000_960_120,X,out,energy,start,end,C,CC,transient,lm,factor,silence,0);
    parameters[0]=IMAX(parameters[0],15);parameters[3]=IMAX(parameters[3],15);
    for(int c=0;c<CC;c++) {
        comb_filter(out[c],out[c],parameters[0],parameters[3],120,parameters[1],parameters[4],parameters[2],parameters[5],window120,120,0);
        if(lm) comb_filter(out[c]+120,out[c]+120,parameters[3],period,N-120,parameters[4],gain,parameters[5],tap,window120,120,0);
        for(int i=0;i<1026;i++) history[1026*c+i]=buffer[c][N+i];
        for(int i=0;i<60;i++) overlap[60*c+i]=out[c][N+i];
    }
    for(int i=0;i<3;i++) parameters[i]=parameters[i+3];
    parameters[3]=period;parameters[4]=gain;parameters[5]=tap;
    if(lm) for(int i=0;i<3;i++) parameters[i]=parameters[i+3];
    deemphasis(out,pcm,N,CC,factor,mode48000_960_120.preemph,memory,0);
}
'''
with tempfile.TemporaryDirectory(prefix='opura-celt-synthesis-') as temp:
    path=Path(temp);(path/'probe.c').write_text(wrapper)
    subprocess.run(['cc','-shared','-fPIC','-O2','-fwrapv','-DFIXED_POINT','-DVAR_ARRAYS','-DOPUS_BUILD','-DDISABLE_FLOAT_API',
                    '-I'+str(source/'include'),'-I'+str(source/'celt'),str(path/'probe.c'),str(source/'celt/mdct.c'),str(source/'celt/kiss_fft.c'),
                    '-o',str(path/'probe.so')],check=True)
    lib=c.CDLL(str(path/'probe.so'));i32=c.POINTER(c.c_int32);i16=c.POINTER(c.c_int16)
    lib.synthesize.argtypes=[c.c_int]*11+[i32]*6+[i16];lib.synthesize.restype=None
    print('# Original complete normalized-spectrum CELT synthesis backend:',REVISION)
    print('# frame RESET FACTOR API_CHANNELS ENCODED_CHANNELS LM START END TRANSIENT SILENCE PERIOD GAIN TAP ENERGIES[21*C] INPUT_Q24[N*C] PCM[N/factor*CC] OVERLAP[60*CC] HISTORY[1026*CC] PARAMETERS[6] DEEMPHASIS[2]')
    random=0x53594e54
    def next_random():
        global random
        random ^= (random<<13)&0xffffffff;random ^= random>>17;random ^= (random<<5)&0xffffffff
        return c.c_int32(random).value
    bands=[0,1,2,3,4,5,6,7,8,10,12,14,16,20,24,28,34,40,48,60,78,100]
    for factor in [1,2,3,4,6]:
        for api in [1,2]:
            for sequence in range(3):
                overlap=(c.c_int32*120)();history=(c.c_int32*2052)();parameters=(c.c_int32*6)();memory=(c.c_int32*2)()
                for index in range(16):
                    reset=index in [0,9]
                    if reset:
                        for array in [overlap,history,parameters,memory]:
                            for i in range(len(array)):array[i]=0
                    C=1+(index//2+sequence)%2;lm=(index+sequence)%4;N=120<<lm;M=1<<lm
                    start,end=[(0,21),(0,13),(17,21),(0,19)][(index//3+sequence)%4]
                    transient=(index//4+sequence)%2;silence=int(index%11==10)
                    period=[0,15,60,121,255,1023][(index+sequence)%6];gain=0 if period==0 else [3072,12288,24576][(index+sequence)%3];tap=(index//3+sequence)%3
                    energy=[];values=[]
                    for channel in range(C):
                        energy.extend([(([ -20,-9,0,8,18,28][(index+channel+band)%6])<<24)+[0,8192,0xffffff][(index+band)%3] for band in range(21)])
                        spectrum=[0]*N
                        for band in range(21):
                            first,last=M*bands[band],M*bands[band+1]
                            denominator=math.isqrt((last-first)*65536)
                            for i in range(first,last):
                                if (index+sequence)%5==0:value=1<<24 if i==first else 0
                                else:value=((next_random()%511)-255)*(1<<24)//denominator
                                spectrum[i]=value
                        values.extend(spectrum)
                    pcm=(c.c_int16*1920)()
                    lib.synthesize(lm,factor,C,api,start,end,transient,silence,period,gain,tap,(c.c_int32*len(values))(*values),(c.c_int32*len(energy))(*energy),overlap,history,parameters,memory,pcm)
                    print('frame',int(reset),factor,api,C,lm,start,end,transient,silence,period,gain,tap,*energy,*values,*pcm[:N//factor*api],*overlap[:60*api],*history[:1026*api],*parameters,*memory)
