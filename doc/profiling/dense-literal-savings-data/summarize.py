#!/usr/bin/env python3
"""Derive the reported paired statistics and wire census from the raw tables."""

import csv
from collections import defaultdict
from pathlib import Path
from statistics import median
import sys


root = Path(sys.argv[1]) if len(sys.argv) > 1 else Path(__file__).parent
pcts = (0, 1, 2, 4)


def rows(file):
    with (root / file).open() as stream:
        yield from csv.DictReader(stream, delimiter='\t')


def print_table(header, data):
    print('|', ' | '.join(header), '|')
    print('|', ' | '.join('---' for _ in header), '|')
    for row in data:
        print('|', ' | '.join(map(str, row)), '|')
    print()


def time(row):
    return float(row['wall_ms']), float(row['user_ms']) + float(row['sys_ms'])


def change(a, b):
    return 100 * (a / b - 1)


def paired(candidates, baselines):
    assert candidates.keys() == baselines.keys()
    return [change(candidates[i], baselines[i]) for i in sorted(baselines)]


enc = defaultdict(lambda: defaultdict(lambda: (0., 0.)))
for row in rows('encode.tsv'):
    key = int(row['round'])
    w, c = time(row)
    pct = int(row['threshold'])
    w0, c0 = enc[pct][key]
    enc[pct][key] = (w + w0, c + c0)
assert all(len(enc[pct]) == 7 for pct in pcts)
print('Encode, paired corpus sums')
print_table(('Threshold', 'Wall ms', 'CPU ms', 'Paired CPU change', 'CPU range'), [
    (pct, *(f'{median(v[i] for v in enc[pct].values()):.1f}' for i in (0, 1)),
     f'{median(delta):+.2f}%', f'[{min(delta):+.2f}%, {max(delta):+.2f}%]')
    for pct in pcts
    for delta in [paired({k: v[1] for k, v in enc[pct].items()}, {k: v[1] for k, v in enc[0].items()})]
])

print('Decode, paired changes and median intervals; deltas in percent, time in ms')
dec = defaultdict(lambda: defaultdict(dict))
for row in rows('decode.tsv'):
    dec[row['form'], row['decoder'], int(row['workers'])][int(row['threshold'])][int(row['round'])] = time(row)
assert len(dec) == 10


def cell(group, pct, idx):
    base = {k: v[idx] for k, v in group[0].items()}
    values = {k: v[idx] for k, v in group[pct].items()}
    assert len(base) == 24
    if not pct:
        return f'{median(base.values()):.3f}'
    d = paired(values, base)
    # Equal-tailed sign interval for n=24; endpoints are the 7th and 18th
    # sorted observations, at least 95% coverage under independent samples.
    s = sorted(d)
    return (f'{median(d):+.2f} [{s[6]:+.2f}, {s[17]:+.2f}] '
            f'({median(d[:12]):+.2f}/{median(d[12:]):+.2f} halves)')


print_table(('Corpus', 'Decoder', 'Workers', 'Threshold', 'Wall', 'CPU'), [
    (*scenario, pct, cell(group, pct, 0), cell(group, pct, 1))
    for scenario, group in sorted(dec.items()) for pct in pcts
])

print('Sizes and removed literal Huffman blocks + symbols')
sizes = defaultdict(dict)
for row in rows('sizes.tsv'):
    sizes[row['file']][int(row['threshold'])] = int(row['archive_bytes'])
removed = defaultdict(lambda: (0, 0))
for row in rows('blocks.tsv'):
    if row['old_mode'] == '2' and row['new_mode'] != '2':
        name, pct, n = row['file'], int(row['threshold']), int(row['literals'])
        for form in [name, 'all'] + (['without-sao'] if name != 'sao' else []):
            blocks, symbols = removed[form, pct]
            removed[form, pct] = blocks + 1, symbols + n


def size_cell(name, pct):
    v, base = sizes[name][pct], sizes[name][0]
    return f'{v:,} ({change(v, base):+.3f}%; {removed[name, pct][0]} / {removed[name, pct][1]:,})'


print_table(('File/corpus', '0%', '1%', '2%', '4%'), [
    (name, f'{sizes[name][0]:,}', *(size_cell(name, pct) for pct in pcts[1:])) for name in sizes
])
