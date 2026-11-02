#!/usr/bin/env python3
"""Optional unchanged scalar Opus frontend and stereo-width control oracles."""
import argparse
import ctypes as c
from pathlib import Path
import re
import subprocess
import tempfile
from audio_baseline import REVISION


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source',type=Path)
    parser.add_argument('output',type=Path)
    parser.add_argument('--width-output',type=Path)
    args=parser.parse_args()
    source=args.source.resolve()
    if subprocess.check_output(['git','-C',str(source),'rev-parse','HEAD'],text=True).strip()!=REVISION:
        raise ValueError('Expected pinned Opus v1.6.1')
    if subprocess.check_output(['git','-C',str(source),'status','--porcelain'],text=True).strip():
        raise ValueError('Reference checkout must be unchanged')
    text=(source/'src/opus_encoder.c').read_text()
    start=text.index('    /* Equivalent 20-ms rate for mode/channel/bandwidth decisions */')
    marker='        st->mode = MODE_SILK_ONLY;'
    end=text.index(marker,text.index('    if (st->mode == MODE_HYBRID && curr_bandwidth <= OPUS_BANDWIDTH_WIDEBAND)',start))+len(marker)
    decisions=text[start:end]
    def manifest(file,variable):
        match=re.search(r'^'+variable+r' = (.*?)(?=\n\n|\Z)',(source/file).read_text(),re.M|re.S)
        if match is None:raise ValueError('Missing native source manifest')
        return match[1].replace('\\','').split()
    files=manifest('silk_sources.mk','SILK_SOURCES')+manifest('silk_sources.mk','SILK_SOURCES_FIXED')+manifest('celt_sources.mk','CELT_SOURCES')+manifest('opus_sources.mk','OPUS_SOURCES')
    wrapper=r'''
#include "opus_encoder.c"
#if !OPUS_FAST_INT64 || DB_SHIFT != 24
#error "Expected scalar fast64 fixed-point Q24 reference"
#endif
int frontend_probe(const int *v,int *out){
 int applications[]={OPUS_APPLICATION_AUDIO,OPUS_APPLICATION_VOIP,OPUS_APPLICATION_RESTRICTED_LOWDELAY,OPUS_APPLICATION_RESTRICTED_SILK,OPUS_APPLICATION_RESTRICTED_CELT};
 int signals[]={OPUS_AUTO,OPUS_SIGNAL_VOICE,OPUS_SIGNAL_MUSIC};
 int error;OpusEncoder *st=opus_encoder_create(v[0],v[1],applications[v[5]],&error);if(!st||error)return -1;
 st->bitrate_bps=v[3];st->signal_type=signals[v[6]];
 st->user_forced_mode=v[7]<0?OPUS_AUTO:MODE_SILK_ONLY+v[7];
 st->force_channels=v[8]==0?OPUS_AUTO:v[8];st->max_bandwidth=OPUS_BANDWIDTH_NARROWBAND+v[9];
 st->user_bandwidth=v[10]<0?OPUS_AUTO:OPUS_BANDWIDTH_NARROWBAND+v[10];
 st->use_vbr=v[11];st->silk_mode.complexity=v[12];st->silk_mode.packetLossPercentage=v[13];
 st->fec_config=v[14];st->silk_mode.useInBandFEC=v[14]!=0;st->use_dtx=v[15];
 st->first=v[18];st->prev_mode=v[19]<0?0:MODE_SILK_ONLY+v[19];st->prev_channels=v[20];
 st->stream_channels=v[21];st->bandwidth=OPUS_BANDWIDTH_NARROWBAND+v[22];
 st->auto_bandwidth=v[23]<0?0:OPUS_BANDWIDTH_NARROWBAND+v[23];
 st->silk_mode.allowBandwidthSwitch=v[24];st->silk_mode.inWBmodeWithoutVariableLP=v[25];
 st->silk_mode.LBRR_coded=v[26];st->silk_mode.toMono=v[27];st->voice_ratio=-1;
 int frame_size=v[2]*st->Fs/48000,frame_rate=st->Fs/frame_size,max_data_bytes=v[4];
 opus_int32 max_rate=bits_to_bitrate(max_data_bytes*8,st->Fs,frame_size),equiv_rate;
 opus_val16 stereo_width=v[17];int is_silence=v[16],voice_est,curr_bandwidth,i;
 int redundancy=0,celt_to_silk=0,to_celt=0,prefill=0,lsb_depth=16;
 void *silk_enc=(char*)st+st->silk_enc_offset;
 CELTEncoder *celt_enc=st->application==OPUS_APPLICATION_RESTRICTED_SILK?NULL:(CELTEncoder*)((char*)st+st->celt_enc_offset);
DECISIONS
 out[0]=st->mode-MODE_SILK_ONLY;out[1]=st->stream_channels;out[2]=st->bandwidth-OPUS_BANDWIDTH_NARROWBAND;
 out[3]=st->auto_bandwidth?st->auto_bandwidth-OPUS_BANDWIDTH_NARROWBAND:-1;
 out[4]=st->silk_mode.LBRR_coded;out[5]=st->silk_mode.toMono;out[6]=redundancy;
 out[7]=celt_to_silk;out[8]=to_celt;out[9]=prefill;out[10]=equiv_rate;out[11]=voice_est;out[12]=st->silk_mode.useDTX;
 opus_encoder_destroy(st);return 0;
}
int width_probe(int Fs,int n,const opus_int16 *pcm,int *state){
 StereoWidthState mem={state[0],state[1],state[2],state[3],state[4]};
 int result=compute_stereo_width(pcm,n,Fs,&mem);
 state[0]=mem.XX;state[1]=mem.XY;state[2]=mem.YY;state[3]=mem.smoothed_width;state[4]=mem.max_follower;
 return result;
}
'''.replace('DECISIONS',decisions)
    with tempfile.TemporaryDirectory(prefix='opura-opus-frontend-') as temp:
        folder=Path(temp)
        (folder/'probe.c').write_text(wrapper)
        subprocess.run(['cc','-shared','-fPIC','-O2','-fwrapv','-DVAR_ARRAYS','-DOPUS_BUILD','-DFIXED_POINT','-DDISABLE_FLOAT_API',
                        *['-I'+str(source/p) for p in ['include','celt','silk','silk/fixed','src']],str(folder/'probe.c'),
                        *[str(source/f) for f in files if f!='src/opus_encoder.c'],'-lm','-o',str(folder/'probe.so')],check=True)
        lib=c.CDLL(str(folder/'probe.so'))
        lib.frontend_probe.argtypes=[c.POINTER(c.c_int),c.POINTER(c.c_int)];lib.frontend_probe.restype=c.c_int
        lib.width_probe.argtypes=[c.c_int,c.c_int,c.POINTER(c.c_int16),c.POINTER(c.c_int)];lib.width_probe.restype=c.c_int
        seed=0x4f505553
        def random():
            nonlocal seed
            seed^=(seed<<13)&0xffffffff;seed^=seed>>17;seed^=(seed<<5)&0xffffffff
            return seed
        rates=[8000,12000,16000,24000,48000]
        durations=[120,240,480,960,1920,2880,3840,4800,5760]
        bitrates=[2400,6000,6999,7000,8999,9000,10000,11000,12000,13500,14000,15000,16000,17000,18000,19000,20000,24000,32000,44000,48000,64000,128000,750000,1500000]
        with args.output.open('w') as out:
            out.write('# Original scalar Opus frontend normal-admission decisions '+REVISION+'\n')
            out.write('#28 inputs: Fs API_C SAMPLES48 BITRATE BYTES APP SIGNAL FORCE_MODE FORCE_C MAX_BAND BAND VBR COMPLEXITY LOSS FEC DTX SILENCE WIDTH FIRST PREV_MODE PREV_C STREAM_C CURRENT_BAND AUTO_BAND SWITCH_ALLOWED IN_WB PREV_FEC TO_MONO;13 outputs\n')
            for row in range(16384):
                fs=rates[random()%5];channels=1+random()%2;duration=durations[random()%9];application=random()%5
                if application==3 and duration<480:duration=480
                n=duration*fs//48000;fps=fs//n
                minimum=3 if fps>=50 else (300+fps-1)//fps
                capacity=max(minimum,[3,8,16,32,64,128,256,640,1276,2552,7656][random()%11])
                variable=random()%2
                bitrate=min(max(bitrates[random()%len(bitrates)],(minimum*8*fs+n-1)//n),750000*channels,capacity*8*fs//n)
                if not variable:
                    capacity=min(capacity,(bitrate*n//fs+4)//8)
                    bitrate=capacity*8*fs//n
                first=random()%2
                previous=[-1,0,1,2][random()%4]
                if application in [2,4]:previous=[-1,2][random()%2]
                if application==3:previous=[-1,0][random()%2]
                forced_channels=0 if channels==1 else random()%3
                previous_channels=0 if previous<0 else channels if channels==1 else 1+random()%2
                current_channels=channels if channels==1 else 1+random()%2
                values=[fs,channels,duration,bitrate,capacity,application,random()%3,[-1,0,1,2][random()%4],forced_channels,random()%5,[-1,0,1,2,3,4][random()%6],variable,[0,1,2,4,5,6,10][random()%7],[0,1,5,6,20,25,50,100][random()%8],random()%3,random()%2,random()%2,[0,1,16384,32766,32767][random()%5],first,previous,previous_channels,current_channels,random()%5,-1 if first else random()%5,random()%2,random()%2,random()%2,random()%2]
                result=(c.c_int*13)()
                if lib.frontend_probe((c.c_int*28)(*values),result):raise ValueError('Original frontend probe failed')
                out.write(' '.join(map(str,[*values,*result]))+'\n')
            # At10ms, forcing stereo input to mono changes overhead correction.
            # These normal-admission cases straddle the resulting mode boundary;
            # random snapshots alone did not detect omitting that recalculation.
            for width in [0,1,16384,32766,32767]:
                for bitrate in [15000,17500,18000,20000,20591,20592,20593,21000,22591,22592,22593,23000]:
                    values=[48000,2,480,bitrate,1000,0,0,-1,1,4,-1,1,10,0,0,0,0,width,1,-1,0,2,4,-1,0,0,0,0]
                    result=(c.c_int*13)()
                    if lib.frontend_probe((c.c_int*28)(*values),result):raise ValueError('Original boundary probe failed')
                    out.write(' '.join(map(str,[*values,*result]))+'\n')
        if args.width_output:
            with args.width_output.open('w') as out:
                out.write('# Original scalar stereo-width analysis '+REVISION+'\n')
                out.write('# Fs SAMPLES48 FRAME PATTERN OUTPUT XX XY YY SMOOTHED FOLLOWER\n')
                for fs in rates:
                    for duration in durations:
                        n=duration*fs//48000;state=(c.c_int*5)()
                        for frame in range(96):
                            pattern=frame//8
                            values=[]
                            for i in range(n):
                                phase=(frame*n+i)%80
                                base=(min(phase,80-phase)-20)*1200
                                pair=(0,0) if pattern in [0,11] else (base,base) if pattern==1 else (base,-base) if pattern==2 else (base,base//4) if pattern==3 else (base,0) if pattern==4 else (0,base) if pattern==5 else (32767,-32768) if pattern==6 else (32767,32767) if pattern==7 else (((i*719+frame*113)%65536)-32768,((i*317+frame*991)%65536)-32768) if pattern==8 else (1,0) if pattern==9 else (base,(((phase+17)%80)-40)*400)
                                values.extend(pair)
                            result=lib.width_probe(fs,n,(c.c_int16*(2*n))(*values),state)
                            out.write(' '.join(map(str,[fs,duration,frame,pattern,result,*state]))+'\n')
    print('Captured 16444 unchanged frontend decisions'+(' and 4320 stereo-width histories' if args.width_output else ''))


if __name__=='__main__':
    main()
