#!/usr/bin/env python3
"""Opt-in two-length overhead series, for the null sink only; not a speed benchmark."""
import argparse
import hashlib
from math import comb
import random
import shutil
import signal
from statistics import median

from common import new_run, perf_lock, require, write_json
from pair import file_record, fixture, inputs, interrupted, preflight, run_one


def median_interval(values):
    ordered = sorted(values)
    n = len(ordered)
    # Conservative distribution-free two-sided order-statistic CI for the median.
    k = max(i for i in range(n // 2) if 40 * sum(comb(n, j) for j in range(i + 1)) <= 2**n)
    return [ordered[k], ordered[n - k - 1]]


def warm(path):
    with path.open('rb') as f:
        while f.read(1024 * 1024):
            pass


def main():
    p = argparse.ArgumentParser(description=__doc__)
    inputs(p)
    p.add_argument('--copies', type=int, default=4)
    p.add_argument('--pairs', type=int, default=12)
    p.add_argument('--seeds', type=int, nargs=2, required=True)
    a = p.parse_args()
    require(a.case == 'single' and a.copies > 0 and a.pairs >= 12 and a.pairs % 4 == 0 and
            len(set(a.seeds)) == 2, 'use single, positive N, >=12 pairs in multiples of 4, two distinct seeds')
    require(a.decoder_cpus and a.collector_cpus, 'choose separate decoder and collector CPU sets')
    out = new_run(a.out)
    info = {'status': 'failed'}

    signal.signal(signal.SIGTERM, interrupted)
    try:
        info.update(preflight(a, out))
        with perf_lock():
            path, compressed, plain = fixture(a)
            info.update(archive=compressed, stdout_per_frame=plain, seeds=a.seeds, pairs=a.pairs, n=a.copies,
                        sink='/dev/null after exact regular-file preflights', contrasts=[])
            exact = run_one(a, out / 'one-frame', 'c', path, a.c_off, 'off', plain)
            require(exact['status'] == 'passed', 'cannot bind repeated outputs')
            one_plain = out / 'one-frame/stdout.bin'
            lengths = [a.copies, 2 * a.copies]
            archives, expected = {}, {}
            for n in lengths:
                archives[n] = out / f'single-x{n}.zxc'
                digest = hashlib.sha256()
                with archives[n].open('xb') as dst:
                    for _ in range(n):
                        with path.open('rb') as src:
                            shutil.copyfileobj(src, dst)
                        with one_plain.open('rb') as src:
                            for chunk in iter(lambda: src.read(1024**2), b''):
                                digest.update(chunk)
                expected[n] = {'bytes': n * plain['bytes'], 'sha256': digest.hexdigest()}
            info['lengths'] = {n: {'archive': file_record(archives[n]), 'plain': expected[n]} for n in lengths}
            configurations = [('c', a.c_off, a.c), ('rust', a.rust_off, a.rust)]
            for impl, off, on in configurations:
                binaries = {'off': off, 'detached': on, 'attached': on}
                for n in lengths:
                    for mode, binary in binaries.items():
                        d = out / f'exact-{impl}-{n}-{mode}'
                        run_one(a, d, impl, archives[n], binary, mode, expected[n], copies=n)
                        (d / 'stdout.bin').unlink()  # digest already checked; do not retain giant plaintext copies
                for series, seed in enumerate(a.seeds):
                    rng = random.Random(seed)
                    contrasts = [('off', 0.03), ('detached', 0.03), ('attached', 0.10)]
                    rng.shuffle(contrasts)
                    for mode, budget in contrasts:
                        orders = [(flip_mode, flip_length) for flip_mode in (False, True)
                                  for flip_length in (False, True)] * (a.pairs // 4)
                        rng.shuffle(orders)
                        changes = {'wall': [], 'cpu': []}
                        reason = []
                        for pair, (flip_mode, flip_length) in enumerate(orders):
                            samples = {}
                            from_flavors = [('base', 'off'), ('test', mode)]
                            for name, flavor in from_flavors[::(-1 if flip_mode else 1)]:
                                for n in lengths[::(-1 if flip_length else 1)]:
                                    warm(archives[n])
                                    d = out / f'{impl}-{series}-{mode}-{pair:02}-{name}-{n}'
                                    samples[name, n] = run_one(a, d, impl, archives[n], binaries[flavor],
                                                               flavor, expected[n], copies=n, null=True)['decoder']
                            for metric, f in [('wall', lambda r: r['wall_ns'] / 1e9),
                                              ('cpu', lambda r: r['user_s'] + r['sys_s'])]:
                                base, test = [f(samples[name, lengths[1]]) - f(samples[name, lengths[0]])
                                              for name in ('base', 'test')]
                                if min(base, test) <= 0:
                                    reason.append(f'non-positive {metric} slope in pair {pair}')
                                else:
                                    changes[metric].append(test / base - 1)
                        stats = {m: {'median': median(v), 'interval': median_interval(v)}
                                 for m, v in changes.items()} if not reason else {}
                        passed = bool(stats) and all(-budget <= x['interval'][0] <= x['interval'][1] <= budget
                                                     for x in stats.values())
                        info['contrasts'].append({'impl': impl, 'series': series, 'mode': mode, 'budget': budget,
                                                  'stats': stats, 'numeric_gate': passed, 'failures': reason})
        info['status'] = 'passed'
        info['qualification'] = 'qualitative pending attach/shutdown jitter and coverage review; inspect numeric_gate'
    except BaseException as exc:
        info['error'] = str(exc)
        raise
    finally:
        write_json(out / 'overhead.json', info)


if __name__ == '__main__':
    main()
