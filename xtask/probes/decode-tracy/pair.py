#!/usr/bin/env python3
"""Capture and verify sequential C/Rust decodes of one fixed archive."""
import argparse
from contextlib import ExitStack
import json
import os
from pathlib import Path
import platform
import re
import signal
import stat
import subprocess as sp
import time

from common import HERE, PINS, new_run, perf_lock, protocol, require, sha, write_json
from events import MAP, verify


def file_record(path):
    st = path.stat()
    require(stat.S_ISREG(st.st_mode), f'not a regular file: {path}')
    return {'bytes': st.st_size, 'sha256': sha(path)}


def fixture(a):
    require(sha(a.manifest) == PINS['fixture_manifest_sha256'], 'pool fixture manifest does not match pin')
    m = json.loads(a.manifest.read_text())
    if a.case == 'single':
        compressed, plain = m['single'], m['plain']
    else:
        member = next(x for x in m['members'] if x['file'] == f'{a.case}.zxc')
        compressed = {key: member[key] for key in ('bytes', 'sha256')}
        plain = {key: member[f'plain_{key}'] for key in ('bytes', 'sha256')}
    if a.case == 'single':
        require(m['single_blocks'] == MAP['cases']['single'][0], 'single physical census mismatch')
    path = (a.fixtures / f'{a.case}.zxc').resolve()
    require(file_record(path) == compressed, f'archive differs from pool manifest: {path}')
    return path, compressed, plain


def stable_binary(path, recorded):
    digest = sha(path)
    require(digest == recorded['sha256'], f'binary hash mismatch: {path}')
    return {'path': str(path), 'sha256': digest}


def tools(a, out):
    pin = PINS['tracy']
    result = {}
    for name, path, flags in [('tracy-capture', a.capture, []), ('tracy-csvexport', a.csvexport, ['-h'])]:
        r = sp.run([path, *flags], stdin=sp.DEVNULL, stdout=sp.PIPE, stderr=sp.STDOUT, timeout=10)
        (out / f'{name}-banner.log').write_bytes(r.stdout)
        first = r.stdout.decode(errors='replace').splitlines()[0:1]
        expected = f"{name} {pin['version']} / {pin['revision'][:7]}"
        require(first == [expected], f'incompatible {name} banner: {first}; expected {expected}')
        digest = sha(path)
        require(platform.machine() == 'x86_64' and digest == pin['tools_linux_x86_64'][name],
                f'{name} hash does not match the pinned Linux x86_64 release binary')
        result[name] = {'path': str(path), 'banner': expected, 'sha256': digest}
    return result


def proc_sockets(kind):
    for suffix in ('', '6'):
        path = Path('/proc/net') / (kind + suffix)
        if path.exists():
            for line in path.read_text().splitlines()[1:]:
                _, address, _, status, *_, inode = line.split()[:10]
                host, port = address.split(':')
                yield host, int(port, 16), status, inode


def listener(port):
    return [(host, inode) for host, p, state, inode in proc_sockets('tcp') if p == port and state == '0A']


def cpu_list(raw):
    result = set()
    for item in raw.split(','):
        lo, _, hi = item.partition('-')
        result.update(range(int(lo), 1 + int(hi or lo)))
    require(result and result <= os.sched_getaffinity(0), f'bad affinity: {raw}')
    return sorted(result)


