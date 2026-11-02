#!/usr/bin/env python3
"""Original classic CELT PLC downsampling/whitening and pitch-search oracle.

Compact deterministic integer signal recipes are fixture inputs; every expected
whitened sample and period comes from unchanged pinned scalar pitch/LPC code.
"""
import argparse
import ctypes as c
from pathlib import Path
import subprocess
import tempfile
REVISION='22244de5a79bd1d6d623c32e72bf1954b56235be'
parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('source',type=Path)
mode=parser.add_mutually_exclusive_group();mode.add_argument('--encoder',action='store_true');mode.add_argument('--encoder-math',action='store_true');mode.add_argument('--refinement',action='store_true')
args=parser.parse_args();source=args.source.resolve()
if subprocess.check_output(['git','-C',str(source),'rev-parse','HEAD'],text=True).strip()!=REVISION:raise SystemExit('Expected pinned Opus v1.6.1')
if subprocess.check_output(['git','-C',str(source),'status','--porcelain'],text=True).strip():raise SystemExit('Reference checkout must be clean')
wrapper=r'''
#include "arch.h"
#include "pitch.h"
#include "pitch.c"
#if !OPUS_FAST_INT64
#error "Canonical pitch fixtures require scalar fast-64-bit math"
#endif
void analyse(int *left,int *right,int channels,int *out) {
    celt_sig *history[2]={left,right};opus_int16 lp[1024];int index;
    pitch_downsample(history,lp,1024,channels,2,0);
    pitch_search(lp+360,lp,1328,620,&index,0);
    out[0]=720-index;
    for(int i=0;i<1024;i++) out[i+1]=lp[i];
}
void analyse_encoder(int LM,int *left,int *right,int channels,int previous,int gain,int *out){
    int N=120<<LM,index;int length=(1024+N)/2;
    celt_sig *history[2]={left,right};opus_int16 lp[992];
    pitch_downsample(history,lp,length,channels,2,0);
    pitch_search(lp+512,lp,N,979,&index,0);out[0]=1024-index;
    int overflow=0;for(int lag=0;lag<=512;lag++){
        opus_int64 energy=0;for(int i=0;i<N/2;i++)energy+=(opus_int64)lp[512-lag+i]*lp[512-lag+i];
        if(energy>2147483647)overflow=1;
    }
    index=out[0];out[1]=overflow?0:remove_doubling(lp,1024,15,N,&index,previous,gain,0);out[2]=index;out[3]=overflow;
    for(int i=0;i<length;i++)out[4+i]=lp[i];
}
int encoder_pitch_gain(int xy,int xx,int yy){return compute_pitch_gain(xy,xx,yy);}
int encoder_refinement(int LM,int period,int previous,int gain,const opus_int16 *input,int *out){
    opus_int16 values[992];int length=(1024+(120<<LM))/2;
    for(int i=0;i<length;i++)values[i]=input[i];
    out[0]=remove_doubling(values,1024,15,120<<LM,&period,previous,gain,0);out[1]=period;return 0;
}
'''
def trunc(a,b):return -(abs(a)//b) if a<0 else a//b
def signal(period,shape,amplitude,offset,seed,length=2048):
    values=[]
    for i in range(length):
        seed=(1664525*seed+1013904223)&0xffffffff
        phase=(i+offset)%period
        if shape==0:
            triangle=min(phase,period-phase)
            value=trunc(amplitude*(4*triangle-period),period)
        elif shape==1:value=amplitude if phase<period//2 else -amplitude
        elif shape==2:value=amplitude if phase==0 else 0
        elif shape==3:value=trunc(amplitude*(2*phase-period),period)
        elif shape==4:value=(c.c_int32(seed).value*amplitude)>>31
        elif shape==5:value=0
        elif shape==6:value=amplitude
        elif shape==7:value=amplitude if i%2 else -amplitude
        elif shape==16:value=amplitude if phase in [0,(period+1)//3] else 0
        else:
            triangle=min(phase,period-phase)
            base=trunc(amplitude*(4*triangle-period),period)
            noise=(c.c_int32(seed).value*amplitude)>>31
            weight=shape-7
            value=trunc(base*weight+noise*(8-weight),8)
        values.append(value)
    return values
with tempfile.TemporaryDirectory(prefix='opura-celt-pitch-') as temp:
    path=Path(temp);(path/'probe.c').write_text(wrapper)
    subprocess.run(['cc','-shared','-fPIC','-O2','-fwrapv','-DFIXED_POINT','-DVAR_ARRAYS','-DOPUS_BUILD','-DDISABLE_FLOAT_API',
                    '-I'+str(source/'include'),'-I'+str(source/'celt'),str(path/'probe.c'),
                    *[str(source/'celt'/f) for f in ['celt_lpc.c','mathops.c','entcode.c']],'-o',str(path/'probe.so')],check=True)
    lib=c.CDLL(str(path/'probe.so'));ptr=c.POINTER(c.c_int32);lib.analyse.argtypes=[ptr,ptr,c.c_int,ptr];lib.analyse.restype=None
    if args.refinement:
        lib.encoder_refinement.argtypes=[c.c_int]*4+[c.POINTER(c.c_int16),ptr];lib.encoder_refinement.restype=c.c_int
        print('# Original isolated remove_doubling over bounded whitened recipes:',REVISION)
        print('# refine LM PERIOD SHAPE AMPLITUDE OFFSET SEED COARSE PREVIOUS GAIN RAW_GAIN REFINED')
        for lm in range(4):
            for period in [7,8,14,15,21,22,30,31,41,42,47,60,85,120,170,171,172,240,341,511]:
                for shape in range(17):
                    for coarse in sorted({47,48,49,60,80,81,82,83,84,89,90,120,240,511,512,681,682,683,684,1023,1024,max(47,min(1024,2*period))}):
                        seed=0x52454649+period+shape;amplitude=[0,1,31,127,511,1024,1023,1024][shape] if shape<8 else 1024;offset=period//2
                        values=signal(period,shape,amplitude,offset,seed,(1024+(120<<lm))//2)
                        third=2*((coarse+3)//6)
                        for previous,gain in [(0,0),(15,24576),(max(15,min(1022,period*2-4)),24576),(max(15,min(1022,period*2)),24576),(max(15,min(1022,period*2+4)),24576),(max(15,min(1022,third-4)),24576),(max(15,min(1022,third+4)),24576)]:
                            out=(c.c_int32*2)();error=lib.encoder_refinement(lm,coarse,previous,gain,(c.c_int16*len(values))(*values),out)
                            if error:raise SystemExit('Original refinement failed')
                            print('refine',lm,period,shape,amplitude,offset,seed,coarse,previous,gain,*out)
        # Minimized one-unit gain-threshold regression (0.3 Q15 = 9830).
        # A local mutated-copy search found this seed; expected output is always
        # emitted solely by unchanged original remove_doubling.
        values=signal(60,10,1024,30,8382,572);out=(c.c_int32*2)()
        if lib.encoder_refinement(0,240,0,0,(c.c_int16*572)(*values),out):raise SystemExit('Original threshold regression failed')
        print('refine',0,60,10,1024,30,8382,240,0,0,*out)
        raise SystemExit
    if args.encoder_math:
        import math
        lib.encoder_pitch_gain.argtypes=[c.c_int]*3;lib.encoder_pitch_gain.restype=c.c_int
        print('# Original normalized encoder pitch gain:',REVISION)
        print('# gain XY XX YY GAIN')
        energies=sorted({0,1,2,3,7,15,31,127,255,1023,4095,16383,16384,16385,32767,32768,32769,65535,65536,65537,1<<20,1<<24,1<<28,2147483647,*[max(1,(1<<k)+d) for k in range(31) for d in [-1,0,1]]})
        for xx in energies:
            for yy in energies:
                root=math.isqrt(xx*yy)
                for xy in sorted({-2147483648,-2147483647,-root,(-root)//2,-1,0,1,root//2,root,2147483647}):
                    print('gain',xy,xx,yy,lib.encoder_pitch_gain(xy,xx,yy))
        raise SystemExit
    if args.encoder:
        lib.analyse_encoder.argtypes=[c.c_int,ptr,ptr,c.c_int,c.c_int,c.c_int,ptr];lib.analyse_encoder.restype=None
        print('# Original encoder whitening/search/remove_doubling:',REVISION)
        print('# pitch LM INPUT_PERIOD SHAPE AMPLITUDE CHANNELS SIDE OFFSET SEED PREVIOUS GAIN COARSE RAW_GAIN REFINED OVERFLOW WHITENED_FNV64_LE16')
        rows=0;overflows=0
        histories=[(0,0),(15,0),(15,24576),(16,3072),(31,12288),(60,24576),(120,18432),(240,6144),(511,21504),(1022,24576)]
        def emit_encoder(lm,period,shape,amplitude,channels,side,offset,seed,previous,gain):
            global rows,overflows
            length=1024+(120<<lm);left=signal(period,shape,amplitude,offset,seed,length)
            right=left if side==0 else [-v for v in left] if side==1 else signal(period if side==2 else period//2+37,shape,amplitude,offset+period//3,seed^0x9e3779b9,length)
            out=(c.c_int32*996)();lib.analyse_encoder(lm,(c.c_int32*length)(*left),(c.c_int32*length)(*right),channels,previous,gain,out)
            fingerprint=14695981039346656037
            for value in out[4:4+length//2]:
                for byte in (value&65535).to_bytes(2,'little'):fingerprint=((fingerprint^byte)*1099511628211)&0xffffffffffffffff
            print('pitch',lm,period,shape,amplitude,channels,side,offset,seed,previous,gain,*out[:4],fingerprint);rows+=1;overflows+=out[3]
            return min(1022,out[2]),max(0,min(24576,out[1]))
        for lm in range(4):
            for period in range(15,1024):
                previous,gain=histories[period%len(histories)]
                emit_encoder(lm,period,period%8,[0,1,31,4096,1<<20,1<<27,1<<28,536870911][(period//8)%8],1+period%2,period%4,period//3,0x454e4350+period,previous,gain)
            for period in [15,16,21,22,31,42,45,46,47,60,120,240,511,512,719,1022,1023]:
                for shape in range(8):
                    for channels in [1,2]:
                        for delta,gain in [(-4,3072),(-2,12288),(-1,24576),(0,24576),(1,12288),(2,3072),(4,24576)]:
                            previous=max(15,min(1022,period+delta))
                            emit_encoder(lm,period,shape,[0,1,31,4096,1<<20,1<<27,1<<28,536870911][shape],channels,(shape+period)%4,period//2,0x444f5542+period+shape,previous,gain)
        for lm in range(4):
            for channels in [1,2]:
                print('sequence',lm,channels)
                previous,gain=0,0
                for frame in range(40):
                    period=[15,31,60,120,240,511,1022,173][frame%8]
                    previous,gain=emit_encoder(lm,period,frame%8,[1,4096,1<<20,1<<28][frame%4],channels,frame%4,frame*(120<<lm),0x53455150+frame,previous,gain)
        print(f'# rows={rows} overflows={overflows}')
        raise SystemExit
    print('# Original classic PLC pitch pipeline:',REVISION)
    print('# pitch INPUT_PERIOD SHAPE AMPLITUDE CHANNELS SIDE_MODE OFFSET SEED PERIOD WHITENED[1024]')
    def emit(period,shape,amplitude,channels,side,offset,seed):
        left=signal(period,shape,amplitude,offset,seed)
        right=left if side==0 else [-v for v in left] if side==1 else signal(period if side==2 else period//2+37,shape,amplitude,offset+period//3,seed^0x9e3779b9)
        out=(c.c_int32*1025)();lib.analyse((c.c_int32*2048)(*left),(c.c_int32*2048)(*right),channels,out)
        print('pitch',period,shape,amplitude,channels,side,offset,seed,*out)
    for period in range(100,721):
        for channels in [1,2]:
            emit(period,period%4,[1,4096,1<<20,536870911][(period//4)%4],channels,period%4,period//3,0x50495443+period)
    for period in [100,101,119,120,127,128,173,239,255,256,360,511,512,719,720]:
        for shape in range(8):
            for channels in [1,2]:
                for side in range(4):
                    emit(period,shape,[0,1,31,4095,4096,1<<20,1<<28,536870911][shape],channels,side,period//2,0x434f4e43+period+shape)
