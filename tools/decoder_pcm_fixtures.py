#!/usr/bin/env python3
"""Optional pristine native decoder oracle for packet offsets, gain and recovery."""
import argparse
import ctypes as c
from pathlib import Path
import tempfile
from audio_baseline import REVISION, compile_reference
from multistream_decoder_fixtures import checked, fingerprint, packets


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('reference', type=Path)
    parser.add_argument('output', type=Path)
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix='opura-pcm-oracle-') as temporary:
        lib = compile_reference(args.reference.resolve(), Path(temporary))
        lib.opus_multistream_encoder_create.argtypes = [c.c_int,c.c_int,c.c_int,c.c_int,c.c_void_p,c.c_int,c.c_void_p]
        lib.opus_multistream_encoder_create.restype = c.c_void_p
        lib.opus_multistream_encoder_ctl.argtypes = [c.c_void_p,c.c_int]
        lib.opus_multistream_encode.argtypes = [c.c_void_p,c.c_void_p,c.c_int,c.c_void_p,c.c_int]
        lib.opus_multistream_encoder_destroy.argtypes = [c.c_void_p]
        lib.opus_decoder_create.argtypes = [c.c_int,c.c_int,c.c_void_p]
        lib.opus_decoder_create.restype = c.c_void_p
        lib.opus_decoder_ctl.argtypes = [c.c_void_p,c.c_int]
        lib.opus_decoder_destroy.argtypes = [c.c_void_p]
        lib.opus_decode.argtypes = [c.c_void_p,c.c_void_p,c.c_int,c.c_void_p,c.c_int,c.c_int]
        lib.opus_repacketizer_create.restype = c.c_void_p
        lib.opus_repacketizer_cat.argtypes = [c.c_void_p,c.c_void_p,c.c_int]
        lib.opus_repacketizer_out.argtypes = [c.c_void_p,c.c_void_p,c.c_int]
        lib.opus_repacketizer_destroy.argtypes = [c.c_void_p]
        lib.opus_packet_has_lbrr.argtypes = [c.c_void_p,c.c_int]
        encoded = {n: packets(lib, 1, n-1) for n in [1,2]}
        def join(data, count):
            owner = lib.opus_repacketizer_create()
            if not owner: raise ValueError('Repacketizer allocation failed')
            try:
                source = (c.c_ubyte*len(data)).from_buffer_copy(data)
                for _ in range(count): checked(lib.opus_repacketizer_cat(owner,source,len(data)))
                output = (c.c_ubyte*65536)()
                size = checked(lib.opus_repacketizer_out(owner,output,len(output)))
                return bytes(output[:size])
            finally:
                lib.opus_repacketizer_destroy(owner)
        total = 0
        modes = [0,0,0]
        backups = 0
        with args.output.open('x') as out:
            out.write('# Pristine native packet PCM offsets/gain/recovery '+REVISION+'\n')
            for channels in [1,2]:
                base = encoded[channels]
                for data in base:
                    modes[0 if data[0]>>3<12 else 1 if data[0]>>3<16 else 2] += 1
                for index in [2,6]:
                    source = (c.c_ubyte*len(base[index])).from_buffer_copy(base[index])
                    backups += checked(lib.opus_packet_has_lbrr(source,len(base[index])))
                for rate in [8000,12000,16000,24000,48000]:
                    for gain in [0,1536,-1536,-1,1,32767,-32768]:
                        error = c.c_int()
                        owner = lib.opus_decoder_create(rate,channels,c.byref(error))
                        checked(error.value)
                        if not owner: raise ValueError('Decoder allocation failed')
                        try:
                            checked(lib.opus_decoder_ctl(owner,4034,c.c_int(gain)))
                            out.write(f'case {rate} {channels} {gain}\n')
                            schedule = []
                            for index,data in enumerate(base):
                                schedule.append(('normal',960,data,0))
                                if index in [1,5,9]:
                                    count = {1:2,5:3,9:6}[index]
                                    schedule += [('normal',960*count,join(data,count),0),
                                                 ('plc',{1:120,5:1920,9:5760}[index],None,0)]
                                if index in [2,6]:
                                    schedule.insert(len(schedule)-1,('fec',5760,data,0))
                            # Reverse mode changes and reset retain gain; packet-local
                            # offsets also apply to the later frames of these packets.
                            for index,count in [(1,6),(6,3),(9,2),(2,1)]:
                                schedule.append(('normal',960*count,join(base[index],count),int(index==9)))
                            for kind,duration,data,reset in schedule:
                                if reset: checked(lib.opus_decoder_ctl(owner,4028))
                                source = None if data is None else (c.c_ubyte*len(data)).from_buffer_copy(data)
                                pcm = (c.c_int16*(5760*channels))()
                                n = checked(lib.opus_decode(owner,source,0 if data is None else len(data),pcm,5760*rate//48000 if kind=='normal' else duration*rate//48000,int(kind=='fec')))
                                if n != duration*rate//48000: raise ValueError('Unexpected native duration')
                                final_range = c.c_uint32()
                                checked(lib.opus_decoder_ctl(owner,4031,c.byref(final_range)))
                                out.write(f'frame {kind} {duration} {reset} '+('-' if data is None else data.hex())+f' {n} {final_range.value} {fingerprint(pcm[:n*channels])}\n')
                                total += 1
                        finally:
                            lib.opus_decoder_destroy(owner)
            if not all(modes) or backups == 0: raise ValueError('Missing actual modes/FEC')
            out.write(f'# rows={total} mode_packets={modes} fec_backups={backups}\n')
        print(f'Captured {total} pristine native PCM/range operations')


if __name__ == '__main__': main()
