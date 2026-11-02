#!/usr/bin/env python3
"""Generate native SILK encoder target-rate/SNR tables from pinned external Opus."""
import argparse
from pathlib import Path
import re
import subprocess
from silk_nsq_fixtures import REVISION


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source', type=Path)
    args = parser.parse_args()
    source = args.source.resolve()
    if subprocess.check_output(['git', '-C', str(source), 'rev-parse', 'HEAD'], text=True).strip() != REVISION:
        raise SystemExit('Expected pinned Opus v1.6.1')
    if subprocess.check_output(['git', '-C', str(source), 'status', '--porcelain'], text=True).strip():
        raise SystemExit('Reference checkout must be unchanged')
    content = (source/'silk/control_SNR.c').read_text()
    print('// Generated from Opus v1.6.1 by tools/silk_encoder_rate_tables.py.')
    print('// Copyright (c) 2006-2011, Skype Limited. See THIRD_PARTY_NOTICES.md.')
    for name, length in [('NB', 107), ('MB', 155), ('WB', 191)]:
        match = re.search(r'silk_TargetRate_'+name+r'_21\[[^]]+\]\s*=\s*\{(.*?)\};', content, re.S)
        if match is None:
            raise SystemExit('Missing native SNR table')
        values = list(map(int, re.findall(r'\d+', match[1])))
        if len(values) != length:
            raise SystemExit('Wrong native SNR table length')
        print(f'pub(super) const {name}: [u8; {length}] = {values};')


if __name__ == '__main__':
    main()
