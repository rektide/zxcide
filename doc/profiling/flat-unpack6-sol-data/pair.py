#!/usr/bin/env python3
"""Width-six trial: saved exact runs first, then C7 pipeline and controls."""
import argparse
from contextlib import ExitStack
import fcntl
import json
import os
from pathlib import Path
import platform
import random
import select
import subprocess as sp
import sys

DOC = Path('doc/profiling').resolve()
sys.path[:0] = [str(DOC/'decode-compact-sol-data'), str(DOC/'decode-replay-data')]
from bench import dump, run_group, summary, verify
from perf import MANIFEST, capture, identity, measure, request, snapshot
from saved import extract


def kernel_group(case, orders, cpu, out):
    with ExitStack() as st:
        procs, ready = {}, {}
        for label, argv in case['argv'].items():
            p = st.enter_context(sp.Popen(argv, stdin=sp.PIPE, stdout=sp.PIPE))
            st.callback(p.stdin.close)
            assert select.select([p.stdout], [], [], 60)[0]
            assert os.sched_getaffinity(p.pid) == {cpu}
            assert len(list(Path(f'/proc/{p.pid}/task').iterdir())) == 1
            row = p.stdout.readline().decode().split()
            assert len(row) == 5 and row[:3] == ['ready', str(case['count']), str(case['units'])] and row[4] == 'flat6'
            procs[label], ready[label] = p, row
        assert ready['a'] == ready['b']
        n = case['repeats']
        warm = {label: request(p, n) for label, p in procs.items()}
        pairs = []
        for target, first in orders:
            samples = [None, None]
            for pos in (first, 1-first):
                samples[pos] = measure(procs[('a', target)[pos]], n)
            pairs.append({'target': target, 'first': first, 'samples': samples})
        for p in procs.values():
            p.stdin.close()
            assert p.wait(timeout=10) == 0
    return {'ready': ready, 'warmup': warm, 'pairs': pairs}


def profile(case, out):
    path = out / 'dispatch.perf.data'
    with sp.Popen(case['argv']['b'], stdin=sp.PIPE, stdout=sp.PIPE) as p:
        try:
            assert select.select([p.stdout], [], [], 60)[0]
            row = p.stdout.readline().decode().split()
            assert row[:3] == ['ready', str(case['count']), str(case['units'])]
            request(p, case['repeats'])
            raw = measure(p, 28, path)
        finally:
            p.stdin.close()
        assert p.wait(timeout=10) == 0
    return raw


