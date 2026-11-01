#!/usr/bin/env python3
"""Optional pinned original periodic-PLC bandwidth and energy safety guards."""
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
def braced(text,start):
    first=text.index('{',start);depth=1;end=first+1
    while depth:
        depth+=(text[end]=='{')-(text[end]=='}');end+=1
    return text[first:end]
decoder=(source/'celt/celt_decoder.c').read_text();static=(source/'celt/static_modes_fixed.h').read_text()
start=static.index('static const celt_coef window120[120]');window=static[start:static.index(';',start)+1]
anchor='         while (1) {'
if decoder.count(anchor)!=1:raise SystemExit('Unexpected LPC expansion loop')
loop=braced(decoder,decoder.index(anchor))
anchor='         /* Check if the synthesis energy is higher than expected'
if decoder.count(anchor)!=1:raise SystemExit('Unexpected synthesis energy guard')
guard=braced(decoder,decoder.index(anchor))
wrapper='''
#include "arch.h"
#include "mathops.h"
#include "celt_lpc.h"
#if !OPUS_FAST_INT64
#error "Canonical PLC guard fixtures require scalar fast-64-bit math"
#endif
'''+window+'\nvoid stabilize(opus_int16 *lpc) {int c=0,i;while(1) '+loop+'}\nvoid energy_guard(opus_int32 *buf,int N,opus_int32 S1) {int i,decode_buffer_size=N,extrapolation_len=N+120,overlap=120;const celt_coef *window=window120;'+guard+'}\n'
with tempfile.TemporaryDirectory(prefix='opura-celt-plc-') as temp:
    path=Path(temp);(path/'probe.c').write_text(wrapper)
    subprocess.run(['cc','-shared','-fPIC','-O2','-fwrapv','-DFIXED_POINT','-DVAR_ARRAYS','-DOPUS_BUILD','-DDISABLE_FLOAT_API',
                    '-I'+str(source/'include'),'-I'+str(source/'celt'),str(path/'probe.c'),str(source/'celt/mathops.c'),str(source/'celt/entcode.c'),'-o',str(path/'probe.so')],check=True)
    lib=c.CDLL(str(path/'probe.so'));lib.stabilize.argtypes=[c.POINTER(c.c_int16)];lib.stabilize.restype=None;lib.energy_guard.argtypes=[c.POINTER(c.c_int32),c.c_int,c.c_int];lib.energy_guard.restype=None
    print('# Original classic periodic PLC safety blocks:',REVISION)
    print('# filter COEFFICIENTS[24] OUTPUT[24]; guard N S1 INPUT[N+120] OUTPUT[N+120]')
    random=0x504c4347
    def next_random():
        global random
        random^=(random<<13)&0xffffffff;random^=random>>17;random^=(random<<5)&0xffffffff
        return c.c_int32(random).value
    def emit_filter(values):
        out=(c.c_int16*24)(*values);lib.stabilize(out);print('filter',*values,*out)
    for amplitude in [0,1,1024,2048,2559,2560,4096,8192,16384,30719,30720,32767]:
        for pattern in range(8):
            values=[(amplitude if pattern==0 else -amplitude if pattern==1 else amplitude*(1 if i%2 else -1) if pattern==2 else amplitude if i<pattern-2 else 0) for i in range(24)]
            emit_filter(values)
    for a in [30718,30719,30720,32767]:
        for b in [30718,30719,30720,32767]:
            for sa in [-1,1]:
                for sb in [-1,1]:emit_filter([sa*a,sb*b]+[0]*22)
    for pattern in range(48):emit_filter([(-32768 if pattern==0 else next_random()>>16) for _ in range(24)])
    for n in [120,240,480,960]:
        for pattern in range(10):
            length=n+120
            values=[(0 if pattern==0 else 1 if pattern==1 else 536870911 if pattern==2 else -536870911 if pattern==3 else 536870911*(1 if i%2 else -1) if pattern==4 else max(-536870911,min(536870911,next_random()>>2)) if pattern==5 else ((i%31)-15)*4096 if pattern==6 else 536870911 if pattern==7 and i==0 else [2047,-2048,2048,-2049,0,1,-1][i%7] if pattern==8 else (1<<27)*(1 if i%2 else -1) if pattern==9 else 0) for i in range(length)]
            s2=sum(max(-32767,min(32767,(v+2048)>>12))**2>>11 for v in values);maximum=length*524256
            for s1 in sorted({0,max(0,s2//4-1),s2//4,min(maximum,s2//4+1),s2//2,max(0,s2-1),s2,min(maximum,s2+1),maximum}):
                out=(c.c_int32*length)(*values);lib.energy_guard(out,n,s1);print('guard',n,s1,*values,*out)
