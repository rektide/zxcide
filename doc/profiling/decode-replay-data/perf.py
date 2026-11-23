#!/usr/bin/env python3
"""Locked, paired perf stat/record of the checked replay and its Huffman mode."""
import argparse
from contextlib import ExitStack
from datetime import datetime, timezone
from decimal import Decimal
import fcntl
import filecmp
import hashlib
from itertools import permutations
import json
import os
from pathlib import Path
import platform
import random
import select
import signal
import subprocess as sp
from tempfile import TemporaryDirectory


LABELS = ('c', 'rust', 'rust-native')
EVENTS = ('cycles:u', 'instructions:u')
# Equal work for the three builds at each point, with ~0.1–0.3 s requests.
REPEATS = {'single': (2, 5, 22), 'mozilla': (8, 17, 85), 'sao': (52, 95, 540)}
PERIOD = 500_000
MANIFEST = Path(__file__).resolve().parent.parent / 'decode-pool-data/fixture-manifest.json'


def identity(path):
    with path.open('rb') as f:
        return {'bytes': path.stat().st_size, 'sha256': hashlib.file_digest(f, 'sha256').hexdigest()}


def word(b):
    assert len(b) == 4
    return int.from_bytes(b, 'little')


def capture(archive, out):
    """Write only an ordered index into the original archive: offset length symbol-count."""
    rows = {'lit': [], 'tok': []}
    with archive.open('rb') as f:
        prologue = f.read(16)
        assert prologue[6] == 0x80  # checked, nonseekable, no dictionary
        limit = 1 << prologue[5]
        blocks = 0
        while True:
            h = f.read(8)
            assert len(h) == 8
            if h[0] == 255:
                assert word(h[3:7]) == 0
                break
            body = f.tell()
            end = body + word(h[3:7])
            d = f.read(12)
            n, nl = word(d[:4]), word(d[4:8])
            lit, tok = d[8:10]
            assert h[0] == 1 and d[11] == 0 and 0 < n <= (limit - nl) // 5
            assert lit in (0, 1, 2) and tok in (0, 2)
            lsize = nl if lit == 0 else word(f.read(4))
            tsize = n if tok == 0 else word(f.read(4))
            start = f.tell()
            assert start + lsize + tsize + 2 * n <= end and start + lsize + 32 <= end
            for mode, kind, row in ((lit, 'lit', (start, lsize, nl)), (tok, 'tok', (start + lsize, tsize, n))):
                if mode == 2:
                    assert row[1] >= 128 and row[2] > 0
                    rows[kind].append(row)
            f.seek(end + 4)
            blocks += 1
    cases = {}
    for kind, seq in rows.items():
        idx = out / f'{archive.stem}-{kind}.idx'
        idx.write_text(''.join(f'{o} {l} {n}\n' for o, l, n in seq))
        cases[kind] = {'count': len(seq), 'units': sum(n for _, _, n in seq),
                       'coded_bytes': sum(l for _, l, _ in seq), 'index': {'path': str(idx), **identity(idx)}}
    return blocks, cases


def snapshot(cpu):
    root = Path(f'/sys/devices/system/cpu/cpu{cpu}')
    paths = {'load': Path('/proc/loadavg'), 'governor': root / 'cpufreq/scaling_governor',
             'siblings': root / 'topology/thread_siblings_list',
             'boost': root.parent / 'cpufreq/boost', 'paranoid': Path('/proc/sys/kernel/perf_event_paranoid')}
    return {'utc': datetime.now(timezone.utc).isoformat(),
            **{k: p.read_text().strip() if p.exists() else None for k, p in paths.items()}}


def request(worker, repeats):
    worker.stdin.write(f'{repeats}\n'.encode())
    worker.stdin.flush()
    assert select.select([worker.stdout], [], [], 60)[0], 'replay response timeout'
    row = [int(x) for x in worker.stdout.readline().split()]
    assert len(row) == 2 and min(row) > 0, row
    return dict(zip(('wall_ns', 'thread_cpu_ns'), row))


