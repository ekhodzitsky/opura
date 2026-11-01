#!/usr/bin/env python3
"""Generate standard MDCT/window tables and original forward or chained inverse output.

Optional pinned external oracle. Read-only instrumentation observes original
headroom decisions; all transforms remain original mdct.c/FFT code.
"""
import argparse
import ctypes as c
from pathlib import Path
import subprocess
import tempfile
REVISION='22244de5a79bd1d6d623c32e72bf1954b56235be'
parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('source',type=Path);parser.add_argument('--tables',action='store_true')
parser.add_argument('--forward',action='store_true')
args=parser.parse_args();source=args.source.resolve()
if args.forward and args.tables: parser.error('--forward and --tables are mutually exclusive')
if subprocess.check_output(['git','-C',str(source),'rev-parse','HEAD'],text=True).strip()!=REVISION:
    raise SystemExit('Expected pinned Opus v1.6.1')
if subprocess.check_output(['git','-C',str(source),'status','--porcelain'],text=True).strip():
    raise SystemExit('Reference checkout must be clean')
wrapper=r'''
#include "mdct.h"
extern const opus_int16 eband5ms[];
extern const unsigned char band_allocation[];
#include "static_modes_fixed.h"
#if !OPUS_FAST_INT64
#error "Canonical MDCT oracle requires scalar fast-64-bit math"
#endif
static const mdct_lookup lookup={1920,3,{&fft_state48000_960_0,&fft_state48000_960_1,&fft_state48000_960_2,&fft_state48000_960_3},mdct_twiddles960};
static int observed[3];
void observe_mdct_shifts(int pre,int post,int fft) {observed[0]=pre;observed[1]=post;observed[2]=fft;}
void observe_forward_headroom(int value) {observed[0]=value;}
int table_value(int kind,int i) {return kind ? window120[i] : mdct_twiddles960[i];}
void synthesize(int lm,int stride,opus_int32 *input,opus_int32 *overlap,opus_int32 *pcm,opus_int32 *meta) {
    opus_int32 out[1020]={0};
    int n=120<<lm;
    for(int i=0;i<60;i++) out[i]=overlap[i];
    clt_mdct_backward_c(&lookup,input,out,window120,120,3-lm,stride,0);
    for(int i=0;i<n;i++) pcm[i]=out[i];
    for(int i=0;i<60;i++) overlap[i]=out[n+i];
    for(int i=0;i<3;i++) meta[i]=observed[i];
}
void analyze(int lm,int stride,opus_int32 *input,opus_int32 *output,opus_int32 *meta) {
    for(int i=0;i<960;i++) output[i]=19088743;
    clt_mdct_forward_c(&lookup,input,output,window120,120,3-lm,stride,0);
    meta[0]=observed[0];
}
'''
with tempfile.TemporaryDirectory(prefix='opura-celt-mdct-') as temp:
    path=Path(temp);(path/'probe.c').write_text(wrapper)
    mdct=(source/'celt/mdct.c').read_text()
    anchor='      fft_shift = pre_shift - post_shift;'
    if mdct.count(anchor)!=1:raise SystemExit('Unexpected MDCT headroom observation site')
    mdct=mdct.replace('#include "mdct.h"','#include "mdct.h"\nextern void observe_mdct_shifts(int,int,int);\nextern void observe_forward_headroom(int);')
    mdct=mdct.replace(anchor,anchor+'\n      observe_mdct_shifts(pre_shift,post_shift,fft_shift);')
    forward_anchor='      headroom = IMAX(0, IMIN(scale_shift, 28-celt_ilog2(maxval)));'
    if mdct.count(forward_anchor)!=1:raise SystemExit('Unexpected forward headroom observation site')
    mdct=mdct.replace(forward_anchor,forward_anchor+'\n      observe_forward_headroom(headroom);')
    (path/'mdct.c').write_text(mdct)
    subprocess.run(['cc','-shared','-fPIC','-O2','-fwrapv','-DFIXED_POINT','-DVAR_ARRAYS','-DOPUS_BUILD','-DDISABLE_FLOAT_API',
                    '-I'+str(source/'include'),'-I'+str(source/'celt'),str(path/'probe.c'),str(path/'mdct.c'),str(source/'celt/kiss_fft.c'),
                    '-o',str(path/'probe.so')],check=True)
    lib=c.CDLL(str(path/'probe.so'));i32=c.POINTER(c.c_int32)
    lib.table_value.argtypes=[c.c_int,c.c_int];lib.table_value.restype=c.c_int
    lib.synthesize.argtypes=[c.c_int,c.c_int,i32,i32,i32,i32];lib.synthesize.restype=None
    lib.analyze.argtypes=[c.c_int,c.c_int,i32,i32,i32];lib.analyze.restype=None
    if args.tables:
        print('// Generated from pinned Opus v1.6.1 by tools/celt_mdct_fixtures.py --tables.')
        print('// Copyright (c) 2007-2008 CSIRO; 2007-2009 Xiph.Org Foundation.')
        print('// See THIRD_PARTY_NOTICES.md.')
        print('pub(super) const TRIG: [i16;1800] = '+str([lib.table_value(0,i) for i in range(1800)])+';')
        print('pub(super) const WINDOW: [i16;120] = '+str([lib.table_value(1,i) for i in range(120)])+';')
        raise SystemExit(0)
    if args.forward:
        print('# Original clt_mdct_forward_c standard fixed-point fast64:',REVISION)
        print('# frame LM STRIDE OUTPUT_LEN HEADROOM INPUT_COUNT INPUT[...] OUTPUT[...]')
        random=0x4657444d
        def random_i32():
            global random
            random ^= (random<<13)&0xffffffff;random ^= random>>17;random ^= (random<<5)&0xffffffff
            return c.c_int32(random).value
        for lm in range(4):
            n=120<<lm
            for stride in [1,2,4,8]:
                if n*stride>960:continue
                for pattern in range(29):
                    values=[]
                    for i in range(n+120):
                        if pattern==0:value=0
                        elif pattern<7:value=(1<<24) if i==[0,59,60,n-1,n+60,n+119][pattern-1] else 0
                        elif pattern==7:value=0x7fffffff
                        elif pattern==8:value=-0x80000000
                        elif pattern==9:value=0x7fffffff if i%2 else -0x80000000
                        elif pattern==10:value=1
                        elif pattern==11:value=-1
                        else:value=random_i32()>>(3 if pattern==28 else 2*(pattern-12))
                        values.append(value)
                    length=n*stride-(stride-1 if pattern%2 else 0)
                    data=(c.c_int32*len(values))(*values);output=(c.c_int32*960)();meta=(c.c_int32*1)()
                    lib.analyze(lm,stride,data,output,meta)
                    if list(data)!=values:raise SystemExit('Unexpected forward input mutation')
                    print('frame',lm,stride,length,meta[0],len(values),*values,*output[:length])
        # Sequential short-block analysis shares overlapped source context and
        # writes successive offsets of one interleaved spectral frame.
        for blocks in [2,4,8]:
            values=[random_i32()>>8 for _ in range(120*blocks+120)]
            for block in range(blocks):
                part=values[120*block:120*block+240]
                data=(c.c_int32*240)(*part);output=(c.c_int32*960)();meta=(c.c_int32*1)()
                lib.analyze(0,blocks,data,output,meta)
                length=120*blocks-block
                print('frame',0,blocks,length,meta[0],240,*part,*output[:length])
        raise SystemExit(0)
    print('# Original clt_mdct_backward_c with standard lookup/window:',REVISION)
    print('# frame RESET LM STRIDE INPUT_COUNT PRE_SHIFT POST_SHIFT FFT_SHIFT INPUT[...] PCM[120<<LM] OVERLAP[60]')
    random=0x4d444354
    def next_random():
        global random
        random ^= (random<<13)&0xffffffff;random ^= random>>17;random ^= (random<<5)&0xffffffff
        return c.c_int32(random).value
    overlap=(c.c_int32*60)()
    def emit(reset,lm,stride,values):
        if reset:
            for i in range(60):overlap[i]=0
        data=(c.c_int32*len(values))(*values);pcm=(c.c_int32*960)();meta=(c.c_int32*3)()
        lib.synthesize(lm,stride,data,overlap,pcm,meta)
        if list(data)!=values:raise SystemExit('Unexpected reference input mutation')
        print('frame',int(reset),lm,stride,len(values),*meta,*values,*pcm[:120<<lm],*overlap)
    for sequence in range(8):
        for index in range(48):
            lm=(index+sequence)%4;n=120<<lm
            strides=[s for s in [1,2,4,8] if n*s<=960];stride=strides[(index//4+sequence)%len(strides)]
            length=n*stride-(stride-1 if index%2 else 0)
            values=[];pattern=(index+sequence*3)%12
            for i in range(length):
                if pattern==0:value=0
                elif pattern==1:value=1<<20 if i==0 else 0
                elif pattern==2:value=-(1<<22) if i==(n-1)*stride else 0
                elif pattern==3:value=1
                elif pattern==4:value=-1
                elif pattern==5:value=0x7fffffff
                elif pattern==6:value=-0x80000000
                elif pattern==7:value=0x7fffffff if i%2 else -0x80000000
                else:value=next_random()>>[0,8,16,24][pattern-8]
                values.append(value)
            emit(index in [0,17,33],lm,stride,values)
    for blocks in [2,4,8]:
        values=[next_random()>>8 for _ in range(120*blocks)]
        for block in range(blocks):emit(block==0,0,blocks,values[block:])
