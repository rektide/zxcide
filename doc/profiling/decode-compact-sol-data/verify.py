#!/usr/bin/env python3
"""Exact EOF, dictionary decode and encode, conformance status and prefix parity."""
import argparse
import fcntl
import hashlib
import json
from pathlib import Path
import subprocess as sp


def digest(data):
    return {'bytes': len(data), 'sha256': hashlib.sha256(data).hexdigest()}


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('build', type=Path)
    p.add_argument('small', type=Path)
    args = p.parse_args()
    variants = [args.build.resolve() / ('cli-' + label) for label in ('a', 'b', 'a-scalar', 'b-scalar')]
    fixtures = args.small.resolve()
    root = Path('tests/conformance/v9')
    hashes = dict(line.split()[::-1] for line in (root / 'vectors.sha256').read_text().splitlines())
    prior = Path('doc/profiling/pivco-queue-data/sha256.txt')
    recorded = dict(line.strip().split()[::-1] for line in prior.read_text().splitlines() if line.strip())
    results = []

    def compare(name, options, expected=None, expected_success=None):
        refs = []
        rows = []
        for exe in variants:
            argv = [str(exe), *map(str, options)]
            p = sp.run(argv, stdout=sp.PIPE, stderr=sp.PIPE, timeout=60)
            if expected_success is not None:
                assert (p.returncode == 0) == expected_success, (argv, p.returncode, p.stderr)
            if expected is not None:
                assert not p.stderr and p.stdout == expected, argv
            refs.append((p.returncode, p.stderr, p.stdout))
            rows.append(dict(argv=argv, status=p.returncode, stderr=p.stderr.decode(), output=digest(p.stdout)))
        assert all(ref == refs[0] for ref in refs), (name, rows)
        results.append({'case': name, 'runs': rows})

    with Path('.test-agent/zxc-perf.lock').open('a') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        dictionaries = []
        for path in sorted((root / 'valid').glob('*.zxd')):
            assert digest(path.read_bytes())['sha256'] == hashes[str(path)]
            dictionaries += ['-D', path]
        for group in ('valid', 'invalid'):
            for arc in sorted((root / group).glob('*.zxc')):
                assert digest(arc.read_bytes())['sha256'] == hashes[str(arc)]
                expected = None
                if group == 'valid':
                    path = arc.with_suffix('.expected')
                    expected = path.read_bytes()
                    assert digest(expected)['sha256'] == hashes[str(path)]
                success = group == 'valid' or arc.stem == 'sek_forged_entry'
                compare(arc.stem, ['decode', '--threads', '1', *dictionaries, arc], expected, success)
        paths = {name: fixtures / name for name in ('dict-cold', 'dict-many', 'dict-cold.zxc',
                                                      'dict-many.zxc', 'traversal.zxd', 'mixed')}
        for name, path in paths.items():
            assert digest(path.read_bytes())['sha256'] == recorded['fixtures/' + name], path
        dictionary = paths['traversal.zxd']
        dictionary_args = ['--dictionary', dictionary]
        for name in ('dict-cold', 'dict-many'):
            args = ['decode', '--threads', '1']
            compare(name, [*args, *dictionary_args, paths[name + '.zxc']], paths[name].read_bytes(), True)
            compare(name + '-missing', [*args, paths[name + '.zxc']], expected_success=False)
        encode = ['encode', '--threads', '1', '--block-log', '12', '--mode', 'dense']
        compare('encode-mixed-inline', [*encode, paths['mixed']], expected_success=True)
        compare('encode-dict-many', [*encode, *dictionary_args, paths['dict-many']], expected_success=True)
    print(json.dumps(results, indent=2))


if __name__ == '__main__':
    main()
