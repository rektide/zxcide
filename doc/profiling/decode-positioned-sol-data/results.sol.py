#!/usr/bin/env python3
"""Render per-series paired medians and distribution-free median intervals."""
import gzip
import json
from pathlib import Path
import sys


def read(name):
    path = Path(name)
    with (gzip.open(path, 'rt') if path.suffix == '.gz' else path.open()) as f:
        return json.load(f)


def change(m):
    low, high = m['ci']
    return f'{m["delta"]:+.2f}% [{low:+.2f}, {high:+.2f}]'


def ms(m):
    return f'{m["a"] / 1e6:.3f} → {m["b"] / 1e6:.3f}'


def triple(a, b):
    n, cpu, clock = a
    p, other_cpu, other_clock = b
    cnt = f'{n}' if n == p else f'{n} → {p}'
    return f'{cnt} | {cpu/1e6:.3f} → {other_cpu/1e6:.3f} | {clock/1e6:.3f} → {other_clock/1e6:.3f}'


print('# Positioned-read trial: complete per-series tables\n')
for filename in sys.argv[1:]:
    raw = read(filename)
    for s in raw['series']:
        print(f'## {raw["phase"].capitalize()} seed {s["seed"]}\n')
        fmt = lambda load: ', '.join(f'{n:.2f}' for n in load)
        print(f'Load (1/5/15 min): {fmt(s["load_start"])} → {fmt(s["load_end"])}. '
              '24 adjacent pairs per row. `A→A`, `S→S` and `B→B` are fresh noise controls for the named contrast, not corrections.\n')
        print('| Archive | Setting | Contrast | Timed modes | Wall ms | Paired wall Δ [≥95% interval] | Child CPU s/GB | Paired CPU Δ [≥95% interval] |')
        print('| --- | --- | --- | --- | ---: | ---: | ---: | ---: |')
        for r in s['summary']:
            cpu = r['cpu']
            modes = '→'.join(r['labels'])
            print(f'| {r["layout"]} | {r["sink"]} | {r["contrast"]} | {modes} | {ms(r["wall"])} | '
                  f'{change(r["wall"])} | {cpu["a"]:.3f} → {cpu["b"]:.3f} | {change(cpu)} |')
        if raw['phase'] == 'diag':
            print('\nSuccessful admission calls only (DS→DB):\n')
            print('| Archive | Jobs | Coordinator CPU ms | CPU ns/block | Paired CPU Δ [≥95% interval] | Admission wall ms | Wall ns/block | Paired wall Δ [≥95% interval] |')
            print('| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |')
            for r in s['summary']:
                if 'next_cpu' not in r:
                    continue
                j = r['counts'][0]['jobs']
                cpu, wall = r['next_cpu'], r['next_wall']
                per = lambda m: f'{m["a"]/j:.0f} → {m["b"]/j:.0f}'
                print(f'| {r["layout"]} | {j} | {ms(cpu)} | {per(cpu)} | {change(cpu)} | {ms(wall)} | {per(wall)} | {change(wall)} |')
            print('\nOther coordinator read phases (DS→DB totals, not included above):\n')
            print('| Archive | Phase | Calls | CPU ms | Elapsed ms |')
            print('| --- | --- | ---: | ---: | ---: |')
            for r in s['summary']:
                if 'phases' not in r:
                    continue
                a, b = r['phases']
                for p in a:
                    if p != 'data':
                        print(f'| {r["layout"]} | {p} | {triple(a[p], b[p])} |')
        print()
