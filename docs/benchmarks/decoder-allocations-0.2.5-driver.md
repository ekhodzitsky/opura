# Exact historical decoder allocation-observation driver

This is the source-bound diagnostic snapshot used by the [0.2.5 study](decoder-allocations-0.2.5.md), not a supported codec or tool API. Create these four files under ignored `target/alloc-025/`; raw output must remain under `target/`. The control programs are external measurement controls, never Cargo dependencies.

## `collect.py`

SHA256: `2f56106f6d60f4c8a7517d8d13dda13e9a938272357078eed6906b271a761418`

```python
"""Historical Linux/GDB allocation-entry study; not a timing benchmark."""
from pathlib import Path
from datetime import datetime, timezone
import argparse
import json
import os
import platform
import signal
import subprocess
import sys

HERE=Path(__file__).resolve().parent
ROOT=HERE.parents[1]
sys.path.insert(0,str(ROOT/'tools'))
import decoder_change as d
import check_conformance as conformance
from decoder_bench import check_sample, compare_samples, CASES, PACKETS_PER_PASS

LIBC=['malloc','calloc','realloc','free','posix_memalign','aligned_alloc','memalign']
RUST=['__rustc::__rust_alloc','__rustc::__rust_alloc_zeroed','__rustc::__rust_realloc','__rustc::__rust_dealloc']

def identity(pid):
    try:return Path(f'/proc/{pid}/stat').read_text().rsplit(') ',1)[1].split()[19]
    except FileNotFoundError:return None

def descendants(pid):
    result=[]
    try:children=Path(f'/proc/{pid}/task/{pid}/children').read_text().split()
    except FileNotFoundError:return result
    for child in children:
        child=int(child); stamp=identity(child)
        if stamp is not None:result.append((child,stamp));result+=descendants(child)
    return result

def invoke(command,env):
    process=subprocess.Popen(command,env=env,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,text=True,start_new_session=True)
    timeout=False
    try:output,_=process.communicate(timeout=90)
    except subprocess.TimeoutExpired:
        timeout=True;owned=descendants(process.pid)
        process.send_signal(signal.SIGINT)
        try:output,_=process.communicate(timeout=5)
        except subprocess.TimeoutExpired:
            owned+=descendants(process.pid)
            for pid,stamp in reversed(owned):
                if identity(pid)==stamp:
                    try:os.kill(pid,signal.SIGKILL)
                    except ProcessLookupError:pass
            process.kill();output,_=process.communicate()
    return process.returncode,output,timeout

def validate(raw,rust,windows):
    if raw['gdb_exit']!=0 or raw['timed_out'] or raw['exit_code']!=0:
        raise ValueError('debugger/target did not exit successfully')
    if raw['errors'] or raw['active'] is not None:raise ValueError('unbalanced or failed observation')
    if len(raw['phases'])!=windows or not all(p['complete'] for p in raw['phases']):raise ValueError('missing complete replay windows')
    names=[name for group in raw['hooks'] for name in group]
    if len(names)!=len(set(names)) or set(names)!=set(LIBC+(RUST if rust else [])):
        raise ValueError('missing/duplicate allocation hooks')
    keys={'|'.join(group) for group in raw['hooks']}
    for phase in raw['phases']:
        if set(phase['events'])!=keys or any(type(v) is not int or v<0 for v in phase['events'].values()):raise ValueError('invalid event counters')
    return {key:sum(p['events'][key] for p in raw['phases']) for key in keys}

def count(events,name):return sum(value for key,value in events.items() if name in key.split('|'))

def positive(raw,rust):
    events=validate(raw,rust,1)
    required={name:1 for name in LIBC}
    required['free']=5
    if rust:required={'__rustc::__rust_alloc':2,'__rustc::__rust_alloc_zeroed':1,'__rustc::__rust_realloc':1,'__rustc::__rust_dealloc':3}
    if any(count(events,name)<minimum for name,minimum in required.items()):raise ValueError('known allocating control was not observed')
    return events

def json_output(log,key):
    rows=[]
    for line in log.splitlines():
        if line.startswith('{'):
            value=json.loads(line)
            if key in value:rows.append(value)
    if len(rows)!=1:raise ValueError('missing/ambiguous target output')
    return rows[0]

def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--output',type=Path,required=True);args=parser.parse_args()
    out=args.output.resolve()
    if not out.is_relative_to(ROOT/'target'):raise ValueError('raw observations must stay in ignored target directory')
    out.mkdir(parents=True,exist_ok=False)
    if os.environ.get('LD_PRELOAD') or os.environ.get('LD_AUDIT'):raise ValueError('unsupported allocator preload/audit environment')
    env=os.environ.copy();env['DEBUGINFOD_URLS']=''
    pvq=json.loads((ROOT/'docs/benchmarks/results/decoder-pvq-0.2.5.json').read_text())
    native=json.loads((ROOT/'docs/benchmarks/results/decoder-0.2.3.json').read_text())
    binaries={'rust':ROOT/'target/pvq-025-binaries/after','native':ROOT/'target/current-decoder-023-binaries/native'}
    hashes={name:d.sha(path) for name,path in binaries.items()}
    assert hashes['rust']==pvq['binary_sha256']['after'] and hashes['native']==native['reference_binary_sha256']
    assert d.sources(ROOT)==pvq['source_sha256']['after']
    d.verified_conformance(ROOT,ROOT/'docs/validation/results/decoder-pvq-0.2.5-conformance.json')
    archive=ROOT/'target/reference/opus_testvectors-rfc8251.tar.gz'
    assert d.sha(archive)==conformance.MANIFEST['archive_sha256']
    conformance.extract_verified(archive,out/'vectors')
    drivers={name:d.sha(HERE/name) for name in ['collect.py','probe.gdb.py','control.rs','control.c']}
    subprocess.run(['rustc','-O','--edition=2024','--crate-name','alloc_probe_control',str(HERE/'control.rs'),'-o',str(out/'control-rust')],cwd=ROOT,check=True)
    subprocess.run(['cc','-O2','-std=c11',str(HERE/'control.c'),'-o',str(out/'control-c')],cwd=ROOT,check=True)
    report=dict(status='in_progress',scope='Observed allocator entry events in five normal replay windows per case; no CPU-time/peak-memory or all-input claim.',created_utc=datetime.now(timezone.utc).isoformat(),source_sha256=pvq['source_sha256']['after'],binary_sha256=hashes,driver_sha256=drivers,archive_sha256=d.sha(archive),reference_revision=native['reference_revision'],control_binary_sha256={name:d.sha(out/name) for name in ['control-rust','control-c']},tool_versions={name:subprocess.check_output(command,text=True).splitlines()[0] for name,command in [('rustc',['rustc','--version']),('cc',['cc','--version']),('gdb',['gdb','--version'])]},python=sys.version,platform=platform.platform(),libc_sha256=d.sha(Path('/lib/x86_64-linux-gnu/libc.so.6')),controls={},cases={})
    def save(): (out/'report.json').write_text(json.dumps(report,indent=2,sort_keys=True)+'\n')
    def probe(name,binary,arguments,rust,windows,**faults):
        config=dict(rust=rust,windows=windows,result=str(out/(name+'.json')),**faults)
        config_path=out/(name+'-config.json');config_path.write_text(json.dumps(config))
        local=env.copy();local['OPURA_ALLOC_CONFIG']=str(config_path)
        command=['taskset','-c','0','gdb','-nx','-q','-batch','-iex','set auto-load off','-iex','set debuginfod enabled off','-x',str(HERE/'probe.gdb.py'),'--args',str(binary),*arguments]
        code,log,timeout=invoke(command,local);(out/(name+'.log')).write_text(log)
        raw=json.loads((out/(name+'.json')).read_text()) if (out/(name+'.json')).exists() else dict(exit_code=None,active=None,errors=['no collector output'],phases=[],hooks=[])
        raw.update(gdb_exit=code,timed_out=timeout)
        return raw,log
    save()
    for name,rust,binary,arguments,window in [('rust-positive',True,out/'control-rust',['alloc'],'alloc_probe_control::replay'),('c-positive',False,out/'control-c',[],'replay'),('empty',True,out/'control-rust',['empty'],'alloc_probe_control::boundary')]:
        raw,log=probe(name,binary,arguments,rust,[window]);checksum=json_output(log,'control_checksum');assert checksum['control_checksum']==20
        events=validate(raw,rust,1)
        if name=='empty':assert not any(events.values())
        else:positive(raw,rust)
        report['controls'][name]=dict(events=events,windows=1,checksum=20,passed=True);save();print(name,'pass',flush=True)
    for name,rust,binary,arguments,windows,faults in [('disabled-hooks',True,out/'control-rust',['alloc'],['alloc_probe_control::replay'],{'disable_hooks':True}),('missing-return',True,out/'control-rust',['alloc'],['alloc_probe_control::replay'],{'omit_finish':True}),('missing-window',True,out/'control-rust',['alloc'],['no_such_allocation_window'],{}),('nonzero-exit',False,out/'control-c',['fail'],['replay'],{})]:
        raw,log=probe(name,binary,arguments,rust,windows,**faults)
        try:
            if name=='disabled-hooks':positive(raw,rust)
            else:validate(raw,rust,1)
        except ValueError as error:report['controls'][name]=dict(rejected=True,reason=str(error))
        else:raise ValueError('negative control accepted: '+name)
        save();print(name,'rejected',flush=True)
    for name,binary in binaries.items():
        rust=name=='rust';windows=['decoder_bench::replay'] if rust else ['replay','replay.constprop.0']
        arguments=[str(out/'vectors'),'48000','2','1','1','0','0000000000000000']
        raw,log=probe(name+'-bad-fingerprint',binary,arguments,rust,windows)
        assert 'fingerprint' in log
        try:validate(raw,rust,5)
        except ValueError as error:report['controls'][name+'-bad-fingerprint']=dict(rejected=True,reason=str(error))
        else:raise ValueError('incorrect fingerprint accepted')
        save();print(name+'-bad-fingerprint','rejected',flush=True)
    for rate,channels in CASES:
        key=f'{rate}/{channels}';report['cases'][key]={};observed=[]
        for name,binary in binaries.items():
            rust=name=='rust';windows=['decoder_bench::replay'] if rust else ['replay','replay.constprop.0']
            raw,log=probe(f'{name}-{rate}-{channels}',binary,[str(out/'vectors'),str(rate),str(channels),'1','1'],rust,windows)
            events=validate(raw,rust,5);sample=json_output(log,'packets');check_sample(sample,PACKETS_PER_PASS,1)
            compare_samples(sample,pvq['cases'][key]['after'][0]);observed.append(sample)
            assert d.sha(binary)==hashes[name] and d.sources(ROOT)==report['source_sha256']
            report['cases'][key][name]=dict(events=events,windows=5,replay_packet_calls=5*PACKETS_PER_PASS,complete=True,exit_code=raw['exit_code'],hooks=raw['hooks'],output={k:sample[k] for k in ['packets','samples_per_pass','fingerprint','passes','mode_packets','decoder_bytes']})
            save();print(key,name,'events',sum(events.values()),flush=True)
        compare_samples(*observed)
    assert all(d.sha(HERE/name)==value for name,value in drivers.items())
    assert d.sha(Path('/lib/x86_64-linux-gnu/libc.so.6'))==report['libc_sha256']
    report['status']='pass';save();print('Complete allocation-entry observations retained',flush=True)

if __name__=='__main__':main()
```

