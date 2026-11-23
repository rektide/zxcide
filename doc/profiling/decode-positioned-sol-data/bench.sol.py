#!/usr/bin/env python3
"""Fixed-archive paired harness adapted from ../decode-pool-data/bench.py (Linux)."""
import argparse
import fcntl
import hashlib
import json
from math import comb
import os
from pathlib import Path
import random
import re
import shutil
import statistics as st
import subprocess as sp
import tempfile
import time

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
M = json.loads((HERE.parent / 'decode-pool-data/fixture-manifest.json').read_text())
LAYOUTS = ('single', 'concat')
MODES = {'A': ('parent', None), 'S': ('trial', 'stream'), 'B': ('trial', 'positioned'),
         'DS': ('diag', 'stream'), 'DB': ('diag', 'positioned'), 'T': ('transports', 'positioned')}
PIPE_SIZE = 8192
POINTS = {'diag': [('null', 'DS', 'DB', 'S'), ('null', 'S', 'DS', 'S'), ('null', 'B', 'DB', 'B')],
          'clean': [('null', 'A', 'B', 'A'), ('null', 'S', 'B', 'A'), ('null', 'A', 'S', 'A'),
                    ('pipe', 'A', 'B', 'A'), ('file', 'A', 'B', 'A')]}
SEEDS = {'diag': (173911, 264353), 'clean': (56813, 88241)}


def contents(f):
    h, n = hashlib.sha256(), 0
    while b := f.read(1024 * 1024):
        h.update(b)
        n += len(b)
    return {'bytes': n, 'sha256': h.hexdigest()}


def identity(path):
    with path.open('rb') as f:
        return contents(f)


def invoke(a, mode, name, *, workers=8, slots=16, backend='crossbeam', action='decode', extra=()):
    variant, setting = MODES[mode]
    argv = [str(a.scratch / 'bin' / f'zxc-{variant}'), action, '--backend', backend,
            '--threads', str(workers), '--slots', str(slots), *map(str, extra), str(name)]
    env = os.environ.copy()
    env.pop('ZXC_TRIAL_INPUT', None)
    if setting is not None:
        env['ZXC_TRIAL_INPUT'] = setting
    return argv, env


def diagnostic(stderr):
    tag, *fields = stderr.split()
    assert tag == 'ZXC_POSITIONED_PROBE', stderr
    return {k: tuple(map(int, v.split(','))) if ',' in v else int(v)
            for k, v in (part.split('=') for part in fields)}


def open_pipe():
    r, w = os.pipe()
    try:
        assert fcntl.fcntl(w, fcntl.F_SETPIPE_SZ, PIPE_SIZE) == PIPE_SIZE
    except BaseException:
        os.close(r)
        os.close(w)
        raise
    return os.fdopen(r, 'rb'), os.fdopen(w, 'wb')


def exact(a, mode, kind, archive, *, backend='crossbeam', workers=8, trace=None):
    path = a.out / 'verify-output'
    extra = ('-o', path) if kind == 'file' else ()
    argv, env = invoke(a, mode, '-' if kind in ('pipe', 'redirected') else archive,
                       workers=workers, backend=backend, extra=extra)
    if trace:
        argv = ['strace', '-f', '-yy', '-s', '3', '-o', str(trace), '-e', 'trace=openat,read,pread64', *argv]
    feeder = None
    with tempfile.TemporaryFile(dir=a.out) as err:
        if kind == 'pipe':
            r, w = open_pipe()
            with w:
                feeder = sp.Popen([a.cat, str(archive)], stdout=w, stderr=sp.PIPE)
        elif kind == 'redirected':
            r = archive.open('rb')
        else:
            r = open(os.devnull, 'rb')
        with r, sp.Popen(argv, env=env, stdin=r, stdout=sp.PIPE, stderr=err) as p:
            measured = contents(p.stdout)
            assert p.wait(timeout=45) == 0, (argv, err.seek(0), err.read())
        if feeder:
            assert feeder.communicate(timeout=10)[1] == b'' and feeder.returncode == 0
        err.seek(0)
        stderr = err.read().decode()
        if mode.startswith('D'):
            d = diagnostic(stderr)
            assert d['jobs'] == d['worker_jobs'] > 0 and d['data'][0] == d['jobs']
        else:
            assert not stderr, (argv, stderr)
    if kind == 'file':
        assert measured['bytes'] == 0
        assert identity(path) == M['plain'], argv
        sp.run(['cmp', str(a.fixtures / 'plain'), str(path)], check=True)
        path.unlink()
    else:
        assert measured == M['plain'], (argv, measured)
    return {'mode': mode, 'sink': kind, 'archive': str(archive), 'backend': backend, 'workers': workers,
            'sha256': M['plain']['sha256'], 'bytes': M['plain']['bytes'], 'status': 0, 'stderr': stderr}


