#!/usr/bin/env python3
"""Reduce a clean, stage or perf paired-run JSON (optionally gzip-compressed)."""
import gzip
import json
from math import comb
from pathlib import Path
from statistics import median
import sys


def median_ci(values):
    v = sorted(values)
    n = len(v)
    k = max(i for i in range(n//2) if sum(comb(n, j) for j in range(i)) <= .025 * 2**n)
    return [median(v), v[k - 1], v[n - k]]


def fmt_range(v):
    return f'{v[0]:+.2f}% [{v[1]:+.2f}, {v[2]:+.2f}]'


def reduce_perf(raw):
    for name, row in raw['cases'].items():
        rounds = row['rounds']
        volume = row['repeats'] * row['units']
        print(f'{name} ({len(rounds)} rounds; {row["count"]} sections/blocks; {row["units"]} output bytes/pass):')
        for label in ('c', 'rust', 'rust-native'):
            r = [r[label] for r in rounds]
            cyc, ins, cpu = (median(float(p[k]) / volume for p in r)
                             for k in ('cycles:u', 'instructions:u', 'thread_cpu_ns'))
            wall = median(volume * 1000 / p['wall_ns'] for p in r)
            off = median(100 * (1 - p['thread_cpu_ns'] / p['wall_ns']) for p in r)
            ghz = median(p['cycles:u'] / p['thread_cpu_ns'] for p in r)
            print(f'  {label:<12} {cyc:.4f} cyc/B {ins:.4f} ins/B {wall:.0f} MB/s {cpu:.4f} s/GB'
                  f' {off:.2f}% off-core {ghz:.2f} user cyc/thread ns')
        for label in ('c', 'rust-native'):
            deltas = [median_ci([100 * (r[label][k] / r['rust'][k] - 1) for r in rounds])
                      for k in ('cycles:u', 'instructions:u', 'thread_cpu_ns')]
            print(f'  {label}/rust: ' + ' | '.join(map(fmt_range, deltas)))


p = Path(sys.argv[1])
raw = json.loads(gzip.decompress(p.read_bytes()) if p.suffix == '.gz' else p.read_bytes())
if 'cases' in raw:
    reduce_perf(raw)
    sys.exit(0)
for series, s in enumerate(raw['series'], 1):
    for g in s['groups']:
        name = g['name']
        size = raw['fixtures'][name]['plain']['bytes'] * g['repeats']
        pairs = [p['results'] for p in g['pairs']]
        assert len(pairs) == raw['pairs'] and sum(p['first'] for p in g['pairs']) == len(pairs) // 2
        labels = '/'.join(g['labels'])
        print(f'S{series} {name} {labels}:', end=' ')
        for side in (0, 1):
            wall = [p[side][0] for p in pairs]
            cpu = [p[side][1] for p in pairs]
            bw, cpg = median(size / x * 1000 for x in wall), median(x / size for x in cpu)
            off = median(100 * (1 - c / w) for c, w in zip(cpu, wall))
            print(f'{bw:.1f} MB/s {cpg:.4f} s/GB {off:.2f}% off-core', end=' | ')
        for i in (0, 1):
            delta = median_ci([100 * (p[1][i] / p[0][i] - 1) for p in pairs])
            print(fmt_range(delta), end=' | ')
        print()
        if raw['mode'] == 'stage':
            assert all(sum(side[2:5]) <= side[0] for p in pairs for side in p)
            for i in (2, 3, 4):
                r, c = [median(p[side][i] / size for p in pairs) for side in (0, 1)]
                delta = median_ci([(p[0][i] - p[1][i]) / size for p in pairs])
                print(f'  {raw["argv_columns"][i]}: {r:.4f}/{c:.4f} s/GB, '
                      f'R−C {delta[0]:+.4f} [{delta[1]:+.4f}, {delta[2]:+.4f}]')