## `probe.gdb.py`

SHA256: `37753491f834805fb56ad39b83bfafe5aa5b8b76b55d22d947ee35e68aa407db`

```python
import gdb
import json
import os
import re
from pathlib import Path

config=json.loads(Path(os.environ['OPURA_ALLOC_CONFIG']).read_text())
phases=[]
errors=[]
active=None
exit_code=None
hooks=[]
finishers=[]

def fail(error):
    errors.append(str(error))
    return True

class Allocation(gdb.Breakpoint):
    def __init__(self,address,names):
        super().__init__('*'+hex(address),internal=True)
        self.names=names
        self.enabled=False
    def stop(self):
        try:
            if active is None:
                return fail('allocation hook active outside a replay window')
            key='|'.join(self.names)
            phases[active]['events'][key]+=1
            return False
        except Exception as error:
            return fail(error)

class End(gdb.FinishBreakpoint):
    def __init__(self,frame,index):
        super().__init__(frame,internal=True)
        self.index=index
    def stop(self):
        global active
        try:
            if active!=self.index:
                return fail('unbalanced replay finish')
            for hook in hooks:hook.enabled=False
            phases[self.index]['complete']=True
            active=None
            return False
        except Exception as error:
            return fail(error)
    def out_of_scope(self):
        errors.append('replay returned out of scope')

class Begin(gdb.Breakpoint):
    def __init__(self,name):
        super().__init__(name,internal=True)
        self.label=name
    def stop(self):
        global active
        try:
            if active is not None:
                return fail('nested replay window')
            active=len(phases)
            phases.append(dict(symbol=self.label,complete=False,events={'|'.join(h.names):0 for h in hooks}))
            if not config.get('omit_finish',False):
                finishers.append(End(gdb.newest_frame(),active))
            for hook in hooks:hook.enabled=not config.get('disable_hooks',False)
            return False
        except Exception as error:
            return fail(error)

def exited(event):
    global exit_code
    exit_code=getattr(event,'exit_code',None)

gdb.events.exited.connect(exited)
try:
    for command in ['set auto-load off','set debuginfod enabled off','set disable-randomization off','set pagination off','set confirm off','set breakpoint pending off']:
        gdb.execute(command,to_string=True)
    gdb.execute('start',to_string=True)
    names=['malloc','calloc','realloc','free','posix_memalign','aligned_alloc','memalign']
    if config['rust']:
        names+=['__rustc::__rust_alloc','__rustc::__rust_alloc_zeroed','__rustc::__rust_realloc','__rustc::__rust_dealloc']
    addresses={}
    for name in names:
        info=gdb.execute('info address '+name,to_string=True)
        match=re.search(r'\bat (?:address )?(0x[0-9a-fA-F]+)\b',info)
        if not match:raise RuntimeError('unresolved allocation symbol: '+name)
        addresses.setdefault(int(match[1],16),[]).append(name)
    for address,aliases in addresses.items():hooks.append(Allocation(address,aliases))
    entries=[Begin(name) for name in config['windows']]
    gdb.execute('continue',to_string=True)
except Exception as error:
    errors.append(str(error))
finally:
    result=dict(exit_code=exit_code,active=active,errors=errors,phases=phases,hooks=[h.names for h in hooks])
    Path(config['result']).write_text(json.dumps(result,indent=2)+'\n')
    if gdb.selected_inferior().pid:
        gdb.execute('kill',to_string=True)
```

