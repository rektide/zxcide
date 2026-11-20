#!/usr/bin/env python3
"""Run only through flock on the absolute shared zxc-perf.lock."""
import argparse
import csv
import hashlib
import os
from pathlib import Path
import random
import subprocess
import sys
import tempfile
import time

w = Path(__file__).resolve().parent
binary = {k: w / 'bin' / k for k in ('base', 'trial')}
names = [('mixed12', 'mixed'), ('mixed19', 'mixed'), ('sao.dense', 'sao'), ('sao.c7', 'sao'),
         ('sao.split', 'sao'), ('silesia.dense', 'silesia'), ('silesia.c7', 'silesia'),
         ('noise12', 'noise'), ('light12', 'light12'), ('raw-long', 'raw-long'), ('light-long', 'light-long'),
         ('dict-cold', 'dict-cold'), ('dict-many', 'dict-many')]
p = argparse.ArgumentParser()
p.add_argument('--confirm', nargs='+', metavar='ARCHIVE:WORKERS')
opts = p.parse_args()
cases = [(archive, plain, n) for archive, plain in names for n in (1, 4)
         if opts.confirm is None or f'{archive}:{n}' in opts.confirm]
assert cases
samples = 24 if opts.confirm else 12


def sample(archive, workers, who, expected=None):
    argv = ['taskset', '-c', '2' if workers == 1 else '2-5', binary[who],
            'decode', '--threads', str(workers)]
    if workers == 4:
        argv += ['--backend', 'crossbeam', '--slots', '8']
    if archive.startswith('dict-'):
        argv += ['--dictionary', w / 'fixtures/traversal.zxd']
    argv += [w / f'fixtures/{archive}.zxc']
    with tempfile.TemporaryFile(dir=w) as stderr:
        start = time.perf_counter_ns()
        child = subprocess.Popen(argv, stderr=stderr,
                            stdout=subprocess.DEVNULL if expected is None else subprocess.PIPE)
        if expected is not None:
            with child.stdout as output:
                actual = hashlib.file_digest(output, 'sha256').hexdigest()
            assert actual == expected, (argv, actual, expected)
        _, status, usage = os.wait4(child.pid, 0)
        elapsed = time.perf_counter_ns() - start
        child.returncode = os.waitstatus_to_exitcode(status)
        stderr.seek(0)
        data = stderr.read()
    assert (child.returncode, data) == (0, b''), (argv, child.returncode, data[:1024])
    return elapsed / 1e6, (usage.ru_utime + usage.ru_stime) * 1e3


for archive, plain, workers in cases:
    with (w / 'fixtures' / plain).open('rb') as f:
        expected = hashlib.file_digest(f, 'sha256').hexdigest()
    for who in binary:
        sample(archive, workers, who, expected)
print(f'verified exact SHA-256, EOF and quiet stderr for {len(cases) * 2} decodes', file=sys.stderr)

writer = csv.writer(sys.stdout, delimiter='\t', lineterminator='\n')
writer.writerow(['run', 'pair', 'archive', 'workers', 'position', 'variant', 'wall_ms', 'cpu_ms'])
for run in (1, 2):
    rng = random.Random((62303 if opts.confirm else 32906) + run)
    order_by_case = {case: ['base'] * (samples // 2) + ['trial'] * (samples // 2) for case in cases}
    for order in order_by_case.values():
        rng.shuffle(order)
    for pair in range(-1, samples):
        rng.shuffle(cases)
        for case in cases:
            archive, _, workers = case
            first = ('base', 'trial')[run % 2] if pair < 0 else order_by_case[case][pair]
            second = 'base' if first == 'trial' else 'trial'
            for position, who in enumerate((first, second), 1):
                wall, cpu = sample(archive, workers, who)
                if pair >= 0:
                    writer.writerow([run, pair, archive, workers, position, who, f'{wall:.6f}', f'{cpu:.6f}'])
        sys.stdout.flush()
