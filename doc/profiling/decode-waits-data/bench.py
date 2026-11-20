#!/usr/bin/env python3
"""Fixed-input benchmark. Entire verification/warmup/timing batch owns the flock."""
import argparse
import csv
import fcntl
import hashlib
import os
from pathlib import Path
import random
import subprocess
import tempfile
import time

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
LOCK = '/home/rektide/src/zxc-rs/.test-agent/zxc-perf.lock'
CASES = ('mozilla.c7', 'mozilla.dense', 'sao.c7', 'sao.dense')
BASE = {'aa': ('clean4', 'clean4'), 'scale': ('clean1', 'clean4'),
        'capture': ('clean4', 'capture'), 'dump': ('clean4', 'dump8')}
SLOTS = {'aa': BASE['aa'], 'slots4': ('clean4', 'clean-slots4'),
         'slots16': ('clean4', 'clean-slots16'), 'dump4': ('clean-slots4', 'dump4'),
         'dump8': BASE['dump'], 'dump16': ('clean-slots16', 'dump16')}


def sha(stream):
    h, count = hashlib.sha256(), 0
    while chunk := stream.read(1 << 20):
        count += len(chunk)
        h.update(chunk)
    return h.hexdigest(), count


def file_sha(path):
    with path.open('rb') as stream:
        return sha(stream)


def invocation(a, case, mode, path):
    binary = a.build / ('clean' if mode.startswith('clean') else 'probe')
    threads = 1 if mode == 'clean1' else 4
    slots = 16 if mode.endswith('16') else 4 if mode in ('clean-slots4', 'dump4') else 8
    argv = [str(binary), 'decode', '--threads', str(threads), '--slots', str(slots),
            '--backend', 'crossbeam', str(a.fixtures / f'{case}.zxc')]
    env = os.environ.copy()
    env.pop('ZXC_WAIT_TRACE', None)
    if mode.startswith('dump'):
        env['ZXC_WAIT_TRACE'] = str(path)
    if mode == 'capture':
        env['ZXC_WAIT_TRACE'] = 'capture'
    return binary, argv, env, {2} if threads == 1 else {2, 3, 4, 5}


def verify(a, cells, out):
    manifest = {p: h for h, p in map(str.split, (HERE.parent / 'pivco-worker-data/sha256.txt').read_text().splitlines())}
    expect = {}
    for name in sorted({c + '.zxc' for c in CASES} | {c.split('.')[0] for c in CASES}):
        digest, size = file_sha(a.fixtures / name)
        if digest != manifest[f'fixtures/{name}']:
            raise ValueError(f'{name}: wrong fixture hash: {digest}')
        expect[name] = digest, size
        print('fixture', name, size, digest, sep='\t', file=out)
    for mode in ('clean', 'probe'):
        binary = a.build / mode
        print('binary', mode, *reversed(file_sha(binary)), sep='\t', file=out)
        if (b'ZXCWAIT1' in binary.read_bytes()) != (mode == 'probe'):
            raise ValueError(f'{binary}: wrong probe signature')
    for case in CASES:
        for mode in sorted(set(x for pair in cells.values() for x in pair)):
            target = a.out / f'verify-{case}-{mode}.bin'
            target.unlink(missing_ok=True)
            _, argv, env, mask = invocation(a, case, mode, target)
            if mode.startswith('clean'):
                env['ZXC_WAIT_TRACE'] = str(target)  # clean must ignore it
            os.sched_setaffinity(0, mask)
            with tempfile.TemporaryFile(dir=a.out) as err:
                with subprocess.Popen(argv, env=env, stdout=subprocess.PIPE, stderr=err) as proc:
                    got = sha(proc.stdout)
                    status = proc.wait()
                err.seek(0)
                stderr = err.read()
            if status or stderr or got != expect[case.split('.')[0]] or target.exists() != mode.startswith('dump'):
                raise ValueError(f'{case}/{mode}: status {status}, stderr {stderr!r}, got {got}')
            print('decoded-to-eof', case, mode, got[1], got[0], 'exit=0', 'stderr=0', sep='\t', file=out)