class Child:
    def __init__(self, argv, stdout, stderr, env, cpu):
        self.start = time.monotonic_ns()
        self.argv = list(map(str, argv))
        # taskset execs the intended process, so the PID and wait4 rusage are for that child.
        if cpu:
            self.argv = ['taskset', '-c', ','.join(map(str, cpu)), *self.argv]
        self.proc = sp.Popen(self.argv, stdin=sp.DEVNULL, stdout=stdout, stderr=stderr,
                             env=env, start_new_session=True)
        self.record = {'argv': self.argv, 'pid': self.proc.pid, 'affinity': cpu or sorted(os.sched_getaffinity(0))}

    def reap(self, block=False):
        if self.proc.returncode is None:
            pid, status, usage = os.wait4(self.proc.pid, 0 if block else os.WNOHANG)
            if pid:
                self.proc.returncode = os.waitstatus_to_exitcode(status)
                self.record.update(status=self.proc.returncode, wall_ns=time.monotonic_ns()-self.start,
                                   user_s=usage.ru_utime, sys_s=usage.ru_stime, maxrss_kib=usage.ru_maxrss)
        return self.proc.returncode

    def close(self):
        if self.reap() is None:
            try:
                os.killpg(self.proc.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            self.reap(block=True)


def await_listener(dec, port, deadline):
    ready_deadline = min(deadline, time.monotonic() + 25)
    while time.monotonic() < ready_deadline:
        require(dec.reap() is None, 'decoder exited before listen')
        sockets = listener(port)
        if sockets:
            require(len(sockets) == 1 and sockets[0][0] == '0100007F', f'not IPv4 loopback: {sockets}')
            fds = set()
            for fd in Path(f'/proc/{dec.proc.pid}/fd').iterdir():
                try:
                    fds.add(os.readlink(fd))
                except FileNotFoundError:
                    pass
            require(f'socket:[{sockets[0][1]}]' in fds, 'listener not owned by the waiting decoder')
            require(not any(f'socket:[{i}]' in fds for *_, i in proc_sockets('udp')), 'waiting decoder has UDP socket')
            return '127.0.0.1 (decoder-owned TCP; no decoder UDP socket at readiness)'
        time.sleep(0.005)
    raise TimeoutError(f'decoder did not listen on {port}')


def run_one(a, directory, impl, path, binary, mode, expected=None, negative=None, copies=1, null=False):
    directory.mkdir()
    env = {k: v for k, v in os.environ.items() if not k.startswith('TRACY_') and k != 'ZXC_WAIT_FOR_TRACY'}
    env.update(TRACY_PORT=str(a.port), LC_ALL='C')
    require(not listener(a.port), f'port {a.port} already listens')
    waiting = mode in ('attached', 'timeout', 'runner-timeout', 'off-wait')
    if waiting and impl == 'c':
        env['ZXC_WAIT_FOR_TRACY'] = '1'
    if impl == 'c':
        # C -q suppresses its error diagnostic; preserve the output contract but
        # require the codec's actual error with quiet off in the two corrupt cases.
        argv = [binary, '-dc' if negative and path.stem.startswith('bad-') else '-qdc', f'-T{a.workers}', path]
    else:
        argv = [binary, *(['--wait-for-tracy'] if waiting else []), 'decode', '--threads', str(a.workers), path]
    record = {'implementation': impl, 'mode': mode, 'fixture': str(path), 'workers': a.workers,
              'port': a.port, 'env': {k: env[k] for k in ('TRACY_PORT', 'LC_ALL', 'ZXC_WAIT_FOR_TRACY') if k in env},
              'sink': '/dev/null' if null else 'regular file, no fsync', 'status': 'failed'}
    stdout, stderr, trace = (directory / name for name in ('stdout.bin', 'stderr.log', 'decode.tracy'))
    dec = cap = None
    try:
        with ExitStack() as files:
            def opened(path):
                return files.enter_context(path.open('xb'))

            deadline = time.monotonic() + (1 if mode == 'runner-timeout' else a.timeout)
            sink = files.enter_context(open(os.devnull, 'wb')) if null else opened(stdout)
            dec = Child(argv, sink, opened(stderr), env, a.decoder_cpus)
            if mode in ('attached', 'timeout', 'runner-timeout'):
                record['listener'] = await_listener(dec, a.port, deadline)
                record['spawn_to_listen_ns'] = time.monotonic_ns() - dec.start
            if mode == 'attached':
                cap = Child([a.capture, '-a', '127.0.0.1', '-p', a.port, '-o', trace],
                            opened(directory / 'capture.log'), sp.STDOUT, env, a.collector_cpus)
                record['collector_spawn_offset_ns'] = cap.start - dec.start
            while True:
                d = dec.reap()
                c = cap.reap() if cap else 0
                require(c in (0, None), f'capture failed: {c}')
                require(d is None or d >= 0, f'decoder terminated by signal: {d}')
                if d is not None and c is not None:
                    break
                if time.monotonic() >= deadline:
                    raise TimeoutError('decoder/capture deadline reached')
                time.sleep(0.002)
        record['stdout'] = None if null else file_record(stdout)
        err = stderr.read_text(errors='replace')
        record['stderr'] = file_record(stderr)
        if negative:
            require(d > 0 and re.search(negative, err, re.I | re.S), f'unexpected {impl} failure: status {d}: {err}')
        else:
            require(d == 0 and (null or record['stdout'] == expected) and not err,
                    f'{impl}/{mode}: status, stdout digest/size or stderr differs: {d}, {record["stdout"]}, {err}')
        if cap:
            require(trace.stat().st_size, 'empty capture')
            csvs = {}
            for kind, options in [('events', ['-u']), ('aggregate', [])]:
                name = directory / f'{kind}.csv'
                with name.open('xb') as f, (directory / f'{kind}.log').open('xb') as log:
                    sp.run([a.csvexport, *options, trace], stdout=f, stderr=log, timeout=a.timeout, check=True)
                csvs[kind] = file_record(name)
            record.update(trace=file_record(trace), csv=csvs,
                          **verify(directory / 'events.csv', impl, path.stem if negative else a.case,
                                   expected['bytes'] if expected else None, copies))
        record['status'] = 'passed'
    except BaseException as exc:
        record['error'] = str(exc)
        raise
    finally:
        for child, name in ((dec, 'decoder'), (cap, 'collector')):
            if child:
                child.close()
                record[name] = child.record
        write_json(directory / 'sample.json', record)
    return record


def inputs(p):
    for name in ('c', 'rust', 'c-off', 'rust-off', 'capture', 'csvexport', 'manifest', 'fixtures', 'build', 'out'):
        p.add_argument('--' + name, type=lambda s: Path(s).resolve(), required=True)
    p.add_argument('--case', choices=('sao', 'single', 'mozilla'), required=True)
    p.add_argument('--workers', type=int, required=True)
    p.add_argument('--port', type=int, required=True)
    p.add_argument('--timeout', type=float, default=90)
    p.add_argument('--decoder-cpus', type=cpu_list)
    p.add_argument('--collector-cpus', type=cpu_list)


def preflight(a, out):
    require(1024 <= a.port <= 65535 and a.workers > 0 and a.timeout > 31, 'invalid port/workers/timeout')
    if a.decoder_cpus and a.collector_cpus:
        require(not set(a.decoder_cpus) & set(a.collector_cpus), 'decoder and collector CPU lists overlap')
    checked_tools = tools(a, out)
    build = json.loads(a.build.read_text())
    # The stock baseline is built and receipted separately, not a paired client's build input.
    expected_pins, build_pins = (json.loads(json.dumps(p)) for p in (PINS, build['pins']))
    for pins in (expected_pins, build_pins):
        pins['rust'].pop('stock_revision', None)
    require(build_pins == expected_pins, 'build receipt has different source/tool pins')
    for key, headers in [('C', build['c_client_public']), ('Rust', build['rust']['locked_client_public'])]:
        require(protocol(Path(headers['public'])) == headers, f'{key} headers differ from build receipt')
    binaries = {key: stable_binary(path, build['binaries'][key]) for key, path in
                [('c_cap', a.c), ('rust_cap', a.rust), ('c_off', a.c_off), ('rust_off', a.rust_off)]}
    return {'schema': MAP['schema'], 'qualification': 'uncalibrated / qualitative only',
            'map_sha256': sha(HERE / 'boundaries.json'), 'tools': checked_tools, 'build': build,
            'binaries': binaries, 'uname': list(os.uname()), 'cpuinfo': Path('/proc/cpuinfo').read_text().split('\n\n')[0],
            'smt_active': Path('/sys/devices/system/cpu/smt/active').read_text().strip(),
            'affinity': sorted(os.sched_getaffinity(0)), 'input_type': 'regular file', 'observed_dispatch': 'unknown',
            'topology': {'c': 'caller/header reader + decode workers + ordered writer; regular W>1 can pread',
                         'rust': 'serial at W=1; default Crossbeam workers and ordered caller commit at W>1',
                         'c_jobs_from_source': a.workers * 4, 'rust_slots_from_source':
                         None if a.workers == 1 else min(2 * a.workers, 256)},
            'workers': a.workers, 'case': a.case}


def negatives(a, out, info, path, plain):
    require(a.case == 'sao' and a.workers == 1, 'negatives are specified for sao W1')
    require(a.stock_c and a.stock_rust and a.verify_c, '--negatives requires --stock-c, --stock-rust and --verify-c')
    build = info['build']
    c_fixture = json.loads(a.manifest.read_text())['c']
    info['binaries']['stock_c'] = stable_binary(a.stock_c, c_fixture)
    stock_rust = json.loads(a.stock_rust.read_text())
    require(stock_rust['revision'] == PINS['rust']['stock_revision'], 'stock Rust source pin mismatch')
    stock_binary = Path(stock_rust['path'])
    info['binaries']['stock_rust'] = stable_binary(stock_binary, stock_rust)
    info['stock_rust_build'] = stock_rust
    info['binaries']['c_verify'] = stable_binary(a.verify_c, build['binaries']['c_verify'])
    invalid = Path(build['c_client_public']['public']).parents[1] / 'c-src/tests/conformance/v9/invalid/spl_forged_offset.zxc'
    require(sha(invalid) == PINS['invalid_reconstruct_sha256'], 'invalid SPL fixture hash mismatch')
    recon = out / 'bad-reconstruct.zxc'
    recon.write_bytes(invalid.read_bytes())
    # The pinned input has 14 checked SPL blocks. Damage only the last block checksum.
    archive = bytearray(path.read_bytes())
    offset = 16
    for _ in range(14):
        require(archive[offset] == 1, 'not the expected SPL block')
        trailer = offset + 8 + int.from_bytes(archive[offset+3:offset+7], 'little')
        offset = trailer + 4
    archive[trailer] ^= 1
    checksum = out / 'bad-checksum.zxc'
    checksum.write_bytes(archive)
    info['negative_inputs'] = {key.name: file_record(key) for key in (checksum, recon)} | {'flipped_byte_offset': trailer}
    for impl, off, cap, stock in [('c', a.c_off, a.c, a.stock_c), ('rust', a.rust_off, a.rust, stock_binary)]:
        result = run_one(a, out / f'{impl}-no-collector', impl, path, cap, 'timeout',
                         negative=r'Tracy connection timed out|no Tracy capture connected')
        require(result['stdout']['bytes'] == 0, 'wait timed out after emitting plaintext')
        for case, corrupt, error, cap_binary in [
            ('bad-checksum', checksum, r'checksum', cap),
            ('bad-reconstruct', recon, r'offset|match.*history|before.*(output|start)|invalid.*distance',
             a.verify_c if impl == 'c' else cap),
        ]:
            rows = [run_one(a, out / f'{impl}-{case}-{mode}', impl, corrupt, bin, mode, negative=error)
                    for mode, bin in [('stock', stock), ('off', off), ('attached', cap_binary)]]
            require(all((r['stdout'], r['decoder']['status'], r['stderr']) ==
                        (rows[0]['stdout'], rows[0]['decoder']['status'], rows[0]['stderr']) for r in rows),
                    f'{impl}/{case} stdout/status/stderr differ from stock')
    run_one(a, out / 'c-off-ignores-wait', 'c', path, a.c_off, 'off-wait', plain)
    release = Path(build['binaries']['rust_release']['path'])
    info['binaries']['rust_release'] = stable_binary(release, build['binaries']['rust_release'])
    run_one(a, out / 'rust-release', 'rust', path, release, 'off', plain)
    run_one(a, out / 'rust-release-no-wait-flag', 'rust', path, release, 'off-wait',
            negative=r'unexpected.*--wait-for-tracy')
    try:
        run_one(a, out / 'c-watchdog', 'c', path, a.c, 'runner-timeout')
    except TimeoutError as exc:
        receipt = json.loads((out / 'c-watchdog/sample.json').read_text())
        require(str(exc) == 'decoder/capture deadline reached' and receipt['decoder']['status'] == -signal.SIGKILL,
                'watchdog did not kill and reap its waiting decoder')
    else:
        raise RuntimeError('watchdog unexpectedly finished')


def main():
    p = argparse.ArgumentParser(description=__doc__)
    inputs(p)
    p.add_argument('--negatives', action='store_true')
    p.add_argument('--null-smoke', action='store_true', help='also check attached zone counts with stdout at /dev/null')
    for name in ('verify-c', 'stock-c', 'stock-rust'):
        p.add_argument('--' + name, type=lambda s: Path(s).resolve())
    a = p.parse_args()
    out = new_run(a.out)
    info = {'status': 'failed'}
    signal.signal(signal.SIGTERM, interrupted)
    try:
        info.update(preflight(a, out))
        with perf_lock():
            path, compressed, plain = fixture(a)
            info.update(archive=compressed, expected_plaintext=plain, manifest_sha256=sha(a.manifest))
            for impl, off, cap in [('c', a.c_off, a.c), ('rust', a.rust_off, a.rust)]:
                for mode, binary in [('off', off), ('detached', cap), ('attached', cap)]:
                    run_one(a, out / f'{impl}-{mode}', impl, path, binary, mode, plain)
                if a.null_smoke:
                    run_one(a, out / f'{impl}-attached-null', impl, path, cap, 'attached', plain, null=True)
            if a.negatives:
                negatives(a, out, info, path, plain)
        info['status'] = 'passed'
    except BaseException as exc:
        info['error'] = str(exc)
        raise
    finally:
        write_json(out / 'run.json', info)
    print(out / 'run.json')


def interrupted(*_):
    raise KeyboardInterrupt('SIGTERM')


if __name__ == '__main__':
    main()
