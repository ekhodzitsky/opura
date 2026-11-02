#!/usr/bin/env python3
"""Optional exact pinned fixed-point native decoding of retained SILK CLI reports."""
import argparse
import ctypes as c
import hashlib
import json
from pathlib import Path
import re
import subprocess
import tempfile
import wave
from audio_baseline import packets
from silk_nsq_fixtures import REVISION


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source', type=Path)
    parser.add_argument('output', type=Path)
    parser.add_argument('reports', type=Path, nargs='+')
    args = parser.parse_args()
    source = args.source.resolve()
    if subprocess.check_output(['git','-C',str(source),'rev-parse','HEAD'],text=True).strip() != REVISION:
        raise SystemExit('Expected pinned Opus v1.6.1')
    if subprocess.check_output(['git','-C',str(source),'status','--porcelain'],text=True).strip():
        raise SystemExit('Reference checkout must be unchanged')
    def manifest(file, variable):
        match = re.search(r'^'+variable+r' = (.*?)(?=\n\n|\Z)',(source/file).read_text(),re.M|re.S)
        if match is None:
            raise SystemExit('Missing native source manifest')
        return match[1].replace('\\','').split()
    files = (manifest('silk_sources.mk','SILK_SOURCES') + manifest('silk_sources.mk','SILK_SOURCES_FIXED') +
             manifest('celt_sources.mk','CELT_SOURCES') + manifest('opus_sources.mk','OPUS_SOURCES'))
    rows=[]
    with tempfile.TemporaryDirectory(prefix='opura-cli-native-') as temp:
        library = Path(temp)/'native.so'
        subprocess.run(['cc','-shared','-fPIC','-O2','-fwrapv','-DVAR_ARRAYS','-DOPUS_BUILD','-DFIXED_POINT','-DDISABLE_FLOAT_API',
                        *['-I'+str(source/p) for p in ['include','celt','silk','silk/fixed','src']],
                        *[str(source/p) for p in files],'-lm','-o',str(library)],check=True)
        lib=c.CDLL(str(library))
        lib.opus_decoder_create.argtypes=[c.c_int,c.c_int,c.POINTER(c.c_int)]
        lib.opus_decoder_create.restype=c.c_void_p
        lib.opus_decode.argtypes=[c.c_void_p,c.POINTER(c.c_ubyte),c.c_int,c.POINTER(c.c_int16),c.c_int,c.c_int]
        lib.opus_decoder_destroy.argtypes=[c.c_void_p]
        for report_path in args.reports:
            report=json.loads(report_path.read_text())
            if report['status']!='pass':
                raise SystemExit('Only completed CLI reports can be validated')
            for case in report['cases']:
                folder=report_path.parent/f"{case['rate']}-{case['bandwidth']}-{case['frame_ms']}"
                channels=case['channels']
                skip,encoded=packets(folder/'encoded.opus')
                error=c.c_int()
                decoder=lib.opus_decoder_create(48000,channels,c.byref(error))
                if not decoder or error.value:
                    raise SystemExit('Native decoder creation failed')
                pcm=bytearray()
                try:
                    for packet in encoded:
                        output=(c.c_int16*(2880*channels))()
                        count=lib.opus_decode(decoder,(c.c_ubyte*len(packet)).from_buffer_copy(packet),len(packet),output,2880,0)
                        if count<0:
                            raise SystemExit(f'Native packet decode failed: {folder}/{count}')
                        pcm.extend(bytes(output)[:count*channels*2])
                finally:
                    lib.opus_decoder_destroy(decoder)
                with wave.open(str(folder/'opura.wav'),'rb') as own:
                    expected=own.readframes(own.getnframes())
                    if (own.getframerate(),own.getnchannels(),own.getsampwidth(),own.getnframes())!=(48000,channels,2,case['decoded_samples_48k']):
                        raise SystemExit('Retained WAV geometry differs from report')
                actual=pcm[skip*channels*2:skip*channels*2+len(expected)]
                if actual!=expected or hashlib.sha256(expected).hexdigest()!=case['opura_pcm_sha256']:
                    raise SystemExit(f'Exact pinned native CLI PCM mismatch: {folder}')
                rows.append({'report':str(report_path),'rate':case['rate'],'bandwidth':case['bandwidth'],
                             'frame_ms':case['frame_ms'],'channels':channels,'pcm_sha256':hashlib.sha256(actual).hexdigest(),
                             'version':report['version'],'binary_sha256':report['binary_sha256']})
    args.output.write_text(json.dumps({'status':'pass','revision':REVISION,'configuration':'fixed-point scalar',
                                      'cases':rows,'scope':'Exact independently decoded CLI PCM after preskip/granule trimming; no quality claim'},indent=2)+'\n')
    print(f'PASS: {len(rows)} CLI files match pinned fixed-point native PCM exactly')


if __name__ == '__main__':
    main()
