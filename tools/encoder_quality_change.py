#!/usr/bin/env python3
"""Optional before/after automatic-encoder recording diagnostics, not a perceptual/SOTA score."""
import argparse
import hashlib
import struct
import json
import platform
from pathlib import Path
import statistics
import subprocess
import tempfile
import time
from audio_baseline import CORPUS, REVISION, compile_reference, native_decode, packet_diagnostics, packets, read_wave, sha, signal_metrics


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('baseline',type=Path);p.add_argument('candidate',type=Path)
    p.add_argument('source',type=Path);p.add_argument('corpus',type=Path)
    p.add_argument('output',type=Path);p.add_argument('--rounds',type=int,default=3)
    args=p.parse_args()
    if not 1<=args.rounds<=20:raise ValueError('rounds must be1..20')
    args.output.mkdir(parents=True,exist_ok=False)
    binaries={'baseline':args.baseline.resolve(),'candidate':args.candidate.resolve()}
    report={'status':'in_progress','scope':'two real recordings; automatic AUDIO20ms CBR; before/after process wall timings and aligned waveform diagnostics, no perceptual or competitive claim',
            'reference_decoder_revision':REVISION,'platform':platform.system(),'architecture':platform.machine(),
            'rounds':args.rounds,'binaries':{name:{'version':subprocess.check_output([str(binary),'--version'],text=True).strip(),'sha256':sha(binary)} for name,binary in binaries.items()},'cases':[]}
    def save():(args.output/'report.json').write_text(json.dumps(report,indent=2)+'\n')
    save()
    with tempfile.TemporaryDirectory(prefix='opura-quality-decoder-') as temp:
        lib=compile_reference(args.source.resolve(),Path(temp))
        for name,expected in CORPUS.items():
            source=args.corpus/(name+'.wav')
            if sha(source)!=expected:raise ValueError('Recording checksum mismatch: '+name)
            channels,original=read_wave(source)
            for complexity in [0,5,10]:
                for bitrate in [32000,64000]:
                    case={'recording':name,'recording_sha256':expected,'frames':len(original)//channels,'channels':channels,'complexity':complexity,'bitrate':bitrate,'results':{}}
                    paths={}
                    samples={variant:[] for variant in binaries}
                    for iteration in range(args.rounds+1):
                        # Alternate execution order; iteration0 is an unmeasured warmup.
                        order=list(binaries) if iteration%2==0 else list(reversed(binaries))
                        for variant in order:
                            output=args.output/f'{name}-{complexity}-{bitrate}-{variant}.opus'
                            if output.exists():output.unlink()
                            command=[str(binaries[variant]),'encode',str(source),str(output),'--mode','auto','--application','audio','--frame-ms','20','--complexity',str(complexity),'--bitrate',str(bitrate)]
                            before=time.perf_counter();subprocess.run(command,check=True,stdout=subprocess.DEVNULL,stderr=subprocess.PIPE);elapsed=time.perf_counter()-before
                            if iteration:samples[variant].append(elapsed)
                            paths[variant]=output
                    decoded={}
                    for variant,output in paths.items():
                        wav=args.output/(output.stem+'.wav')
                        subprocess.run([str(binaries[variant]),'decode',str(output),str(wav)],check=True,stdout=subprocess.DEVNULL,stderr=subprocess.PIPE)
                        actual_channels,pcm=read_wave(wav)
                        if actual_channels!=channels or len(pcm)!=len(original):raise ValueError('Decoded duration/layout mismatch')
                        skip,encoded=packets(output)
                        if any(len(packet)!=(bitrate//400) for packet in encoded):raise ValueError('Whole CBR budget mismatch')
                        if pcm!=native_decode(lib,channels,encoded,skip,len(original)//channels):raise ValueError('Native PCM mismatch')
                        decoded[variant]=pcm
                        case['results'][variant]={'packet_diagnostics':packet_diagnostics(lib,encoded,960),'mode_packets':{mode:sum((('silk' if p[0]>>3<12 else 'hybrid' if p[0]>>3<16 else 'celt')==mode) for p in encoded) for mode in ['silk','hybrid','celt']},'wire_channel_packets':{str(c):sum((1+((p[0]>>2)&1))==c for p in encoded) for c in [1,2]},'file_sha256':sha(output),'packet_stream_sha256':hashlib.sha256(b''.join(struct.pack('<H',len(p))+p for p in encoded)).hexdigest(),'packet_bytes':sum(map(len,encoded)),'packets':len(encoded),'process_seconds':samples[variant],
                            'median_process_seconds':statistics.median(samples[variant]),'native_pcm_match':True,'waveform':signal_metrics(original,pcm)}
                    case['candidate_vs_baseline_waveform']=signal_metrics(decoded['baseline'],decoded['candidate'])
                    case['payload_bytes_equal']=case['results']['candidate']['packet_bytes']==case['results']['baseline']['packet_bytes']
                    case['candidate_to_baseline_process_ratio']=case['results']['candidate']['median_process_seconds']/case['results']['baseline']['median_process_seconds']
                    report['cases'].append(case);save();print(name,complexity,bitrate,'pass',flush=True)
    report['status']='pass';save()

if __name__=='__main__':main()
