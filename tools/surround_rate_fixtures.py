#!/usr/bin/env python3
"""Optional pristine native surround-factory total/LFE allocation oracle."""
import argparse
import ctypes as c
from pathlib import Path
import tempfile
from audio_baseline import REVISION
from multistream_rate_fixtures import compile_oracle, DURATIONS


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source', type=Path)
    parser.add_argument('output', type=Path)
    args = parser.parse_args()
    if args.output.exists():
        raise ValueError('Output must be new')
    rows = 0
    with tempfile.TemporaryDirectory(prefix='opura-surround-rates-') as temporary:
        lib = compile_oracle(args.source.resolve(), Path(temporary))
        lib.opus_multistream_surround_encoder_create.argtypes = [c.c_int]*3 + [c.c_void_p]*3 + [c.c_int, c.c_void_p]
        lib.opus_multistream_surround_encoder_create.restype = c.c_void_p
        lib.opus_multistream_encoder_ctl.argtypes, lib.opus_multistream_encoder_ctl.restype = [c.c_void_p,c.c_int],c.c_int
        lib.opus_multistream_encoder_destroy.argtypes = [c.c_void_p]
        lib.opura_ms_total.argtypes, lib.opura_ms_total.restype = [c.c_void_p], c.c_int
        lib.opura_ms_rates.argtypes, lib.opura_ms_rates.restype = [c.c_void_p,c.c_int,c.c_void_p],c.c_int
        with args.output.open('x') as output:
            output.write('# Original surround factory/allocation ' + REVISION + '\n')
            output.write('# layout RATE CHANNELS STREAMS COUPLED MAP\n# rates DURATION48 REQUESTED NORMALIZED SUM STREAM_RATES...\n')
            for rate in [8000,12000,16000,24000,48000]:
                for channels in range(1,9):
                    streams,coupled,error = c.c_int(),c.c_int(),c.c_int()
                    mapping = (c.c_ubyte*channels)()
                    owner = lib.opus_multistream_surround_encoder_create(rate,channels,1,c.byref(streams),c.byref(coupled),mapping,2049,c.byref(error))
                    if not owner or error.value:
                        raise ValueError('Native factory failed')
                    try:
                        output.write(f'layout {rate} {channels} {streams.value} {coupled.value} '+','.join(map(str,mapping))+'\n')
                        for duration in DURATIONS:
                            for requested in [1,499,500,64000,96000,510400,1500000,750000*channels,191250000]:
                                if lib.opus_multistream_encoder_ctl(owner,4002,c.c_int(requested)):
                                    raise ValueError('Native rate setting failed')
                                rates=(c.c_int32*streams.value)()
                                total=lib.opura_ms_rates(owner,duration*rate//48000,rates)
                                if total!=sum(rates):
                                    raise ValueError('Native allocation total differs')
                                output.write(f'rates {duration} {requested} {lib.opura_ms_total(owner)} {total} '+' '.join(map(str,rates))+'\n')
                                rows+=1
                    finally:
                        lib.opus_multistream_encoder_destroy(owner)
            output.write(f'# rows={rows}\n')
    if rows!=3240:
        raise ValueError('Incomplete rate matrix')
    print(f'Captured{rows}original native surround allocation boundaries',flush=True)


if __name__=='__main__':
    main()
