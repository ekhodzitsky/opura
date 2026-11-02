#!/usr/bin/env python3
"""Optional independent FFmpeg/FFprobe interoperability check of the audio CLI.

No external executable is required for ordinary Cargo tests. These checks prove
format/duration/packet accounting, not perceptual quality or performance leadership.
"""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import struct
import subprocess
import tempfile
import wave
parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('binary',type=Path);parser.add_argument('--report',type=Path)
args=parser.parse_args();binary=args.binary.resolve();root=Path(__file__).resolve().parent.parent
for executable in ['ffmpeg','ffprobe']:
    if not shutil.which(executable):raise SystemExit('Optional interoperability check requires '+executable)
def run(command):
    result=subprocess.run(list(map(str,command)),capture_output=True,text=True)
    if result.returncode:raise SystemExit(result.stderr or result.stdout or f'Command failed: {command[0]}')
    return result.stdout
report={'opura_version':run([binary,'--version']).strip(),'ffmpeg_version':run(['ffmpeg','-version']).splitlines()[0],'claim':'format, geometry, exact duration and packet-byte accounting only','encoded_cases':[],'original_reference_cases':[]}
with tempfile.TemporaryDirectory(prefix='opura-audio-interop-') as temp:
    folder=Path(temp);cases=[]
    for rate in [8000,12000,16000,24000,48000]:
        for channels in [1,2]:
            for frame,n in [('2.5',120),('5',240),('10',480),('20',960)]:cases.append((rate,channels,frame,n,2*n*rate//48000+7,64321))
    cases.extend((48000,1,'20',960,count,64000) for count in [0,1,959,960,961])
    cases.extend((48000,2,'20',960,1933,bitrate) for bitrate in [102000,204000,510000,510400])
    cases.extend([(48000,2,'20',960,48013,64321),(48000,2,'2.5',120,48013,64321)])
    for index,(rate,channels,frame,n,count,bitrate) in enumerate(cases):
        source=folder/f'{index}.wav';encoded=folder/f'{index}.opus';own=folder/f'{index}-own.wav';external=folder/f'{index}-ffmpeg.wav'
        with wave.open(str(source),'wb') as w:
            w.setnchannels(channels);w.setsampwidth(2);w.setframerate(rate)
            values=[((i//channels+(i%channels)*31)%239-119)*120 for i in range(count*channels)]
            w.writeframes(struct.pack('<'+'h'*len(values),*values))
        run([binary,'encode',source,encoded,'--bitrate',bitrate,'--frame-ms',frame]);run([binary,'decode',encoded,own]);run(['ffmpeg','-v','error','-n','-i',encoded,'-c:a','pcm_s16le',external])
        expected=count*(48000//rate)
        for path in [own,external]:
            with wave.open(str(path),'rb') as w:
                if (w.getframerate(),w.getnchannels(),w.getsampwidth(),w.getnframes())!=(48000,channels,2,expected):raise SystemExit('Decoded geometry/duration mismatch: '+path.name)
        metadata=json.loads(run(['ffprobe','-v','error','-show_packets','-show_entries','packet=size:stream=codec_name,sample_rate,channels','-of','json',encoded]))
        stream=metadata['streams'][0]
        if (stream['codec_name'],int(stream['sample_rate']),stream['channels'])!=('opus',48000,channels):raise SystemExit('FFprobe stream mismatch')
        packets=metadata['packets'];frames=(expected+120+n-1)//n
        packet_bytes=sum(int(p['size']) for p in packets);den=48000*8
        if len(packets)!=frames or packet_bytes!=(frames*bitrate*n+den//2)//den:raise SystemExit('Packet count/fractional byte accounting mismatch')
        report['encoded_cases'].append({'input_rate':rate,'channels':channels,'frame_ms':frame,'input_samples':count,'output_samples_48k':expected,'bitrate':bitrate,'packets':frames,'packet_bytes':packet_bytes,'file_bytes':encoded.stat().st_size})
    manifest=json.loads((root/'tests/audio/manifest.json').read_text());report['reference_revision']=manifest['reference_revision']
    for case in manifest['cases']:
        name=case['name'];source=root/'tests/audio'/f'{name}.opus';own=folder/f'{name}-own.wav';external=folder/f'{name}-ffmpeg.wav'
        run([binary,'decode',source,own]);run(['ffmpeg','-v','error','-n','-i',source,'-c:a','pcm_s16le',external])
        expected=(root/'tests/audio'/f'{name}.pcm').read_bytes()
        with wave.open(str(own),'rb') as w:
            actual=w.readframes(w.getnframes())
            if w.getnframes()!=case['samples'] or actual!=expected:raise SystemExit('Pinned native PCM mismatch: '+name)
        with wave.open(str(external),'rb') as w:
            if (w.getframerate(),w.getnchannels(),w.getnframes())!=(48000,case['channels'],case['samples']):raise SystemExit('External native-file duration mismatch: '+name)
        report['original_reference_cases'].append({'name':name,'mode':case['mode'],'samples':case['samples'],'channels':case['channels'],'pcm_sha256':hashlib.sha256(expected).hexdigest()})
text=json.dumps(report,indent=2)+'\n'
if args.report:args.report.parent.mkdir(parents=True,exist_ok=True);args.report.write_text(text)
print(f"Verified {len(report['encoded_cases'])} independently decoded encodes and {len(report['original_reference_cases'])} pinned native mode files.")
