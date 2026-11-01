#!/usr/bin/env python3
"""Generate normalized CELT PVQ bands from original pinned alg_unquant.

Optional developer oracle; original generic CWRS, vq.c and scalar math are
compiled into a temporary library. Cargo never links or loads the reference.
"""
import argparse
import ctypes as c
import math
from pathlib import Path
import subprocess
import tempfile
REVISION='22244de5a79bd1d6d623c32e72bf1954b56235be'
parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('source',type=Path)
parser.add_argument('--encode',action='store_true')
args=parser.parse_args()
source=args.source.resolve()
if subprocess.check_output(['git','-C',str(source),'rev-parse','HEAD'],text=True).strip()!=REVISION:
    raise SystemExit('Expected pinned Opus v1.6.1')
if subprocess.check_output(['git','-C',str(source),'status','--porcelain'],text=True).strip():
    raise SystemExit('Reference checkout must be clean')
wrapper=r'''
#include "cwrs.c"
#include "vq.h"
#if !OPUS_FAST_INT64
#error "Canonical fixture regeneration requires scalar fast-64-bit math"
#endif
int root(int x) { return celt_rsqrt_norm32(x); }
int encode_indices(int n,int k,const unsigned *indices,int count,unsigned char *data) {
    unsigned u[130];
    unsigned total=ncwrs_urow(n,k,u);
    ec_enc enc;
    ec_enc_init(&enc,data,1024);
    for(int i=0;i<count;i++) ec_enc_uint(&enc,indices[i],total);
    int len=(ec_tell(&enc)+7)/8;
    ec_enc_shrink(&enc,len);ec_enc_done(&enc);
    return ec_get_error(&enc) ? -1 : len;
}
int quantize_probe(int n,int k,int spread,int blocks,int gain,int prefix_bits,unsigned prefix_value,const int *input,int *out,unsigned char *data) {
    celt_norm x[176];for(int i=0;i<n;i++) x[i]=input[i];
    ec_enc enc;ec_enc_init(&enc,data,64);
    if(prefix_bits) ec_enc_uint(&enc,prefix_value,1U<<prefix_bits);
    out[0]=alg_quant(x,n,k,spread,blocks,&enc,gain,1,0);
    out[1]=(int)enc.rng;out[2]=ec_tell(&enc);out[3]=ec_tell_frac(&enc);
    int length=(ec_tell(&enc)+7)/8;ec_enc_shrink(&enc,length);ec_enc_done(&enc);
    if(ec_get_error(&enc))return -1;
    ec_dec dec;ec_dec_init(&dec,data,length);
    if(prefix_bits && ec_dec_uint(&dec,1U<<prefix_bits)!=prefix_value)return -2;
    out[4]=decode_pulses(out+5,n,k,&dec);
    for(int i=0;i<n;i++) out[5+n+i]=x[i];
    if(dec.rng!=enc.rng || ec_tell(&dec)!=ec_tell(&enc) || ec_tell_frac(&dec)!=ec_tell_frac(&enc))return -3;
    ec_dec_init(&dec,data,length);
    if(prefix_bits) ec_dec_uint(&dec,1U<<prefix_bits);
    celt_norm reconstructed[176];
    unsigned mask=alg_unquant(reconstructed,n,k,spread,blocks,&dec,gain);
    if(mask!=(unsigned)out[0])return -4;
    for(int i=0;i<n;i++)if(x[i]!=reconstructed[i])return -5;
    return length;
}
void decode_bands(const unsigned char *data,int len,int n,int k,int spread,int blocks,int gain,int count,int *out) {
    ec_dec dec;
    ec_dec_init(&dec,(unsigned char*)data,len);
    for(int i=0;i<count;i++) {
        int *row=out+i*(n+4);
        row[0]=alg_unquant(row+4,n,k,spread,blocks,&dec,gain);
        row[1]=dec.rng;row[2]=ec_tell(&dec);row[3]=ec_tell_frac(&dec);
    }
}
'''
u=[[0]*130 for _ in range(177)];u[0][0]=1
for n in range(1,177):
    for k in range(1,130):u[n][k]=u[n-1][k]+u[n][k-1]+u[n-1][k-1]
