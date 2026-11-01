#!/usr/bin/env python3
"""Generate original classic CELT comb postfilter and decoder scheduling data.

Optional oracle. The original generic constant filter and comb_filter bodies
are extracted unchanged from the external pinned source into a temporary probe.
"""
import argparse
import ctypes as c
from pathlib import Path
import subprocess
import tempfile
REVISION='22244de5a79bd1d6d623c32e72bf1954b56235be'
parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('source',type=Path)
args=parser.parse_args();source=args.source.resolve()
if subprocess.check_output(['git','-C',str(source),'rev-parse','HEAD'],text=True).strip()!=REVISION:
    raise SystemExit('Expected pinned Opus v1.6.1')
if subprocess.check_output(['git','-C',str(source),'status','--porcelain'],text=True).strip():
    raise SystemExit('Reference checkout must be clean')
text=(source/'celt/celt.c').read_text()
def function(signature,occurrence=0):
    starts=[];at=0
    while (at:=text.find(signature,at))>=0:starts.append(at);at+=len(signature)
    start=starts[occurrence];brace=text.index('{',start);depth=1;end=brace+1
    while depth:
        if text[end]=='{':depth+=1
        elif text[end]=='}':depth-=1
        end+=1
    return text[start:end]
# Only the original active Q15 window branch; FFT/mode descriptors are unused.
window=(source/'celt/static_modes_fixed.h').read_text().split('static const celt_coef window120[120] = {')[1].split('};')[0]
window=window.split('#else')[1].split('#endif')[0]
wrapper='''
#include "arch.h"
#include "celt.h"
#include "pitch.h"
#include "os_support.h"
#include "mathops.h"
#if !OPUS_FAST_INT64
#error "Canonical comb oracle requires scalar fast-64-bit math"
#endif
static const celt_coef window[120]={
'''+window+'\n};\n'+function('void comb_filter_const_c(',1)+'\n'+function('void comb_filter(')+r'''
void filter_frame(const opus_int32 *input,int n,int period,int gain,int tap,opus_int32 *history,int *state,opus_int32 *out) {
    opus_int32 buffer[1986];
    for(int i=0;i<1026;i++) buffer[i]=history[i];
    for(int i=0;i<n;i++) buffer[1026+i]=input[i];
    state[0]=IMAX(state[0],15);state[3]=IMAX(state[3],15);
    comb_filter(buffer+1026,buffer+1026,state[0],state[3],120,state[1],state[4],state[2],state[5],window,120,0);
    if(n>120) comb_filter(buffer+1146,buffer+1146,state[3],period,n-120,state[4],gain,state[5],tap,window,120,0);
    for(int i=0;i<n;i++) out[i]=buffer[1026+i];
    for(int i=0;i<1026;i++) history[i]=buffer[n+i];
    for(int i=0;i<3;i++) state[i]=state[3+i];
    state[3]=period;state[4]=gain;state[5]=tap;
    if(n>120) for(int i=0;i<3;i++) state[i]=state[3+i];
}
'''
with tempfile.TemporaryDirectory(prefix='opura-celt-postfilter-') as temp:
    path=Path(temp);(path/'probe.c').write_text(wrapper)
    subprocess.run(['cc','-shared','-fPIC','-O2','-fwrapv','-DFIXED_POINT','-DVAR_ARRAYS','-DOPUS_BUILD','-DDISABLE_FLOAT_API',
                    '-I'+str(source/'include'),'-I'+str(source/'celt'),str(path/'probe.c'),'-o',str(path/'probe.so')],check=True)
    lib=c.CDLL(str(path/'probe.so'));i32=c.POINTER(c.c_int32)
    lib.filter_frame.argtypes=[i32,c.c_int,c.c_int,c.c_int,c.c_int,i32,i32,i32];lib.filter_frame.restype=None
    print('# Original comb_filter and normal celt_decoder scheduling:',REVISION)
    print('# frame RESET N PERIOD GAIN TAP INPUT[N] OUTPUT[N] PARAMETERS[old_period,old_gain,old_tap,current_period,current_gain,current_tap] HISTORY[1026]')
    random=0x434f4d42
    def next_random():
        global random
        random ^= (random<<13)&0xffffffff;random ^= random>>17;random ^= (random<<5)&0xffffffff
        return c.c_int32(random).value
    for sequence in range(12):
        history=(c.c_int32*1026)();state=(c.c_int32*6)()
        previous=(0,0,0)
        for index in range(48):
            reset=index in [0,23]
            if reset:
                for i in range(1026):history[i]=0
                for i in range(6):state[i]=0
            n=120<<((index+sequence)%4)
            period=[0,15,16,17,60,120,121,255,511,960,1022,1023][(index+sequence)%12]
            gain=[0,3072,6144,12288,24576][(index//2+sequence)%5]
            if period==0:gain=0
            tap=(index//3+sequence)%3
            if index%7 in [1,2]:period,gain,tap=previous
            previous=(period,gain,tap)
            values=[];pattern=(index+sequence)%8
            for i in range(n):
                if pattern==0:value=0
                elif pattern==1:value=(1<<26) if i==0 else 0
                elif pattern==2:value=327680
                elif pattern==3:value=0x7fffffff if i%2 else -0x80000000
                else:value=next_random()>>[0,4,12,20][pattern-4]
                values.append(value)
            out=(c.c_int32*n)()
            lib.filter_frame((c.c_int32*n)(*values),n,period,gain,tap,history,state,out)
            print('frame',int(reset),n,period,gain,tap,*values,*out,*state,*history)
