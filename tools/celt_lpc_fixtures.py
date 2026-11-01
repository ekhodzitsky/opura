#!/usr/bin/env python3
"""Original pinned scalar celt_lpc autocorrelation/analysis/FIR/IIR evidence.

Optional external oracle; Cargo builds/tests never compile or load C code.
"""
import argparse
import ctypes as c
import math
from pathlib import Path
import subprocess
import tempfile
REVISION='22244de5a79bd1d6d623c32e72bf1954b56235be'
parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('source',type=Path)
args=parser.parse_args();source=args.source.resolve()
if subprocess.check_output(['git','-C',str(source),'rev-parse','HEAD'],text=True).strip()!=REVISION:raise SystemExit('Expected pinned Opus v1.6.1')
if subprocess.check_output(['git','-C',str(source),'status','--porcelain'],text=True).strip():raise SystemExit('Reference checkout must be clean')
static=(source/'celt/static_modes_fixed.h').read_text();start=static.index('static const celt_coef window120[120]');window=static[start:static.index(';',start)+1]
wrapper='''
#include "arch.h"
#include "celt_lpc.h"
#include "mathops.h"
#if !OPUS_FAST_INT64
#error "Canonical LPC fixtures require scalar fast-64-bit math"
#endif
'''+window+'''
void autocorrelation(const opus_int16 *x,int n,int lag,int windowed,opus_int32 *out) {
    out[0]=_celt_autocorr(x,out+1,window120,windowed?120:0,lag,n,0);
}
void analyse(const opus_int32 *ac,opus_int16 *lpc,int p) {_celt_lpc(lpc,ac,p);}
void fir(const opus_int16 *x,const opus_int16 *lpc,int n,int p,opus_int16 *out) {celt_fir(x+p,lpc,out,n,p,0);}
void iir(opus_int32 *x,const opus_int16 *lpc,int n,int p,opus_int16 *mem) {celt_iir(x,lpc,x,n,p,mem,0);}
'''
with tempfile.TemporaryDirectory(prefix='opura-celt-lpc-') as temp:
    path=Path(temp);(path/'probe.c').write_text(wrapper)
    subprocess.run(['cc','-shared','-fPIC','-O2','-fwrapv','-DFIXED_POINT','-DVAR_ARRAYS','-DOPUS_BUILD','-DDISABLE_FLOAT_API',
                    '-I'+str(source/'include'),'-I'+str(source/'celt'),str(path/'probe.c'),
                    *[str(source/'celt'/f) for f in ['celt_lpc.c','pitch.c','mathops.c','entcode.c']],'-o',str(path/'probe.so')],check=True)
    lib=c.CDLL(str(path/'probe.so'));i16=c.POINTER(c.c_int16);i32=c.POINTER(c.c_int32)
    for name,types in [('autocorrelation',[i16,c.c_int,c.c_int,c.c_int,i32]),('analyse',[i32,i16,c.c_int]),('fir',[i16,i16,c.c_int,c.c_int,i16]),('iir',[i32,i16,c.c_int,c.c_int,i16])]:
        fn=getattr(lib,name);fn.argtypes=types;fn.restype=None
    random=0x4c504341
    def next_random():
        global random
        random^=(random<<13)&0xffffffff;random^=random>>17;random^=(random<<5)&0xffffffff
        return c.c_int32(random).value
    def signal(n,pattern):
        return [(0 if pattern==0 else 1 if pattern==1 else 32767 if pattern==2 else -32768 if pattern==3 else [32767,-32768][i%2] if pattern==4 else ((i%17)-8)*3000 if pattern==5 else (next_random()>>16) if pattern==6 else (next_random()>>24)) for i in range(n)]
    print('# Original scalar fixed-point celt_lpc:',REVISION)
    print('# acf N LAG WINDOW INPUT[N] SHIFT ACF[LAG+1]; lpc ORDER ACF[P+1] INITIAL[P] OUTPUT[P]')
    print('# fir N ORDER COEFF[P] INPUT[N+P] OUTPUT[N]; iir N ORDER COEFF[P] MEM[P] INPUT[N] OUTPUT[N] MEM[P]')
    for n in [3,4,8,16,32,120,240,480,960,1024,1927,2048]:
        for lag in [0,1,2,4,8,12,16,24]:
            if lag+3>n:continue
            for windowed in ([0,1] if n>=240 else [0]):
                for pattern in range(8):
                    values=signal(n,pattern);out=(c.c_int32*(lag+2))()
                    lib.autocorrelation((c.c_int16*n)(*values),n,lag,windowed,out)
                    print('acf',n,lag,windowed,*values,*out)
                    if lag:
                        ac=list(out[1:]);ac[0]+=ac[0]>>13
                        for k in range(1,lag+1):ac[k]-=(2*k*k*ac[k])>>15
                        initial=signal(lag,6);coeff=(c.c_int16*lag)(*initial)
                        lib.analyse((c.c_int32*(lag+1))(*ac),coeff,lag)
                        print('lpc',lag,*ac,*initial,*coeff)
    for p in range(1,25):
        ac=[0]*(p+1);initial=signal(p,6);out=(c.c_int16*p)(*initial)
        lib.analyse((c.c_int32*(p+1))(*ac),out,p);print('lpc',p,*ac,*initial,*out)
    # Target the estimator's exact power-of-two boundaries, including its
    # rounding headroom, rather than relying on random amplitudes to hit them.
    for n in [3,7,32,120,1024,1927,2048]:
        estimate_shift=(n+n//16).bit_length()-1
        def estimate(a):
            value=1+n*128+n*((a*a)>>estimate_shift)
            return value+(value>>7)
        amplitudes=set()
        for bit in range(31):
            lo,hi=0,32768
            while lo<hi:
                mid=(lo+hi)//2
                if estimate(mid)>=(1<<bit):hi=mid
                else:lo=mid+1
            amplitudes.update(range(max(0,lo-2),min(32768,lo+3)))
        lag=min(4,n-3)
        for amp in sorted(amplitudes):
            values=(c.c_int16*n)(*[amp]*n);out=(c.c_int32*(lag+2))()
            lib.autocorrelation(values,n,lag,0,out)
            print('dc',n,lag,amp,*out)
    # Positive Toeplitz covariance models with clustered sinusoidal spectra
    # exercise high-order coefficient fitting beyond ordinary windowed noise.
    p=24
    for modes in [2,3,4,6,8,10,12,16,24]:
        for width in [.05,.1,.2,.4,.8]:
            for floor in [1e-6,1e-4,5e-4,1e-3,1e-2]:
                for pattern in range(8):
                    frequencies=[math.pi*width*((i+.25+(next_random()&255)/512)/modes) for i in range(modes)]
                    weights=[1+(next_random()&255) for _ in range(modes)]
                    total=sum(weights)
                    ac=[1<<29]+[round((1<<29)*(1-floor)*sum(w*math.cos(k*f) for w,f in zip(weights,frequencies))/total) for k in range(1,p+1)]
                    initial=signal(p,6);out=(c.c_int16*p)(*initial)
                    lib.analyse((c.c_int32*(p+1))(*ac),out,p)
                    print('lpc',p,*ac,*initial,*out)
    for p in [3,4,5,8,12,16,20,24]:
        for n in [1,2,3,4,5,7,8,16,120,200,240,720,1024]:
            for pattern in range(8):
                coeff=signal(p,(pattern+3)%8);values=signal(n+p,pattern);out=(c.c_int16*n)()
                lib.fir((c.c_int16*(n+p))(*values),(c.c_int16*p)(*coeff),n,p,out)
                print('fir',n,p,*coeff,*values,*out)
    for p in [4,8,12,16,20,24]:
        for n in [4,8,12,24,120,240,480,720,1080]:
            if n<p:continue
            for pattern in range(8):
                coeff=signal(p,(pattern+2)%8);mem=signal(p,pattern)
                values=[(0 if pattern==0 else i*4096 if pattern==1 else 0x7fffffff if pattern==2 else -0x80000000 if pattern==3 else [0x7fffffff,-0x80000000][i%2] if pattern==4 else next_random() if pattern==5 else (next_random()>>4) if pattern==6 else (next_random()>>16)<<12) for i in range(n)]
                out=(c.c_int32*n)(*values);memory=(c.c_int16*p)(*mem)
                lib.iir(out,(c.c_int16*p)(*coeff),n,p,memory)
                print('iir',n,p,*coeff,*mem,*values,*out,*memory)