def measure(a, case, mode, target, null, err):
    binary, argv, env, mask = invocation(a, case, mode, target)
    if mode.startswith('dump'):
        target.unlink(missing_ok=True)
    os.sched_setaffinity(0, mask)
    err.seek(0)
    err.truncate()
    t = time.perf_counter_ns()
    pid = os.posix_spawn(binary, argv, env, file_actions=[
        (os.POSIX_SPAWN_DUP2, null.fileno(), 1), (os.POSIX_SPAWN_DUP2, err.fileno(), 2)])
    _, status, usage = os.wait4(pid, 0)
    elapsed = time.perf_counter_ns() - t
    err.seek(0)
    stderr = err.read()
    if status or stderr:
        raise ValueError(f'{argv}: wait status {status}, stderr {stderr!r}')
    if mode.startswith('dump') and not target.is_file():
        raise ValueError(f'expected fresh sidecar {target}')
    return elapsed, round(usage.ru_utime * 1e6), round(usage.ru_stime * 1e6), usage.ru_maxrss


def environment(out, mask):
    print(time.strftime('%FT%TZ', time.gmtime()), file=out)
    print('lock:', LOCK, '; parent affinity before pinning:', sorted(mask), file=out)
    print('loadavg:', os.getloadavg(), file=out)
    for cmd in (['uname', '-r'], ['lscpu', '-e=CPU,CORE,SOCKET,NODE,ONLINE,MAXMHZ'], ['lscpu']):
        print(subprocess.check_output(cmd, text=True), file=out)
    for cpu in range(16):
        p = Path(f'/sys/devices/system/cpu/cpu{cpu}/cpufreq/scaling_governor')
        if p.exists():
            print(cpu, p.read_text().strip(), file=out)
    for name in ('/sys/devices/system/cpu/amd_pstate/status', '/sys/devices/system/cpu/cpufreq/boost',
                 '/proc/self/cgroup', '/proc/stat'):
        p = Path(name)
        if p.exists():
            print(name, '\n' + '\n'.join(p.read_text().splitlines()[:17]), file=out)


def run(a):
    cells = SLOTS if a.slots else BASE
    a.out.mkdir(parents=True)  # fresh directory: no stale sidecars
    traces = a.out / 'traces'
    traces.mkdir()
    mask = os.sched_getaffinity(0)
    with open(LOCK, 'a') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        with (a.out / 'environment.txt').open('w') as out:
            environment(out, mask)
        with (a.out / 'verification.tsv').open('w') as out:
            verify(a, cells, out)
        with (a.out / 'timings.tsv').open('w') as out, open(os.devnull, 'wb') as null, tempfile.TemporaryFile(dir=a.out) as err:
            writer = csv.writer(out, delimiter='\t', lineterminator='\n')
            writer.writerow(('series', 'seed', 'run_order', 'case', 'test', 'pair', 'member', 'mode',
                             'wall_ns', 'user_us', 'sys_us', 'maxrss_kib', 'trace'))
            for series, seed in enumerate(a.seeds, 1):
                rng = random.Random(seed)
                schedule = []
                warm = [(case, name, 0, rng.choice((0, 1))) for case in CASES for name in cells]
                rng.shuffle(warm)
                for case in CASES:
                    for name in cells:
                        orders = [0, 1] * (a.pairs // 2)
                        rng.shuffle(orders)
                        schedule += [(case, name, pair, order) for pair, order in enumerate(orders, 1)]
                rng.shuffle(schedule)
                for order, (case, name, pair, reverse) in enumerate(warm + schedule, 1):
                    for member in ((1, 0) if reverse else (0, 1)):
                        mode = cells[name][member]
                        target = traces / f'{series}-{case}-{name}-{pair:03}.bin' if pair else a.out / 'warm.bin'
                        nums = measure(a, case, mode, target, null, err)
                        writer.writerow((series, seed, order, case, name, pair, member, mode, *nums,
                                         target.name if mode.startswith('dump') else ''))
                    out.flush()
                print(f'series {series}: {len(schedule)} adjacent pairs', flush=True)
        os.sched_setaffinity(0, mask)
        with (a.out / 'environment.txt').open('a') as out:
            environment(out, mask)


if __name__ == '__main__':
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('build', type=lambda x: Path(x).resolve(), help='directory from build.sh')
    p.add_argument('out', type=lambda x: Path(x).resolve(), help='new output directory')
    p.add_argument('--fixtures', type=lambda x: Path(x).resolve(), default=ROOT / '.test-agent/pivco-worker/fixtures')
    p.add_argument('--pairs', type=int, default=32)
    p.add_argument('--seeds', nargs=2, type=int, default=[1729, 9437])
    p.add_argument('--slots', action='store_true', help='follow-up cells, including traces at 4/8/16 slots')
    args = p.parse_args()
    if args.pairs < 12 or args.pairs % 2 or len(set(args.seeds)) != 2:
        p.error('need an even count >=12 and independent seeds')
    run(args)
