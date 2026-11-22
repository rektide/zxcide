#!/usr/bin/env python3
"""Paired single-core, in-process timing; archive/verification I/O is untimed."""
import argparse
from collections import Counter
from datetime import datetime, timezone
import fcntl
import hashlib
import json
import os
from pathlib import Path
import platform
import random
import subprocess as sp
from time import perf_counter_ns


def identity(path):
    with path.open('rb') as f:
        return {'bytes': path.stat().st_size, 'sha256': hashlib.file_digest(f, 'sha256').hexdigest()}


def verify(cmd):
    h = hashlib.sha256()
    length = 0
    with sp.Popen(cmd, stdout=sp.PIPE, stderr=sp.PIPE) as p:
        for chunk in iter(lambda: p.stdout.read(2**20), b''):
            h.update(chunk)
            length += len(chunk)
        assert p.stderr.read() == b'' and p.wait() == 0, cmd
    return {'bytes': length, 'sha256': h.hexdigest()}


def census(path):
    with path.open('rb') as f:
        prologue = f.read(16)
        assert prologue[6] == 0x80
        modes, symbols = Counter(), Counter()
        while (h := f.read(8))[0] != 255:
            n = int.from_bytes(h[3:7], 'little')
            d = f.read(12)
            assert h[0] == 1 and d[11] == 0 and n >= 12
            l, t = d[8:10]
            for key, mode, count in [('tokens', t, d[:4]), ('literals', l, d[4:8])]:
                label = f'{key}:{["raw", "rle", "huf"][mode]}'
                modes[label] += 1
                symbols[label] += int.from_bytes(count, 'little')
            f.seek(n - 12 + 4, 1)
    return {'blocks': sum(v for k, v in modes.items() if k.startswith('tokens:')),
            'block_limit': 1 << prologue[5], 'section_counts': dict(modes), 'symbols': dict(symbols)}


def snapshot():
    def read(path):
        p = Path(path)
        return p.read_text().strip() if p.exists() else None
    cpu_dir = f'/sys/devices/system/cpu/cpu{a.cpu}'
    return {'utc': datetime.now(timezone.utc).isoformat(), 'load': read('/proc/loadavg'),
            'governor': read(f'{cpu_dir}/cpufreq/scaling_governor'),
            'scaling_cur_freq_khz': read(f'{cpu_dir}/cpufreq/scaling_cur_freq'),
            'boost': read('/sys/devices/system/cpu/cpufreq/boost'),
            'thread_siblings': read(f'{cpu_dir}/topology/thread_siblings_list')}


def sample(p, repeats):
    start = perf_counter_ns()
    p.stdin.write(f'{repeats}\n')
    p.stdin.flush()
    data = [int(x) for x in p.stdout.readline().split()]
    assert len(data) == (5 if a.stage else 2) and min(data) > 0, data
    data.append(perf_counter_ns() - start)
    return data


parser = argparse.ArgumentParser(description=__doc__)
for k in ('fixtures', 'build', 'clean_build', 'out'):
    parser.add_argument(k, type=lambda p: Path(p).resolve())
parser.add_argument('--cpu', type=int, default=3)
parser.add_argument('--pairs', type=int, default=24)
parser.add_argument('--stage', action='store_true')
parser.add_argument('--verify-only', action='store_true')
parser.add_argument('--lock', type=Path, default=Path('.test-agent/zxc-perf.lock'))
a = parser.parse_args()
assert a.cpu in os.sched_getaffinity(0) and a.pairs >= 6 and a.pairs % 2 == 0
prefix = ['taskset', '-c', str(a.cpu)]
bins = {label: a.build / f'replay-{label}' for label in ('rust', 'c')}
paths = {name: a.fixtures / f'{name}.zxc' for name in ('single', 'mozilla', 'sao')}
repeats = {'single': 1, 'mozilla': 2, 'sao': 6}
m = json.loads((a.fixtures / 'manifest.json').read_text())
archives = {n.removesuffix('.zxc'): row for row in m['members'] if (n := row['file']) in {p.name for p in paths.values()}}
archives['single'] = {**m['single'], 'plain_bytes': m['plain']['bytes'], 'plain_sha256': m['plain']['sha256']}
c_cli = a.fixtures / 'c'
assert identity(c_cli)['sha256'] == m['c']['sha256']
rust_cli = a.clean_build / 'rust-cli'
meta = {'mode': 'stage' if a.stage else 'clean', 'cpu': a.cpu, 'pairs': a.pairs,
        'lock': str(a.lock.resolve()), 'host': platform.uname()._asdict(),
        'cpu_model': next(x.split(':', 1)[1].strip() for x in Path('/proc/cpuinfo').read_text().splitlines()
                          if x.startswith('model name')),
        'argv_columns': ['wall_ns', 'thread_cpu_ns'] + (['section_ns', 'reconstruct_ns', 'checksum_ns'] if a.stage else [])
                        + ['driver_round_trip_ns'],
        'binaries': {label: {'path': str(path), **identity(path)}
                     for label, path in {**bins, 'c_cli': c_cli, 'rust_cli': rust_cli}.items()},
        'fixtures': {}, 'verification': [], 'series': []}