def measure(worker, repeats, output=None):
    # Scope the event to the idle resident thread. Ack after enable/disable;
    # clocks within the worker time just the request loop.
    attach = ['--no-inherit', '-p', str(worker.pid), '--delay=-1']
    mode = (['record', '--no-buildid-cache', '-e', 'cycles:u', '-c', str(PERIOD), '-o', str(output)]
            if output else ['stat', '-j', '--no-big-num', '-e', '{' + ','.join(EVENTS) + '}'])
    with ExitStack() as stack:
        def pipe():
            r, w = os.pipe()
            return tuple(stack.enter_context(os.fdopen(fd, m, buffering=0)) for fd, m in ((r, 'rb'), (w, 'wb')))

        ctl_r, ctl_w = pipe()
        ack_r, ack_w = pipe()
        cmd = ['perf', *mode, *attach, '--control', f'fd:{ctl_r.fileno()},{ack_w.fileno()}']
        with sp.Popen(cmd, pass_fds=(ctl_r.fileno(), ack_w.fileno()), stderr=sp.PIPE) as perf:
            ctl_r.close()
            ack_w.close()

            def control(op):
                ctl_w.write(op + b'\n')
                assert select.select([ack_r], [], [], 10)[0], (cmd, op, 'no perf ack')
                ack = ack_r.read(32)
                assert ack.rstrip(b'\0') == b'ack\n', (cmd, op, ack)

            try:
                control(b'enable')
                result = request(worker, repeats)
                control(b'disable')
            finally:
                if perf.poll() is None:
                    perf.send_signal(signal.SIGINT)
                _, stderr = perf.communicate(timeout=15)
                assert perf.returncode in (0, -signal.SIGINT), (cmd, stderr)
    result.update(perf_argv=cmd, perf_stderr=stderr.decode())
    if output:
        assert output.stat().st_size > 0
        return result
    events = {r['event']: r for ln in stderr.splitlines() if ln.startswith(b'{') for r in [json.loads(ln)]}
    assert set(events) == set(EVENTS), result
    for k, row in events.items():
        assert row['pcnt-running'] == 100.0 and row['event-runtime'] > 0, row
        # Do not silently accept an unsupported/scaled/nonintegral count.
        val = Decimal(row['counter-value'])
        assert val > 0 and val == int(val), row
        result[k] = int(val)
    return result


def verify(cmds, out, case, expected):
    rows = {}
    with TemporaryDirectory(prefix='verify-', dir=out) as tmp:
        ref = Path(tmp) / LABELS[0]
        for cmd, label in zip(cmds, LABELS):
            path = Path(tmp) / label
            argv = [x if x != 'serve' else 'verify' for x in cmd]
            with path.open('wb') as f:
                sp.run(argv, stdout=f, check=True)
            output = identity(path)
            assert output['bytes'] == case['units'] and (expected is None or expected == output), (argv, output)
            assert label == LABELS[0] or filecmp.cmp(ref, path, shallow=False), argv
            rows[label] = {'argv': argv, **output}
    return rows


