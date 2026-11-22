#!/usr/bin/env python3
import argparse
import fcntl
import hashlib
import json
from math import comb
import os
from pathlib import Path
import random
import statistics as st
import subprocess as sp
import tempfile
import time


def sha(path):
    with path.open('rb') as f:
        return hashlib.file_digest(f, 'sha256').hexdigest()


def exact(argv, digest, size):
    with tempfile.TemporaryFile(dir=a.out) as err, sp.Popen(argv, stdout=sp.PIPE, stderr=err) as p:
        h, count = hashlib.sha256(), 0
        with p.stdout as stdout:
            while buf := stdout.read(1024*1024):
                h.update(buf)
                count += len(buf)
        status = p.wait()
        err.seek(0)
        stderr = err.read(4096)
        if status != 0 or stderr or count != size or h.hexdigest() != digest:
            raise RuntimeError(f'failed exact verification: {argv}: {count} B, {stderr!r}')


def interval(xs):
    # Two-sided distribution-free CI for the median. Balanced AB/BA controls order;
    # as usual this interval does not account for temporally correlated load.
    xs = sorted(xs)
    n = len(xs)
    tail = 0
    for k in range(n//2):
        tail += comb(n, k)
        if 40*tail > 2**n:
            rank = max(0, k - 1)
            return [xs[rank], xs[-1-rank]]
    return [xs[0], xs[-1]]


def once(commands, sink, err):
    wall = cpu = rss = 0
    for argv in commands:
        start = time.perf_counter_ns()
        p = sp.Popen(argv, stdout=sink, stderr=err)
        _, status, use = os.wait4(p.pid, 0)
        wall += time.perf_counter_ns() - start
        p.returncode = os.waitstatus_to_exitcode(status)
        if p.returncode or err.tell():
            raise RuntimeError(f'decode failed: {argv!r}, {p.returncode}; see stderr.log')
        cpu += round((use.ru_utime + use.ru_stime)*1_000_000)
        rss = max(rss, use.ru_maxrss)
    return [wall, cpu, rss]


parser = argparse.ArgumentParser()
for name in ('fixtures', 'baseline', 'candidate', 'out'):
    parser.add_argument(name, type=lambda p: Path(p).resolve())
parser.add_argument('--pairs', type=int, default=24)
parser.add_argument('--wide', action='store_true')
parser.add_argument('--real-output', action='store_true')
parser.add_argument('--confirm', action='store_true', help='target concat (all counts with --wide) and single/separate at 12')
a = parser.parse_args()
assert a.pairs >= 6 and a.pairs % 2 == 0
a.out.mkdir(parents=True, exist_ok=False)
f = a.fixtures
m = json.loads((f / 'manifest.json').read_text())
archives = {f'{k}.zxc': (m[k]['sha256'], m['plain']['sha256'], m['plain']['bytes']) for k in ('concat', 'single')}
archives.update({e['file']: (e['sha256'], e['plain_sha256'], e['plain_bytes']) for e in m['members']})
for name, (arc, _, _) in archives.items():
    assert sha(f / name) == arc, name
assert sha(f / 'c') == m['c']['sha256']

bins = {'base': a.baseline, 'trial': a.candidate, 'c': f / 'c'}
cases = {k: [f'{k}.zxc'] for k in ('concat', 'single')}
cases['separate'] = [e['file'] for e in m['members']]
points = (1, 2, 3, 4, 6, 8, 12, 16) if a.wide else (1, 4, 8, 12)
comparisons = [(case, n, 'base', 'trial') for case in cases for n in points]
comparisons.extend([('concat', 8, 'base', 'base'), ('concat', 8, 'base', 'c'),
                    ('single', 1, 'base', 'c'), ('single', 8, 'base', 'c')])
if a.confirm:
    comparisons = [('concat', w, 'base', 'trial') for w in (points if a.wide else (8,))]
    comparisons.extend([('concat', 8, 'trial', 'c'), ('concat', 8, 'base', 'base')])
    if not a.real_output:
        comparisons.extend([('single', 12, 'base', 'trial'), ('separate', 12, 'base', 'trial')])


def invoke(label, workers, name):
    flags = ['-qdc', f'-T{workers}'] if label == 'c' else ['decode', '--threads', str(workers)]
    return [str(bins[label]), *flags, str(f / name)]


checks = {(b, w, name) for case, w, x, y in comparisons for name in cases[case] for b in (x, y)}
sink_path = a.out / 'plain.out' if a.real_output else Path('/dev/null')
raw = dict(sink=str(sink_path), binaries={k: [str(p), sha(p)] for k, p in bins.items()}, fixture=m,
           affinity=sorted(os.sched_getaffinity(0)), platform=os.uname().release, cases=cases,
           comparisons=comparisons, invocations={f'{b}/{w}/{n}': invoke(b, w, n) for b, w, n in sorted(checks)},
           series=[], verification=[])
raw['cpu_governors'] = {str(n): p.read_text().strip() for n in raw['affinity']
                        if (p := Path(f'/sys/devices/system/cpu/cpu{n}/cpufreq/scaling_governor')).exists()}
lock_path = Path(__file__).resolve().parents[3] / '.test-agent/zxc-perf.lock'
lock_path.parent.mkdir(exist_ok=True)
with lock_path.open('a') as lock, (a.out / 'stderr.log').open('wb') as err:
    fcntl.flock(lock, fcntl.LOCK_EX)
    raw['loadavg_start'] = os.getloadavg()
    for b, w, n in sorted(checks):
        argv = invoke(b, w, n)
        exact(argv, *archives[n][1:])
        raw['verification'].append([b, w, n])
    print(f'{len(checks)} exact decodes passed; beginning paired timing', flush=True)
    for seed in (4173, 8609):
        rng = random.Random(seed)
        orders = []
        for _ in comparisons:
            order = [0, 1]*(a.pairs//2)
            rng.shuffle(order)
            orders.append(order)
        series = dict(seed=seed, load_start=os.getloadavg(), pairs=[])
        with sink_path.open('wb') as sink:
            for p in range(-1, a.pairs):
                order = list(range(len(comparisons)))
                rng.shuffle(order)
                for i in order:
                    case, workers, *labels = comparisons[i]
                    first = orders[i][p] if p >= 0 else rng.randrange(2)
                    results = [None, None]
                    for pos in (first, 1-first):
                        if a.real_output:
                            sink.seek(0)
                            sink.truncate()
                        results[pos] = once([invoke(labels[pos], workers, n) for n in cases[case]], sink, err)
                    series['pairs'].append(dict(pass_index=p, comparison=i, first=first, results=results))
        series['load_end'] = os.getloadavg()
        raw['series'].append(series)
        (a.out / 'raw.json').write_text(json.dumps(raw, indent=2) + '\n')
        print(f'completed series {seed}, load {series["load_start"]} → {series["load_end"]}', flush=True)

lines = ['# Paired fixed-archive decode', '',
         '| Case | Workers | Compare | Baseline MB/s (S1 / S2) | Other MB/s (S1 / S2) | Paired wall Δ [95% CI] | Paired CPU Δ | Peak RSS MiB, median base → other |',
         '| --- | ---: | --- | ---: | ---: | --- | --- | --- |']
for i, (case, w, x, y) in enumerate(comparisons):
    group = [[] for _ in range(5)]
    for series in raw['series']:
        pairs = [pair['results'] for pair in series['pairs'] if pair['comparison'] == i and pair['pass_index'] >= 0]
        def delta(k):
            return [100*(b[k]/a[k]-1) for a, b in pairs]
        for pos in (0, 1):
            group[pos].append(f'{m["plain"]["bytes"]*1000/st.median(p[pos][0] for p in pairs):,.0f}')
        wvalues = delta(0)
        lo, hi = interval(wvalues)
        group[2].append(f'{st.median(wvalues):+.1f}% [{lo:+.1f},{hi:+.1f}]')
        group[3].append(f'{st.median(delta(1)):+.1f}%')
        group[4].append(' → '.join(f'{st.median(p[pos][2] for p in pairs)/1024:.1f}' for pos in (0, 1)))
    lines.append('| ' + ' | '.join([case, str(w), f'{x} → {y}', *[' / '.join(c) for c in group]]) + ' |')
result = '\n'.join(lines) + '\n'
(a.out / 'results.md').write_text(result)
print(result, end='')
