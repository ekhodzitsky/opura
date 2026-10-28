#!/usr/bin/env python3
"""Generate LPC fixtures using clean pinned Opus; optional, never a build dependency.

Usage: python3 tools/silk_lpc_fixtures.py /path/to/opus [--table]
"""
import argparse
import ctypes as c
from pathlib import Path
import re
import subprocess
import tempfile

REVISION = '22244de5a79bd1d6d623c32e72bf1954b56235be'
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('source', type=Path)
parser.add_argument('--table', action='store_true')
args = parser.parse_args()
source = args.source.resolve()
if subprocess.check_output(['git', '-C', str(source), 'rev-parse', 'HEAD'], text=True).strip() != REVISION:
    raise SystemExit('Expected Opus v1.6.1 at ' + REVISION)
if subprocess.check_output(['git', '-C', str(source), 'status', '--porcelain', '--untracked-files=no']):
    raise SystemExit('Reference checkout has modified tracked files')
if args.table:
    data = (source / 'silk/table_LSF_cos.c').read_text()
    values = [int(s) for s in re.search(r'FIX_Q12\[[^]]+\]\s*=\s*\{([^}]+)', data)[1].split(',') if s.strip()]
    assert len(values) == 129
    print('// Generated from Opus v1.6.1 by tools/silk_lpc_fixtures.py --table.')
    print('// Copyright (c) 2006-2011, Skype Limited. See THIRD_PARTY_NOTICES.md.')
    print('pub(super) const COS_Q12: [i16; 129] = ' + str(values) + ';')
    raise SystemExit(0)

root = Path(__file__).resolve().parent.parent
spectra = set()
for line in (root / 'tests/silk-nlsf-fixtures.txt').read_text().splitlines():
    if line.startswith('#'):
        continue
    if line.startswith('frame'):
        _, data, order, _, _ = line.split()
        order = int(order)
        budget = len(data) * 4
        continue
    values = list(map(int, line.split()))
    if values[-2] <= budget:
        spectra.add(tuple(values[order + 3:2 * order + 3]))
        spectra.add(tuple(values[2 * order + 3:3 * order + 3]))
for line in (root / 'tests/silk-nlsf-stabilize-fixtures.txt').read_text().splitlines():
    if not line.startswith('#'):
        values = list(map(int, line.split()))
        spectra.add(tuple(values[values[0] + 1:]))

with tempfile.TemporaryDirectory(prefix='opura-lpc-reference-') as temp:
    path = Path(temp)
    files = ['silk/NLSF2A.c', 'silk/LPC_fit.c', 'silk/LPC_inv_pred_gain.c',
             'silk/bwexpander_32.c', 'silk/table_LSF_cos.c']
    subprocess.run(['cc', '-shared', '-fPIC', '-O2', '-I' + str(source / 'include'),
                    '-I' + str(source / 'celt'), '-I' + str(source / 'silk')]
                   + [str(source / file) for file in files] + ['-o', str(path / 'probe.so')], check=True)
    lib = c.CDLL(str(path / 'probe.so'))
    lib.silk_NLSF2A.argtypes = [c.POINTER(c.c_int16), c.POINTER(c.c_int16), c.c_int, c.c_int]
    lib.silk_NLSF2A.restype = None
    lib.silk_LPC_inverse_pred_gain_c.argtypes = [c.POINTER(c.c_int16), c.c_int]
    lib.silk_LPC_inverse_pred_gain_c.restype = c.c_int32
    print('# Opus v1.6.1 NLSF2A/LPC_inverse_pred_gain:', REVISION)
    print('# ORDER NLSF_Q15[ORDER] LPC_Q12[ORDER] INVERSE_GAIN_Q30')
    for spectrum in sorted(spectra, key=lambda v: (len(v), v)):
        order = len(spectrum)
        data = (c.c_int16 * order)(*spectrum)
        out = (c.c_int16 * order)()
        lib.silk_NLSF2A(out, data, order, 0)
        gain = lib.silk_LPC_inverse_pred_gain_c(out, order)
        print(order, *spectrum, *out, gain)