## `control.rs`

SHA256: `0286ab44f15663ea3d89324a03460acd0e10b8a0fa7ce2d9f5ca4cae8f293147`

```rust
#![forbid(unsafe_code)]
use std::hint::black_box;
#[repr(align(64))]
struct Aligned([u8; 64]);
#[inline(never)]
fn replay() -> u64 {
    let mut data = Vec::<u8>::with_capacity(black_box(13));
    data.extend_from_slice(black_box(&[3u8; 13]));
    data.reserve_exact(black_box(4096));
    let zeroed = vec![0u8; black_box(256)];
    let aligned = Box::new(Aligned([17; 64]));
    black_box(&data);
    black_box(&zeroed);
    black_box(&aligned);
    u64::from(data[12]) + u64::from(zeroed[255]) + u64::from(aligned.0[63])
}
#[inline(never)]
fn boundary() -> u64 { black_box(20) }
fn main() {
    let outside = vec![black_box(9u8); black_box(4096)];
    black_box(&outside);
    let empty = std::env::args().nth(1).is_some_and(|v| v == "empty");
    let checksum = if empty { boundary() } else { replay() };
    drop(outside);
    println!("{{\"control_checksum\":{checksum},\"empty\":{empty}}}");
}
```

## `control.c`

SHA256: `e34c2868eff2b3c1916a9eeab02b51c6b4d6df5e97514f0a62a8c3d13d0127e0`

```c
#define _POSIX_C_SOURCE 200809L
#include <malloc.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
static void *(*volatile m)(size_t)=malloc;
static void *(*volatile c)(size_t,size_t)=calloc;
static void *(*volatile r)(void *,size_t)=realloc;
static void *(*volatile a)(size_t,size_t)=aligned_alloc;
static void *(*volatile mm)(size_t,size_t)=memalign;
static int (*volatile pm)(void **,size_t,size_t)=posix_memalign;
static void (*volatile f)(void *)=free;
__attribute__((noinline)) static unsigned replay(void) {
    unsigned char *one=m(13),*two=c(3,32);void *three=NULL;
    if(!one||!two||pm(&three,64,64))exit(2);
    one=r(one,4096);void *four=a(64,128),*five=mm(64,192);
    if(!one||!four||!five)exit(3);
    one[0]=3;two[0]=17;unsigned checksum=one[0]+two[0];
    f(one);f(two);f(three);f(four);f(five);return checksum;
}
int main(int argc,char **argv){(void)argv;printf("{\"control_checksum\":%u}\n",replay());return argc>1?7:0;}
```
