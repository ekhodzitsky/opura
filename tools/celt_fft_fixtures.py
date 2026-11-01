#!/usr/bin/env python3
"""Generate standard CELT scalar FFT plans and exact transform output.

Optional original kiss_fft.c/static_modes_fixed.h oracle; never a build/test
library dependency. --tables dumps active non-QEXT plans in FrameSize order.
"""
import argparse
import ctypes as c
from pathlib import Path
import subprocess
import tempfile
REVISION='22244de5a79bd1d6d623c32e72bf1954b56235be'
parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('source',type=Path)
parser.add_argument('--tables',action='store_true')
args=parser.parse_args();source=args.source.resolve()
if subprocess.check_output(['git','-C',str(source),'rev-parse','HEAD'],text=True).strip()!=REVISION:
    raise SystemExit('Expected pinned Opus v1.6.1')
if subprocess.check_output(['git','-C',str(source),'status','--porcelain'],text=True).strip():
    raise SystemExit('Reference checkout must be clean')
wrapper=r'''
#include "kiss_fft.h"
// Unused mode descriptors reference these non-FFT tables.
extern const opus_int16 eband5ms[];
extern const unsigned char band_allocation[];
#include "static_modes_fixed.h"
#if !OPUS_FAST_INT64
#error "Canonical FFT oracle requires scalar fast-64-bit math"
#endif
static const kiss_fft_state *const plans[4]={&fft_state48000_960_3,&fft_state48000_960_2,&fft_state48000_960_1,&fft_state48000_960_0};
int plan_field(int plan,int field,int position) {
    const kiss_fft_state *st=plans[plan];
    switch(field) {
    case 0:return st->nfft; case 1:return st->scale; case 2:return st->scale_shift; case 3:return st->shift;
    case 4:return st->factors[position]; case 5:return st->bitrev[position];
    case 6:return st->twiddles[position].r;case 7:return st->twiddles[position].i;
    default:abort();
    }
}
void transform(int plan,int kind,int downshift,const kiss_fft_cpx *input,kiss_fft_cpx *output) {
    const kiss_fft_state *st=plans[plan];
    if (kind==1) opus_fft_c(st,input,output);
    else if (kind==2) opus_ifft_c(st,input,output);
    else {
        for(int i=0;i<st->nfft;i++) output[st->bitrev[i]]=input[i];
        opus_fft_impl(st,output,downshift);
    }
}
'''
with tempfile.TemporaryDirectory(prefix='opura-celt-fft-') as temp:
    path=Path(temp);(path/'probe.c').write_text(wrapper)
    subprocess.run(['cc','-shared','-fPIC','-O2','-fwrapv','-DFIXED_POINT','-DVAR_ARRAYS','-DOPUS_BUILD','-DDISABLE_FLOAT_API',
                    '-I'+str(source/'include'),'-I'+str(source/'celt'),str(path/'probe.c'),str(source/'celt/kiss_fft.c'),
                    '-o',str(path/'probe.so')],check=True)
    lib=c.CDLL(str(path/'probe.so'))
    lib.plan_field.argtypes=[c.c_int]*3;lib.plan_field.restype=c.c_int
    lib.transform.argtypes=[c.c_int]*3+[c.POINTER(c.c_int32)]*2;lib.transform.restype=None
    def field(plan,key,pos=0):return lib.plan_field(plan,key,pos)
    if args.tables:
        print('// Generated from pinned Opus v1.6.1 by tools/celt_fft_fixtures.py --tables.')
        print('// Copyright (c) 2003-2004 Mark Borgerding; 2005-2008 Xiph.Org Foundation; 2008 CSIRO.')
        print('// See THIRD_PARTY_NOTICES.md.')
        print('pub(super) const TWIDDLES: [[i16;2];480] = '+str([[field(0,6,i),field(0,7,i)] for i in range(480)])+';')
        for lm in range(4):
            n=field(lm,0)
            print(f'const BITREV_{n}: [u16;{n}] = '+str([field(lm,5,i) for i in range(n)])+';')
        print('pub(super) const BITREV: [&[u16];4] = [&BITREV_60,&BITREV_120,&BITREV_240,&BITREV_480];')
        for name,key in [('SCALE',1),('SCALE_SHIFT',2),('TWIDDLE_SHIFT',3)]:
            values=[max(0,field(lm,key)) for lm in range(4)]
            kind='i32' if name=='SCALE' else 'u8'
            print(f'pub(super) const {name}: [{kind};4] = '+str(values)+';')
        factors=[]
        for lm in range(4):
            stages=[]
            for i in range(8):
                radix,m=field(lm,4,2*i),field(lm,4,2*i+1)
                stages.append((radix,m))
                if m==1:break
            factors.append('&'+repr(stages))
        print('pub(super) const FACTORS: [&[(usize,usize)];4] = ['+','.join(factors)+'];')
        raise SystemExit(0)
    print('# Original fixed-point kiss_fft.c, standard plans:',REVISION)
    print('# case LM KIND(0=forward,1=scaled,2=inverse) DOWNSHIFT INPUT_RE_IM[2*N] OUTPUT_RE_IM[2*N]')
    random=0x46465431
    def next_random():
        global random
        random ^= (random<<13)&0xffffffff;random ^= random>>17;random ^= (random<<5)&0xffffffff
        return c.c_int32(random).value
    for lm in range(4):
        n=field(lm,0)
        for pattern in range(12):
            values=[]
            for i in range(n):
                if pattern==0:pair=[0,0]
                elif pattern==1:pair=[1<<20 if i==0 else 0,0]
                elif pattern==2:pair=[0,-(1<<20) if i==n-1 else 0]
                elif pattern==3:pair=[4096,-8192]
                elif pattern==4:pair=[32767 if i%2 else -32768,0]
                elif pattern==5:pair=[0x7fffffff,-0x80000000]
                elif pattern==6:pair=[next_random(),next_random()]
                else:pair=[next_random()>>[4,8,12,16,20][pattern-7],next_random()>>[4,8,12,16,20][pattern-7]]
                values.extend(pair)
            input_data=(c.c_int32*(2*n))(*values)
            for kind,shift in [(0,s) for s in range(32)]+[(1,0),(2,0)]:
                out=(c.c_int32*(2*n))()
                lib.transform(lm,kind,shift,input_data,out)
                print('case',lm,kind,shift,*values,*out)