def run_group(a, case, cmds, order, profile_order, prefix):
    with ExitStack() as stack:
        workers = []
        for argv in cmds:
            worker = stack.enter_context(sp.Popen(argv, stdin=sp.PIPE, stdout=sp.PIPE))
            # Close pipes before the Popen context waits, including on errors.
            stack.callback(worker.stdin.close)
            workers.append(worker)
        ready = []
        for worker in workers:
            assert select.select([worker.stdout], [], [], 60)[0], 'replay startup timeout'
            hello = worker.stdout.readline().decode().split()
            assert len(hello) == 5, hello
            assert (hello[0], int(hello[1]), int(hello[2]), hello[4]) == (
                'ready', case['count'], case['units'], case['reject'])
            assert os.sched_getaffinity(worker.pid) == {a.cpu}
            assert len(list(Path(f'/proc/{worker.pid}/task').iterdir())) == 1
            ready.append(hello)
        assert ready[0] == ready[1] == ready[2]
        case['ready'] = ready[0]
        case['warmup'] = {label: request(w, case['repeats']) for label, w in zip(LABELS, workers)}
        case['rounds'] = []
        for perm in order:
            case['rounds'].append({LABELS[i]: measure(workers[i], case['repeats']) for i in perm})
        if profile_order:
            case['profiles'] = {}
            for i in profile_order:
                path = a.out / f'{prefix}-{LABELS[i]}.perf.data'
                reps = case['repeats'] * 7
                row = measure(workers[i], reps, path)
                row.update(repeats=reps, path=str(path))
                case['profiles'][LABELS[i]] = row
        for worker in workers:
            worker.stdin.close()
            assert worker.wait(timeout=10) == 0


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for key in ('fixtures', 'build', 'out'):
        parser.add_argument(key, type=lambda p: Path(p).resolve())
    parser.add_argument('--cpu', type=int, default=3)
    parser.add_argument('--cycles', type=int, default=2, help='six balanced rounds per cycle')
    parser.add_argument('--only', choices=REPEATS, help='one fixture (smoke test)')
    parser.add_argument('--no-record', action='store_true')
    a = parser.parse_args()
    assert a.cpu in os.sched_getaffinity(0) and a.cycles > 0
    a.out.mkdir()
    bins = {label: a.build / f'replay-{label}' for label in LABELS}
    manifest = json.loads(MANIFEST.read_text())
    archives = {m['file'].removesuffix('.zxc'): m for m in manifest['members']}
    archives['single'] = {**manifest['single'], **{f'plain_{k}': v for k, v in manifest['plain'].items()}}
    rng = random.Random(826233)
    fixtures = [a.only] if a.only else list(REPEATS)
    lock = Path('.test-agent/zxc-perf.lock').resolve()
    meta = {'cpu': a.cpu, 'lock': str(lock), 'host': platform.uname()._asdict(),
            'cpu_model': next(ln.split(':', 1)[1].strip() for ln in Path('/proc/cpuinfo').read_text().splitlines()
                              if ln.startswith('model name')),
            'perf': sp.check_output(['perf', 'version'], text=True).strip(), 'period': PERIOD,
            'binaries': {label: {'path': str(path), **identity(path)} for label, path in bins.items()},
            'fixtures': {}, 'cases': {}}
    with lock.open('a') as lck:
        fcntl.flock(lck, fcntl.LOCK_EX)
        meta['initial'] = snapshot(a.cpu)
        cmds = {}
        for fixture in fixtures:
            archive = a.fixtures / f'{fixture}.zxc'
            arc, expected = identity(archive), archives[fixture]
            assert all(expected[k] == v for k, v in arc.items())
            plain = {k: expected[f'plain_{k}'] for k in arc}
            blocks, cases = capture(archive, a.out)
            cases['block'] = {'count': blocks, 'units': plain['bytes'], 'coded_bytes': arc['bytes']}
            meta['fixtures'][fixture] = {'path': str(archive), 'archive': arc, 'plain': plain}
            for kind, repeats in zip(('block', 'lit', 'tok'), REPEATS[fixture]):
                case = cases[kind]
                cmd = [archive, case['index']['path']] if 'index' in case else [archive]
                argv = [['taskset', '-c', str(a.cpu), str(b), 'serve', *map(str, cmd)] for b in bins.values()]
                name = f'{fixture}-{kind}'
                case.update(repeats=repeats, reject='bad-checksum-rejected' if kind == 'block' else 'bad-table-rejected')
                case['verify'] = verify(argv, a.out, case, plain if kind == 'block' else None)
                meta['cases'][name], cmds[name] = case, argv
        names = list(cmds)
        rng.shuffle(names)
        for name in names:
            order = []
            for _ in range(a.cycles):
                choices = list(permutations(range(len(LABELS))))
                rng.shuffle(choices)
                order.extend(choices)
            record = not a.no_record and (name == 'single-block' or not name.endswith('-block'))
            profile_order = rng.sample(range(len(LABELS)), len(LABELS)) if record else []
            case = meta['cases'][name]
            case['before'] = snapshot(a.cpu)
            run_group(a, case, cmds[name], order, profile_order, name)
            case['after'] = snapshot(a.cpu)
            (a.out / f'{name}.json').write_text(json.dumps(case, indent=2) + '\n')
            print(name, 'complete', flush=True)
        meta['final'] = snapshot(a.cpu)
    meta['group_order'] = names
    (a.out / 'perf.json').write_text(json.dumps(meta, indent=2) + '\n')


if __name__ == '__main__':
    main()
