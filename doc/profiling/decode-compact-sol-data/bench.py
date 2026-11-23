#!/usr/bin/env python3
"""Checked C7 replay, serial W1 CLI, and a fixed 4 KiB scalar control."""
import argparse
from contextlib import ExitStack
import fcntl
import filecmp
import gzip
import json
from math import comb
import os
from pathlib import Path
import platform
import random
import select
import statistics as st
import subprocess as sp
import sys
from tempfile import TemporaryDirectory, TemporaryFile
import time

DOC = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(DOC / 'decode-replay-data'))
from perf import MANIFEST, capture, identity, measure, request, snapshot

SEEDS = (713531, 396833)
REPEATS = {'single': (1, 4, 16), 'mozilla': (4, 10, 50), 'sao': (24, 48, 250)}


def interval(xs):
    assert len(xs) == 24
    xs = sorted(xs)
    rank = max(k for k in range(len(xs)//2) if 40 * sum(comb(len(xs), j) for j in range(k+1)) <= 2**len(xs))
    return xs[rank], xs[-rank-1]


def emit(argv, out):
    with out.open('wb') as output:
        child = sp.run(argv, stdout=output, stderr=sp.PIPE, timeout=60, check=True)
    assert not child.stderr, (argv, child.stderr)
    return identity(out)


def verify(commands, out, units, expected=None):
    with TemporaryDirectory(prefix='verify-', dir=out) as path:
        result = {}
        for label, argv in commands.items():
            decoded = Path(path) / label
            found = emit(argv, decoded)
            assert found['bytes'] == units and (expected is None or found == expected), (argv, found)
            assert label == 'a' or filecmp.cmp(Path(path) / 'a', decoded, shallow=False), argv
            result[label] = {'argv': argv, 'output': found, 'exit_code': 0, 'stderr': ''}
    return result


def cli_once(argv, sink, err):
    start = time.monotonic_ns()
    child = sp.Popen(argv, stdout=sink, stderr=err)
    _, status, used = os.wait4(child.pid, 0)
    wall = time.monotonic_ns() - start
    child.returncode = os.waitstatus_to_exitcode(status)
    err.seek(0)
    stderr = err.read()
    assert child.returncode == 0 and not stderr, (argv, child.returncode, stderr)
    return {'wall_ns': wall, 'child_user_ns': round(1e9*used.ru_utime), 'child_sys_ns': round(1e9*used.ru_stime),
            'child_cpu_ns': round(1e9*(used.ru_utime + used.ru_stime)), 'exit_code': child.returncode, 'stderr': ''}


def run_group(case, orders, cpu, out):
    labels = case['labels']
    repeats = case['repeats']
    with ExitStack() as stack:
        ready = {}
        if case['kind'] == 'cli':
            sink = stack.enter_context(open('/dev/null', 'wb'))
            err = stack.enter_context(TemporaryFile(dir=out))
            run = warm = lambda label: cli_once(case['argv']['a' if label == 'aa' else label], sink, err)
        else:
            workers = {}
            for label in ('a', *labels):
                argv = case['argv']['a' if label == 'aa' else label]
                p = stack.enter_context(sp.Popen(argv, stdin=sp.PIPE, stdout=sp.PIPE))
                stack.callback(p.stdin.close)
                workers[label] = p
                assert select.select([p.stdout], [], [], 60)[0], ('startup timeout', argv)
                row = p.stdout.readline().decode().split()
                assert len(row) == 5 and row[:3] == ['ready', str(case['count']), str(case['units'])], row
                assert row[4] == ('bad-checksum-rejected' if case['kind'] == 'block' else 'bad-table-rejected'), row
                assert os.sched_getaffinity(p.pid) == {cpu}
                assert len(list(Path(f'/proc/{p.pid}/task').iterdir())) == 1
                ready[label] = row
            assert all(row == ready['a'] for row in ready.values())
            warm = lambda label: request(workers[label], repeats)
            run = lambda label: measure(workers[label], repeats)
        warmup = {label: warm(label) for label in ('a', *labels)}
        pairs = []
        for target, first in orders:
            names = ('a', target)
            samples = [None, None]
            for pos in (first, 1 - first):
                samples[pos] = run(names[pos])
            pairs.append({'target': target, 'first': first, 'samples': samples})
        if case['kind'] != 'cli':
            for p in workers.values():
                p.stdin.close()
                assert p.wait(timeout=10) == 0
    return {'ready': ready, 'warmup': warmup, 'pairs': pairs}


def dump(path, obj):
    with gzip.GzipFile(filename='', mode='wb', fileobj=path.open('wb'), mtime=0) as f:
        f.write((json.dumps(obj, indent=2) + '\n').encode())


def run(a):
    assert a.cpu in os.sched_getaffinity(0)
    a.out.mkdir()
    m = json.loads(MANIFEST.read_text())
    arcs = {r['file'].removesuffix('.zxc'): r for r in m['members']}
    arcs['single'] = {**m['single'], **{'plain_'+k: v for k, v in m['plain'].items()}}
    raw = {'host': platform.uname()._asdict(), 'cpu': a.cpu, 'perf': sp.check_output(['perf', 'version'], text=True).strip(),
           'cpu_model': next(l.split(':', 1)[1].strip() for l in Path('/proc/cpuinfo').read_text().splitlines()
                             if l.startswith('model name')), 'argv': sys.argv, 'binaries': {}, 'fixtures': {},
           'cases': {}, 'series': []}
    pin = ['taskset', '-c', str(a.cpu)]
    lock = Path('.test-agent/zxc-perf.lock').resolve()
    raw['lock'] = str(lock)
    with lock.open('a') as f:
        fcntl.flock(f, fcntl.LOCK_EX)
        raw['before'] = snapshot(a.cpu)
        for fixture in (*REPEATS, 'small'):
            if fixture == 'small':
                archive = a.small / 'mixed12.zxc'
                arc, plain = identity(archive), identity(a.small / 'mixed')
                assert arc['sha256'] == 'b466221ef7a66c65d9cd8ab555ae4db42b38d4ef5298be94000b8373d635e075'
                assert plain['sha256'] == '4ceb8833b6722e35e33a35e11b57cd7af03defcaf18502dd68907a2fae43e2bd'
                blocks, indexes = 256, {}
                kinds = {'block': 24, 'cli': 1}
            else:
                archive = a.fixtures / f'{fixture}.zxc'
                arc = identity(archive)
                exp = arcs[fixture]
                assert arc == {k: exp[k] for k in arc}
                plain = {k: exp['plain_'+k] for k in arc}
                blocks, indexes = capture(archive, a.out)
                kinds = dict(zip(('block', 'lit', 'tok', 'cli'), (*REPEATS[fixture], 1)))
            if a.repeat:
                if fixture != 'single':
                    continue
                kinds = {k: kinds[k] for k in ('block', 'cli')}
            raw['fixtures'][fixture] = {'path': str(archive), 'archive': arc, 'plain': plain}
            scalar = '-scalar' if fixture == 'small' else ''
            for kind, reps in kinds.items():
                labels = ['b', 'aa'] + (['c'] if fixture != 'small' and kind != 'cli' and not a.repeat else [])
                index = indexes.get(kind)
                units = index['units'] if index else plain['bytes']
                case = dict(kind=kind, count=index['count'] if index else blocks, units=units,
                            repeats=reps, labels=labels, argv={}, index=index)
                for label in ['a', *[l for l in labels if l != 'aa']]:
                    stem = 'cli' if kind == 'cli' else 'replay'
                    exe = a.build / f'{stem}-{label}{scalar}'
                    if str(exe) not in raw['binaries']:
                        raw['binaries'][str(exe)] = identity(exe)
                    flags = ['decode', '--threads', '1'] if kind == 'cli' else ['serve']
                    case['argv'][label] = [*pin, str(exe), *flags, str(archive)] + ([index['index']['path']] if index else [])
                case['verify'] = verify({l: [*v[:4], 'verify', *v[5:]] if kind != 'cli' else v
                                         for l, v in case['argv'].items()}, a.out, units, None if index else plain)
                if kind == 'cli' and fixture != 'small':
                    exe = a.build / 'cli-c'
                    argv = [*pin, str(exe), '-qdc', '-T1', str(archive)]
                    case['verify']['c'] = verify({'a': argv}, a.out, units, plain)['a']
                    raw['binaries'][str(exe)] = identity(exe)
                raw['cases'][f'{fixture}-{kind}'] = case
        path = a.out / 'pairs.json.gz'
        for seed in (502661,) if a.repeat else SEEDS:
            rng = random.Random(seed)
            order = list(raw['cases'])
            rng.shuffle(order)
            series = {'seed': seed, 'before': snapshot(a.cpu), 'group_order': order, 'cases': {}}
            raw['series'].append(series)
            for name in order:
                case = raw['cases'][name]
                balanced = {label: rng.sample([0, 1]*12, 24) for label in case['labels']}
                orders = [(label, balanced[label][i]) for i in range(24)
                          for label in rng.sample(case['labels'], len(case['labels']))]
                before = snapshot(a.cpu)
                series['cases'][name] = {**run_group(case, orders, a.cpu, a.out), 'before': before, 'after': snapshot(a.cpu)}
                dump(path, raw)
                print(seed, name, 'complete', flush=True)
            series['after'] = snapshot(a.cpu)
        raw['after'] = snapshot(a.cpu)
        dump(path, raw)


def summary(path):
    with gzip.open(path, 'rt') as f:
        raw = json.load(f)
    print('series\tcase\tcomparison\tmetric\tbase\tother\tpaired_pct\tci_lo\tci_hi')
    for series in raw['series']:
        for name, case in raw['cases'].items():
            if name not in series['cases']:
                continue
            norm = case['units'] * case['repeats']
            for target in case['labels']:
                samples = [p['samples'] for p in series['cases'][name]['pairs'] if p['target'] == target]
                keys = ['cycles:u', 'instructions:u', 'thread_cpu_ns', 'wall_ns'] if case['kind'] != 'cli' else ['child_cpu_ns', 'wall_ns']
                for key in keys:
                    a, b = zip(*[(p[0][key], p[1][key]) for p in samples])
                    modes = [(key.replace('_ns', '_s/GB'), [x/norm for x in a], [y/norm for y in b])]
                    if key == 'wall_ns':
                        modes.append(('MB/s', [1000*norm/x for x in a], [1000*norm/y for y in b]))
                    for metric, x, y in modes:
                        ds = [100*(v/u-1) for u, v in zip(x, y)]
                        print(series['seed'], name, 'a/'+target, metric, *[f'{v:.5f}' for v in
                              (st.median(x), st.median(y), st.median(ds), *interval(ds))], sep='\t')


if __name__ == '__main__':
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--cpu', type=int, default=3)
    p.add_argument('--summary', type=Path)
    p.add_argument('--repeat', action='store_true', help='single targeted 24-pair repeat of Rust single checked and W1')
    for name in ('fixtures', 'small', 'build', 'out'):
        p.add_argument(name, nargs='?', type=lambda arg: Path(arg).resolve())
    args = p.parse_args()
    summary(args.summary) if args.summary else run(args)