def run(a):
    a.out.mkdir()
    manifest = json.loads(MANIFEST.read_text())
    arcs = {r['file'].removesuffix('.zxc'): r for r in manifest['members']}
    arcs['single'] = {**manifest['single'], **{'plain_'+k: v for k, v in manifest['plain'].items()}}
    raw = {'host': platform.uname()._asdict(), 'cpu': a.cpu, 'perf': sp.check_output(['perf', 'version'], text=True).strip(),
           'cpu_model': next(l.split(':', 1)[1].strip() for l in Path('/proc/cpuinfo').read_text().splitlines()
                             if l.startswith('model name')), 'argv': sys.argv, 'binaries': {}, 'fixtures': {},
           'cases': {}, 'series': []}
    pin = ['taskset', '-c', str(a.cpu)]
    lock = Path('.test-agent/zxc-perf.lock').resolve()
    raw['lock'] = str(lock)
    path = a.out / 'pairs.json.gz'

    def prepare(name, kind, stem, argvs, count, units, repeats, expected, index=None):
        commands = {}
        for label in ('a', 'b'):
            exe = a.build / stem.format(label=label)
            if str(exe) not in raw['binaries']:
                raw['binaries'][str(exe)] = identity(exe)
            commands[label] = [*pin, str(exe), *argvs]
        checks = verify({l: [*v[:4], 'verify', *v[5:]] if kind != 'cli' else v
                         for l, v in commands.items()}, a.out, units, expected)
        raw['cases'][name] = dict(kind=kind, count=count, units=units, repeats=repeats, labels=['b'],
                                  argv=commands, index=index, verify=checks)

    def series(seed, names):
        rng = random.Random(seed)
        order = list(names)
        rng.shuffle(order)
        section = dict(seed=seed, before=snapshot(a.cpu), group_order=order, cases={})
        raw['series'].append(section)
        for name in order:
            case = raw['cases'][name]
            orders = [('b', first) for first in rng.sample([0, 1]*12, 24)]
            before = snapshot(a.cpu)
            fn = kernel_group if case['kind'] == 'kernel' else run_group
            section['cases'][name] = {**fn(case, orders, a.cpu, a.out), 'before': before, 'after': snapshot(a.cpu)}
            dump(path, raw)
            print(seed, name, 'complete', flush=True)
        section['after'] = snapshot(a.cpu)
        dump(path, raw)

    with lock.open('a') as f:
        fcntl.flock(f, fcntl.LOCK_EX)
        raw['before'] = snapshot(a.cpu)
        fixtures = ('single',) if a.repeat else ('single', 'mozilla', 'sao', 'small')
        for fixture in fixtures:
            if fixture == 'small':
                archive = a.small / 'mixed12.zxc'
                arc, plain = identity(archive), identity(a.small / 'mixed')
                assert arc['sha256'] == 'b466221ef7a66c65d9cd8ab555ae4db42b38d4ef5298be94000b8373d635e075'
                assert plain['sha256'] == '4ceb8833b6722e35e33a35e11b57cd7af03defcaf18502dd68907a2fae43e2bd'
                blocks, indexes = 256, {}
            else:
                archive = a.fixtures / f'{fixture}.zxc'
                arc = identity(archive)
                assert arc == {k: arcs[fixture][k] for k in arc}
                plain = {k: arcs[fixture]['plain_'+k] for k in arc}
                blocks, indexes = capture(archive, a.out)
            raw['fixtures'][fixture] = {'path': str(archive), 'archive': arc, 'plain': plain}
            if fixture == 'single' and not a.repeat:
                index = indexes['lit']
                saved = a.out / 'single-flat6.bin'
                shape = extract(archive, Path(index['index']['path']), saved)
                assert shape['count'] == 334 and shape['units'] == 8441486
                raw['saved'] = {**shape, **identity(saved)}
                prepare('single-flat6', 'kernel', 'flat6-{label}', ['serve', str(saved)], shape['count'],
                        shape['units'], 32, None, index)
                series(836993, ['single-flat6'])
                prepare('single-lit', 'lit', 'replay-{label}', ['serve', str(archive), index['index']['path']],
                        index['count'], index['units'], 4, None, index)

            copies = ('', '-scalar') if fixture == 'small' else ('',)
            for scalar in copies:
                name = fixture + scalar
                reps = {'single': 1, 'mozilla': 4, 'sao': 24, 'small': 24}[fixture]
                prepare(f'{name}-block', 'block', 'replay-{label}'+scalar, ['serve', str(archive)],
                        blocks, plain['bytes'], reps, plain)
                prepare(f'{name}-cli', 'cli', 'cli-{label}'+scalar, ['decode', '--threads', '1', str(archive)],
                        blocks, plain['bytes'], 1, plain)

        names = [name for name in raw['cases'] if name != 'single-flat6']
        series(725977 if a.repeat else 842797, names)
        if not a.repeat:
            series(848941, ('single-lit', 'single-block', 'single-cli', 'mozilla-block', 'mozilla-cli'))
            raw['dispatch'] = profile(raw['cases']['single-lit'], a.out)
        raw['after'] = snapshot(a.cpu)
        dump(path, raw)


if __name__ == '__main__':
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--cpu', type=int, default=3)
    p.add_argument('--summary', type=Path)
    p.add_argument('--repeat', action='store_true')
    for name in ('fixtures', 'small', 'build', 'out'):
        p.add_argument(name, nargs='?', type=lambda s: Path(s).resolve())
    a = p.parse_args()
    summary(a.summary) if a.summary else run(a)
