# Historical decoder stack-sampling driver

This is the exact experimental driver used for the 0.2.5 diagnostic collection.
It is a reproducibility snapshot, not a supported codec API or ordinary build/
test requirement. Save the fenced Python to `target/profile-025-gdb.py` in the
0.2.5 checkout. Input binaries must match the retained reports; the relative
archive/vector locations are described in the [profile report](decoder-profile-0.2.5.md).

For newly built binaries, first produce fresh source-bound correctness and
benchmark reports. Change the input bindings in a local copy of this historical
driver, record its new SHA256, and preserve those fresh reports separately.
Never modify retained reports to make a binary match. GDB auto-load and remote
debuginfod are disabled; the driver launches only the selected benchmark child.

Driver SHA256: `4a2c388d3c979450983ca75e1b953918e74119bc635332ec0773ebc3ef86b3ef`.

```python
"""External jittered wall-clock stack samples; never codec speed evidence."""
import argparse
from collections import deque
import json
import os
from pathlib import Path
import random
import re
import selectors
import signal
import subprocess
import sys
import time

root = Path(__file__).resolve().parent.parent
sys.path.insert(0,str(root/'tools'))
import decoder_change as change
from decoder_bench import check_sample,compare_samples

class Debugger:
    def __init__(self,binary,passes,log):
        environment=os.environ.copy()
        environment['DEBUGINFOD_URLS']=''
        self.process=subprocess.Popen(['taskset','-c','0','gdb','-q','-nx',
            '-iex','set auto-load off','-iex','set debuginfod enabled off',
            '--interpreter=mi2','--args',str(binary),str(root/'target/decoder-bench/vectors'),
            '48000','2',str(passes),'1','0','1d15af7e1c0f5b9c'],
            stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,env=environment)
        self.selector=selectors.DefaultSelector()
        self.selector.register(self.process.stdout,selectors.EVENT_READ)
        self.buffer=b''
        self.lines=deque()
        self.token=0
        self.running=False
        self.exited=False
        self.exit_code=None
        self.target_pid=None
        self.sample=None
        self.log=log
        self.binary=binary
    def consume(self,line):
        self.log.write(line+'\n')
        if line.startswith('*running'):
            self.running=True
        if line.startswith('*stopped'):
            self.running=False
            if 'reason="exited-normally"' in line:
                self.exited=True
                self.exit_code=0
        if line.startswith('=thread-group-started'):
            match=re.search(r'pid="(\d+)"',line)
            if match:self.target_pid=int(match[1])
        if line.startswith('=thread-group-exited'):
            self.exited=True
            self.running=False
            match=re.search(r'exit-code="([0-7]+)"',line)
            self.exit_code=int(match[1],8) if match else None
        text=line
        if line.startswith(('@','~')):
            try:text=json.loads(line[1:]).strip()
            except json.JSONDecodeError:return
        if text.startswith('{"packets":'):
            self.sample=json.loads(text)
    def read(self,deadline):
        while not self.lines:
            remaining=deadline-time.monotonic()
            if remaining<=0 or not self.selector.select(remaining):raise TimeoutError('MI response timeout')
            chunk=os.read(self.process.stdout.fileno(),65536)
            if not chunk:raise RuntimeError('Unexpected GDB EOF')
            self.buffer+=chunk
            while b'\n' in self.buffer:
                line,self.buffer=self.buffer.split(b'\n',1)
                self.lines.append(line.decode(errors='replace').strip())
        line=self.lines.popleft()
        self.consume(line)
        return line
    def command(self,text,allow_error=False):
        self.token+=1
        prefix=str(self.token)+'^'
        self.process.stdin.write((str(self.token)+text+'\n').encode())
        self.process.stdin.flush()
        deadline=time.monotonic()+15
        while True:
            line=self.read(deadline)
            if line.startswith(prefix):
                if line.startswith(prefix+'error') and not allow_error:raise RuntimeError(line)
                return line
    def cleanup(self):
        if not self.exited and self.target_pid:
            try:
                if Path(f'/proc/{self.target_pid}/exe').resolve()==self.binary.resolve():
                    os.kill(self.target_pid,signal.SIGKILL)
            except (ProcessLookupError,FileNotFoundError):pass
        if self.process.poll() is None:
            self.process.terminate()
            try:self.process.wait(3)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait()
        self.selector.close()

def collect(binary,passes,log):
    debugger=Debugger(binary,passes,log)
    rng=random.Random(62587)
    samples=[]
    shared=None
    try:
        for command in ['-gdb-set mi-async on','-gdb-set pagination off',
                        '-gdb-set confirm off','-gdb-set disable-randomization off']:
            debugger.command(command)
        debugger.command('-exec-run')
        start=time.monotonic()
        while not debugger.exited:
            if time.monotonic()-start>180:raise TimeoutError('Bounded sampling runtime exceeded')
            until=time.monotonic()+rng.uniform(.012,.028)
            try:
                while not debugger.exited:debugger.read(until)
            except TimeoutError:pass
            if debugger.exited:break
            debugger.command('-exec-interrupt --all',allow_error=True)
            deadline=time.monotonic()+15
            while debugger.running and not debugger.exited:debugger.read(deadline)
            if debugger.exited:break
            frames=debugger.command('-stack-list-frames 0 24')
            functions=[json.loads('"'+v+'"') for v in re.findall(r'func="((?:\\.|[^"\\])*)"',frames)]
            address=re.search(r'addr="(0x[0-9a-f]+)"',frames)
            if not functions or not address:raise RuntimeError('Missing physical stack/PC')
            samples.append(dict(wall_offset=time.monotonic()-start,top_pc=address[1],functions=functions))
            if shared is None:
                shared=debugger.command('-file-list-shared-libraries')
                debugger.command('-interpreter-exec console "info proc mappings"')
            debugger.command('-exec-continue')
        debugger.command('-gdb-exit')
        debugger.process.wait(10)
        assert debugger.exit_code==0 and debugger.sample is not None
        return dict(exit_code=debugger.exit_code,sample=debugger.sample,
                    stack_samples=samples,shared_libraries=shared)
    finally:debugger.cleanup()

parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('--passes',type=int,default=50,choices=[1,50])
parser.add_argument('--output',type=Path,required=True)
args=parser.parse_args()
args.output.mkdir(exist_ok=False)
r=json.loads((root/'docs/benchmarks/results/decoder-pvq-0.2.5.json').read_text())
n=json.loads((root/'docs/benchmarks/results/decoder-0.2.3.json').read_text())
binaries={'rust':root/'target/pvq-025-binaries/after','native':root/'target/current-decoder-023-binaries/native'}
hashes={'rust':r['binary_sha256']['after'],'native':n['reference_binary_sha256']}
assert change.sources(root)==r['source_sha256']['after']
status=dict(status='in_progress',method='External GDB async MI wall-clock jittered PC/physical-stack sampling',
            scope='Diagnostic attribution only; debugger overhead/elapsed time are not speed evidence.',
            cpu=0,interval_seconds=[.012,.028],seed=62587,passes=args.passes,
            binary_sha256=hashes,rust_source_sha256=r['source_sha256']['after'],
            native_revision=n['reference_revision'],native_profile=n['reference_profile'],
            gdb_version=subprocess.check_output(['gdb','--version'],text=True).splitlines()[0],
            driver_sha256=change.sha(Path(__file__)),runs={})
def save():(args.output/'status.json').write_text(json.dumps(status,indent=2)+'\n')
save()
try:
    for name,binary in binaries.items():
        assert change.sha(binary)==hashes[name]
        print('Start',name,flush=True)
        with (args.output/(name+'-mi.log')).open('w') as log:result=collect(binary,args.passes,log)
        check_sample(result['sample'],20075,args.passes)
        compare_samples(r['cases']['48000/2']['after'][0],result['sample'])
        assert change.sha(binary)==hashes[name]
        assert len(result['stack_samples'])>=20
        status['runs'][name]=result
        save()
        print('Complete',name,len(result['stack_samples']),'samples',flush=True)
    assert change.sources(root)==status['rust_source_sha256']
    status['status']='pass'
    save()
except BaseException as error:
    status['status']='failed'
    status['error']=str(error)
    save()
    raise
```
