#!/usr/bin/env python3
"""Run-level paired intervals, trace budgets and within-trace percentiles."""
import argparse
from collections import defaultdict
import csv
from math import comb, ceil
from pathlib import Path
from statistics import median


def read(path):
    with path.open() as f:
        return list(csv.DictReader(f, delimiter='\t'))


def ci(xs, alpha=.05):
    """Binomial order-statistic (sign) interval for the paired median."""
    xs = sorted(xs)
    n = len(xs)
    k = max(i for i in range(n // 2 + 1) if 2 * sum(comb(n, j) for j in range(i)) / (2 ** n) <= alpha)
    return median(xs), xs[k - 1] if k else float('-inf'), xs[-k] if k else float('inf')


def timing(path):
    trials = defaultdict(lambda: defaultdict(dict))
    for r in read(path):
        if r['pair'] != '0':
            trials[r['case'], r['test'], r['series']][r['pair']][r['member']] = r
    print('PAIR DELTAS (%) / 95% sign intervals; clean scale left/right wall and CPU ms, CPU-ms/GB, mean cores')
    for label, trial in sorted(trials.items()):
        wall, cpu, rss = [], [], []
        abs_ms = ([], [], [], [], [], [], [], [])
        gb = (51220480 if label[0].startswith('mozilla') else 7251944) / 1e9
        for pair in trial.values():
            a, b = pair['0'], pair['1']
            wa, wb = [int(r['wall_ns']) / 1e6 for r in (a, b)]
            ca, cb = [(int(r['user_us']) + int(r['sys_us'])) / 1e3 for r in (a, b)]
            wall.append(100 * (wb / wa - 1))
            cpu.append(100 * (cb / ca - 1))
            rss.append(int(b['maxrss_kib']) - int(a['maxrss_kib']))
            for out, v in zip(abs_ms, (wa, wb, ca, cb, ca / gb, cb / gb, ca / wa, cb / wb)):
                out.append(v)
        print('\t'.join(label), f'n={len(trial)}',
              *(f'{m:+.2f}[{lo:+.2f},{hi:+.2f}]' for m, lo, hi in map(ci, (wall, cpu))),
              'RSSKiB Δ', f'{median(rss):+.0f}',
              *(f'{median(ms):.2f}' for ms in abs_ms) if label[1] == 'scale' else (), sep='\t')


def trace(run):
    grouped = defaultdict(list)
    sources = defaultdict(lambda: defaultdict(dict))
    for r in read(run / 'budgets.tsv'):
        grouped[r['case'], r.get('mode', r['test']), r['series']].append(r)
        if r['test'] == 'source':
            sources[r['case'], r['series']][r['pair']][r['mode']] = r
    print('TRACE medians; ns converted to ms, or fraction / denominator stated in column')
    keys = (
        'pool', 'startup_wall', 'interior_wall', 'drain_wall', 'frame', 'prologue', 'post_pool', 'footer',
        'outer_residual', 'session_residual', 'result_recv', 'a', 'b', 'b_pending', 'c', 'next', 'eof_next', 'commit', 'job_send', 'join',
        'coord_other', 'full', 'full_unread', 'full_unread_recv', 'full_unread_commit', 'full_unread_other',
        'failed_pop', 'failed_pop_unread', 'eof_reserved', 'reserved', 'pending_peak', 'pending_depth_time/pool',
        'compute', 'recv', 'send', 'other_live', 'outside_live', 'compute/pool', 'compute/4pool%',
        'startup_recv', 'interior_recv', 'drain_recv', 'terminal_recv', 'recv_empty', 'recv_ambiguous', 'recv_nonempty',
        'concurrency0', 'concurrency1', 'concurrency2', 'concurrency3', 'concurrency4',
        'interior_concurrency4/interior_wall%', 'interior_b', 'interior_a', 'interior_c',
        'interior_exposed_pending', 'interior_exposed', 'interior_exposed_worker', 'interior_exposed_pending_worker',
        'interior_supply_next', 'interior_supply_next_worker', 'interior_supply_commit', 'interior_supply_commit_worker',
        'drain_b', 'drain_recv_empty')

    def get(row, key):
        if '/' not in key:
            return float(row.get(key, 0)) / (1 if key.endswith('_peak') else 1e6)
        num, den = key.split('/')
        count = float(row.get(num, 0))
        pct = den.endswith('%')
        den = den.rstrip('%')
        denom = 4 * float(row['pool']) if den == '4pool' else float(row.get(den, 0))
        return (100 if pct else 1) * count / denom if denom else 0

    print('case', 'test', 'series', 'traces', *keys, 'representative', sep='\t')
    for label, rows in sorted(grouped.items()):
        med = {k: median(get(r, k) for r in rows) for k in keys}
        # Select near median exposure and pool time jointly (weights in ms).
        rep = min(rows, key=lambda r: sum(abs(get(r, k) - med[k]) for k in ('interior_exposed', 'pool')))
        print(*label, len(rows), *(f'{med[k]:.4f}' for k in keys), rep['trace'], sep='\t')
    print('trace exposure min/max and 0 count; not a confidence interval')
    for label, rows in sorted(grouped.items()):
        print(*label, *(f'{min(xs):.4f}..{max(xs):.4f} ({xs.count(0)} zeros)' for xs in (
            [get(r, key) for r in rows] for key in ('interior_exposed', 'interior_exposed_pending'))), sep='\t')
    if sources:
        fields = ('pool', 'next', 'interior_supply_next', 'commit')
        print('SOURCE paired diagnostic deltas (memory minus file, ms), 95% sign intervals')
        print('case', 'series', *fields, sep='\t')
        for label, pairs in sorted(sources.items()):
            changes = (ci([get(p['dump-memory'], k) - get(p['dump-file'], k) for p in pairs.values()]) for k in fields)
            print(*label, *(f'{m:+.3f}[{lo:+.3f},{hi:+.3f}]' for m, lo, hi in changes), sep='\t')

    grouped = defaultdict(lambda: defaultdict(list))
    per_index = defaultdict(lambda: defaultdict(list))
    for r in read(run / 'blocks.tsv'):
        key = r['case'], r.get('mode', r['test']), r['series']
        grouped[key][r['trace']].append(r)
        per_index[key][int(r['index'])].append(r)
    stats = ('compute_ns', 'job_handoff_ns', 'job_postsend_ns', 'result_handoff_ns', 'result_postsend_ns',
             'result_collection_ns', 'pending_ns')
    print('BLOCKS p50/p95 (µs): median across traces of each within-trace percentile; slowest index medians')
    print('case', 'test', 'series', *stats, 'slowest indexes (kind/packed payload bytes/median compute us)', sep='\t')
    for label, rows in sorted(grouped.items()):
        def perc(xs, p):
            return sorted(xs)[ceil(p * len(xs)) - 1]

        def per_trace(stat, p):
            return median(perc([int(r[stat]) / 1e3 for r in blocks], p) for blocks in rows.values())

        indexes = []
        for i, blocks in per_index[label].items():
            r = blocks[0]
            indexes.append((median(int(r['compute_ns']) / 1e3 for r in blocks), i, r['kind'], r['payload_bytes']))
        print(*label, *(f'{per_trace(stat, .5):.1f}/{per_trace(stat, .95):.1f}' for stat in stats),
              ', '.join(f'{i}({kind}/{p}/{us:.1f})' for us, i, kind, p in sorted(indexes, reverse=True)[:5]), sep='\t')


if __name__ == '__main__':
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('run', type=Path)
    a = p.parse_args()
    timing(a.run / 'timings.tsv')
    trace(a.run)