with tempfile.TemporaryDirectory(prefix='opura-celt-pvq-') as temp:
    path=Path(temp);(path/'probe.c').write_text(wrapper)
    subprocess.run(['cc','-shared','-fPIC','-O2','-fwrapv','-DVAR_ARRAYS','-DOPUS_BUILD','-DFIXED_POINT',
                    '-DDISABLE_FLOAT_API','-DSMALL_FOOTPRINT',*[ '-I'+str(source/p) for p in ['include','celt','silk']],
                    str(path/'probe.c'),*[str(source/'celt'/f) for f in ['vq.c','mathops.c','entcode.c','entenc.c','entdec.c']],
                    '-o',str(path/'probe.so')],check=True)
    lib=c.CDLL(str(path/'probe.so'))
    i32=c.POINTER(c.c_int);u32=c.POINTER(c.c_uint32);u8=c.POINTER(c.c_ubyte)
    for name,types,result in [('root',[c.c_int],c.c_int),
                              ('encode_indices',[c.c_int,c.c_int,u32,c.c_int,u8],c.c_int),
                              ('decode_bands',[u8,*[c.c_int]*7,i32],None),
                              ('quantize_probe',[c.c_int]*6+[c.c_uint32,i32,i32,u8],c.c_int)]:
        fn=getattr(lib,name);fn.argtypes=types;fn.restype=result
    if args.encode:
        print('# Original fixed-point alg_quant with native resynthesis/pulse decoding:',REVISION)
        print('# band N K SPREAD BLOCKS GAIN PREFIX_BITS PREFIX_VALUE HEX MASK RANGE TELL FRAC ENERGY INPUT[N] PULSES[N] RECONSTRUCTED[N]')
        print('# Initial capacity64, shrunk to ceil(tell/8) after the band; selected inputs have Q24 squared norm<=2.')
        seed=0x5155414e
        def random_i16():
            global seed
            seed^=(seed<<13)&0xffffffff;seed^=seed>>17;seed^=(seed<<5)&0xffffffff
            return c.c_int16(seed).value
        for n in [2,3,4,5,6,8,10,14,16,18,22,24,32,36,44,48,64,72,88,96,144,176]:
            for k in [1,2,3,4,8,13,36,95,128]:
                if u[n][k]+u[n][k+1]>0xffffffff:continue
                for blocks in [1,2,4,8,16]:
                    if n%blocks:continue
                    for spread in range(4):
                        for pattern in range(11):
                            amplitude=(1<<24)//(1+math.isqrt(n-1))
                            if pattern==0:values=[0]*n
                            elif pattern==1:values=[1<<24]+[0]*(n-1)
                            elif pattern==2:values=[0]*(n-1)+[-(1<<24)]
                            elif pattern==3:values=[amplitude]*n
                            elif pattern==4:values=[amplitude if i%2 else -amplitude for i in range(n)]
                            elif pattern==5:values=[1 if i%2 else -1 for i in range(n)]
                            elif pattern==6:
                                raw=[random_i16() for _ in range(n)];root=max(1,math.isqrt(sum(x*x for x in raw)))
                                values=[x*(1<<24)//root for x in raw]
                            elif pattern==7:values=[random_i16()%257-128 for _ in range(n)]
                            elif pattern==8:values=[((1<<24)*math.isqrt(2<<40))//(math.isqrt(n<<40)+1)]*n
                            elif pattern==9:values=[1,k-1]+[0]*(n-2)
                            else:values=[1,k]+[0]*(n-2)
                            if sum(x*x for x in values)>1<<49:raise SystemExit('Unexpected input energy')
                            gain=[0,1<<30,0x7fffffff][(pattern+n+k+blocks+spread)%3]
                            prefix_bits=[0,3,9][(pattern+k+spread)%3]
                            prefix_value=(n*13+k*7+pattern)%(1<<prefix_bits)
                            data=(c.c_ubyte*64)();out=(c.c_int*(5+2*n))();input_data=(c.c_int*n)(*values)
                            length=lib.quantize_probe(n,k,spread,blocks,gain,prefix_bits,prefix_value,input_data,out,data)
                            if length<1:raise SystemExit('Original quantize/decode mismatch: '+str((n,k,spread,blocks,gain,pattern,length)))
                            if list(input_data)!=values:raise SystemExit('Unexpected probe input mutation')
                            print('band',n,k,spread,blocks,gain,prefix_bits,prefix_value,bytes(data[:length]).hex(),out[0],out[1]&0xffffffff,out[2],out[3],out[4],*values,*out[5:])
        raise SystemExit(0)
    print('# Original fixed-point alg_unquant and reciprocal sqrt:',REVISION)
    print('# rootblock START COUNT STEP OFFSET FNV1A64(le-i32); stream N K SPREAD BLOCKS GAIN_Q31 HEX COUNT')
    print('# band COLLAPSE_MASK RANGE TELL TELL_FRAC Q24_VALUES[N]')
    for offset in [0,1,16383,32767]:
        for first in range(16384,65536,1024):
            fingerprint=14695981039346656037
            for value in range(first,first+1024):
                result=lib.root((value<<15)+offset)
                for byte in (result&0xffffffff).to_bytes(4,'little'):
                    fingerprint=((fingerprint^byte)*1099511628211)&0xffffffffffffffff
            print('rootblock',first<<15,1024,32768,offset,fingerprint)
    random=0x42414e44
    def next_random():
        global random
        random ^= (random<<13)&0xffffffff;random ^= random>>17;random ^= (random<<5)&0xffffffff
        return random
    for n in [2,3,4,5,6,8,10,14,16,18,22,24,32,36,44,48,64,72,88,96,144,176]:
        for k in [1,2,3,4,8,13,36,95,128]:
            total=u[n][k]+u[n][k+1]
            if total>0xffffffff:continue
            indices=sorted({0,1,total//2-1,total//2,total-2,total-1,next_random()%total})
            data=(c.c_ubyte*1024)()
            length=lib.encode_indices(n,k,(c.c_uint32*len(indices))(*indices),len(indices),data)
            if length<=0:raise SystemExit('Index encoding failed')
            for spread in range(4):
                for blocks in [1,2,4,8]:
                    if n%blocks:continue
                    for gain in [0,1<<30,0x7fffffff]:
                        out=(c.c_int*((n+4)*len(indices)))()
                        lib.decode_bands(data,length,n,k,spread,blocks,gain,len(indices),out)
                        print('stream',n,k,spread,blocks,gain,bytes(data[:length]).hex(),len(indices))
                        for i in range(len(indices)):
                            row=out[i*(n+4):(i+1)*(n+4)]
                            print('band',row[0],c.c_uint32(row[1]).value,*row[2:])