def verify(a):
    verified = []
    traces = []
    fixture = a.fixtures / 'single.zxc'
    for layout in LAYOUTS:
        archive = a.fixtures / f'{layout}.zxc'
        assert identity(archive) == {k: M[layout][k] for k in ('bytes', 'sha256')}
        for mode in ('A', 'S', 'B', 'DS', 'DB'):
            for kind in (('null',) if mode.startswith('D') else ('null', 'pipe', 'file')):
                trace = a.out / f'{mode}-{layout}-{kind}.strace' if mode in ('A', 'S', 'B') and kind == 'null' else None
                verified.append(exact(a, mode, kind, archive, trace=trace))
                if trace:
                    traces.append((trace, str(archive), mode == 'B', 1))
    assert identity(a.fixtures / 'plain') == M['plain']
    for mode, kind, backend, workers in [('B', 'null', 'crossbeam', 1), ('B', 'pipe', 'crossbeam', 8),
                                           ('B', 'redirected', 'crossbeam', 8),
                                           *[('T', 'null', name, 8) for name in ('flume', 'kanal', 'crossfire', 'rayon')]]:
        trace = a.out / f'fallback-{kind}-{backend}-{workers}.strace'
        verified.append(exact(a, mode, kind, fixture, backend=backend, workers=workers, trace=trace))
        pattern = str(fixture) if kind in ('null', 'redirected') else 'pipe:['
        traces.append((trace, pattern, False, int(kind == 'null')))
    cases = ROOT / 'tests/conformance/v9'
    dictfiles = ['dict_http.zxd', 'dict_text.zxd', 'dict_id_zero.zxd']
    extra = tuple(v for d in dictfiles for v in ('-D', cases / 'valid' / d))
    for path in sorted(cases.glob('*/*.zxc')):
        runs = []
        for mode in ('A', 'S', 'B'):
            argv, env = invoke(a, mode, path, extra=extra)
            p = sp.run(argv, env=env, capture_output=True, timeout=10)
            runs.append((p.returncode, p.stdout, p.stderr))
        status, output, err = runs[0]
        assert all((code, data) == (status, output) and (not stderr) == (not err)
                   for code, data, stderr in runs), path
        if path.parent.name == 'valid':
            assert status == 0 and not err and output == path.with_suffix('.expected').read_bytes(), path
        verified.append({'conformance': str(path), 'status': status, 'bytes': len(output),
                         'sha256': hashlib.sha256(output).hexdigest(),
                         'stderr': [err.decode() for _, _, err in runs]})
    valid = cases / 'valid' / 'multiblock_checksum.zxc'
    corrupt = cases / 'invalid' / 'bad_block_checksum.zxc'
    late = a.out / 'late.zxc'
    late.write_bytes(valid.read_bytes() + corrupt.read_bytes() + valid.read_bytes())
    output = a.out / 'no-publish'
    for mode in ('A', 'S', 'B'):
        for archive in (valid, late):
            argv, env = invoke(a, mode, archive, extra=('-o', output))
            if archive == late:
                p = sp.run(argv, env=env, capture_output=True, timeout=10)
                assert p.returncode != 0 and b'block checksum mismatch' in p.stderr and not output.exists()
            output.write_bytes(b'sentinel')
            p = sp.run(argv, env=env, capture_output=True, timeout=10)
            assert p.returncode != 0 and not p.stdout and p.stderr and output.read_bytes() == b'sentinel'
            output.unlink()
        for archive, success in ((valid, True), (late, False)):
            argv, env = invoke(a, mode, archive, action='test')
            p = sp.run(argv, env=env, capture_output=True, timeout=10)
            assert (p.returncode == 0) == success and not p.stdout and (not p.stderr) == success
        verified.append({'mode': mode, 'fresh_output_checksum_reject': True, 'existing_sentinel_untouched': True,
                         'test_valid_and_bad_checksum': True})
    fifo = a.out / 'in.fifo'
    os.mkfifo(fifo)
    trace = a.out / 'fallback-fifo.strace'
    argv, env = invoke(a, 'B', fifo)
    p = sp.run(['timeout', '--kill-after=2s', '15s', 'bash', '-c',
                '"$1" "$2" > "$3" & feeder=$!; shift 3; "$@"; a=$?; wait "$feeder"; b=$?; exit $((a || b))',
                'fifo', a.cat, str(valid), str(fifo), 'strace', '-f', '-yy', '-s', '3', '-o', str(trace),
                '-e', 'trace=openat,read,pread64', *argv], env=env, capture_output=True, timeout=20)
    assert p.returncode == 0 and not p.stderr and p.stdout == valid.with_suffix('.expected').read_bytes(), p.stderr
    traces.append((trace, str(fifo), False, 1))
    fifo.unlink()
    for name in ('/dev/null', '/proc/self/cmdline'):
        trace = a.out / f'fallback-{Path(name).name}.strace'
        argv, env = invoke(a, 'B', name)
        p = sp.run(['strace', '-f', '-yy', '-s', '3', '-o', str(trace), '-e', 'trace=openat,read,pread64', *argv],
                   env=env, capture_output=True, timeout=10)
        assert p.returncode != 0 and not p.stdout and p.stderr
        traces.append((trace, str(Path(name).name), False, 1))
    selections = []
    for trace, target, positioned, opens in traces:
        lines = [line for line in trace.read_text().splitlines() if target in line]
        counts = {call: sum(f' {call}(' in line for line in lines) for call in ('openat', 'read', 'pread64')}
        tids = sorted({line.split()[0] for line in lines if ' pread64(' in line})
        assert (counts['pread64'] > 0) == positioned and counts['openat'] == opens, (trace, counts)
        assert (counts['read'] > 0) != positioned, (trace, counts)
        assert not positioned or len(tids) == 9, (trace, tids)
        selections.append({'trace': trace.name, 'pattern': target, 'positioned': positioned, **counts, 'pread_tids': tids})
    (a.out / 'verification.sol.json').write_text(json.dumps(dict(checks=verified, selection=selections), indent=2) + '\n')
    print(f'{len(verified)} conformance/preflight records; {len(selections)} selection traces passed', flush=True)


