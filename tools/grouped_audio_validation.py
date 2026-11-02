#!/usr/bin/env python3
"""Optional actual long-packet file/PCM/range/loss/FEC interoperability."""
import argparse
import ctypes as c
import hashlib
import json
from pathlib import Path
import struct
import subprocess
import tempfile
from audio_baseline import CORPUS, REVISION, compile_reference, native_decode, packets, read_wave, sha
from hybrid_file_validation import pcm_hash
from silk_mono_validation import fingerprint


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source', type=Path)
    parser.add_argument('binary', type=Path)
    parser.add_argument('replay', type=Path)
    parser.add_argument('corpus', type=Path)
    parser.add_argument('output', type=Path)
    args = parser.parse_args()
    binary, replay = args.binary.resolve(), args.replay.resolve()
    args.output.mkdir(parents=True, exist_ok=False)
    report = {'status': 'in_progress', 'reference_revision': REVISION,
              'version': subprocess.check_output([str(binary), '--version'], text=True).strip(),
              'binary_sha256': sha(binary), 'replay_sha256': sha(replay), 'tool_sha256': sha(Path(__file__)),
              'scope': 'actual grouped CLI/native decode PCM/range/loss/FEC and framing/rate diagnostics only; no native encoder quality/speed claim',
              'cases': []}
    root = Path(__file__).resolve().parent.parent
    source_hash = hashlib.sha256()
    for path in sorted((root/'src').rglob('*.rs')):
        source_hash.update(path.relative_to(root).as_posix().encode())
        source_hash.update(b'\0')
        source_hash.update(path.read_bytes())
    report['codec_source_sha256'] = source_hash.hexdigest()
    def save():
        (args.output/'report.json').write_text(json.dumps(report, indent=2)+'\n')
    save()
    vectors = args.output/'replay.bin'
    records = streams = 0
    with tempfile.TemporaryDirectory(prefix='opura-grouped-audio-') as temp, vectors.open('wb') as vector_out:
        lib = compile_reference(args.source.resolve(), Path(temp))
        lib.opus_packet_parse.argtypes = [c.c_void_p,c.c_int32,c.c_void_p,c.c_void_p,c.c_void_p,c.c_void_p]
        lib.opus_packet_parse.restype = c.c_int
        lib.opus_packet_get_nb_samples.argtypes = [c.c_void_p,c.c_int32,c.c_int]
        lib.opus_packet_get_nb_samples.restype = c.c_int
        vector_out.write(b'OPURARI1')
        for name, corpus_hash in CORPUS.items():
            recording = args.corpus/(name+'.wav')
            if sha(recording) != corpus_hash:
                raise ValueError('Wrong hash-pinned recording: '+name)
            channels, original = read_wave(recording)
            original_count = len(original)//channels
            profiles = [('celt',ms,group,policy,'',0,False) for ms,group in [('2.5','120'),('5','40'),('10','80'),('20','120')] for policy in ['cbr','cvbr']]
            profiles += [('celt','2.5','7.5','vbr','',0,False),('celt','20','60','vbr','',0,False)]
            profiles += [('silk',ms,group,'cbr',band,20,True) for ms,group,band in [('10','120','wb'),('20','100','auto'),('40','80','wb'),('60','120','auto')]]
            profiles += [('hybrid','10','60','cbr','fb',20,True),('hybrid','20','120','vbr','fb',20,True),('hybrid','20','120','cbr','fb',0,True)]
            for number,(mode,ms,group,policy,band,fec,dtx) in enumerate(profiles):
                bitrate = (64001 if channels==1 else 128001) if mode=='celt' else (32001 if channels==1 else 64001)
                frame_samples, group_samples = round(float(ms)*48), round(float(group)*48)
                components = group_samples//frame_samples
                maximum = min(1276*components,max(1200,192*components)) if policy!='cbr' else 1276*components
                stem = f'{name}-{number}-{mode}-{ms}-{group}-{policy}'
                opus, wav = args.output/(stem+'.opus'), args.output/(stem+'.wav')
                command = [str(binary),'encode',str(recording),str(opus),'--mode',mode,'--frame-ms',ms,'--packet-ms',group,'--bitrate',str(bitrate)]
                if policy!='cbr':
                    command += ['--'+policy,'--max-packet-bytes',str(maximum)]
                if band:
                    command += ['--bandwidth',band]
                if fec:
                    command += ['--fec',str(fec)]
                if dtx:
                    command += ['--dtx']
                subprocess.run(command,check=True)
                subprocess.run([str(binary),'decode',str(opus),str(wav)],check=True)
                skip, encoded = packets(opus)
                if skip != (120 if mode=='celt' else 312):
                    raise ValueError('Wrong pre-skip')
                decoded_channels, decoded = read_wave(wav)
                native = native_decode(lib,channels,encoded,skip,original_count)
                if decoded_channels!=channels or decoded!=native:
                    raise ValueError('Native grouped file PCM/duration mismatch')
                credit = 384000//2
                durations, counts = [], []
                dtx_packets = lbrr_packets = 0
                configurations = set()
                for packet in encoded:
                    data = (c.c_ubyte*len(packet)).from_buffer_copy(packet)
                    toc, offset = c.c_ubyte(), c.c_int()
                    lengths = (c.c_int16*48)()
                    frames = (c.c_void_p*48)()
                    count = lib.opus_packet_parse(data,len(packet),c.byref(toc),frames,lengths,c.byref(offset))
                    duration = lib.opus_packet_get_nb_samples(data,len(packet),48000)
                    if count<=0 or not 0<duration<=group_samples or duration%frame_samples or len(packet)>maximum:
                        raise ValueError('Invalid grouped packet geometry/cap')
                    suppressed = all(lengths[i]==0 for i in range(count))
                    credit += bitrate*duration
                    target,credit = divmod(credit,384000)
                    if policy=='cbr' and not suppressed and len(packet)!=target:
                        raise ValueError('Wrong aggregate CBR packet budget')
                    if suppressed and (not dtx or len(packet)>2):
                        raise ValueError('Unexpected or oversized grouped DTX')
                    dtx_packets += int(suppressed)
                    lbrr = lib.opus_packet_has_lbrr(data,len(packet))
                    if lbrr<0 or (not fec and lbrr):
                        raise ValueError('Unexpected grouped LBRR')
                    lbrr_packets += int(lbrr>0)
                    durations.append(duration)
                    counts.append(count)
                    configurations.add(toc.value>>3)
                if max(counts)<=1:
                    raise ValueError('Missing actual multi-frame packets')
                # Replay a bounded prefix in three independent decoder streams.
                # FEC can protect only the previous final frame; native PLC
                # covers the rest of a missing long interval.
                selected = list(zip(encoded[:96],durations[:96]))
                for schedule in ['normal','plc','fec']:
                    first = True
                    for index,(packet,duration) in enumerate(selected):
                        calls = []
                        if schedule=='plc' and index%13==6:
                            calls.append((b'',duration,False))
                        elif schedule=='fec' and index%11==4:
                            if index+1==len(selected):
                                calls.append((b'',duration,False))
                            else:
                                continue
                        else:
                            if schedule=='fec' and index%11==5:
                                calls.append((packet,selected[index-1][1],True))
                            calls.append((packet,duration,False))
                        for data,duration,is_fec in calls:
                            flags = int(first)+2*int(is_fec)
                            vector_out.write(struct.pack('<BBHH',flags,channels,len(data),duration)+data)
                            streams += int(first)
                            records += 1
                            first = False
                report['cases'].append({'recording':name,'corpus_sha256':corpus_hash,'mode':mode,'channels':channels,'frame_ms':float(ms),'packet_ms':float(group),'policy':policy,'bandwidth':band,'fec_loss_percent':fec,'dtx':dtx,'bitrate':bitrate,'max_packet_bytes':maximum,'samples_per_channel':original_count,'skip':skip,'packets':len(encoded),'packet_bytes':sum(map(len,encoded)),'min_packet_bytes':min(map(len,encoded)),'max_actual_packet_bytes':max(map(len,encoded)),'packet_durations_samples':sorted(set(durations)),'frame_counts':sorted(set(counts)),'configurations':sorted(configurations),'dtx_packets':dtx_packets,'lbrr_packets':lbrr_packets,'opus_sha256':sha(opus),'pcm_sha256':pcm_hash(native),'native_pcm_match':True})
                save()
    if len(report['cases'])!=34:
        raise ValueError('Unexpected file matrix size')
    actual = args.output/'replay-results.bin'
    subprocess.run([str(replay),str(vectors),str(actual)],check=True)
    with tempfile.TemporaryDirectory(prefix='opura-grouped-replay-') as temp:
        lib = compile_reference(args.source.resolve(),Path(temp))
        lib.opus_decoder_ctl.argtypes = [c.c_void_p,c.c_int]
        lib.opus_decoder_ctl.restype = c.c_int
        decoder = None
        checked = lost = fec_requests = fec_with_lbrr = 0
        try:
            with vectors.open('rb') as input_vectors, actual.open('rb') as results:
                if input_vectors.read(8)!=b'OPURARI1' or results.read(8)!=b'OPURARO1':
                    raise ValueError('Wrong replay format')
                while header := input_vectors.read(6):
                    flags,channels,length,duration = struct.unpack('<BBHH',header)
                    data = input_vectors.read(length)
                    if len(data)!=length:
                        raise ValueError('Truncated replay data')
                    if flags&1:
                        if decoder:
                            lib.opus_decoder_destroy(decoder)
                        error = c.c_int()
                        decoder = lib.opus_decoder_create(48000,channels,c.byref(error))
                        if not decoder or error.value:
                            raise ValueError('Native replay decoder creation failed')
                    count,expected_range,expected_hash = struct.unpack('<HIQ',results.read(14))
                    pcm = (c.c_int16*11520)()
                    packet = (c.c_ubyte*length).from_buffer_copy(data) if length else None
                    samples = lib.opus_decode(decoder,packet,length,pcm,duration,int(bool(flags&2)))
                    final_range = c.c_uint32()
                    status = lib.opus_decoder_ctl(decoder,4031,c.byref(final_range))
                    if samples*channels!=count or status or final_range.value!=expected_range or fingerprint(pcm[:count])!=expected_hash:
                        raise ValueError(f'Native replay mismatch at record{checked}: samples{samples*channels}/{count}, range{final_range.value}/{expected_range}')
                    checked += 1
                    lost += int(not length)
                    fec_requests += int(bool(flags&2))
                    fec_with_lbrr += int(bool(flags&2) and bool(length) and lib.opus_packet_has_lbrr(packet,length)>0)
                if results.read(1):
                    raise ValueError('Unexpected replay result suffix')
        finally:
            if decoder:
                lib.opus_decoder_destroy(decoder)
    if checked!=records or lost==0 or fec_requests==0 or fec_with_lbrr==0:
        raise ValueError('Missing replay coverage')
    report['replay'] = {'streams':streams,'records':checked,'lost':lost,'fec_requests':fec_requests,'fec_requests_with_any_lbrr':fec_with_lbrr,'input_sha256':sha(vectors),'results_sha256':sha(actual)}
    report['status'] = 'pass'
    save()
    print(json.dumps({'status':'pass','files':len(report['cases']),'replay':report['replay']}))


if __name__ == '__main__':
    main()