with a.lock.open('a') as lock:
    fcntl.flock(lock, fcntl.LOCK_EX)
    meta['initial'] = snapshot()
    for name, path in paths.items():
        expected = archives[name]
        arc = identity(path)
        assert all(expected[k] == v for k, v in arc.items()), name
        plain = {'bytes': expected['plain_bytes'], 'sha256': expected['plain_sha256']}
        meta['fixtures'][name] = {'path': str(path), 'wire': arc, 'plain': plain, **census(path)}
        for cmd in ([str(c_cli), '-qdc', '-T1'], [str(rust_cli), 'decode', '--threads', '1'],
                    *([str(p), 'verify'] for p in bins.values())):
            argv = prefix + cmd + [str(path)]
            result = verify(argv)
            assert result == plain, (argv, result)
            meta['verification'].append({'argv': argv, **result})
    if not a.verify_only:
        for seed in (237610, 188814) if not a.stage else (557281, 165034):
            rng = random.Random(seed)
            groups = list((n, ('rust', 'c')) for n in paths)
            if not a.stage:
                groups.append(('single', ('rust', 'rust')))
            rng.shuffle(groups)
            series = {'seed': seed, 'groups': []}
            for name, labels in groups:
                argv = [prefix + [str(bins[l]), 'serve', str(paths[name])] for l in labels]
                file_rows = meta['fixtures'][name]
                order = [0, 1] * (a.pairs // 2)
                rng.shuffle(order)
                grp = {'name': name, 'labels': labels, 'argv': argv, 'repeats': repeats[name],
                       'before': snapshot(), 'pairs': []}
                with sp.Popen(argv[0], stdin=sp.PIPE, stdout=sp.PIPE, stderr=sp.PIPE, text=True) as p0, \
                     sp.Popen(argv[1], stdin=sp.PIPE, stdout=sp.PIPE, stderr=sp.PIPE, text=True) as p1:
                    procs = [p0, p1]
                    ready = [p.stdout.readline().strip().split() for p in procs]
                    assert ready[0] == ready[1], ready
                    for p, (msg, blocks, length, tag, reject) in zip(procs, ready):
                        assert os.sched_getaffinity(p.pid) == {a.cpu}
                        assert len(list(Path(f'/proc/{p.pid}/task').iterdir())) == 1
                        assert (msg, int(blocks), int(length), reject) == (
                            'ready', file_rows['blocks'], file_rows['plain']['bytes'], 'bad-checksum-rejected')
                    grp['ready'] = ready[0]
                    # One explicit untimed request to each resident worker, after its full initial pass.
                    grp['warmup'] = [sample(p, grp['repeats']) for p in procs]
                    for first in order:
                        pair = [None, None]
                        for i in (first, 1 - first):
                            pair[i] = sample(procs[i], grp['repeats'])
                        grp['pairs'].append({'first': first, 'results': pair})
                    for p in procs:
                        p.stdin.close()
                        assert p.stderr.read() == '' and p.wait() == 0
                grp['after'] = snapshot()
                series['groups'].append(grp)
                print(seed, name, '/'.join(labels), 'complete', flush=True)
            meta['series'].append(series)
    meta['final'] = snapshot()
a.out.write_text(json.dumps(meta, indent=2) + '\n')
