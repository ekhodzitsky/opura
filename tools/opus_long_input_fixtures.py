#!/usr/bin/env python3
"""Optional original long-packet input, speech and actual CELT-call oracle."""
import argparse
import ctypes as c
from pathlib import Path
import re
import subprocess
import tempfile
from audio_baseline import REVISION
from opus_pcm_fixtures import pcm
from opus_application_fixtures import WRAPPER, CELT_WRAPPER

WRAPPER = WRAPPER.replace('unsigned char packet[1276];', 'unsigned char packet[7656];').replace('pcm,n,packet,640)', 'pcm,n,packet,2560)')
WRAPPER += r'''
void long_flags(OpusEncoder *s,int *out) {
 out[0]=s->silk_mode.allowBandwidthSwitch;out[1]=s->silk_mode.opusCanSwitch;out[2]=s->silk_mode.switchReady;
 if(s->application==OPUS_APPLICATION_RESTRICTED_CELT){out[3]=out[4]=0;}
 else{silk_encoder *speech=(silk_encoder*)((char*)s+s->silk_enc_offset);out[3]=speech->state_Fxx[0].sCmn.sLP.transition_frame_no;out[4]=speech->state_Fxx[0].sCmn.sLP.mode;}
 out[5]=s->silk_bw_switch;
}
'''
CELT_WRAPPER = CELT_WRAPPER.replace('calls[40]', 'calls[80]').replace('call_count+5>40', 'call_count+5>80')

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source',type=Path)
    parser.add_argument('output',type=Path)
    parser.add_argument('--calls-output',type=Path,required=True)
    args=parser.parse_args()
    source=args.source.resolve()
    if subprocess.check_output(['git','-C',str(source),'rev-parse','HEAD'],text=True).strip()!=REVISION:
        raise ValueError('Expected pinned Opus v1.6.1')
    if subprocess.check_output(['git','-C',str(source),'status','--porcelain'],text=True).strip():
        raise ValueError('Reference checkout must be unchanged')
    def manifest(file, variable):
        match = re.search(r'^'+variable+r' = (.*?)(?=\n\n|\Z)', (source/file).read_text(), re.M | re.S)
        if match is None:
            raise ValueError('Missing native source manifest')
        return match[1].replace('\\', '').split()

    files = (manifest('silk_sources.mk', 'SILK_SOURCES') + manifest('silk_sources.mk', 'SILK_SOURCES_FIXED') +
             manifest('celt_sources.mk', 'CELT_SOURCES') + manifest('opus_sources.mk', 'OPUS_SOURCES'))
    with tempfile.TemporaryDirectory(prefix='opura-long-input-') as temp:
        folder = Path(temp)
        (folder/'probe.c').write_text(WRAPPER)
        (folder/'transform.c').write_text(CELT_WRAPPER)
        subprocess.run(['cc', '-shared', '-fPIC', '-O2', '-fwrapv', '-DVAR_ARRAYS', '-DOPUS_BUILD',
                        '-DFIXED_POINT', '-DDISABLE_FLOAT_API',
                        *['-I'+str(source/p) for p in ['include', 'celt', 'silk', 'silk/fixed', 'src']],
                        str(folder/'probe.c'), str(folder/'transform.c'), *[str(source/f) for f in files if f not in ['src/opus_encoder.c','celt/celt_encoder.c']],
                        '-Wl,--wrap=silk_encode_do_VAD_FIX', '-Wl,--wrap=silk_InitEncoder', '-Wl,--wrap=celt_encode_with_ec',
                        '-lm', '-o', str(folder/'probe.so')], check=True)
        lib=c.CDLL(str(folder/'probe.so'))
        lib.application_new.argtypes=[c.c_int]*4
        lib.application_new.restype=c.c_void_p
        lib.stream_probe.argtypes=[c.c_void_p,c.POINTER(c.c_int16),*[c.c_int]*4,c.POINTER(c.c_int),c.POINTER(c.c_uint64)]
        lib.stream_probe.restype=c.c_int
        lib.application_info.argtypes=[c.c_void_p,c.POINTER(c.c_int),c.POINTER(c.c_uint64)]
        lib.opus_encoder_destroy.argtypes=[c.c_void_p]
        lib.call_trace_get.argtypes=[c.POINTER(c.c_uint64)]
        lib.call_trace_get.restype=c.c_int
        lib.long_flags.argtypes=[c.c_void_p,c.POINTER(c.c_int)]
        rows=['# Original long packets '+REVISION+' scalar fixed fast64 Q24',
              '# RATE API APP VBR INDEX MS MODE REQUEST SEED PATTERN WIRE TO_MONO INTERNAL PREV_SPEECH RS0 RS1 MID_PCM_HASH HP2 HP1 LOOKAHEAD48 DELAY_HASH BAND ALLOW CAN READY LP_POS LP_DIRECTION BW_PENDING']
        calls=['# Per packet original CELT calls: count, samples48/start/prediction/capacity/PCM_FNV16']
        def capture(rates, applications, frames, switching):
            for rate in rates:
                for channels in [1,2]:
                    for application in applications:
                        for vbr in [0,1]:
                            state=lib.application_new(rate,channels,application,vbr)
                            if not state:raise ValueError('Native application init failed')
                            try:
                                for i in range(frames):
                                    ms=[20,40,60,80,100,120][i%6]
                                    mode=1000 if switching else [1002,1000,1001,1002][i//4%4]
                                    band=[1103,1101,1103,1101][i//64%4] if switching else (1103 if mode==1000 else 1105)
                                    request=1+(channels-1)*(i//7%2)
                                    seed=(0x61707073+i*982451653+rate*ms)&0xffffffff
                                    pattern=0 if (switching and i%32<16) or 16<=i<24 else 1 if i%7==0 else 3
                                    n=rate*ms//1000
                                    values=(c.c_int16*(n*channels))(*pcm(seed,n*channels,pattern))
                                    info=(c.c_int*4)();hashes=(c.c_uint64*3)()
                                    length=lib.stream_probe(state,values,n,mode,request,band,info,hashes)
                                    if length<1:raise ValueError(f'Native failed {rate}/{channels}/{application}/{vbr}/{i}: {length}')
                                    hp=(c.c_int*3)();delay=(c.c_uint64*1)()
                                    lib.application_info(state,hp,delay)
                                    flags=(c.c_int*6)();lib.long_flags(state,flags)
                                    rows.append(' '.join(map(str,[rate,channels,application,vbr,i,ms,mode,request,seed,pattern,*info,*hashes,*hp,*delay,band,*flags])))
                                    trace=(c.c_uint64*80)();count=lib.call_trace_get(trace)
                                    calls.append(' '.join(map(str,[count//5,*trace[:count]])))
                            finally:lib.opus_encoder_destroy(state)
        capture([8000,12000,16000,24000,48000],[2049,2048,2051,2052,2053],32,False)
        capture([16000,48000],[2049],256,True)
        args.output.write_text('\n'.join(rows)+'\n')
        args.calls_output.write_text('\n'.join(calls)+'\n')
        print('Captured',len(rows)-2,'original long packet histories')

if __name__ == '__main__':
    main()