def wait4(p):
    _, status, r = os.wait4(p.pid, 0)
    p.returncode = os.waitstatus_to_exitcode(status)
    return {'status': p.returncode, 'user_us': round(r.ru_utime * 1e6), 'sys_us': round(r.ru_stime * 1e6)}


def run_once(a, layout, kind, mode, err, f_err, sink):
    archive = a.fixtures / f'{layout}.zxc'
    real = a.out / 'timed-output'
    argv, env = invoke(a, mode, '-' if kind == 'pipe' else archive, extra=('-o', real) if kind == 'file' else ())
    err.seek(0)
    err.truncate()
    f_err.seek(0)
    f_err.truncate()
    feeder = None
    if kind == 'pipe':
        r, w = open_pipe()
        with w:
            # The fixed cat child is launched before the decoder start edge.
            feeder = sp.Popen([a.cat, str(archive)], stdout=w, stderr=f_err)
    else:
        r = open(os.devnull, 'rb')
    with r:
        start = time.perf_counter_ns()
        p = sp.Popen(argv, env=env, stdin=r, stdout=sink, stderr=err)
        ret = wait4(p)
        ret['wall_ns'] = time.perf_counter_ns() - start
    err.seek(0)
    ret['stderr'] = err.read().decode()
    assert ret['status'] == 0, (argv, ret)
    ret['cpu_s_GB'] = (ret['user_us'] + ret['sys_us']) * 1000 / M['plain']['bytes']
    if mode.startswith('D'):
        ret['probe'] = diagnostic(ret['stderr'])
    else:
        assert not ret['stderr'], (argv, ret)
    if feeder:
        ret['feeder'] = wait4(feeder)
        f_err.seek(0)
        ret['feeder']['stderr'] = f_err.read().decode()
        assert ret['feeder']['status'] == 0 and not ret['feeder']['stderr'], ret
    if kind == 'file':
        assert identity(real) == M['plain']
        real.unlink()
    return ret


