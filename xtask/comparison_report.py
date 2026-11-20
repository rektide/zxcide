#!/usr/bin/env python3
"""Stage, verify and measure the pinned Silesia CLI comparison."""
import argparse
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import random
import resource
import shlex
import shutil
import subprocess as sp
import sys
import time
import traceback
import uuid
import zipfile

from comparison_build import PERF_REV, ROOT, build_current, build_reference, host, snapshot
from comparison_tables import (QUICK, SAMPLES, SEED, archive_key, archive_path, bundle_members, bundle_path,
                                check, decode_points, encode_argv, files_arg, jobs_v2, levels_arg, matrix_lines, render, rows, source_key,
                                validate, verification_plan, worker_policy, workers_arg)
from scorecard import FILES, URL, ZIP_SHA, sha, verified_zip

MANIFEST = ROOT / 'doc/profiling/scorecard/dense-full-smoke-20261120.json'
LEVEL_HASHES = MANIFEST.with_name('upstream-level-hashes.json')
CHUNK = 1024 * 1024


def identity(path):
    return dict(bytes=path.stat().st_size, sha256=sha(path))


def load_before(path, run):
    check(not path.name.startswith('incomplete-') and (path / 'COMPLETE').is_file(),
          f'not a completed comparison run: {path}')
    raw_path = path / 'raw.json'
    check((path / 'COMPLETE').read_text().strip() == sha(raw_path), f'baseline raw evidence checksum mismatch: {raw_path}')
    old = json.loads(raw_path.read_text())
    validate(old)
    current = old['binaries']['current']
    info = snapshot(path / 'bin/current', run, 'before', current['build'])
    check(info['sha256'] == current['sha256'], f'baseline executable checksum mismatch: {path}/bin/current')
    info['source_run'] = str(path)
    return info


def exact_decode(argv, run, names, log):
    command = shlex.join(argv)
    with sp.Popen(argv, cwd=run, stdout=sp.PIPE, stderr=log) as proc:
        try:
            with proc.stdout as stream:
                for name in names:
                    with (run / 'scratch' / name).open('rb') as original:
                        while chunk := original.read(CHUNK):
                            decoded = stream.read(len(chunk))
                            if decoded != chunk:
                                # If output ended early, report a CLI rejection before a byte mismatch.
                                # Do not await a child still writing to our pipe on a content mismatch.
                                if len(decoded) < len(chunk):
                                    if (status := proc.wait()) != 0:
                                        raise sp.CalledProcessError(status, argv)
                                raise ValueError(f'decoded bytes mismatch at {name}: {command}')
                check(stream.read(1) == b'', f'decoded bytes mismatch (trailing bytes): {command}')
            if (status := proc.wait()) != 0:
                raise sp.CalledProcessError(status, argv)
        except BaseException:
            proc.kill()
            raise


def prepare(raw, run, corpus, log):
    old = json.loads(MANIFEST.read_text())['files']
    pins = json.loads(LEVEL_HASHES.read_text())
    check(pins['upstream_revision'] == PERF_REV and pins['corpus_zip_sha256'] == ZIP_SHA, 'wrong level manifest pins')
    check(set(pins['files']) == set(FILES) == set(old), 'wrong manifest scope')
    with zipfile.ZipFile(corpus) as z:
        check(sorted(z.namelist()) == sorted(FILES), f'unexpected ZIP member set: {corpus}')
        for name in raw['selected_files']:
            print(f'Staging {name}', file=log, flush=True)
            plain = run / 'scratch' / name
            with z.open(name) as src, plain.open('wb') as dst:
                shutil.copyfileobj(src, dst, CHUNK)
            entry = identity(plain)
            check(all(old[name][k] == v for k, v in entry.items()), f'plaintext identity mismatch: {name}')
            archives = entry['archives'] = {}
            raw['files'][name] = entry
            for point, impl in rows(raw, 'encode'):
                key = archive_key('encode', point, impl)
                archive = archive_path(name, key)
                with (run / archive).open('wb') as out:
                    sp.run(encode_argv(impl, point, name), cwd=run, stdout=out, stderr=log, check=True)
                arc = archives[key] = identity(run / archive)
                if impl == 'c':
                    pin = pins['files'][name][point[1:]]
                    check(arc['sha256'] == pin, f'fixed archive hash mismatch: {archive}')
                    if point in ('C3', 'C7'):
                        check(pin == old[name]['archives'][point.lower()]['sha256'], f'conflicting manifests: {archive}')
                    arc['frozen_sha256'] = pin

    if raw['concat']:
        for point in decode_points(raw):
            bundle = run / bundle_path(point)
            members = bundle_members(raw, point)
            with bundle.open('wb') as out:
                for member in members:
                    with (run / archive_path(member['file'], source_key(point))).open('rb') as src:
                        shutil.copyfileobj(src, out, CHUNK)
            raw['bundles'][point] = dict(identity(bundle), members=members)

    raw['jobs'] = dict(jobs_v2(raw))
    for key, (record, names) in verification_plan(raw).items():
        argv = record['argv']
        print(f'Exact decode: {shlex.join(argv)}', file=log, flush=True)
        try:
            exact_decode(argv, run, names, log)
        except (OSError, ValueError, sp.CalledProcessError) as err:
            # A saved binary may predate the requested option or concatenated-frame support.
            raise ValueError(f'Exact decode failed for {argv[0]} ({run / argv[0]}) with argv {argv}: {err}. '
                             'If this CLI does not support the workload, try --decode-workers 1 or omit --concat.') from err
        raw['verification'][key] = record


