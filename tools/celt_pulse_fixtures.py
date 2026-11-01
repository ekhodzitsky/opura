#!/usr/bin/env python3
"""Generate CWRS pulse-vector evidence using original pinned SMALL_FOOTPRINT.

The original generic recurrence supports all validated geometries, including
ones absent from the optimized standard-mode table. Python big integers select
only codebooks representable by u32 before calling the reference C routines.
"""
import argparse
import ctypes as c
from pathlib import Path
import subprocess
import tempfile
REVISION='22244de5a79bd1d6d623c32e72bf1954b56235be'
parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('source',type=Path)
args=parser.parse_args()
source=args.source.resolve()
if subprocess.check_output(['git','-C',str(source),'rev-parse','HEAD'],text=True).strip()!=REVISION:
    raise SystemExit('Expected pinned Opus v1.6.1')
if subprocess.check_output(['git','-C',str(source),'status','--porcelain'],text=True).strip():
    raise SystemExit('Reference checkout must be clean')
wrapper=r'''
#include "cwrs.c"
unsigned count_book(int n,int k) {
    if (!k) return 1;
    unsigned u[130];
    return ncwrs_urow(n,k,u);
}
int index_vector(int n,int k,unsigned index,int *values) {
    if (!k) { for(int i=0;i<n;i++) values[i]=0;return 0; }
    unsigned u[130];
    ncwrs_urow(n,k,u);
    return cwrsi(n,k,index,values,u);
}
int write_sequence(int n,int k,const unsigned *indices,int count,unsigned char *data) {
    ec_enc enc;
    ec_enc_init(&enc,data,1024);
    for(int i=0;i<count;i++) ec_enc_uint(&enc,indices[i],count_book(n,k));
    int len=(ec_tell(&enc)+7)/8;
    ec_enc_shrink(&enc,len);
    ec_enc_done(&enc);
    return ec_get_error(&enc) ? -1 : len;
}
void read_sequence(const unsigned char *data,int len,int n,int k,int count,int *out) {
    ec_dec dec;
    ec_dec_init(&dec,(unsigned char*)data,len);
    for(int i=0;i<count;i++) {
        int *row=out+i*(n+4);
        row[3]=decode_pulses(row+4,n,k,&dec);
        row[0]=dec.rng;row[1]=ec_tell(&dec);row[2]=ec_tell_frac(&dec);
    }
}
'''
u=[[0]*130 for _ in range(177)]
u[0][0]=1
for n in range(1,177):
    for k in range(1,130):u[n][k]=u[n-1][k]+u[n][k-1]+u[n-1][k-1]
with tempfile.TemporaryDirectory(prefix='opura-celt-pulses-') as temp:
    path=Path(temp)
    (path/'probe.c').write_text(wrapper)
    subprocess.run(['cc','-shared','-fPIC','-O2','-fwrapv','-DVAR_ARRAYS','-DOPUS_BUILD','-DFIXED_POINT',
                    '-DDISABLE_FLOAT_API','-DSMALL_FOOTPRINT','-I'+str(source/'include'),'-I'+str(source/'celt'),
                    str(path/'probe.c'),*[str(source/'celt'/f) for f in ['entcode.c','entenc.c','entdec.c']],
                    '-o',str(path/'probe.so')],check=True)
    lib=c.CDLL(str(path/'probe.so'))
    i32=c.POINTER(c.c_int);u32=c.POINTER(c.c_uint32);u8=c.POINTER(c.c_ubyte)
    for name,types,result in [('count_book',[c.c_int,c.c_int],c.c_uint32),
                              ('index_vector',[c.c_int,c.c_int,c.c_uint32,i32],c.c_int),
                              ('write_sequence',[c.c_int,c.c_int,u32,c.c_int,u8],c.c_int),
                              ('read_sequence',[u8,c.c_int,c.c_int,c.c_int,c.c_int,i32],None)]:
        fn=getattr(lib,name);fn.argtypes=types;fn.restype=result
    print('# Original SMALL_FOOTPRINT cwrs.c:',REVISION)
    print('# book N K CARDINALITY (0=overflow); all N K COUNT FNV1A64(le-i32 norm then coordinates per index)')
    print('# vector N K INDEX SQUARED_NORM COORDINATES[N]; stream N K HEX COUNT; step RANGE TELL TELL_FRAC SQUARED_NORM COORDINATES[N]')
    random=0x50565132
    def next_random():
        global random
        random ^= (random<<13)&0xffffffff
        random ^= random>>17
        random ^= (random<<5)&0xffffffff
        return random
    for n in range(2,177):
        for k in range(129):
            cardinality=1 if k==0 else u[n][k]+u[n][k+1]
            if cardinality>0xffffffff:
                print('book',n,k,0)
                continue
            if lib.count_book(n,k)!=cardinality:raise SystemExit('Reference cardinality mismatch')
            print('book',n,k,cardinality)
            values=(c.c_int*n)()
            if n<=8 and k<=8 and cardinality<=20000:
                fingerprint=14695981039346656037
                for index in range(cardinality):
                    norm=lib.index_vector(n,k,index,values)
                    for value in [norm,*values]:
                        for byte in (value&0xffffffff).to_bytes(4,'little'):
                            fingerprint=((fingerprint^byte)*1099511628211)&0xffffffffffffffff
                print('all',n,k,cardinality,fingerprint)
            indices={0,cardinality-1,next_random()%cardinality,next_random()%cardinality}
            if cardinality>1:indices.update([1,cardinality-2,cardinality//2-1,cardinality//2])
            for index in sorted(indices):
                norm=lib.index_vector(n,k,index,values)
                print('vector',n,k,index,norm,*values)
            if k and (n in [2,3,4,6,8,14,16,32,64,96,144,176]) and k in [1,2,3,4,8,13,36,95,128]:
                indices=sorted(indices)
                data=(c.c_ubyte*1024)()
                size=lib.write_sequence(n,k,(c.c_uint32*len(indices))(*indices),len(indices),data)
                if size<=0:raise SystemExit('Entropy encoding failed')
                out=(c.c_int*((n+4)*len(indices)))()
                lib.read_sequence(data,size,n,k,len(indices),out)
                print('stream',n,k,bytes(data[:size]).hex(),len(indices))
                for i in range(len(indices)):
                    row=out[i*(n+4):(i+1)*(n+4)]
                    print('step',c.c_uint32(row[0]).value,*row[1:])
