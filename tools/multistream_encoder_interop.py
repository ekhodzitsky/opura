#!/usr/bin/env python3
"""Optional independent native decoding of synthetic Rust multistream output.

No encoder byte/quality equality or performance claim follows. Run only after
other timed benchmark jobs terminate. All foreign compilation stays external.
"""
import argparse
import ctypes as c
from datetime import datetime,timezone
import hashlib
import json
from pathlib import Path
import re
import subprocess
import tempfile
import tomllib
from audio_baseline import CFLAGS,REVISION,compile_reference,sha
from multistream_decoder_fixtures import features,fingerprint

ROOT=Path(__file__).resolve().parent.parent
RATES=[8000,12000,16000,24000,48000]
DURATIONS=[120,240,480,960,1920,2880,3840,4800,5760]
LAYOUTS=[(1,0,[0]),(1,1,[0,1]),(2,1,[0,2,1]),(2,2,[0,1,2,3]),
         (3,2,[0,4,1,2,3]),(4,2,[0,4,1,2,3,5]),(4,3,[0,4,1,2,3,5,6]),
         (5,3,[0,6,1,2,3,4,5,7]),(2,1,[2,0,1,255,0])]


def case_protocols():
    """Predeclared matrix, independent of the Rust export's counts/metadata."""
    result=[]
    for rate in RATES:
        for streams,coupled,mapping in LAYOUTS:
            for application in range(5):
                frames=[]
                for duration in DURATIONS:
                    if application==3 and duration<480:continue
                    for mode in range(3):
                        cap=7664*streams if mode==0 else 120*streams if mode==1 else 2*streams-1+int(duration==4800)*streams
                        frames.append(('normal',duration,cap,32000*(streams+coupled),int(mode!=0),int(mode==2),0,0))
                result.append(((rate,streams,coupled,application,mapping),frames))
    for layout_index in [2,7]:
        streams,coupled,mapping=LAYOUTS[layout_index]
        for fec in [0,1]:
            frames=[];pending=False
            for step in range(400):
                common=(960,8192,32000*(streams+coupled),1,0,fec,1)
                if step%17==3:
                    if fec:pending=True
                    else:frames.append(('plc',*common))
                    continue
                if pending:frames.append(('fec',*common));pending=False
                frames.append(('normal',*common))
            result.append(((48000,streams,coupled,1,mapping),frames))
    return result


def sources():
    files=[*ROOT.glob('src/**/*.rs'),*[ROOT/name for name in ['Cargo.toml','Cargo.lock','rust-toolchain.toml','examples/multistream_encode_vectors.rs','tools/multistream_encoder_interop.py','tools/multistream_decoder_fixtures.py','tools/multistream_rate_fixtures.py','tools/audio_baseline.py']]]
    digest=hashlib.sha256()
    for path in sorted(files):digest.update(str(path.relative_to(ROOT)).encode()+b'\0'+path.read_bytes()+b'\0')
    return digest.hexdigest()


def create(lib,case):
    mapping=(c.c_ubyte*case['channels'])(*case['mapping'])
    error=c.c_int()
    owner=lib.opus_multistream_decoder_create(case['rate'],case['channels'],case['streams'],case['coupled'],mapping,c.byref(error))
    if not owner or error.value:raise ValueError('Native decoder creation failed')
    return owner


