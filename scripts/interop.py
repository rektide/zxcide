#!/usr/bin/env python3
"""Cross-decode with the pinned C CLI. Usage: scripts/interop.py C_ZXC RUST_ZXC"""
from pathlib import Path
import subprocess
import sys


def run(exe, flags, data):
    return subprocess.run([exe, *flags], input=data, stdout=subprocess.PIPE, check=True).stdout


def main(c, rust):
    valid = Path(__file__).resolve().parents[1] / 'tests/conformance/v9/valid'
    checks = 0
    for path in sorted(valid.glob('*.expected')):
        plain = path.read_bytes()
        dictionary = {'dict_http': 'dict_http', 'dict_seekable_l7': 'dict_text',
                      'dict_no_checksum': 'dict_http', 'dict_id_zero': 'dict_id_zero'}.get(path.stem)
        d = ['-D', str(valid / (dictionary + '.zxd'))] if dictionary else []
        for mode in ('split', 'packed', 'dense'):
            flags = ['--seekable', '--block-log', '12'] if mode == 'split' else []
            if mode == 'packed':
                flags.append('--no-checksum')
            archive = run(rust, ['encode', '--mode', mode, *flags, *d], plain)
            if run(c, ['-qdc', '-T1', *d], archive) != plain:
                raise ValueError(f'C decode: {path.name}, {mode}')
            checks += 1
        for level in range(1, 8):
            flags = ['-C', '-S'] if level % 2 else ['-N']
            archive = run(c, ['-qzc', '-T1', f'-{level}', *flags, *d], plain)
            if run(rust, ['decode', *d], archive) != plain:
                raise ValueError(f'Rust decode: {path.name}, level {level}')
            checks += 1
    print(f'{checks} byte-for-byte cross-decoding checks passed')


if __name__ == '__main__':
    main(*sys.argv[1:])
