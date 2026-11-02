#!/usr/bin/env python3
"""Optional original Opus fixed-point AUDIO bandwidth/FEC policy oracle."""
import argparse
from pathlib import Path
import subprocess
import tempfile
from silk_nsq_fixtures import REVISION
from silk_encoder_fec_policy import function


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source', type=Path)
    parser.add_argument('output', type=Path)
    args = parser.parse_args()
    source = args.source.resolve()
    if subprocess.check_output(['git', '-C', str(source), 'rev-parse', 'HEAD'], text=True).strip() != REVISION:
        raise SystemExit('Expected pinned Opus v1.6.1')
    if subprocess.check_output(['git', '-C', str(source), 'status', '--porcelain'], text=True).strip():
        raise SystemExit('Reference checkout must be unchanged')
    opus = (source/'src/opus_encoder.c').read_text()
    arrays = '\n'.join(function(opus, 'static const opus_int32 '+name)+';' for name in [
        'mono_voice_bandwidth_thresholds[8]', 'mono_music_bandwidth_thresholds[8]',
        'stereo_voice_bandwidth_thresholds[8]', 'stereo_music_bandwidth_thresholds[8]', 'fec_thresholds[]'])
    selection = function(opus, '    /* Automatic (rate-dependent) bandwidth selection */')
    api_bounds = opus[opus.index('    /* Prevents Opus from wasting bits'):opus.index('#ifndef DISABLE_FLOAT_API\n    /* Use detected bandwidth')]
    desired = opus[opus.index('        if (curr_bandwidth == OPUS_BANDWIDTH_NARROWBAND) {'):opus.index('        st->silk_mode.useCBR =')]
    shim = r'''
#include "main_FIX.h"
#include "API.h"
#include "opus_private.h"
#include <stdio.h>
typedef struct { int channels, force_channels, mode, first, auto_bandwidth, bandwidth, Fs;
 struct { int allowBandwidthSwitch,inWBmodeWithoutVariableLP,desiredInternalSampleRate,minInternalSampleRate,maxInternalSampleRate; } silk_mode; } State;
'''
    program = shim+arrays+'\n'+function(opus,'static opus_int32 compute_equiv_rate(')+'\n'+function(opus,'static int decide_fec(')+r'''
int main(int argc,char **argv){
 if(argc!=2)return 1;FILE *out=fopen(argv[1],"w");if(!out)return 2;
 fprintf(out,"# Original automatic Opus fixed-point bandwidth/FEC policy REVISION\n");
 int apis[]={8000,12000,16000,24000,48000},durations[]={10,20,40,60},losses[]={0,1,5,6,20,100},complexities[]={0,1,2,6,10},voices[]={0,48,115,127};
 int budgets[]={3,8,12,13,14,15,17,18,19,20,21,24,25,26,30,35,40,45,50,60,80,100,160,256,512,1276};int id=0;
 for(int api=0;api<5;api++)for(int channels=1;channels<=2;channels++)for(int duration=0;duration<4;duration++)for(int previous=0;previous<2;previous++)for(int loss=0;loss<6;loss++)for(int budget=0;budget<26;budget++,id++){
  State state={0},*st=&state;int initialized=id%7!=0,allowed=id%3!=0,inwb=id%5!=0,voice_est=voices[id%4],vbr=id%2,complexity=complexities[id%5],enabled=id%7!=0;
  st->channels=channels;st->force_channels=channels;st->mode=MODE_SILK_ONLY;st->first=!initialized;st->auto_bandwidth=1101+(id/7)%5;st->bandwidth=1101+(id/35)%3;st->Fs=apis[api];st->silk_mode.allowBandwidthSwitch=allowed;st->silk_mode.inWBmodeWithoutVariableLP=inwb;
  int i;int frame_size=st->Fs*durations[duration]/1000,frame_rate=st->Fs/frame_size,max_data_bytes=budgets[budget];
  int bitrate=bits_to_bitrate(max_data_bytes*8,st->Fs,frame_size),equiv_rate=compute_equiv_rate(bitrate,channels,frame_rate,vbr,MODE_SILK_ONLY,complexity,losses[loss]);
  fprintf(out,"%d %d %d %d %d %d %d %d %d %d %d %d %d %d %d",apis[api],durations[duration],channels,max_data_bytes,complexity,vbr,losses[loss],enabled,allowed,inwb,voice_est,initialized,st->auto_bandwidth-1101,st->bandwidth-1101,previous);
SELECTION
  if(bitrate<15000)st->bandwidth=IMIN(st->bandwidth,OPUS_BANDWIDTH_WIDEBAND);
API_BOUNDS
  int fec=decide_fec(enabled,losses[loss],previous,MODE_SILK_ONLY,&st->bandwidth,equiv_rate);
  st->bandwidth=IMIN(st->bandwidth,OPUS_BANDWIDTH_WIDEBAND);int curr_bandwidth=st->bandwidth;
DESIRED
  fprintf(out," %d %d %d %d %d %d\n",equiv_rate,st->silk_mode.desiredInternalSampleRate/1000,st->silk_mode.maxInternalSampleRate/1000,st->auto_bandwidth-1101,st->bandwidth-1101,fec);
 }
 if(fclose(out))return 3;return 0;
}
'''
    program = program.replace('SELECTION',selection).replace('API_BOUNDS',api_bounds).replace('DESIRED',desired).replace('REVISION',REVISION)
    with tempfile.TemporaryDirectory(prefix='opura-automatic-policy-') as temp:
        folder = Path(temp)
        (folder/'probe.c').write_text(program)
        subprocess.run(['cc', '-O2', '-fwrapv', '-DVAR_ARRAYS', '-DOPUS_BUILD', '-DFIXED_POINT', '-DDISABLE_FLOAT_API',
                        *['-I'+str(source/p) for p in ['include','celt','silk','silk/fixed','src']],str(folder/'probe.c'), '-o',str(folder/'probe')],check=True)
        subprocess.run([str(folder/'probe'),str(args.output.resolve())],check=True)
    print('Captured12480 original automatic bandwidth/equivalent-rate/FEC decisions')


if __name__ == '__main__':
    main()