def decode(lib,owner,case,packet,duration,fec,expected_range,expected_samples,expected_hash):
    if duration not in DURATIONS:raise ValueError('Unexpected output duration')
    data=None if packet is None else (c.c_ubyte*len(packet)).from_buffer_copy(packet)
    pcm=(c.c_int16*(5760*case['channels']))()
    n=lib.opus_multistream_decode(owner,data,0 if packet is None else len(packet),pcm,duration*case['rate']//48000,int(fec))
    if n!=expected_samples or n!=duration*case['rate']//48000:raise ValueError('Native output sample count differs')
    final_range=c.c_uint32()
    if lib.opus_multistream_decoder_ctl(owner,4031,c.byref(final_range)):raise ValueError('Native final-range query failed')
    if final_range.value!=expected_range:raise ValueError('Final range differs')
    if fingerprint(pcm[:n*case['channels']])!=expected_hash:raise ValueError('PCM fingerprint differs')


def suppressed_frames(lib,packet,streams):
    source=(c.c_ubyte*len(packet)).from_buffer_copy(packet)
    offset=0;empty=0
    for stream in range(streams):
        sizes=(c.c_int16*48)();consumed=c.c_int32()
        count=lib.opus_packet_parse_impl(c.byref(source,offset),len(packet)-offset,int(stream+1<streams),None,None,sizes,None,c.byref(consumed),None,None)
        if count<1 or consumed.value<=0:raise ValueError('Native subpacket parsing failed')
        empty+=sum(n<=1 for n in sizes[:count])
        offset+=consumed.value
    if offset!=len(packet):raise ValueError('Native framing did not consume packet')
    return empty


def validate_case(case,expected_frames):
    if case['rows']!=len(expected_frames) or not case['rows'] or case['operations']['normal']==0:raise ValueError('Empty/incomplete interoperability case')
    if case['dtx_requested'] and case['suppressed_frames']==0:raise ValueError('DTX case never suppresses frames')
    if case['fec_requested'] and (case['operations']['fec']==0 or case['fec_step_lbrr_streams']==0):raise ValueError('FEC case has no actual recovery backup')


def verify(lib,path,version):
    if path.stat().st_size>64*1024*1024:raise ValueError('Export exceeds64MiB')
    cases=[];owner=None;case=None;rows=0;faults={};footer=None
    protocols=case_protocols();expected_frames=[]
    try:
        with path.open() as source:
            if source.readline().rstrip()!=f'# OPURA-MULTISTREAM-ENCODE01 {version}':raise ValueError('Wrong export header/version')
            for line in source:
                if line.startswith('# cases='):
                    if footer is not None:raise ValueError('Duplicate export footer')
                    footer=line.strip();continue
                if footer is not None:raise ValueError('Data after export footer')
                fields=line.split()
                if not fields:raise ValueError('Empty export row')
                if fields[0]=='case':
                    if len(fields)!=6 or len(cases)>=1000:raise ValueError('Invalid case geometry')
                    if owner is not None:
                        validate_case(case,expected_frames);lib.opus_multistream_decoder_destroy(owner);owner=None
                    rate,streams,coupled,application=map(int,fields[1:5]);mapping=list(map(int,fields[5].split(',')))
                    if len(cases)>=len(protocols):raise ValueError('Extra workload case')
                    expected_geometry,expected_frames=protocols[len(cases)]
                    if rate not in RATES or not 1<=streams<=255 or not 0<=coupled<=streams or streams+coupled>255 or application not in range(5) or not 1<=len(mapping)<=255 or any(n!=255 and not 0<=n<streams+coupled for n in mapping):raise ValueError('Invalid mapped export case')
                    if (rate,streams,coupled,application,mapping)!=expected_geometry:raise ValueError('Workload case geometry/order differs')
                    case=dict(rate=rate,streams=streams,coupled=coupled,application=application,mapping=mapping,channels=len(mapping),rows=0,operations=dict(normal=0,plc=0,fec=0),mode_packets=[0,0,0],lbrr_stream_packets=0,suppressed_frames=0,fec_step_lbrr_streams=0,dtx_requested=False,fec_requested=False)
                    owner=create(lib,case);cases.append(case)
                elif fields[0]=='frame':
                    if len(fields)!=13 or owner is None or rows>=100000:raise ValueError('Invalid frame record')
                    kind=fields[1];duration,cap,bitrate,variable,constrained,fec,dtx,final_range,samples,digest=map(int,fields[2:12])
                    if kind not in case['operations'] or not 1<=cap<=16*1024*1024 or not 1<=bitrate<=191250000 or any(n not in [0,1] for n in [variable,constrained,fec,dtx]) or not 0<=final_range<=0xffffffff or not 0<=digest<=0xffffffffffffffff:raise ValueError('Invalid frame controls/metadata')
                    if case['rows']>=len(expected_frames) or (kind,duration,cap,bitrate,variable,constrained,fec,dtx)!=expected_frames[case['rows']]:raise ValueError('Workload frame schedule/controls differ')
                    packet=None if fields[12]=='-' else bytes.fromhex(fields[12])
                    if (packet is None)!=(kind=='plc') or (packet is not None and not 1<=len(packet)<=cap):raise ValueError('Invalid packet/control binding')
                    decode(lib,owner,case,packet,duration,kind=='fec',final_range,samples,digest)
                    if not faults and kind=='normal' and case['rows']==0:
                        for name,wrong_range,wrong_hash,message in [('wrong_range',final_range^1,digest,'Final range differs'),('wrong_pcm',final_range,digest^1,'PCM fingerprint differs')]:
                            probe=create(lib,case)
                            try:
                                try:decode(lib,probe,case,packet,duration,False,wrong_range,samples,wrong_hash)
                                except ValueError as error:
                                    if str(error)!=message:raise
                                    faults[name]='rejected'
                                else:raise ValueError('Corrupted expected output was accepted')
                            finally:lib.opus_multistream_decoder_destroy(probe)
                    if packet is not None:
                        modes,backups=features(lib,packet,case['streams'])
                        if kind=='normal':
                            case['mode_packets']=[a+b for a,b in zip(case['mode_packets'],modes)]
                            case['lbrr_stream_packets']+=backups
                            if dtx:case['suppressed_frames']+=suppressed_frames(lib,packet,case['streams'])
                        if kind=='fec':case['fec_step_lbrr_streams']+=backups
                    case['dtx_requested']|=bool(dtx);case['fec_requested']|=bool(fec)
                    case['operations'][kind]+=1;case['rows']+=1;rows+=1
                else:raise ValueError('Unexpected export row')
        if case is not None:validate_case(case,expected_frames)
        if footer!='# cases=229 rows=7405 encoded=7405 dropped=96' or len(cases)!=229 or rows!=7405 or faults!={'wrong_range':'rejected','wrong_pcm':'rejected'}:raise ValueError('Missing complete matrix/footer/rejection probes')
        return cases,faults
    finally:
        if owner is not None:lib.opus_multistream_decoder_destroy(owner)


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source',type=Path)
    parser.add_argument('output',type=Path)
    args=parser.parse_args();args.output.mkdir(parents=True,exist_ok=False)
    before=sources();version=tomllib.loads((ROOT/'Cargo.toml').read_text())['package']['version']
    subprocess.run(['cargo','build','--offline','--locked','--release','--example','multistream_encode_vectors'],cwd=ROOT,check=True)
    binary=ROOT/'target/release/examples/multistream_encode_vectors'
    exported=args.output.resolve()/'vectors.txt'
    subprocess.run([str(binary),str(exported)],cwd=ROOT,check=True,timeout=600)
    with tempfile.TemporaryDirectory(prefix='opura-multistream-interop-') as temp:
        library=compile_reference(args.source.resolve(),Path(temp))
        library.opus_multistream_decoder_create.argtypes=[c.c_int,c.c_int,c.c_int,c.c_int,c.c_void_p,c.c_void_p];library.opus_multistream_decoder_create.restype=c.c_void_p
        library.opus_multistream_decoder_destroy.argtypes=[c.c_void_p]
        library.opus_multistream_decode.argtypes=[c.c_void_p,c.c_void_p,c.c_int,c.c_void_p,c.c_int,c.c_int];library.opus_multistream_decode.restype=c.c_int
        library.opus_multistream_decoder_ctl.argtypes=[c.c_void_p,c.c_int];library.opus_multistream_decoder_ctl.restype=c.c_int
        library.opus_packet_parse_impl.argtypes=[c.c_void_p,c.c_int,c.c_int,c.c_void_p,c.c_void_p,c.c_void_p,c.c_void_p,c.c_void_p,c.c_void_p,c.c_void_p];library.opus_packet_parse_impl.restype=c.c_int
        library.opus_packet_has_lbrr.argtypes=[c.c_void_p,c.c_int];library.opus_packet_has_lbrr.restype=c.c_int
        cases,faults=verify(library,exported,version)
        native_hash=sha(Path(temp)/'reference.so')
    if sources()!=before:raise ValueError('Source changed during validation')
    report=dict(schema_version=1,status='pass',opura_version=version,source_sha256=before,base_revision=subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),worktree_modified=bool(subprocess.check_output(['git','status','--porcelain'],cwd=ROOT,text=True).strip()),reference_revision=REVISION,reference_checkout_modified=False,cflags=CFLAGS,created_utc=datetime.now(timezone.utc).isoformat(),rustc=subprocess.check_output(['rustc','--version','--verbose'],cwd=ROOT,text=True).strip(),cc=subprocess.check_output(['cc','--version'],text=True).splitlines()[0],binary_sha256=sha(binary),native_library_sha256=native_hash,export_sha256=sha(exported),cases=cases,fault_sensitivity=faults,limitations=['Synthetic compatibility evidence, not representative perceptual quality.','No native encoder byte/quality equality claim; intentional Q24 encoder correction remains.','Generic mapping, not surround energy-mask/LFE equivalence.','No performance or SOTA claim.'])
    (args.output/'report.json').write_text(json.dumps(report,indent=2,sort_keys=True)+'\n')
    print('Verified229 mapped cases/7405 native PCM/range operations; no performance/quality claim.')


if __name__=='__main__':main()
