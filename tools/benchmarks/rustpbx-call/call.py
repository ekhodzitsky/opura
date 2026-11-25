"""Run one bidirectional SIP/RTP Opus-to-PCMU case in a loopback network namespace."""
import argparse
import array
import json
import math
import os
import re
from pathlib import Path
from statistics import median
import socket
import subprocess
import time
from urllib.request import urlopen
import wave


def create_input(root, case, count):
    path = case / 'input.wav'
    with wave.open(str(path), 'wb') as out:
        if count == 1:
            out.setparams((1, 2, 16000, 0, 'NONE', 'not compressed'))
            samples = array.array('h', (int(9000 * math.sin(2*math.pi*440*i/16000)) for i in range(16000*30)))
            out.writeframes(samples.tobytes())
        else:
            with wave.open(str(root/'speech.wav'), 'rb') as speech:
                out.setparams(speech.getparams())
                out.writeframes(speech.readframes(speech.getnframes())*6)
    return path


def sample(pid):
    proc = Path('/proc') / str(pid)
    stat = (proc/'stat').read_text().split()
    fields = {line.split()[0]: int(line.split()[1]) for line in (proc/'smaps_rollup').read_text().splitlines()
              if line.startswith(('Rss:', 'Private_Dirty:'))}
    return (time.monotonic(), int(stat[13])+int(stat[14]), fields['Rss:'], fields['Private_Dirty:'])


def wait_health(server):
    for _ in range(100):
        if server.poll() is not None:
            raise RuntimeError(f'RustPBX exited with {server.returncode}')
        try:
            if urlopen('http://127.0.0.1:18083/ami/v1/health', timeout=.5).status == 200:
                return
        except OSError:
            pass
        time.sleep(.25)
    raise RuntimeError('RustPBX health endpoint timed out')


def rx_tone(path):
    # sipbot records interleaved RX (left), TX (right), both at 16 kHz.
    with wave.open(str(path), 'rb') as wav:
        assert (wav.getnchannels(), wav.getsampwidth(), wav.getframerate()) == (2, 2, 16000)
        pcm = array.array('h', wav.readframes(wav.getnframes()))[::2]
    window = 4000
    sin = [math.sin(2*math.pi*440*i/16000) for i in range(window)]
    cos = [math.cos(2*math.pi*440*i/16000) for i in range(window)]
    good, total = 0, 0
    for start in range(0, len(pcm)-window+1, window):
        block = pcm[start:start+window]
        energy = sum(x*x for x in block)
        in_phase = sum(x*y for x, y in zip(block, cos))
        quadrature = sum(x*y for x, y in zip(block, sin))
        total += 1
        if energy > 500**2*window and 2*(in_phase**2+quadrature**2) > .5*energy*window:
            good += 1
    return {'seconds': len(pcm)/16000, 'tone_windows': good, 'complete_windows': total}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('root', type=Path, help='Output of prepare.py')
    parser.add_argument('backend', choices=['stock', 'opura'])
    parser.add_argument('case', help='New results directory name inside root')
    parser.add_argument('--calls', type=int, choices=[1, 8], default=1)
    args = parser.parse_args()
    if {name for _, name in socket.if_nameindex()} != {'lo'}:
        raise RuntimeError('Run in a network namespace containing only lo')
    root = args.root.resolve()
    case = root / args.case
    case.mkdir()
    cfg = Path(__file__).with_name('site.toml').read_text()
    (case/'site.toml').write_text(cfg.replace('sqlite://pbx.sqlite', 'sqlite://'+str(case/'pbx.sqlite')))
    sound = create_input(root, case, args.calls)
    children = []
    def launch(name, command):
        with (case/(name+'.log')).open('wb') as log:
            process = subprocess.Popen(command, cwd=case, stdout=log, stderr=subprocess.STDOUT)
        children.append(process)
        return process
    try:
        srv = launch('server', [root/args.backend, '--conf', case/'site.toml'])
        wait_health(srv)
        client = [root/'sipbot', '--conf', case/'unused.toml']
        account = ['--password', 'smoke-only', '--register', '127.0.0.1:15061']
        quality = ['--audio-quality', '-v']
        def record(leg):
            return ['--record', case/(leg+'.wav')] if args.calls == 1 else []
        uas = launch('uas', client+['wait', '--username', 'bob', *account, '--addr', '127.0.0.1:15090',
                     '--codecs', 'pcmu', '--echo', '--hangup', '18', *quality, *record('bob')])
        for _ in range(30):
            if uas.poll() is not None:
                raise RuntimeError('PCMU endpoint exited before the call')
            log = (case/'uas.log').read_text(errors='replace').lower()
            if 'registered' in log or 'registration successful' in log:
                break
            time.sleep(.2)
        samples = [sample(srv.pid)]
        uac = launch('uac', client+['call', '--target', 'sip:bob@127.0.0.1:15061', '--username', 'alice', *account,
                     '--addr', '127.0.0.1:15091', '--codecs', 'opus', '--hangup', '10',
                     '--total', str(args.calls), '--cps', str(min(4, args.calls)), '--play', sound, *quality, *record('alice')])
        while time.monotonic()-samples[0][0] < 45 and uac.poll() is None:
            time.sleep(.25)
            samples.append(sample(srv.pid))
        (case/'samples.json').write_text(json.dumps(samples))
        if uac.poll() != 0:
            raise RuntimeError(f'Caller did not finish successfully ({uac.poll()})')
    finally:
        for child in reversed(children):
            if child.poll() is None:
                child.terminate()
                try:
                    child.wait(timeout=4)
                except subprocess.TimeoutExpired:
                    child.kill()
                    child.wait()
    bridge = (case/'server.log').read_text(errors='replace')
    activations = sum('transcoding activated' in line and 'a_codec=Opus b_codec=PCMU has_webrtc_leg=false' in line
                      for line in bridge.splitlines())
    print('RustPBX Opus to PCMU bridge activations', activations)
    passed = activations >= args.calls
    for side in ('uac', 'uas'):
        log = (case/(side+'.log')).read_text(errors='replace')
        lines = [line for line in log.splitlines() if 'Avg Loss:' in line]
        line = lines[-1] if lines else ''
        print(side, line)
        rtp = re.search(r'TX: (\d+)p/\d+b, RX: (\d+)p/\d+b', line)
        successes = re.search(r'\b200:(\d+)\b', line)
        passed &= bool(rtp and min(int(v) for v in rtp.groups()) >= args.calls * 50 and
                       successes and int(successes[1]) == args.calls and 'has_audio=true' in line)
    window = [s for s in samples if 3 <= s[0]-samples[0][0] <= 9]
    if len(window) < 2:
        raise RuntimeError('Call ended before the CPU sampling window')
    first, last = window[0], window[-1]
    print('PBX core equivalents', (last[1]-first[1])/os.sysconf('SC_CLK_TCK')/(last[0]-first[0]))
    print('idle RSS and private dirty (KiB)', *samples[0][2:])
    print('live median RSS and private dirty (KiB)', median(s[2] for s in window), median(s[3] for s in window))
    if args.calls == 1:
        for leg in ('alice', 'bob'):
            recordings = list(case.glob(leg+'*.wav'))
            if len(recordings) != 1:
                raise RuntimeError(f'Expected a single receive recording for {leg}')
            tone = rx_tone(recordings[0])
            print(leg+' RX', tone)
            passed &= tone['tone_windows'] >= 20
    if not passed:
        raise RuntimeError('Incomplete media delivery; inspect the retained call logs')


if __name__ == '__main__':
    main()