def interval(xs):
    xs = sorted(xs)
    n = len(xs)
    tail = 0
    for k in range(n // 2):
        tail += comb(n, k)
        if 40 * tail > 2**n:
            rank = max(0, k - 1)
            return [xs[rank], xs[-1-rank]]
    return [xs[0], xs[-1]]


def summary(series, points):
    results = []
    for index, point in enumerate(points):
        pairs = [p['results'] for p in series['pairs'] if p['point'] == index and p['round'] >= 0]
        row = {'layout': point[0], 'sink': point[1], 'contrast': point[2], 'labels': point[3:]}
        for name, metric in [('wall', lambda r: r['wall_ns']), ('cpu', lambda r: r['cpu_s_GB'])]:
            changes = [100 * (metric(b) / metric(a) - 1) for a, b in pairs]
            row[name] = dict(a=st.median(metric(a) for a, _ in pairs), b=st.median(metric(b) for _, b in pairs),
                             delta=st.median(changes), ci=interval(changes))
        if point[3:] == ('DS', 'DB'):
            for a, b in pairs:
                x, y = a['probe'], b['probe']
                assert x['jobs'] == x['worker_jobs'] == y['jobs'] == y['worker_jobs'] == M['single_blocks'] if point[0] == 'single' else x['jobs'] == x['worker_jobs'] == y['jobs'] == y['worker_jobs']
                assert x['caller_body_bytes'] == y['worker_body_bytes'] > 0 and x['worker_body_bytes'] == y['caller_body_bytes'] == 0
                assert x['worker_header_bytes'] == 0 and y['worker_header_bytes'] == x['caller_data_header_bytes'] == y['caller_data_header_bytes']
            for name, metric in [('next_cpu', lambda r: r['probe']['data'][1]), ('next_wall', lambda r: r['probe']['data'][2])]:
                changes = [100 * (metric(b) / metric(a) - 1) for a, b in pairs]
                row[name] = dict(a=st.median(metric(a) for a, _ in pairs), b=st.median(metric(b) for _, b in pairs),
                                 delta=st.median(changes), ci=interval(changes))
            row['counts'] = [{k: v for k, v in r['probe'].items() if isinstance(v, int)} for r in pairs[0]]
            row['phases'] = [{p: [st.median(pair[i]['probe'][p][j] for pair in pairs) for j in range(3)]
                              for p in ('data', 'marker', 'finish', 'prologue', 'physical_eof', 'stable')} for i in (0, 1)]
        results.append(row)
    return results


def system():
    affinity = sorted(os.sched_getaffinity(0))
    def text(p):
        p = Path('/sys/devices/system/cpu') / p
        return p.read_text().strip() if p.exists() else None
    return {'affinity': affinity, 'siblings_reserved': False, 'host': list(os.uname()),
            'topology': {cpu: text(f'cpu{cpu}/topology/thread_siblings_list') for cpu in affinity},
            'governor': {cpu: text(f'cpu{cpu}/cpufreq/scaling_governor') for cpu in affinity},
            'boost': text('cpufreq/boost'), 'pstate_status': text('amd_pstate/status'),
            'pipe_size': PIPE_SIZE, 'feeder_affinity': affinity}


def run(a, phase):
    env = system()
    assert len(env['affinity']) == len(set(env['topology'].values())) == 8, env
    points = [(layout, sink, f'{x}/{y}', u, v) for layout in LAYOUTS for sink, x, y, aa in POINTS[phase]
              for u, v in [(x, y), (aa, aa)]]
    raw = {'phase': phase, 'system': env, 'points': points, 'cwd': str(Path.cwd()),
           'argv': os.sys.argv, 'cat': a.cat, 'fixtures': {k: M[k] for k in (*LAYOUTS, 'plain')},
           'builds': {path.name: identity(path) for path in sorted((a.scratch / 'bin').glob('zxc-*'))},
           'patches': {path.name: identity(path) for path in HERE.glob('*.patch')}, 'series': []}
    dst = a.out / f'{phase}.sol.json'
    for seed in SEEDS[phase]:
        rng = random.Random(seed)
        orders = []
        for _ in points:
            order = [0, 1] * 12
            rng.shuffle(order)
            orders.append(order)
        series = {'seed': seed, 'load_start': os.getloadavg(), 'pairs': []}
        with tempfile.TemporaryFile(dir=a.out) as err, tempfile.TemporaryFile(dir=a.out) as f_err, open(os.devnull, 'wb') as sink:
            for round_idx in range(-2, 24):
                shuffled = list(range(len(points)))
                rng.shuffle(shuffled)
                for i in shuffled:
                    layout, kind, _, *modes = points[i]
                    first = orders[i][round_idx] if round_idx >= 0 else rng.randrange(2)
                    ret = [None, None]
                    for j in (first, 1-first):
                        ret[j] = run_once(a, layout, kind, modes[j], err, f_err, sink)
                    series['pairs'].append({'round': round_idx, 'point': i, 'first': first, 'results': ret})
        series['load_end'] = os.getloadavg()
        series['summary'] = summary(series, points)
        raw['series'].append(series)
        dst.write_text(json.dumps(raw, separators=(',', ':')) + '\n')
        print(f'{phase} series {seed} done; load {series["load_start"]} -> {series["load_end"]}', flush=True)


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('scratch', type=lambda p: Path(p).resolve())
    p.add_argument('fixtures', type=lambda p: Path(p).resolve())
    p.add_argument('out', type=lambda p: Path(p).resolve())
    p.add_argument('--verify-only', action='store_true')
    a = p.parse_args()
    a.cat = shutil.which('cat')
    a.out.mkdir(parents=True)
    with (ROOT / '.test-agent/zxc-perf.lock').open('a') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        verify(a)
        if not a.verify_only:
            run(a, 'diag')
            run(a, 'clean')


if __name__ == '__main__':
    main()