def sample(raw, run, log):
    rng = random.Random(SEED)
    order = list(raw['jobs'])
    with open(os.devnull, 'wb') as sink:
        for p in range(SAMPLES + 1):
            phase = 'sample' if p else 'warmup'
            rng.shuffle(order)
            print(f'Pass {p} ({phase}, {len(order)} jobs)', file=log, flush=True)
            for pos, jid in enumerate(order):
                argv = raw['jobs'][jid]['argv']
                # Sample only an individual awaited child and all of its CLI threads.
                cpu_start = resource.getrusage(resource.RUSAGE_CHILDREN)
                start = time.perf_counter_ns()
                sp.run(argv, cwd=run, stdout=sink, stderr=log, check=True)
                ns = time.perf_counter_ns() - start
                cpu_end = resource.getrusage(resource.RUSAGE_CHILDREN)
                raw['observations'].append({'pass': p, 'phase': phase, 'order': pos, 'job': jid, 'elapsed_ns': ns,
                                            'user_us': round((cpu_end.ru_utime - cpu_start.ru_utime) * 1e6),
                                            'system_us': round((cpu_end.ru_stime - cpu_start.ru_stime) * 1e6)})


def measure(args, run, log):
    (run / 'bin').mkdir()
    corpus = args.target / 'scorecard/silesia.zip'
    cached = corpus.exists()
    machine = host()
    scope = 'quick' if args.quick else 'subset' if args.files else 'full'
    files = list(QUICK if args.quick else args.files or FILES)
    raw = dict(schema=3, argv=sys.argv, timestamp_utc=datetime.now(timezone.utc).isoformat(),
               scope=scope, selected_files=files, levels=args.levels, decode_workers=args.decode_workers,
               concat=args.concat, seed=SEED, warmups=1, samples=SAMPLES, host=machine, python=sys.version,
               binaries={k: None for k in (('c', 'current', 'before') if args.before else ('c', 'current'))},
               corpus=dict(url=URL, zip_sha256=ZIP_SHA, cached=cached,
                           manifests={str(p.relative_to(ROOT)): sha(p) for p in (MANIFEST, LEVEL_HASHES)}),
               files={}, bundles={}, verification={}, jobs={}, observations=[])
    for line in matrix_lines(raw):
        print(line, flush=True)
        print(line, file=log, flush=True)
    print(f'Corpus {"cache hit" if cached else "download"}: {corpus}', file=log, flush=True)
    verified_zip(corpus)
    raw['binaries']['c'] = build_reference(args.target / 'comparison-report/cache', run, machine, log)
    raw['binaries']['current'] = build_current(run, log)
    if args.before:
        raw['binaries']['before'] = load_before(args.before.resolve(), run)
    raw['decode_policy'] = worker_policy(raw)
    work = run / 'scratch'
    work.mkdir()
    try:
        prepare(raw, run, corpus, log)
        sample(raw, run, log)
    finally:
        shutil.rmtree(work)
    raw['finished_utc'] = datetime.now(timezone.utc).isoformat()
    report, summary = render(raw)
    (run / 'raw.json').write_text(json.dumps(raw, indent=2) + '\n')
    (run / 'report.md').write_text(report)
    print(report, file=log)
    (run / 'COMPLETE').write_text(sha(run / 'raw.json') + '\n')
    return summary


def publish(args):
    runs = args.target / 'comparison-report/runs'
    rid = datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%SZ-') + uuid.uuid4().hex[:12]
    run = runs / f'incomplete-{rid}'
    run.mkdir(parents=True)
    print(f'In progress: {run}/run.log', file=sys.stderr, flush=True)
    with (run / 'run.log').open('w', buffering=1) as log:
        try:
            summary = measure(args, run, log)
        except BaseException:
            traceback.print_exc(file=log)
            raise
    dest = runs / rid
    run.rename(dest)
    print(f'{summary}\n\nReport: {dest}/report.md\nLog: {dest}/run.log')
    return dest


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--target', type=Path, required=True)
    scope = p.add_mutually_exclusive_group()
    scope.add_argument('--quick', action='store_true')
    scope.add_argument('--files', type=files_arg)
    p.add_argument('--before', type=Path)
    p.add_argument('--levels', type=levels_arg, default='1,3,7')
    p.add_argument('--decode-workers', type=workers_arg, default='1,4,8')
    p.add_argument('--concat', action='store_true')
    args = p.parse_args()
    if args.concat and args.files and len(args.files) < 2:
        p.error('--concat requires at least two files')
    args.target = args.target.resolve()
    publish(args)


if __name__ == '__main__':
    main()
