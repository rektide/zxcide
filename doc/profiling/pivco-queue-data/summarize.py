#!/usr/bin/env python3
import csv
from collections import defaultdict
from pathlib import Path
from statistics import median, quantiles
import sys

print('run\tarchive\tworkers\tmetric\tpairs\tbaseline_median_ms\ttrial_median_ms\tpaired_median_pct\tpaired_q1_pct\tpaired_q3_pct')
for path in sys.argv[1:]:
    with Path(path).open() as f:
        pairs = defaultdict(dict)
        for row in csv.DictReader(f, delimiter='\t'):
            key = tuple(row[k] for k in ('run', 'archive', 'workers', 'pair'))
            assert row['variant'] not in pairs[key]
            pairs[key][row['variant']] = row
        groups = defaultdict(list)
        for key, pair in sorted(pairs.items()):
            assert set(pair) == {'base', 'trial'}
            groups[key[:-1]].append((pair['base'], pair['trial']))
        for key, group in sorted(groups.items()):
            assert len(group) in (12, 24)
            assert sum(a['position'] == '1' for a, _ in group) * 2 == len(group)
            for metric in ('wall_ms', 'cpu_ms'):
                if not group[0][0][metric]:
                    continue
                base, trial = zip(*[(float(a[metric]), float(b[metric])) for a, b in group])
                deltas = [100 * (b / a - 1) for a, b in zip(base, trial)]
                q1, _, q3 = quantiles(deltas, n=4, method='inclusive')
                print(*key, metric, len(group), f'{median(base):.6f}', f'{median(trial):.6f}',
                      f'{median(deltas):.4f}', f'{q1:.4f}', f'{q3:.4f}', sep='\t')
